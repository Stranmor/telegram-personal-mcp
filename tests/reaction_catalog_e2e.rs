use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger,
};

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
    anyhow::bail!("default reaction mutation did not become applied; last={last:?}")
}

async fn prepare_and_approve(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    bot: &FakeApprovalBot,
    state_db_path: &std::path::Path,
    reaction: Value,
) -> anyhow::Result<(String, i64, Value)> {
    let prepared =
        client
            .call_tool(CallToolRequestParams::new("set_default_reaction").with_arguments(
                json!({ "reaction": reaction }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared_data = prepared.structured_content.expect("prepared default reaction");
    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: подготовка мутации к подтверждению",
                    "mutation_id": prepared_data
                        .pointer("/data/mutation_id")
                        .and_then(Value::as_str)
                        .expect("mutation id"),
                    "approved_payload_sha256": prepared_data
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
    let requested_data = requested.structured_content.expect("approval request");
    let approval_id = requested_data
        .pointer("/data/approval_id")
        .and_then(Value::as_str)
        .expect("approval id")
        .to_string();
    let message_id = requested_data
        .pointer("/data/approval_message_id")
        .and_then(Value::as_i64)
        .expect("approval message id");
    bot.push_mutation_decision(&approval_id, ApprovalAction::Approve, 700, 700, message_id).await;
    wait_for_applied(state_db_path, &approval_id).await?;
    Ok((approval_id, message_id, prepared_data))
}

#[tokio::test]
async fn reaction_catalogs_are_live_typed_bounded_and_hash_aware() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let temp = tempfile::tempdir()?;
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

    let default = client.call_tool(CallToolRequestParams::new("get_default_reaction")).await?;
    assert_eq!(default.is_error, Some(false));
    assert_eq!(
        default
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/reaction/kind"))
            .and_then(Value::as_str),
        Some("emoji")
    );
    assert_eq!(
        default
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/reaction/emoji"))
            .and_then(Value::as_str),
        Some("👍")
    );

    let top = client
        .call_tool(
            CallToolRequestParams::new("get_top_reactions")
                .with_arguments(json!({ "limit": 3 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(top.is_error, Some(false));
    assert_eq!(
        top.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/hash"))
            .and_then(Value::as_i64),
        Some(11_001)
    );
    assert_eq!(
        top.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/reactions/2/kind"))
            .and_then(Value::as_str),
        Some("paid")
    );

    let top_not_modified = client
        .call_tool(CallToolRequestParams::new("get_top_reactions").with_arguments(
            json!({ "limit": 3, "hash": 11_001 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(top_not_modified.is_error, Some(false));
    assert_eq!(
        top_not_modified
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/not_modified"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        top_not_modified
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/reactions"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    let recent = client
        .call_tool(
            CallToolRequestParams::new("get_recent_reactions")
                .with_arguments(json!({ "limit": 2 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(recent.is_error, Some(false));
    assert_eq!(
        recent
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/reactions/0/document_id"))
            .and_then(Value::as_i64),
        Some(8_001)
    );
    assert_eq!(
        recent
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/reactions/1/emoji"))
            .and_then(Value::as_str),
        Some("🔥")
    );

    for arguments in [json!({ "limit": 0 }), json!({ "limit": 101 })] {
        let invalid = client
            .call_tool(
                CallToolRequestParams::new("get_recent_reactions")
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(invalid.is_error, Some(true));
    }
    let unknown_field = client
        .call_tool(
            CallToolRequestParams::new("get_recent_reactions")
                .with_arguments(json!({ "extra": 1 }).as_object().expect("object").clone()),
        )
        .await;
    assert!(unknown_field.is_err());

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn default_reaction_mutation_is_approved_read_back_and_exactly_once() -> anyhow::Result<()> {
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

    for invalid_reaction in [
        json!({ "kind": "emoji", "emoji": "" }),
        json!({ "kind": "custom_emoji", "document_id": 0 }),
    ] {
        let invalid = client
            .call_tool(CallToolRequestParams::new("set_default_reaction").with_arguments(
                json!({ "reaction": invalid_reaction }).as_object().expect("object").clone(),
            ))
            .await?;
        assert_eq!(invalid.is_error, Some(true));
    }
    let unsupported_kind = client
        .call_tool(CallToolRequestParams::new("set_default_reaction").with_arguments(
            json!({ "reaction": { "kind": "paid" } }).as_object().expect("object").clone(),
        ))
        .await;
    assert!(unsupported_kind.is_err());

    let (approval_id, message_id, prepared) = prepare_and_approve(
        &client,
        &bot,
        &state_db_path,
        json!({ "kind": "custom_emoji", "document_id": 9_001 }),
    )
    .await?;
    assert_eq!(prepared.pointer("/data/action/before/emoji").and_then(Value::as_str), Some("👍"));
    assert_eq!(backend.default_reaction_call_count().await, 1);

    bot.push_mutation_decision(&approval_id, ApprovalAction::Approve, 700, 700, message_id).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(backend.default_reaction_call_count().await, 1);

    let readback = client.call_tool(CallToolRequestParams::new("get_default_reaction")).await?;
    assert_eq!(readback.is_error, Some(false));
    assert_eq!(
        readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/reaction/document_id"))
            .and_then(Value::as_i64),
        Some(9_001)
    );

    backend.make_next_default_reaction_outcome_uncertain().await;
    let (_, _, _) = prepare_and_approve(
        &client,
        &bot,
        &state_db_path,
        json!({ "kind": "emoji", "emoji": "🔥" }),
    )
    .await?;
    assert_eq!(backend.default_reaction_call_count().await, 2);
    let recovered = client.call_tool(CallToolRequestParams::new("get_default_reaction")).await?;
    assert_eq!(
        recovered
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/reaction/emoji"))
            .and_then(Value::as_str),
        Some("🔥")
    );

    let cards = format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(!cards.contains("access_hash"));
    assert!(!cards.contains("file_reference"));

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}
