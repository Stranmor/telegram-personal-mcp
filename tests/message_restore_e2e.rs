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
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger, TelegramBackend,
};

async fn wait_for_applied(
    state_db_path: &std::path::Path,
    approval_id: &str,
) -> anyhow::Result<()> {
    let ledger = MutationLedger::new(state_db_path.to_path_buf());
    for _ in 0..500 {
        if ledger
            .get_approval(approval_id)?
            .as_ref()
            .is_some_and(|approval| approval.state == MutationApprovalState::Applied)
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("mutation did not become applied")
}

#[tokio::test]
async fn deleted_media_restore_preserves_content_and_requires_sender_change_opt_in()
-> anyhow::Result<()> {
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

    let blocked_irrecoverable_delete = client
        .call_tool(
            CallToolRequestParams::new("prepare_delete_messages").with_arguments(
                json!({
                    "chat_id": "-202",
                    "message_ids": [20],
                    "revoke": true
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(blocked_irrecoverable_delete.is_error, Some(true));
    assert_eq!(
        blocked_irrecoverable_delete
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(Value::as_str),
        Some("MESSAGE_RESTORE_UNSUPPORTED")
    );
    let explicitly_irrecoverable_delete = client
        .call_tool(
            CallToolRequestParams::new("prepare_delete_messages").with_arguments(
                json!({
                    "chat_id": "-202",
                    "message_ids": [20],
                    "revoke": true,
                    "allow_irrecoverable": true
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(explicitly_irrecoverable_delete.is_error, Some(false));
    assert!(
        explicitly_irrecoverable_delete
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/action/messages/0/restore"))
            .is_none_or(Value::is_null)
    );

    let prepared_delete = client
        .call_tool(
            CallToolRequestParams::new("prepare_delete_messages").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_ids": [11],
                    "revoke": false
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_delete.is_error, Some(false));
    let prepared_delete = prepared_delete.structured_content.expect("prepared deletion");
    assert_eq!(
        prepared_delete
            .pointer("/data/action/messages/0/restore/media/kind")
            .and_then(Value::as_str),
        Some("file")
    );
    let requested_delete = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "mutation_id": prepared_delete
                        .pointer("/data/mutation_id")
                        .and_then(Value::as_str)
                        .unwrap(),
                    "approved_payload_sha256": prepared_delete
                        .pointer("/data/payload_sha256")
                        .and_then(Value::as_str)
                        .unwrap()
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    let requested_delete = requested_delete.structured_content.expect("delete approval");
    let deletion_approval_id =
        requested_delete.pointer("/data/approval_id").and_then(Value::as_str).unwrap().to_string();
    bot.push_mutation_decision(
        &deletion_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        requested_delete.pointer("/data/approval_message_id").and_then(Value::as_i64).unwrap(),
    )
    .await;
    wait_for_applied(&state_db_path, &deletion_approval_id).await?;
    assert!(backend.get_message(None, "@alex", 11).await?.is_none());

    let blocked = client
        .call_tool(
            CallToolRequestParams::new("prepare_restore_deleted_messages").with_arguments(
                json!({
                    "deletion_approval_id": deletion_approval_id,
                    "message_ids": [11]
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(blocked.is_error, Some(true));
    assert_eq!(
        blocked
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(Value::as_str),
        Some("RESTORE_SENDER_CHANGE_REQUIRES_OPT_IN")
    );

    let prepared_restore = client
        .call_tool(
            CallToolRequestParams::new("prepare_restore_deleted_messages").with_arguments(
                json!({
                    "deletion_approval_id": deletion_approval_id,
                    "message_ids": [11],
                    "allow_sender_change": true
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared_restore.is_error, Some(false));
    let prepared_restore = prepared_restore.structured_content.expect("prepared restoration");
    let requested_restore = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "mutation_id": prepared_restore
                        .pointer("/data/mutation_id")
                        .and_then(Value::as_str)
                        .unwrap(),
                    "approved_payload_sha256": prepared_restore
                        .pointer("/data/payload_sha256")
                        .and_then(Value::as_str)
                        .unwrap()
                })
                .as_object()
                .unwrap()
                .clone(),
            ),
        )
        .await?;
    let requested_restore = requested_restore.structured_content.expect("restore approval");
    let restore_approval_id =
        requested_restore.pointer("/data/approval_id").and_then(Value::as_str).unwrap().to_string();
    let approval_cards =
        format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(approval_cards.contains("ВОССТАНОВЛЕНИЕ УДАЛЁННЫХ СООБЩЕНИЙ"));
    assert!(approval_cards.contains("Будут созданы новые Telegram message_id"));
    assert!(approval_cards.contains("Изменение отправителя разрешено: да"));
    bot.push_mutation_decision(
        &restore_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        requested_restore.pointer("/data/approval_message_id").and_then(Value::as_i64).unwrap(),
    )
    .await;
    wait_for_applied(&state_db_path, &restore_approval_id).await?;
    let approval = MutationLedger::new(state_db_path).get_approval(&restore_approval_id)?.unwrap();
    let result = serde_json::to_value(approval.result.unwrap())?;
    let restored_message_id = result
        .pointer("/mappings/0/restored_message_id")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .expect("restored message id");
    let restored =
        backend.get_message(None, "@alex", restored_message_id).await?.expect("restored document");
    assert!(restored.outgoing);
    assert_eq!(restored.text.as_deref(), Some("Please check contract.pdf"));
    assert_eq!(restored.media_type.as_deref(), Some("document"));
    assert_eq!(restored.reply_to_message_id, Some(10));

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}
