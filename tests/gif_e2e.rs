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
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, FileSendMode, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger,
};

const ONE_PIXEL_GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff!\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;";

async fn wait_for_applied(
    state_db_path: &std::path::Path,
    approval_id: &str,
) -> anyhow::Result<()> {
    let ledger = MutationLedger::new(state_db_path.to_path_buf());
    let mut last = None;
    for _ in 0..900 {
        let approval = ledger.get_approval(approval_id)?;
        if approval
            .as_ref()
            .is_some_and(|approval| approval.state == MutationApprovalState::Applied)
        {
            return Ok(());
        }
        last = approval.map(|approval| {
            (approval.state, approval.last_error_code, approval.automated_retry_count)
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("GIF mutation did not become applied; last={last:?}")
}

#[tokio::test]
async fn gif_send_is_magic_validated_hash_bound_approved_and_exactly_once() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: state_db_path.clone(),
        },
        ApprovalPolicy {
            approval_chat_id: 700,
            approval_user_id: 700,
            ttl_seconds: 3_600,
            poll_timeout_seconds: 1,
            owner_grant_file: None,
            owner_permission_file: None,
        },
        bot.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let worker = server.start_approval_worker()?.expect("approval worker");
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let downloaded = client
        .call_tool(CallToolRequestParams::new("download_attachment").with_arguments(
            json!({ "chat_id": "@alex", "message_id": 11 }).as_object().expect("object").clone(),
        ))
        .await?;
    let managed_dir = std::path::Path::new(
        downloaded
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/local_path"))
            .and_then(Value::as_str)
            .expect("managed attachment path"),
    )
    .parent()
    .expect("managed attachment directory")
    .to_path_buf();

    let invalid_path = managed_dir.join("not-really.gif");
    let invalid_bytes = b"not a gif";
    std::fs::write(&invalid_path, invalid_bytes)?;
    let invalid_sha256 = format!("{:x}", Sha256::digest(invalid_bytes));
    let invalid = client
        .call_tool(
            CallToolRequestParams::new("send_gif").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": invalid_path,
                    "approved_sha256": invalid_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(invalid.is_error, Some(true));
    assert_eq!(
        invalid
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(Value::as_str),
        Some("INVALID_ARGUMENT")
    );

    let gif_path = managed_dir.join("approved-animation.gif");
    std::fs::write(&gif_path, ONE_PIXEL_GIF)?;
    let gif_sha256 = format!("{:x}", Sha256::digest(ONE_PIXEL_GIF));
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("send_gif").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": gif_path,
                    "caption": "Approved GIF",
                    "reply_to_message_id": 11,
                    "approved_sha256": gif_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared GIF mutation");
    assert_eq!(prepared.pointer("/data/action/kind").and_then(Value::as_str), Some("send_file"));
    assert_eq!(prepared.pointer("/data/action/mode").and_then(Value::as_str), Some("gif"));
    assert_eq!(
        prepared.pointer("/data/action/artifact/sha256").and_then(Value::as_str),
        Some(gif_sha256.as_str())
    );

    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": prepared
                        .pointer("/data/mutation_id")
                        .and_then(Value::as_str)
                        .expect("mutation id"),
                    "approved_payload_sha256": prepared
                        .pointer("/data/payload_sha256")
                        .and_then(Value::as_str)
                        .expect("payload hash")
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(requested.is_error, Some(false));
    let requested = requested.structured_content.expect("GIF approval request");
    let approval_id = requested
        .pointer("/data/approval_id")
        .and_then(Value::as_str)
        .expect("approval id")
        .to_string();
    let approval_message_id = requested
        .pointer("/data/approval_message_id")
        .and_then(Value::as_i64)
        .expect("approval message id");
    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    wait_for_applied(&state_db_path, &approval_id).await?;

    let sent = backend.sent_messages().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].file_mode, Some(FileSendMode::Gif));
    assert_eq!(sent[0].reply_to_message_id, Some(11));
    let message = client
        .call_tool(
            CallToolRequestParams::new("get_message").with_arguments(
                json!({ "chat_id": "@alex", "message_id": sent[0].message_id })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(
        message
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/message/media_type"))
            .and_then(Value::as_str),
        Some("gif")
    );

    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(backend.sent_messages().await.len(), 1);
    let cards = format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(cards.contains("Gif"));
    assert!(cards.contains(gif_sha256.as_str()));

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}
