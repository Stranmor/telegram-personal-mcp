#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::json;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use telegram_personal_mcp::{
    ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer, McpServerSettings,
    PreparedSendState, SendApprovalState, SendLedger,
};
use telegram_personal_mcp::domain::{
    PreparedNonTelegramSource, PreparedOutboundSource, PreparedSendTarget,
};

fn grant_policy(grant_path: std::path::PathBuf) -> ApprovalPolicy {
    ApprovalPolicy {
        approval_chat_id: 700,
        approval_user_id: 700,
        ttl_seconds: 3_600,
        poll_timeout_seconds: 1,
        owner_grant_file: Some(grant_path),
    }
}

fn write_grant(path: &std::path::Path, remaining: u32) -> anyhow::Result<()> {
    let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
    let grant = serde_json::json!({
        "granted_by": 700,
        "granted_at_unix": now,
        "expires_at_unix": now + 3_600,
        "remaining_messages": remaining,
        "allow_peers": ["@alex", "alex", "Alex", "101"],
        "reason": "owner-grant send contract test",
    });
    std::fs::write(path, serde_json::to_vec_pretty(&grant)?)?;
    Ok(())
}

fn grant_remaining(path: &std::path::Path) -> anyhow::Result<u32> {
    let raw = std::fs::read_to_string(path)?;
    let grant: serde_json::Value = serde_json::from_str(&raw)?;
    Ok(grant
        .get("remaining_messages")
        .and_then(serde_json::Value::as_u64)
        .expect("remaining_messages") as u32)
}

#[tokio::test]
async fn owner_grant_send_reaches_sent() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let grant_path = temp.path().join("grant.json");
    write_grant(&grant_path, 10)?;
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: state_db_path.clone(),
        },
        grant_policy(grant_path.clone()),
        bot.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Owner-grant contract: first send",
                    "source": { "kind": "self_originated" },
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
    let payload_sha256 = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("payload hash")
        .to_string();

    let sent = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval").with_arguments(
                json!({
                    "draft_id": draft_id,
                    "approved_payload_sha256": payload_sha256,
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(sent.is_error, Some(false));
    let state = sent
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/state"))
        .and_then(|value| value.as_str())
        .expect("approval state");
    assert_eq!(state, "sent");
    let approval_id = sent
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("approval id")
        .to_string();
    let target_message_id = sent
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/target_message_id"))
        .and_then(|value| value.as_i64())
        .expect("target message id");

    assert_eq!(backend.sent_messages().await.len(), 1);
    assert!(bot.sent_cards().await.is_empty());
    assert_eq!(grant_remaining(&grant_path)?, 9);

    let ledger = SendLedger::new(state_db_path);
    let approval = ledger.get_approval(&approval_id)?.expect("approval record");
    assert_eq!(approval.state, SendApprovalState::Sent);
    assert_eq!(approval.target_message_id, Some(target_message_id as i32));
    let prepared_record = ledger.get(&draft_id)?.expect("prepared record");
    assert_eq!(prepared_record.public.state, PreparedSendState::Sent);
    assert_eq!(prepared_record.public.message_id, Some(target_message_id as i32));

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn owner_grant_repeat_returns_same_receipt_without_second_send() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let grant_path = temp.path().join("grant.json");
    write_grant(&grant_path, 10)?;
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: state_db_path.clone(),
        },
        grant_policy(grant_path.clone()),
        bot.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Owner-grant contract: idempotent repeat",
                    "source": { "kind": "self_originated" },
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
    let arguments = json!({
        "draft_id": draft_id,
        "approved_payload_sha256": payload_sha256,
    })
    .as_object()
    .expect("object")
    .clone();

    let first = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval")
                .with_arguments(arguments.clone()),
        )
        .await?;
    assert_eq!(first.is_error, Some(false));
    let first_approval_id = first
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("first approval id")
        .to_string();
    let first_target = first
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/target_message_id"))
        .and_then(|value| value.as_i64())
        .expect("first target");
    assert_eq!(backend.sent_messages().await.len(), 1);
    assert_eq!(grant_remaining(&grant_path)?, 9);

    let second = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval")
                .with_arguments(arguments.clone()),
        )
        .await?;
    assert_eq!(second.is_error, Some(false));
    let second_approval_id = second
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("second approval id");
    let second_target = second
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/target_message_id"))
        .and_then(|value| value.as_i64())
        .expect("second target");
    let second_state = second
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/state"))
        .and_then(|value| value.as_str())
        .expect("second state");
    assert_eq!(second_approval_id, first_approval_id);
    assert_eq!(second_target, first_target);
    assert_eq!(second_state, "sent");
    assert_eq!(backend.sent_messages().await.len(), 1);
    assert!(bot.sent_cards().await.is_empty());
    assert_eq!(
        grant_remaining(&grant_path)?,
        9,
        "the repeat must not consume a second grant message"
    );

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn approval_state_write_failure_is_unrecorded_not_retryable() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let grant_path = temp.path().join("grant.json");
    write_grant(&grant_path, 10)?;

    SendLedger::new(state_db_path.clone()).get("init")?;
    {
        let connection = rusqlite::Connection::open(&state_db_path)?;
        connection.execute_batch(
            "CREATE TRIGGER block_approval_sent
             BEFORE UPDATE OF state ON send_approvals
             WHEN NEW.state = 'sent'
             BEGIN
                 SELECT RAISE(ABORT, 'injected approval sent failure');
             END;",
        )?;
    }

    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: state_db_path.clone(),
        },
        grant_policy(grant_path),
        bot.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Owner-grant contract: unrecorded state",
                    "source": { "kind": "self_originated" },
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

    let failed = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval").with_arguments(
                json!({
                    "draft_id": draft_id,
                    "approved_payload_sha256": payload_sha256,
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(failed.is_error, Some(true));
    let error = failed.structured_content.as_ref().expect("error payload");
    assert_eq!(
        error.pointer("/error/code").and_then(|value| value.as_str()),
        Some("DELIVERY_STATE_UNRECORDED")
    );
    assert_eq!(
        error.pointer("/error/retryable").and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        error
            .pointer("/error/delivery_confirmed")
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    let message =
        error.pointer("/error/message").and_then(|value| value.as_str()).unwrap_or("");
    assert!(message.contains("was sent"), "message must confirm delivery: {message}");
    assert!(
        message.contains("do not resend"),
        "message must forbid a retry that would duplicate: {message}"
    );

    assert_eq!(
        backend.sent_messages().await.len(),
        1,
        "the Telegram send happened once even though the receipt write failed"
    );
    let ledger = SendLedger::new(state_db_path);
    let prepared_record = ledger.get(&draft_id)?.expect("prepared record");
    assert_eq!(prepared_record.public.state, PreparedSendState::Sent);
    assert_eq!(
        prepared_record.public.message_id,
        backend.sent_messages().await.first().map(|sent| sent.message_id)
    );

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[test]
fn approval_transition_refusal_names_states() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = SendLedger::new(temp.path().join("state.sqlite"));
    let prepared = ledger
        .prepare(
            7,
            "101".to_string(),
            PreparedSendTarget {
                display_name: "Alex".to_string(),
                username: Some("alex".to_string()),
                kind: "user".to_string(),
            },
            PreparedOutboundSource::NonTelegram(PreparedNonTelegramSource::SelfOriginated),
            "Hello".to_string(),
            None,
            "abc".to_string(),
        )
        .expect("prepare");
    let approval = ledger
        .create_or_get_approval(&prepared.public.draft_id, "abc", 700, 700, 3_600)
        .expect("approval")
        .public;
    assert_eq!(approval.state, SendApprovalState::DispatchingCard);
    let error = ledger
        .mark_approval_sent(&approval.approval_id, 42)
        .expect_err("dispatching_card must not transition straight to sent");
    let text = error.to_string();
    assert!(text.contains("dispatching_card"), "names the source state: {text}");
    assert!(text.contains("sent"), "names the target state: {text}");
    assert!(
        !text.contains("Conversion error"),
        "must not look like a type conversion: {text}"
    );
}
