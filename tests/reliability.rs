use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use telegram_personal_mcp::{
    ApprovalPolicy, Config, FakeApprovalBot, FakeTelegramBackend, McpServer, McpServerSettings,
    MutationAction, MutationApprovalState, MutationLedger, PreparedSendState, SendApprovalState,
    SendLedger, TelegramBackend, TelegramClient,
};

#[tokio::test]
async fn prepared_send_deduplicates_after_crash_reopen() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let settings = McpServerSettings {
        attachment_cache_dir: temp.path().join("attachments"),
        max_download_bytes: 1024 * 1024,
        max_upload_bytes: 1024 * 1024,
        state_db_path: state_db_path.clone(),
    };

    let first_server = McpServer::with_settings(backend.clone(), settings.clone());
    let (first_server_transport, first_client_transport) = tokio::io::duplex(64 * 1024);
    let first_server_task = tokio::spawn(async move {
        first_server.serve(first_server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let first_client = ().serve(first_client_transport).await?;

    let prepared = first_client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Prepared reply after a simulated crash",
                    "source": { "kind": "self_originated" },
                    "reply_to_message_id": 11
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let draft_id = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/draft_id"))
        .and_then(|value| value.as_str())
        .expect("draft id")
        .to_string();
    let approved_payload_sha256 = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("prepared payload hash")
        .to_string();

    let ledger = SendLedger::new(state_db_path.clone());
    let record = ledger.get(&draft_id)?.expect("prepared record");
    ledger.mark_dispatching(&draft_id)?;

    // Telegram accepts the request, but the process dies before persisting its receipt.
    let accepted_message_id = backend
        .send_text_with_random_id(
            None,
            &record.public.chat_id,
            &record.public.text,
            record.public.reply_to_message_id,
            record.random_id,
        )
        .await?;
    assert_eq!(backend.sent_messages().await.len(), 1);
    assert_eq!(
        ledger.get(&draft_id)?.expect("dispatching record").public.state,
        PreparedSendState::Dispatching
    );

    first_client.cancel().await?;
    first_server_task.await??;

    let reopened_ledger = SendLedger::new(state_db_path.clone());
    let reopened_record = reopened_ledger.get(&draft_id)?.expect("reopened record");
    assert_eq!(reopened_record.public.state, PreparedSendState::Dispatching);
    assert_eq!(reopened_record.random_id, record.random_id);

    let reopened_server = McpServer::with_settings(backend.clone(), settings);
    let (reopened_server_transport, reopened_client_transport) = tokio::io::duplex(64 * 1024);
    let reopened_server_task = tokio::spawn(async move {
        reopened_server.serve(reopened_server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let reopened_client = ().serve(reopened_client_transport).await?;

    let retried = reopened_client
        .call_tool(
            CallToolRequestParams::new("send_prepared").with_arguments(
                json!({
                    "draft_id": draft_id,
                    "approved_payload_sha256": approved_payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(retried.is_error, Some(false));
    assert_eq!(
        retried
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/message_id"))
            .and_then(|value| value.as_i64()),
        Some(i64::from(accepted_message_id))
    );
    assert_eq!(backend.sent_messages().await.len(), 1);

    let sent_record = reopened_ledger.get(&draft_id)?.expect("sent record");
    assert_eq!(sent_record.public.state, PreparedSendState::Sent);
    assert_eq!(sent_record.public.message_id, Some(accepted_message_id));
    assert_eq!(sent_record.random_id, record.random_id);

    reopened_client.cancel().await?;
    reopened_server_task.await??;
    Ok(())
}

#[tokio::test]
async fn approval_worker_recovers_after_target_acceptance_and_process_restart() -> anyhow::Result<()>
{
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let settings = McpServerSettings {
        attachment_cache_dir: temp.path().join("attachments"),
        max_download_bytes: 1024 * 1024,
        max_upload_bytes: 1024 * 1024,
        state_db_path: state_db_path.clone(),
    };
    let policy = ApprovalPolicy {
        approval_chat_id: 700,
        approval_user_id: 700,
        ttl_seconds: 3_600,
        poll_timeout_seconds: 1,
        owner_grant_file: None,
    };

    let first_server =
        McpServer::with_approval(backend.clone(), settings.clone(), policy.clone(), bot.clone())
            .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        first_server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Crash-safe approval send",
                    "source": { "kind": "self_originated" }
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let draft_id = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/draft_id"))
        .and_then(|value| value.as_str())
        .expect("draft id")
        .to_string();
    let payload_sha256 = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("payload hash")
        .to_string();
    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval").with_arguments(
                json!({
                    "draft_id": draft_id,
                    "approved_payload_sha256": payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let approval_id = requested
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("approval id")
        .to_string();
    client.cancel().await?;
    server_task.await??;

    let ledger = SendLedger::new(state_db_path.clone());
    let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
    assert!(ledger.mark_approval_sending(&approval_id, now)?);
    let prepared_record = ledger.get(&draft_id)?.expect("prepared record");
    let accepted_message_id = backend
        .send_text_with_random_id(
            None,
            &prepared_record.public.chat_id,
            &prepared_record.public.text,
            prepared_record.public.reply_to_message_id,
            prepared_record.random_id,
        )
        .await?;
    assert_eq!(backend.sent_messages().await.len(), 1);
    assert_eq!(
        ledger.get_approval(&approval_id)?.expect("sending approval").state,
        SendApprovalState::Sending
    );

    let reopened = McpServer::with_approval(backend.clone(), settings, policy, bot)
        .map_err(anyhow::Error::msg)?;
    let worker = reopened.start_approval_worker()?.expect("approval worker");
    for _ in 0..200 {
        if ledger.get_approval(&approval_id)?.is_some_and(|approval| {
            approval.state == SendApprovalState::Sent
                && approval.target_message_id == Some(accepted_message_id)
        }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let recovered = ledger.get_approval(&approval_id)?.expect("recovered approval");
    assert_eq!(recovered.state, SendApprovalState::Sent);
    assert_eq!(recovered.target_message_id, Some(accepted_message_id));
    assert_eq!(backend.sent_messages().await.len(), 1);
    assert_eq!(ledger.get(&draft_id)?.expect("sent draft").public.state, PreparedSendState::Sent);
    worker.abort();
    let _ = worker.await;
    Ok(())
}

#[tokio::test]
async fn file_approval_recovers_after_acceptance_without_duplicate_upload_message()
-> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let settings = McpServerSettings {
        attachment_cache_dir: temp.path().join("attachments"),
        max_download_bytes: 1024 * 1024,
        max_upload_bytes: 1024 * 1024,
        state_db_path: state_db_path.clone(),
    };
    let policy = ApprovalPolicy {
        approval_chat_id: 700,
        approval_user_id: 700,
        ttl_seconds: 3_600,
        poll_timeout_seconds: 1,
        owner_grant_file: None,
    };

    let first_server =
        McpServer::with_approval(backend.clone(), settings.clone(), policy.clone(), bot.clone())
            .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        first_server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let downloaded = client
        .call_tool(CallToolRequestParams::new("download_attachment").with_arguments(
            json!({ "chat_id": "@alex", "message_id": 11 }).as_object().expect("object").clone(),
        ))
        .await?;
    let downloaded_path = downloaded
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/local_path"))
        .and_then(|value| value.as_str())
        .expect("download path");
    let file_path = std::path::Path::new(downloaded_path)
        .parent()
        .expect("download directory")
        .join("crash-safe-file.txt");
    let bytes = b"accepted before process crash\n";
    std::fs::write(&file_path, bytes)?;
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_file_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": file_path,
                    "caption": "Crash-safe file",
                    "reply_to_message_id": 11,
                    "mode": "document",
                    "approved_sha256": sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let mutation_id = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/mutation_id"))
        .and_then(|value| value.as_str())
        .expect("mutation id");
    let payload_sha256 = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("payload hash");
    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": mutation_id,
                    "approved_payload_sha256": payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let approval_id = requested
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("approval id")
        .to_string();
    client.cancel().await?;
    server_task.await??;

    let ledger = MutationLedger::new(state_db_path.clone());
    let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
    assert!(ledger.mark_approval_applying(&approval_id, now)?);
    let approval = ledger.get_approval(&approval_id)?.expect("applying approval");
    let MutationAction::SendFile {
        chat_id,
        artifact,
        caption,
        reply_to_message_id,
        mode,
        random_id,
    } = approval.action.clone()
    else {
        anyhow::bail!("prepared action was not a file send")
    };
    let artifact_path = state_db_path
        .with_extension("outbox")
        .join(&artifact.artifact_id)
        .join(&artifact.file_name);
    let accepted_message_id = backend
        .send_file_with_random_id(
            None,
            &chat_id,
            &artifact_path,
            &caption,
            reply_to_message_id,
            mode,
            random_id,
        )
        .await?;
    assert_eq!(backend.sent_messages().await.len(), 1);
    assert_eq!(
        ledger.get_approval(&approval_id)?.expect("still applying").state,
        MutationApprovalState::Applying
    );

    let reopened = McpServer::with_approval(backend.clone(), settings, policy, bot)
        .map_err(anyhow::Error::msg)?;
    let worker = reopened.start_approval_worker()?.expect("approval worker");
    for _ in 0..300 {
        if ledger.get_approval(&approval_id)?.is_some_and(|approval| {
            approval.state == MutationApprovalState::Applied
                && approval.result
                    == Some(telegram_personal_mcp::MutationResult::FileSent {
                        message_id: accepted_message_id,
                        sha256: artifact.sha256.clone(),
                    })
        }) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let recovered = ledger.get_approval(&approval_id)?.expect("recovered file approval");
    assert_eq!(recovered.state, MutationApprovalState::Applied);
    assert_eq!(backend.sent_messages().await.len(), 1);
    assert!(!artifact_path.exists());
    worker.abort();
    let _ = worker.await;
    Ok(())
}

#[tokio::test]
async fn multiple_session_files_return_stable_ambiguous_session_error() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let session_dir = temp.path().join("sessions");
    std::fs::create_dir_all(&session_dir)?;
    let later_path = session_dir.join("z-account.session");
    let earlier_path = session_dir.join("a-account.session");
    std::fs::write(&later_path, b"fixture")?;
    std::fs::write(&earlier_path, b"fixture")?;

    let backend = Arc::new(TelegramClient::new(Config {
        session_path: None,
        session_cache_dir: Some(session_dir),
        proxy: None,
        api_id: 1,
        api_hash: "fixture".to_string(),
    }));
    let server = McpServer::with_settings(
        backend,
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
    );
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let first = client.call_tool(CallToolRequestParams::new("health")).await?;
    let second = client.call_tool(CallToolRequestParams::new("health")).await?;
    for response in [&first, &second] {
        assert_eq!(response.is_error, Some(true));
        assert_eq!(
            response
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/error/code"))
                .and_then(|value| value.as_str()),
            Some("AMBIGUOUS_SESSION")
        );
    }

    let first_message = first
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/error/message"))
        .and_then(|value| value.as_str())
        .expect("first error message");
    let second_message = second
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/error/message"))
        .and_then(|value| value.as_str())
        .expect("second error message");
    assert_eq!(first_message, second_message);
    assert_eq!(
        first_message,
        "Multiple Telegram sessions are available; select one in the server configuration"
    );
    assert!(!first_message.contains(earlier_path.to_str().expect("utf-8 path")));
    assert!(!first_message.contains(later_path.to_str().expect("utf-8 path")));

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
