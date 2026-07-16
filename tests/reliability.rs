use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::json;
use std::sync::Arc;
use telegram_personal_mcp::{
    Config, FakeTelegramBackend, McpServer, McpServerSettings, PreparedSendState, SendLedger,
    TelegramBackend, TelegramClient,
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
