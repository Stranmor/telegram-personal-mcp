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
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger, MutationResult, PreparedSendKind,
    ReactionSpec, SendApprovalState, SendLedger, TelegramBackend,
};

#[tokio::test]
async fn telegram_button_is_the_only_send_path_and_cannot_duplicate_delivery() -> anyhow::Result<()>
{
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

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Точный текст первого платного предложения",
                    "source": { "kind": "self_originated" },
                    "reply_to_message_id": 11
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

    let request_arguments = json!({
        "draft_id": draft_id,
        "approved_payload_sha256": payload_sha256
    })
    .as_object()
    .expect("object")
    .clone();
    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval")
                .with_arguments(request_arguments.clone()),
        )
        .await?;
    assert_eq!(requested.is_error, Some(false));
    assert_eq!(
        requested
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/state"))
            .and_then(|value| value.as_str()),
        Some("pending")
    );
    let approval_id = requested
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("approval id")
        .to_string();
    let approval_message_id = requested
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("approval message id");

    let requested_again = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval")
                .with_arguments(request_arguments.clone()),
        )
        .await?;
    assert_eq!(requested_again.is_error, Some(false));
    let sent_cards = bot.sent_cards().await;
    assert_eq!(sent_cards.len(), 1);
    assert_eq!(
        sent_cards.first().expect("approval card").card.approval.text,
        "Точный текст первого платного предложения"
    );

    let bypass = client
        .call_tool(
            CallToolRequestParams::new("send_prepared").with_arguments(request_arguments.clone()),
        )
        .await?;
    assert_eq!(bypass.is_error, Some(true));
    assert_eq!(
        bypass
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("APPROVAL_REQUIRED")
    );
    let direct_text = client
        .call_tool(CallToolRequestParams::new("send_message").with_arguments(
            json!({ "chat_id": "@alex", "text": "Bypass" }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(
        direct_text
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("APPROVAL_REQUIRED")
    );
    let direct_file = client
        .call_tool(
            CallToolRequestParams::new("send_file").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": "/tmp/not-used",
                    "approved_sha256": "0".repeat(64)
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(
        direct_file
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("APPROVAL_REQUIRED")
    );
    assert!(backend.sent_messages().await.is_empty());

    bot.push_decision(&approval_id, ApprovalAction::Approve, 701, 700, approval_message_id).await;
    wait_for_answers(&bot, 1).await?;
    assert_eq!(
        SendLedger::new(state_db_path.clone()).get_approval(&approval_id)?.expect("approval").state,
        SendApprovalState::Pending
    );
    assert!(backend.sent_messages().await.is_empty());

    bot.push_decision(&approval_id, ApprovalAction::Approve, 700, 700, approval_message_id).await;
    wait_for_state(&state_db_path, &approval_id, SendApprovalState::Sent).await?;
    assert_eq!(backend.sent_messages().await.len(), 1);

    bot.push_decision(&approval_id, ApprovalAction::Approve, 700, 700, approval_message_id).await;
    wait_for_answers(&bot, 3).await?;
    assert_eq!(backend.sent_messages().await.len(), 1);
    let final_approval =
        SendLedger::new(state_db_path).get_approval(&approval_id)?.expect("final approval");
    assert_eq!(final_approval.state, SendApprovalState::Sent);
    assert_eq!(final_approval.target_message_id, Some(100));
    assert!(
        bot.edits().await.iter().any(|edit| edit.card.approval.state == SendApprovalState::Sent)
    );

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}

#[tokio::test]
async fn scheduled_approval_expiry_is_capped_to_the_delivery_time() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let server = McpServer::with_approval(
        backend,
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
        ApprovalPolicy {
            approval_chat_id: 700,
            approval_user_id: 700,
            ttl_seconds: 3_600,
            poll_timeout_seconds: 1,
            owner_grant_file: None,
            owner_permission_file: None,
        },
        bot,
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let schedule_at_unix =
        i32::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs().saturating_add(600))?;

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_schedule_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Deadline-bound scheduled message",
                    "source": { "kind": "self_originated" },
                    "schedule_at_unix": schedule_at_unix
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": prepared
                        .structured_content
                        .as_ref()
                        .and_then(|value| value.pointer("/data/mutation_id"))
                        .and_then(|value| value.as_str())
                        .expect("mutation id"),
                    "approved_payload_sha256": prepared
                        .structured_content
                        .as_ref()
                        .and_then(|value| value.pointer("/data/payload_sha256"))
                        .and_then(|value| value.as_str())
                        .expect("payload hash")
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(requested.is_error, Some(false));
    assert_eq!(
        requested
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/expires_at_unix"))
            .and_then(|value| value.as_i64()),
        Some(i64::from(schedule_at_unix))
    );

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn current_session_scheduled_rich_text_is_structured_for_send_and_edit() -> anyhow::Result<()>
{
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
            state_db_path,
        },
        ApprovalPolicy {
            approval_chat_id: 700,
            approval_user_id: 700,
            ttl_seconds: 3_600,
            poll_timeout_seconds: 1,
            owner_grant_file: None,
            owner_permission_file: None,
        },
        bot,
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let schedule_at_unix =
        i32::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs().saturating_add(600))?;
    let markdown = "# Native scheduled Rich Text\n\n- **Bold**\n- _Italic_";

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_schedule_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": markdown,
                    "parse_mode": "rich_markdown",
                    "source": { "kind": "self_originated" },
                    "schedule_at_unix": schedule_at_unix
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    assert_eq!(
        prepared
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/parse_mode"))
            .and_then(|value| value.as_str()),
        Some("rich_markdown")
    );
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
    let applied = client
        .call_tool(
            CallToolRequestParams::new("apply_current_session_scheduled_mutation").with_arguments(
                json!({
                    "mutation_id": mutation_id,
                    "approved_payload_sha256": payload_sha256,
                    "current_session_approval": "Ivan explicitly confirmed this exact scheduled rich-text mutation in the current chat"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(applied.is_error, Some(false));
    let scheduled = backend.get_scheduled_messages(None, "@alex").await?;
    let created = scheduled
        .iter()
        .find(|message| message.date == schedule_at_unix)
        .expect("scheduled rich message");
    assert_eq!(created.text, None);
    let rich = backend
        .get_rich_message_snapshot(None, "@alex", created.message_id)
        .await?
        .expect("native rich message snapshot");
    assert!(rich.block_count > 0);

    let edit_schedule_at_unix = schedule_at_unix.saturating_add(60);
    let prepared_edit = client
        .call_tool(
            CallToolRequestParams::new("prepare_edit_scheduled_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 301,
                    "text": markdown,
                    "parse_mode": "rich_markdown",
                    "schedule_at_unix": edit_schedule_at_unix
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_edit.is_error, Some(false));
    let edit_mutation_id = prepared_edit
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/mutation_id"))
        .and_then(|value| value.as_str())
        .expect("edit mutation id");
    let edit_payload_sha256 = prepared_edit
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("edit payload hash");
    let applied_edit = client
        .call_tool(
            CallToolRequestParams::new("apply_current_session_scheduled_mutation").with_arguments(
                json!({
                    "mutation_id": edit_mutation_id,
                    "approved_payload_sha256": edit_payload_sha256,
                    "current_session_approval": "Ivan explicitly confirmed this exact scheduled rich-text edit in the current chat"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(applied_edit.is_error, Some(false));
    let edited = backend
        .get_scheduled_messages(None, "@alex")
        .await?
        .into_iter()
        .find(|message| message.message_id == 301)
        .expect("edited scheduled rich message");
    assert_eq!(edited.text, None);
    assert_eq!(edited.date, edit_schedule_at_unix);
    assert!(
        backend
            .get_rich_message_snapshot(None, "@alex", 301)
            .await?
            .is_some_and(|snapshot| snapshot.block_count > 0)
    );

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn rich_markdown_keeps_its_kind_through_button_approval_and_dispatch() -> anyhow::Result<()> {
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

    let markdown = "# Approved Rich Text\n\n- **Exact**\n- _Structured_";
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_rich_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "markdown": markdown,
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
        .expect("draft id");
    let payload_sha256 = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("payload hash");
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
    assert_eq!(requested.is_error, Some(false));
    let approval_id = requested
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("approval id")
        .to_string();
    let approval_message_id = requested
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("approval message id");
    let cards = bot.sent_cards().await;
    let card = cards.first().expect("rich approval card");
    assert_eq!(card.card.approval.kind, PreparedSendKind::RichMarkdown);
    assert_eq!(card.card.approval.text, markdown);

    bot.push_decision(&approval_id, ApprovalAction::Approve, 700, 700, approval_message_id).await;
    wait_for_state(&state_db_path, &approval_id, SendApprovalState::Sent).await?;
    let approval =
        SendLedger::new(state_db_path).get_approval(&approval_id)?.expect("sent rich approval");
    assert_eq!(approval.kind, PreparedSendKind::RichMarkdown);
    let target_message_id = approval.target_message_id.expect("target message id");
    assert!(backend.get_rich_message_snapshot(None, "@alex", target_message_id).await?.is_some());
    assert_eq!(backend.sent_messages().await.len(), 1);

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}

#[tokio::test]
async fn operator_bot_rich_message_is_fixed_to_the_configured_private_chat() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let server = McpServer::with_approval(
        backend,
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
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
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let html = "<h1>Plus accounts</h1><table><tr><th>Date</th><th>Account</th></tr><tr><td>2026-08-09</td><td>user@example.com</td></tr></table>";

    let sent = client
        .call_tool(
            CallToolRequestParams::new("send_operator_bot_rich_message").with_arguments(
                json!({
                    "html": html,
                    "current_session_approval": "User explicitly requested this exact private bot delivery"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(sent.is_error, Some(false));
    assert_eq!(
        sent.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/chat_id"))
            .and_then(|value| value.as_i64()),
        Some(700)
    );
    assert_eq!(
        sent.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/representation"))
            .and_then(|value| value.as_str()),
        Some("telegram_rich_message")
    );
    let delivered = bot.sent_operator_rich_messages().await;
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].0, 700);
    assert_eq!(delivered[0].1, html);

    let weak_authority = client
        .call_tool(
            CallToolRequestParams::new("send_operator_bot_rich_message").with_arguments(
                json!({ "html": html, "current_session_approval": "yes" })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(weak_authority.is_error, Some(true));
    assert_eq!(bot.sent_operator_rich_messages().await.len(), 1);

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn a_callback_can_bind_a_card_whose_bot_api_receipt_was_lost() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    bot.lose_next_send_receipt().await;
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

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Receipt-loss recovery",
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
        .expect("draft id");
    let payload_sha256 = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("payload hash");
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
    assert_eq!(
        requested
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/state"))
            .and_then(|value| value.as_str()),
        Some("card_delivery_uncertain")
    );
    let approval_id = requested
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("approval id")
        .to_string();
    assert!(
        requested
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/approval_message_id"))
            .is_none_or(|value| value.is_null())
    );
    let delivered_message_id =
        bot.sent_cards().await.first().expect("delivered approval card").message_id;

    bot.push_decision(&approval_id, ApprovalAction::Approve, 700, 700, delivered_message_id).await;
    wait_for_state(&state_db_path, &approval_id, SendApprovalState::Sent).await?;
    assert_eq!(backend.sent_messages().await.len(), 1);
    assert_eq!(
        SendLedger::new(state_db_path)
            .get_approval(&approval_id)?
            .expect("approval")
            .approval_message_id,
        Some(delivered_message_id)
    );

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}

#[tokio::test]
async fn mutation_buttons_apply_once_and_cancel_without_side_effects() -> anyhow::Result<()> {
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

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_edit_message").with_arguments(
                json!({
                    "chat_id": "-202",
                    "message_id": 21,
                    "text": "Release passed and deployed"
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
    assert_eq!(requested.is_error, Some(false));
    let approval_id = requested
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("approval id")
        .to_string();
    let approval_message_id = requested
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("approval message id");
    let cards = bot.sent_mutation_cards().await;
    let card = cards.first().expect("mutation approval card");
    assert_eq!(card.card.approval.action, prepared_action(&prepared)?);

    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        701,
        700,
        approval_message_id,
    )
    .await;
    wait_for_answers(&bot, 1).await?;
    assert_eq!(
        MutationLedger::new(state_db_path.clone())
            .get_approval(&approval_id)?
            .expect("approval")
            .state,
        MutationApprovalState::Pending
    );
    assert_eq!(backend.mutation_call_counts().await, (0, 0));

    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::Applied).await?;
    let edited = backend.get_message(None, "-202", 21).await?.expect("edited message");
    assert_eq!(edited.text.as_deref(), Some("Release passed and deployed"));
    assert_eq!(backend.mutation_call_counts().await, (1, 0));

    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    wait_for_answers(&bot, 3).await?;
    assert_eq!(backend.mutation_call_counts().await, (1, 0));

    let prepared_delete = client
        .call_tool(
            CallToolRequestParams::new("prepare_delete_messages").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_ids": [10],
                    "revoke": false
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let delete_mutation_id = prepared_delete
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/mutation_id"))
        .and_then(|value| value.as_str())
        .expect("delete mutation id");
    let delete_hash = prepared_delete
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("delete payload hash");
    let requested_delete = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": delete_mutation_id,
                    "approved_payload_sha256": delete_hash
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let delete_approval_id = requested_delete
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("delete approval id")
        .to_string();
    let delete_card_id = requested_delete
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("delete approval message id");
    bot.push_mutation_decision(
        &delete_approval_id,
        ApprovalAction::Cancel,
        700,
        700,
        delete_card_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &delete_approval_id, MutationApprovalState::Cancelled)
        .await?;
    assert!(backend.get_message(None, "@alex", 10).await?.is_some());
    assert_eq!(backend.mutation_call_counts().await, (1, 0));

    let prepared_unpin = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_message_pinned").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 10,
                    "pinned": false,
                    "silent": true,
                    "one_sided": false
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let unpin_mutation_id = prepared_unpin
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/mutation_id"))
        .and_then(|value| value.as_str())
        .expect("unpin mutation id");
    let unpin_hash = prepared_unpin
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("unpin payload hash");
    let requested_unpin = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": unpin_mutation_id,
                    "approved_payload_sha256": unpin_hash
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let unpin_approval_id = requested_unpin
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("unpin approval id")
        .to_string();
    let unpin_card_id = requested_unpin
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("unpin card id");
    bot.push_mutation_decision(
        &unpin_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        unpin_card_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &unpin_approval_id, MutationApprovalState::Applied)
        .await?;
    assert_eq!(backend.pin_call_count().await, 1);
    assert!(!backend.get_message(None, "@alex", 10).await?.expect("unpinned message").pinned);
    let answers_before_unpin_duplicate = bot.answers().await.len();
    bot.push_mutation_decision(
        &unpin_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        unpin_card_id,
    )
    .await;
    wait_for_answers(&bot, answers_before_unpin_duplicate.saturating_add(1)).await?;
    assert_eq!(backend.pin_call_count().await, 1);

    let prepared_reaction = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_message_reaction").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 10,
                    "reaction": {
                        "kind": "emoji",
                        "emoji": "🔥",
                        "big": false,
                        "add_to_recent": true
                    }
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_reaction.is_error, Some(false));
    let reaction_mutation_id = prepared_reaction
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/mutation_id"))
        .and_then(|value| value.as_str())
        .expect("reaction mutation id");
    let reaction_payload_sha256 = prepared_reaction
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("reaction payload hash");
    let requested_reaction = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": reaction_mutation_id,
                    "approved_payload_sha256": reaction_payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let reaction_approval_id = requested_reaction
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("reaction approval id")
        .to_string();
    let reaction_card_id = requested_reaction
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("reaction approval message id");
    bot.push_mutation_decision(
        &reaction_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        reaction_card_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &reaction_approval_id, MutationApprovalState::Applied)
        .await?;
    assert_eq!(backend.reaction_call_count().await, 1);
    assert_eq!(
        backend.get_my_reaction(None, "@alex", 10).await?,
        Some(ReactionSpec::Emoji { emoji: "🔥".to_string(), big: false, add_to_recent: true })
    );
    let answers_before_reaction_duplicate = bot.answers().await.len();
    bot.push_mutation_decision(
        &reaction_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        reaction_card_id,
    )
    .await;
    wait_for_answers(&bot, answers_before_reaction_duplicate.saturating_add(1)).await?;
    assert_eq!(backend.reaction_call_count().await, 1);

    let prepared_reaction_removal = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_message_reaction").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 10,
                    "reaction": { "kind": "remove" }
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let removal_mutation_id = prepared_reaction_removal
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/mutation_id"))
        .and_then(|value| value.as_str())
        .expect("reaction removal mutation id");
    let removal_payload_sha256 = prepared_reaction_removal
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("reaction removal payload hash");
    let requested_removal = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": removal_mutation_id,
                    "approved_payload_sha256": removal_payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let removal_approval_id = requested_removal
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("reaction removal approval id")
        .to_string();
    let removal_card_id = requested_removal
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("reaction removal approval message id");
    bot.push_mutation_decision(
        &removal_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        removal_card_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &removal_approval_id, MutationApprovalState::Applied)
        .await?;
    assert_eq!(backend.reaction_call_count().await, 2);
    assert_eq!(backend.get_my_reaction(None, "@alex", 10).await?, None);

    let prepared_draft = client
        .call_tool(
            CallToolRequestParams::new("prepare_set_draft").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Draft approved through the private card"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_draft.is_error, Some(false));
    let draft_mutation_id = prepared_draft
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/mutation_id"))
        .and_then(|value| value.as_str())
        .expect("draft mutation id");
    let draft_payload_sha256 = prepared_draft
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("draft payload hash");
    let requested_draft = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": draft_mutation_id,
                    "approved_payload_sha256": draft_payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let draft_approval_id = requested_draft
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("draft approval id")
        .to_string();
    let draft_card_id = requested_draft
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("draft approval message id");
    bot.push_mutation_decision(
        &draft_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        draft_card_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &draft_approval_id, MutationApprovalState::Applied)
        .await?;
    assert_eq!(backend.draft_call_count().await, 1);
    assert_eq!(
        backend.get_draft(None, "@alex").await?.expect("saved draft").message,
        "Draft approved through the private card"
    );

    let prepared_draft_clear = client
        .call_tool(CallToolRequestParams::new("prepare_set_draft").with_arguments(
            json!({ "chat_id": "@alex", "text": null }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(prepared_draft_clear.is_error, Some(false));
    let clear_mutation_id = prepared_draft_clear
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/mutation_id"))
        .and_then(|value| value.as_str())
        .expect("draft clear mutation id");
    let clear_payload_sha256 = prepared_draft_clear
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("draft clear payload hash");
    let requested_draft_clear = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": clear_mutation_id,
                    "approved_payload_sha256": clear_payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let clear_approval_id = requested_draft_clear
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("draft clear approval id")
        .to_string();
    let clear_card_id = requested_draft_clear
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("draft clear approval message id");
    bot.push_mutation_decision(
        &clear_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        clear_card_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &clear_approval_id, MutationApprovalState::Applied)
        .await?;
    assert_eq!(backend.draft_call_count().await, 2);
    assert_eq!(backend.get_draft(None, "@alex").await?, None);

    macro_rules! apply_dialog_mutation {
        ($tool:literal, $arguments:expr) => {{
            let prepared = client
                .call_tool(
                    CallToolRequestParams::new($tool).with_arguments(
                        $arguments.as_object().expect("object").clone(),
                    ),
                )
                .await?;
            assert_eq!(prepared.is_error, Some(false), "{tool}", tool = $tool);
            let mutation_id = prepared
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/data/mutation_id"))
                .and_then(|value| value.as_str())
                .expect("dialog mutation id");
            let payload_sha256 = prepared
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/data/payload_sha256"))
                .and_then(|value| value.as_str())
                .expect("dialog payload hash");
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
                .expect("dialog approval id")
                .to_string();
            let card_id = requested
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/data/approval_message_id"))
                .and_then(|value| value.as_i64())
                .expect("dialog approval card id");
            bot.push_mutation_decision(&approval_id, ApprovalAction::Approve, 700, 700, card_id)
                .await;
            wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::Applied)
                .await?;
            (approval_id, card_id)
        }};
    }

    let (unread_approval_id, unread_card_id) = apply_dialog_mutation!(
        "prepare_set_dialog_unread",
        json!({ "chat_id": "@alex", "unread": false })
    );
    let (archive_approval_id, archive_card_id) = apply_dialog_mutation!(
        "prepare_set_dialog_archived",
        json!({ "chat_id": "@alex", "archived": true })
    );
    let (pin_dialog_approval_id, pin_dialog_card_id) = apply_dialog_mutation!(
        "prepare_set_dialog_pinned",
        json!({ "chat_id": "@alex", "pinned": true })
    );
    assert_eq!(backend.dialog_state_call_counts().await, (1, 1, 1));
    let answers_before_dialog_duplicate = bot.answers().await.len();
    bot.push_mutation_decision(
        &unread_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        unread_card_id,
    )
    .await;
    bot.push_mutation_decision(
        &archive_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        archive_card_id,
    )
    .await;
    bot.push_mutation_decision(
        &pin_dialog_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        pin_dialog_card_id,
    )
    .await;
    wait_for_answers(&bot, answers_before_dialog_duplicate.saturating_add(3)).await?;
    assert_eq!(backend.dialog_state_call_counts().await, (1, 1, 1));
    let dialog_state = backend.get_dialog_state(None, "@alex").await?;
    assert!(!dialog_state.unread_mark);
    assert!(dialog_state.archived);
    assert!(dialog_state.pinned);

    let (block_approval_id, block_card_id) = apply_dialog_mutation!(
        "prepare_set_user_blocked",
        json!({ "user_ref": "@alex", "blocked": true })
    );
    assert_eq!(backend.block_call_count().await, 1);
    assert!(backend.get_user_info(None, "@alex").await?.blocked);
    let (unblock_approval_id, unblock_card_id) = apply_dialog_mutation!(
        "prepare_set_user_blocked",
        json!({ "user_ref": "@alex", "blocked": false })
    );
    assert_eq!(backend.block_call_count().await, 2);
    assert!(!backend.get_user_info(None, "@alex").await?.blocked);
    let answers_before_block_duplicate = bot.answers().await.len();
    bot.push_mutation_decision(
        &block_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        block_card_id,
    )
    .await;
    bot.push_mutation_decision(
        &unblock_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        unblock_card_id,
    )
    .await;
    wait_for_answers(&bot, answers_before_block_duplicate.saturating_add(2)).await?;
    assert_eq!(backend.block_call_count().await, 2);

    let (add_contact_approval_id, add_contact_card_id) = apply_dialog_mutation!(
        "prepare_set_contact",
        json!({
            "user_ref": "@dana_candidate",
            "contact": true,
            "first_name": "Dana",
            "last_name": "Candidate",
            "phone": "+15550000404",
            "add_phone_privacy_exception": false
        })
    );
    assert_eq!(backend.contact_call_count().await, 1);
    assert!(backend.get_user_info(None, "@dana_candidate").await?.user.is_contact);
    let (delete_contact_approval_id, delete_contact_card_id) = apply_dialog_mutation!(
        "prepare_set_contact",
        json!({ "user_ref": "@dana_candidate", "contact": false })
    );
    assert_eq!(backend.contact_call_count().await, 2);
    assert!(!backend.get_user_info(None, "@dana_candidate").await?.user.is_contact);
    let answers_before_contact_duplicate = bot.answers().await.len();
    bot.push_mutation_decision(
        &add_contact_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        add_contact_card_id,
    )
    .await;
    bot.push_mutation_decision(
        &delete_contact_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        delete_contact_card_id,
    )
    .await;
    wait_for_answers(&bot, answers_before_contact_duplicate.saturating_add(2)).await?;
    assert_eq!(backend.contact_call_count().await, 2);

    let (privacy_approval_id, privacy_card_id) = apply_dialog_mutation!(
        "prepare_set_privacy_rules",
        json!({
            "key": "phone_number",
            "rules": [
                { "kind": "allow_users", "user_refs": ["@dana_candidate"] },
                { "kind": "disallow_all" }
            ]
        })
    );
    assert_eq!(backend.privacy_call_count().await, 1);
    let privacy_rules =
        backend.get_privacy_rules(None, telegram_personal_mcp::PrivacyKey::PhoneNumber).await?;
    assert_eq!(privacy_rules.len(), 2);
    assert_eq!(privacy_rules[0].kind, "allow_users");
    assert_eq!(privacy_rules[0].user_ids, vec![404]);
    assert_eq!(privacy_rules[1].kind, "disallow_all");
    let answers_before_privacy_duplicate = bot.answers().await.len();
    bot.push_mutation_decision(
        &privacy_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        privacy_card_id,
    )
    .await;
    wait_for_answers(&bot, answers_before_privacy_duplicate.saturating_add(1)).await?;
    assert_eq!(backend.privacy_call_count().await, 1);

    let (notifications_approval_id, notifications_card_id) = apply_dialog_mutation!(
        "prepare_set_notification_settings",
        json!({
            "chat_id": "@alex",
            "show_previews": false,
            "silent": true,
            "mute_until_unix": 2_000_000_000,
            "sound": { "kind": "default" },
            "stories_muted": true,
            "stories_hide_sender": true,
            "stories_sound": { "kind": "none" }
        })
    );
    assert_eq!(backend.notification_call_count().await, 1);
    let notification_settings = backend.get_notification_settings(None, "@alex").await?;
    assert_eq!(notification_settings.show_previews, Some(false));
    assert_eq!(notification_settings.silent, Some(true));
    assert_eq!(notification_settings.mute_until_unix, Some(2_000_000_000));
    assert_eq!(
        notification_settings.other_sound.as_ref().map(|sound| sound.kind.as_str()),
        Some("default")
    );
    assert_eq!(notification_settings.stories_muted, Some(true));
    assert_eq!(notification_settings.stories_hide_sender, Some(true));
    assert_eq!(
        notification_settings.stories_other_sound.as_ref().map(|sound| sound.kind.as_str()),
        Some("none")
    );
    let answers_before_notifications_duplicate = bot.answers().await.len();
    bot.push_mutation_decision(
        &notifications_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        notifications_card_id,
    )
    .await;
    wait_for_answers(&bot, answers_before_notifications_duplicate.saturating_add(1)).await?;
    assert_eq!(backend.notification_call_count().await, 1);

    let (scheduled_edit_approval_id, scheduled_edit_card_id) = apply_dialog_mutation!(
        "prepare_edit_scheduled_message",
        json!({
            "chat_id": "@alex",
            "message_id": 301,
            "text": "Scheduled fixture follow-up edited",
            "schedule_at_unix": 2_000_000_200
        })
    );
    let (schedule_approval_id, schedule_card_id) = apply_dialog_mutation!(
        "prepare_schedule_message",
        json!({
            "chat_id": "@alex",
            "text": "New scheduled approval fixture",
            "source": { "kind": "self_originated" },
            "reply_to_message_id": 11,
            "schedule_at_unix": 2_000_000_100,
            "silent": true,
            "noforwards": true
        })
    );
    let scheduled_after_create = backend.get_scheduled_messages(None, "@alex").await?;
    assert!(scheduled_after_create.iter().any(|message| {
        message.message_id == 301
            && message.text.as_deref() == Some("Scheduled fixture follow-up edited")
            && message.date == 2_000_000_200
    }));
    let created = scheduled_after_create
        .iter()
        .find(|message| message.message_id != 301)
        .expect("new scheduled message");
    assert_eq!(created.message_id, 100);
    assert!(created.silent);
    assert!(created.noforwards);
    assert_eq!(created.reply_to_message_id, Some(11));

    let (scheduled_send_now_approval_id, scheduled_send_now_card_id) = apply_dialog_mutation!(
        "prepare_send_scheduled_messages_now",
        json!({ "chat_id": "@alex", "message_ids": [100] })
    );
    let sent_now =
        backend.get_message(None, "@alex", 101).await?.expect("sent scheduled message readback");
    assert_eq!(sent_now.text.as_deref(), Some("New scheduled approval fixture"));
    assert!(sent_now.silent);
    assert!(sent_now.noforwards);
    assert_eq!(sent_now.reply_to_message_id, Some(11));

    let (scheduled_delete_approval_id, scheduled_delete_card_id) = apply_dialog_mutation!(
        "prepare_delete_scheduled_messages",
        json!({ "chat_id": "@alex", "message_ids": [301] })
    );
    assert!(backend.get_scheduled_messages(None, "@alex").await?.is_empty());
    assert_eq!(backend.scheduled_call_counts().await, (1, 1, 1, 1));

    let answers_before_scheduled_duplicates = bot.answers().await.len();
    for (approval_id, card_id) in [
        (&scheduled_edit_approval_id, scheduled_edit_card_id),
        (&schedule_approval_id, schedule_card_id),
        (&scheduled_send_now_approval_id, scheduled_send_now_card_id),
        (&scheduled_delete_approval_id, scheduled_delete_card_id),
    ] {
        bot.push_mutation_decision(approval_id, ApprovalAction::Approve, 700, 700, card_id).await;
    }
    wait_for_answers(&bot, answers_before_scheduled_duplicates.saturating_add(4)).await?;
    assert_eq!(backend.scheduled_call_counts().await, (1, 1, 1, 1));

    let (poll_vote_approval_id, poll_vote_card_id) = apply_dialog_mutation!(
        "prepare_set_poll_vote",
        json!({ "chat_id": "-202", "message_id": 20, "option_indices": [2] })
    );
    assert_eq!(backend.poll_call_counts().await, (0, 1, 0));
    let voted_poll = backend.get_poll(None, "-202", 20).await?.expect("poll vote readback");
    assert_eq!(voted_poll.selected_option_indices(), vec![2]);
    assert_eq!(voted_poll.total_voters, Some(3));

    let (poll_stop_approval_id, poll_stop_card_id) =
        apply_dialog_mutation!("prepare_stop_poll", json!({ "chat_id": "-202", "message_id": 20 }));
    assert_eq!(backend.poll_call_counts().await, (0, 1, 1));
    assert!(backend.get_poll(None, "-202", 20).await?.expect("stopped poll readback").closed);

    let (poll_create_approval_id, poll_create_card_id) = apply_dialog_mutation!(
        "prepare_create_poll",
        json!({
            "chat_id": "-202",
            "question": "Which invariant must the next release preserve?",
            "answers": ["Continuity", "Convenience", "Novelty"],
            "public_voters": true,
            "quiz": true,
            "correct_option_index": 0,
            "shuffle_answers": true,
            "countries_iso2": ["us", "de"],
            "close_period_seconds": 60,
            "solution": "Continuity keeps the exact live consumer path usable.",
            "reply_to_message_id": 21,
            "silent": true,
            "noforwards": true
        })
    );
    assert_eq!(backend.poll_call_counts().await, (1, 1, 1));
    let sent_poll = backend.sent_messages().await.pop().expect("poll send receipt");
    let created_poll =
        backend.get_poll(None, "-202", sent_poll.message_id).await?.expect("created poll readback");
    assert!(created_poll.quiz);
    assert_eq!(created_poll.question, "Which invariant must the next release preserve?");
    assert!(created_poll.answers[0].correct);
    assert_eq!(created_poll.countries_iso2, vec!["US", "DE"]);
    let created_poll_message = backend
        .get_message(None, "-202", sent_poll.message_id)
        .await?
        .expect("created poll message readback");
    assert_eq!(created_poll_message.reply_to_message_id, Some(21));
    assert!(created_poll_message.silent);
    assert!(created_poll_message.noforwards);

    let answers_before_poll_duplicates = bot.answers().await.len();
    for (approval_id, card_id) in [
        (&poll_vote_approval_id, poll_vote_card_id),
        (&poll_stop_approval_id, poll_stop_card_id),
        (&poll_create_approval_id, poll_create_card_id),
    ] {
        bot.push_mutation_decision(approval_id, ApprovalAction::Approve, 700, 700, card_id).await;
    }
    wait_for_answers(&bot, answers_before_poll_duplicates.saturating_add(3)).await?;
    assert_eq!(backend.poll_call_counts().await, (1, 1, 1));

    let (topic_edit_approval_id, topic_edit_card_id) = apply_dialog_mutation!(
        "prepare_edit_forum_topic",
        json!({
            "chat_id": "-202",
            "topic_id": 44,
            "title": "Release execution",
            "icon_emoji_id": 123_456,
            "closed": true,
            "hidden": true
        })
    );
    assert_eq!(backend.forum_topic_call_counts().await, (0, 1, 0, 0));
    let edited_topic =
        backend.get_forum_topic(None, "-202", 44).await?.expect("edited forum topic readback");
    assert_eq!(edited_topic.title.as_deref(), Some("Release execution"));
    assert_eq!(edited_topic.icon_emoji_id, Some(123_456));
    assert!(edited_topic.closed);
    assert!(edited_topic.hidden);

    let (topic_pin_approval_id, topic_pin_card_id) = apply_dialog_mutation!(
        "prepare_set_forum_topic_pinned",
        json!({ "chat_id": "-202", "topic_id": 44, "pinned": false })
    );
    assert_eq!(backend.forum_topic_call_counts().await, (0, 1, 1, 0));
    assert!(
        !backend
            .get_forum_topic(None, "-202", 44)
            .await?
            .expect("unpinned forum topic readback")
            .pinned
    );

    let (topic_create_approval_id, topic_create_card_id) = apply_dialog_mutation!(
        "prepare_create_forum_topic",
        json!({
            "chat_id": "-202",
            "title": "Post-release review",
            "icon_color": 7_322_320
        })
    );
    assert_eq!(backend.forum_topic_call_counts().await, (1, 1, 1, 0));
    let topic_create_approval = client
        .call_tool(CallToolRequestParams::new("get_mutation_approval").with_arguments(
            json!({ "approval_id": topic_create_approval_id }).as_object().expect("object").clone(),
        ))
        .await?;
    let created_topic_id = topic_create_approval
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/result/topic_id"))
        .and_then(|value| value.as_i64())
        .and_then(|value| i32::try_from(value).ok())
        .expect("created forum topic id");
    let created_topic = backend
        .get_forum_topic(None, "-202", created_topic_id)
        .await?
        .expect("created forum topic readback");
    assert_eq!(created_topic.title.as_deref(), Some("Post-release review"));
    assert_eq!(created_topic.icon_color, Some(7_322_320));

    let (topic_delete_approval_id, topic_delete_card_id) = apply_dialog_mutation!(
        "prepare_delete_forum_topic",
        json!({ "chat_id": "-202", "topic_id": created_topic_id })
    );
    assert_eq!(backend.forum_topic_call_counts().await, (1, 1, 1, 1));
    assert!(
        backend
            .get_forum_topic(None, "-202", created_topic_id)
            .await?
            .expect("deleted forum topic marker")
            .deleted
    );

    let answers_before_topic_duplicates = bot.answers().await.len();
    for (approval_id, card_id) in [
        (&topic_edit_approval_id, topic_edit_card_id),
        (&topic_pin_approval_id, topic_pin_card_id),
        (&topic_create_approval_id, topic_create_card_id),
        (&topic_delete_approval_id, topic_delete_card_id),
    ] {
        bot.push_mutation_decision(approval_id, ApprovalAction::Approve, 700, 700, card_id).await;
    }
    wait_for_answers(&bot, answers_before_topic_duplicates.saturating_add(4)).await?;
    assert_eq!(backend.forum_topic_call_counts().await, (1, 1, 1, 1));

    let (session_approval_id, session_card_id) =
        apply_dialog_mutation!("prepare_terminate_session", json!({ "authorization_id": 2 }));
    assert_eq!(backend.session_terminate_call_count().await, 1);
    assert!(backend.get_sessions(None).await?.iter().all(|session| session.authorization_id != 2));
    let answers_before_session_duplicate = bot.answers().await.len();
    bot.push_mutation_decision(
        &session_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        session_card_id,
    )
    .await;
    wait_for_answers(&bot, answers_before_session_duplicate.saturating_add(1)).await?;
    assert_eq!(backend.session_terminate_call_count().await, 1);

    let prepared_forward = client
        .call_tool(
            CallToolRequestParams::new("prepare_forward_messages").with_arguments(
                json!({
                    "source_chat_id": "@alex",
                    "destination_chat_id": "-202",
                    "message_ids": [11],
                    "silent": true,
                    "drop_author": false,
                    "drop_media_captions": false,
                    "noforwards": false
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_forward.is_error, Some(false));
    let forward_mutation_id = prepared_forward
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/mutation_id"))
        .and_then(|value| value.as_str())
        .expect("forward mutation id");
    let forward_payload_sha256 = prepared_forward
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("forward payload hash");
    let requested_forward = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": forward_mutation_id,
                    "approved_payload_sha256": forward_payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let forward_approval_id = requested_forward
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("forward approval id")
        .to_string();
    let forward_card_id = requested_forward
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("forward approval message id");
    bot.push_mutation_decision(
        &forward_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        forward_card_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &forward_approval_id, MutationApprovalState::Applied)
        .await?;
    assert_eq!(backend.forward_call_count().await, 1);
    let forward_result = MutationLedger::new(state_db_path.clone())
        .get_approval(&forward_approval_id)?
        .expect("forward approval")
        .result
        .expect("forward result");
    let destination_message_id = match forward_result {
        MutationResult::MessagesForwarded { destination_message_ids, .. } => {
            assert_eq!(destination_message_ids.len(), 1);
            destination_message_ids[0]
        },
        other => panic!("unexpected forward result: {other:?}"),
    };
    let forwarded = backend
        .get_message(None, "-202", destination_message_id)
        .await?
        .expect("forwarded message");
    assert!(forwarded.outgoing);
    assert_eq!(forwarded.text.as_deref(), Some("Please check contract.pdf"));
    assert_eq!(forwarded.media_type.as_deref(), Some("document"));
    assert!(forwarded.forward_origin.is_some());
    let answers_before_forward_duplicate = bot.answers().await.len();
    bot.push_mutation_decision(
        &forward_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        forward_card_id,
    )
    .await;
    wait_for_answers(&bot, answers_before_forward_duplicate.saturating_add(1)).await?;
    assert_eq!(backend.forward_call_count().await, 1);

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
    let approved_file_path = std::path::Path::new(downloaded_path)
        .parent()
        .expect("download directory")
        .join("approved-evidence.txt");
    let approved_bytes = b"exact file bytes approved through the private Telegram card\n";
    std::fs::write(&approved_file_path, approved_bytes)?;
    let approved_file_sha256 = format!("{:x}", Sha256::digest(approved_bytes));
    let prepared_file = client
        .call_tool(
            CallToolRequestParams::new("prepare_file_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": approved_file_path,
                    "caption": "Exact approved evidence",
                    "reply_to_message_id": 11,
                    "mode": "document",
                    "approved_sha256": approved_file_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_file.is_error, Some(false));
    let file_mutation_id = prepared_file
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/mutation_id"))
        .and_then(|value| value.as_str())
        .expect("file mutation id");
    let file_payload_sha256 = prepared_file
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("file payload hash");
    let artifact_id = prepared_file
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/action/artifact/artifact_id"))
        .and_then(|value| value.as_str())
        .expect("artifact id");
    let artifact_directory = state_db_path.with_extension("outbox").join(artifact_id);
    assert!(artifact_directory.is_dir());
    let requested_file = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": file_mutation_id,
                    "approved_payload_sha256": file_payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let file_approval_id = requested_file
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("file approval id")
        .to_string();
    let file_card_id = requested_file
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("file approval message id");
    bot.push_mutation_decision(&file_approval_id, ApprovalAction::Approve, 700, 700, file_card_id)
        .await;
    wait_for_mutation_state(&state_db_path, &file_approval_id, MutationApprovalState::Applied)
        .await?;
    wait_for_path_missing(&artifact_directory).await?;
    let sent_files = backend.sent_messages().await;
    assert_eq!(sent_files.iter().filter(|message| message.file_name.is_some()).count(), 1);
    let sent_file =
        sent_files.iter().find(|message| message.file_name.is_some()).expect("approved file send");
    assert_eq!(sent_file.text, "Exact approved evidence");
    assert_eq!(sent_file.reply_to_message_id, Some(11));
    assert_eq!(sent_file.file_mode, Some(telegram_personal_mcp::FileSendMode::Document));
    let final_file_approval = MutationLedger::new(state_db_path.clone())
        .get_approval(&file_approval_id)?
        .expect("file approval");
    assert_eq!(
        final_file_approval.result,
        Some(telegram_personal_mcp::MutationResult::FileSent {
            message_id: sent_file.message_id,
            sha256: approved_file_sha256.clone(),
        })
    );

    let answers_before_duplicate = bot.answers().await.len();
    let sent_count_before_duplicate = backend.sent_messages().await.len();
    bot.push_mutation_decision(&file_approval_id, ApprovalAction::Approve, 700, 700, file_card_id)
        .await;
    wait_for_answers(&bot, answers_before_duplicate.saturating_add(1)).await?;
    assert_eq!(backend.sent_messages().await.len(), sent_count_before_duplicate);

    let prepared_scheduled_file = client
        .call_tool(
            CallToolRequestParams::new("prepare_schedule_file_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": approved_file_path,
                    "caption": "Exact scheduled evidence",
                    "parse_mode": "plain",
                    "source": { "kind": "self_originated" },
                    "reply_to_message_id": 11,
                    "schedule_at_unix": 2_000_000_300,
                    "silent": true,
                    "noforwards": true,
                    "mode": "document",
                    "approved_sha256": approved_file_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_scheduled_file.is_error, Some(false));
    let scheduled_file_mutation_id = prepared_scheduled_file
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/mutation_id"))
        .and_then(|value| value.as_str())
        .expect("scheduled file mutation id");
    let scheduled_file_payload_sha256 = prepared_scheduled_file
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("scheduled file payload hash");
    let scheduled_file_artifact_id = prepared_scheduled_file
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/action/artifact/artifact_id"))
        .and_then(|value| value.as_str())
        .expect("scheduled file artifact id");
    let scheduled_file_artifact_directory =
        state_db_path.with_extension("outbox").join(scheduled_file_artifact_id);
    let scheduled_file_approval = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": scheduled_file_mutation_id,
                    "approved_payload_sha256": scheduled_file_payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    let scheduled_file_approval_id = scheduled_file_approval
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("scheduled file approval id")
        .to_string();
    let scheduled_file_card_id = scheduled_file_approval
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("scheduled file approval message id");
    bot.push_mutation_decision(
        &scheduled_file_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        scheduled_file_card_id,
    )
    .await;
    wait_for_mutation_state(
        &state_db_path,
        &scheduled_file_approval_id,
        MutationApprovalState::Applied,
    )
    .await?;
    wait_for_path_missing(&scheduled_file_artifact_directory).await?;
    let scheduled_messages = backend.get_scheduled_messages(None, "@alex").await?;
    let scheduled_file = scheduled_messages
        .iter()
        .find(|message| message.text.as_deref() == Some("Exact scheduled evidence"))
        .expect("scheduled file readback");
    assert_eq!(scheduled_file.date, 2_000_000_300);
    assert!(scheduled_file.silent);
    assert!(scheduled_file.noforwards);
    assert_eq!(scheduled_file.reply_to_message_id, Some(11));
    assert_eq!(scheduled_file.media_type.as_deref(), Some("document"));
    let scheduled_attachments =
        backend.list_attachments(None, "@alex", scheduled_file.message_id).await?;
    assert_eq!(scheduled_attachments.len(), 1);
    assert_eq!(scheduled_attachments[0].kind, "document");
    let final_scheduled_file_approval = MutationLedger::new(state_db_path.clone())
        .get_approval(&scheduled_file_approval_id)?
        .expect("scheduled file approval");
    assert_eq!(
        final_scheduled_file_approval.result,
        Some(MutationResult::FileScheduled {
            message_id: scheduled_file.message_id,
            schedule_date_unix: 2_000_000_300,
            sha256: approved_file_sha256.clone(),
        })
    );

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}

#[tokio::test]
async fn changed_message_snapshot_blocks_an_approved_edit() -> anyhow::Result<()> {
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

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_edit_message").with_arguments(
                json!({
                    "chat_id": "-202",
                    "message_id": 21,
                    "text": "Approved replacement"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert!(
        backend
            .set_fixture_message_text(
                202,
                21,
                Some("Concurrent external edit".to_string()),
                Some(1_700_002_021),
            )
            .await
    );
    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": prepared
                        .structured_content
                        .as_ref()
                        .and_then(|value| value.pointer("/data/mutation_id"))
                        .and_then(|value| value.as_str())
                        .expect("mutation id"),
                    "approved_payload_sha256": prepared
                        .structured_content
                        .as_ref()
                        .and_then(|value| value.pointer("/data/payload_sha256"))
                        .and_then(|value| value.as_str())
                        .expect("payload hash")
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
    let message_id = requested
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_message_id"))
        .and_then(|value| value.as_i64())
        .expect("approval message id");
    bot.push_mutation_decision(&approval_id, ApprovalAction::Approve, 700, 700, message_id).await;
    wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::Failed).await?;
    let approval =
        MutationLedger::new(state_db_path).get_approval(&approval_id)?.expect("approval");
    assert_eq!(approval.last_error_code.as_deref(), Some("MUTATION_PRECONDITION_FAILED"));
    assert_eq!(backend.mutation_call_counts().await, (0, 0));
    assert_eq!(
        backend.get_message(None, "-202", 21).await?.expect("current message").text.as_deref(),
        Some("Concurrent external edit")
    );

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}

fn prepared_action(
    result: &rmcp::model::CallToolResult,
) -> anyhow::Result<telegram_personal_mcp::MutationAction> {
    let action = result
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/action"))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("prepared mutation action missing"))?;
    Ok(serde_json::from_value(action)?)
}

async fn wait_for_state(
    state_db_path: &std::path::Path,
    approval_id: &str,
    wanted: SendApprovalState,
) -> anyhow::Result<()> {
    for _ in 0..200 {
        let state = SendLedger::new(state_db_path.to_path_buf())
            .get_approval(approval_id)?
            .map(|approval| approval.state);
        if state == Some(wanted) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("approval did not reach {wanted:?}")
}

async fn wait_for_mutation_state(
    state_db_path: &std::path::Path,
    approval_id: &str,
    wanted: MutationApprovalState,
) -> anyhow::Result<()> {
    for _ in 0..200 {
        let state = MutationLedger::new(state_db_path.to_path_buf())
            .get_approval(approval_id)?
            .map(|approval| approval.state);
        if state == Some(wanted) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("mutation approval did not reach {wanted:?}")
}

async fn wait_for_answers(bot: &FakeApprovalBot, wanted: usize) -> anyhow::Result<()> {
    for _ in 0..200 {
        if bot.answers().await.len() >= wanted {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("approval bot did not record {wanted} callback answers")
}

async fn wait_for_path_missing(path: &std::path::Path) -> anyhow::Result<()> {
    for _ in 0..200 {
        if !path.exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("path was not cleaned after terminal delivery: {}", path.display())
}
