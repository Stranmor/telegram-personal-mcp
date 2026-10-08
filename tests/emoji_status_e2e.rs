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
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, EmojiStatusCatalogKind, EmojiStatusKind, EmojiStatusSummary,
    FakeApprovalBot, FakeTelegramBackend, McpServer, McpServerSettings, MutationApprovalState,
    MutationLedger, TelegramBackend,
};

async fn wait_for_mutation_state(
    state_db_path: &std::path::Path,
    approval_id: &str,
    wanted: MutationApprovalState,
) -> anyhow::Result<()> {
    let mut last = None;
    for _ in 0..1_000 {
        let approval =
            MutationLedger::new(state_db_path.to_path_buf()).get_approval(approval_id)?;
        let state = approval.as_ref().map(|approval| approval.state);
        if state == Some(wanted) {
            return Ok(());
        }
        last = approval.map(|approval| {
            (
                approval.state,
                approval.last_error_code,
                approval.automated_retry_count,
                approval.next_retry_at_unix,
            )
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("mutation approval did not reach {wanted:?}; last={last:?}")
}

async fn rejected_code(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: &'static str,
    arguments: Value,
) -> anyhow::Result<String> {
    let rejected = client
        .call_tool(
            CallToolRequestParams::new(tool)
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(rejected.is_error, Some(true));
    Ok(rejected
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/error/code"))
        .and_then(Value::as_str)
        .expect("typed error code")
        .to_string())
}

async fn prepare_and_request(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: &'static str,
    arguments: Value,
) -> anyhow::Result<(String, i64, Value, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new(tool)
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared emoji-status mutation");
    let mutation_id = prepared
        .pointer("/data/mutation_id")
        .and_then(Value::as_str)
        .expect("mutation id")
        .to_string();
    let payload_sha256 = prepared
        .pointer("/data/payload_sha256")
        .and_then(Value::as_str)
        .expect("payload hash")
        .to_string();
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
    let approval = requested.structured_content.expect("emoji-status approval");
    let approval_id = approval
        .pointer("/data/approval_id")
        .and_then(Value::as_str)
        .expect("approval id")
        .to_string();
    let approval_message_id = approval
        .pointer("/data/approval_message_id")
        .and_then(Value::as_i64)
        .expect("approval message id");
    Ok((approval_id, approval_message_id, prepared, approval))
}

fn default_status(document_id: i64) -> EmojiStatusSummary {
    EmojiStatusSummary {
        kind: EmojiStatusKind::Default,
        document_id: Some(document_id.to_string()),
        collectible_id: None,
        until_unix: None,
        title: None,
        slug: None,
        pattern_document_id: None,
        center_color: None,
        edge_color: None,
        pattern_color: None,
        text_color: None,
    }
}

#[tokio::test]
async fn emoji_status_catalogues_and_mutations_are_exact_hash_bound_and_recoverable()
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

    let closed_schema = client
        .call_tool(CallToolRequestParams::new("list_emoji_statuses").with_arguments(
            json!({ "kind": "default", "includeRawTl": true }).as_object().expect("object").clone(),
        ))
        .await;
    assert!(closed_schema.is_err(), "unknown fields must fail at the JSON-RPC boundary");

    let current = client
        .call_tool(
            CallToolRequestParams::new("get_emoji_status")
                .with_arguments(json!({}).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(current.is_error, Some(false));
    let current = current.structured_content.expect("current emoji status");
    assert_eq!(current.pointer("/data/kind").and_then(Value::as_str), Some("empty"));
    assert!(current.pointer("/data/document_id").is_some_and(Value::is_null));

    for (kind, expected_count, expected_hash) in [
        ("default", 2_u64, "10001"),
        ("recent", 2_u64, "10002"),
        ("channel_default", 1_u64, "10003"),
        ("collectible", 1_u64, "10004"),
    ] {
        let page = client
            .call_tool(
                CallToolRequestParams::new("list_emoji_statuses")
                    .with_arguments(json!({ "kind": kind }).as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(page.is_error, Some(false));
        let page = page.structured_content.expect("emoji status page");
        assert_eq!(page.pointer("/data/kind").and_then(Value::as_str), Some(kind));
        assert_eq!(
            page.pointer("/data/available_count").and_then(Value::as_u64),
            Some(expected_count)
        );
        assert_eq!(
            page.pointer("/data/response_hash").and_then(Value::as_str),
            Some(expected_hash)
        );
        assert_eq!(page.pointer("/data/not_modified").and_then(Value::as_bool), Some(false));
    }

    let collectible =
        client
            .call_tool(CallToolRequestParams::new("list_emoji_statuses").with_arguments(
                json!({ "kind": "collectible" }).as_object().expect("object").clone(),
            ))
            .await?
            .structured_content
            .expect("collectible catalogue");
    assert_eq!(
        collectible.pointer("/data/statuses/0/collectible_id").and_then(Value::as_str),
        Some("8001")
    );
    assert_eq!(
        collectible.pointer("/data/statuses/0/pattern_document_id").and_then(Value::as_str),
        Some("17101")
    );
    assert_eq!(
        collectible.pointer("/data/statuses/0/center_color").and_then(Value::as_i64),
        Some(0x123456)
    );
    assert_eq!(
        collectible.pointer("/data/statuses/0/text_color").and_then(Value::as_i64),
        Some(0x456789)
    );

    let not_modified = client
        .call_tool(CallToolRequestParams::new("list_emoji_statuses").with_arguments(
            json!({ "kind": "collectible", "hash": "10004" }).as_object().expect("object").clone(),
        ))
        .await?
        .structured_content
        .expect("not-modified catalogue");
    assert_eq!(not_modified.pointer("/data/not_modified").and_then(Value::as_bool), Some(true));
    assert_eq!(
        not_modified.pointer("/data/statuses").and_then(Value::as_array).map(Vec::len),
        Some(0)
    );
    assert!(not_modified.pointer("/data/response_hash").is_some_and(Value::is_null));

    let limited = client
        .call_tool(CallToolRequestParams::new("list_emoji_statuses").with_arguments(
            json!({ "kind": "default", "limit": 1 }).as_object().expect("object").clone(),
        ))
        .await?
        .structured_content
        .expect("limited catalogue");
    assert_eq!(limited.pointer("/data/available_count").and_then(Value::as_u64), Some(2));
    assert_eq!(limited.pointer("/data/returned_count").and_then(Value::as_u64), Some(1));
    assert_eq!(limited.pointer("/data/truncated").and_then(Value::as_bool), Some(true));
    assert_eq!(
        rejected_code(&client, "list_emoji_statuses", json!({ "kind": "default", "hash": "01" }))
            .await?,
        "INVALID_ARGUMENT"
    );
    assert_eq!(
        rejected_code(
            &client,
            "set_emoji_status",
            json!({ "documentId": "7001", "collectibleId": "8001" })
        )
        .await?,
        "INVALID_ARGUMENT"
    );
    assert_eq!(
        rejected_code(&client, "set_emoji_status", json!({ "untilUnix": 1 })).await?,
        "INVALID_ARGUMENT"
    );

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let until = i32::try_from(now + 300)?;
    backend.make_next_emoji_status_set_outcome_uncertain().await;
    let (set_id, set_message_id, set_prepared, set_approval) = prepare_and_request(
        &client,
        "set_emoji_status",
        json!({ "documentId": "7001", "untilUnix": until }),
    )
    .await?;
    assert_eq!(
        set_prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("set_account_emoji_status")
    );
    assert_eq!(
        set_prepared.pointer("/data/action/before/kind").and_then(Value::as_str),
        Some("empty")
    );
    assert_eq!(
        set_prepared.pointer("/data/action/status/document_id").and_then(Value::as_str),
        Some("7001")
    );
    assert!(
        set_approval
            .pointer("/data/expires_at_unix")
            .and_then(Value::as_i64)
            .is_some_and(|expires| expires <= i64::from(until) && expires > now)
    );
    assert_eq!(backend.emoji_status_set_call_count().await, 0);
    let card_debug = format!("{:?}", bot.sent_mutation_cards().await.last());
    assert!(card_debug.contains("7001"));
    assert!(card_debug.contains(&until.to_string()));
    bot.push_mutation_decision(&set_id, ApprovalAction::Approve, 700, 700, set_message_id).await;
    wait_for_mutation_state(&state_db_path, &set_id, MutationApprovalState::Applied).await?;
    let set_applied = MutationLedger::new(state_db_path.clone())
        .get_approval(&set_id)?
        .expect("applied emoji status");
    assert_eq!(set_applied.automated_retry_count, 1);
    assert_eq!(backend.emoji_status_set_call_count().await, 1);
    let set_result = serde_json::to_value(set_applied.result.expect("emoji status receipt"))?;
    assert_eq!(
        set_result.pointer("/kind").and_then(Value::as_str),
        Some("account_emoji_status_set")
    );
    assert_eq!(set_result.pointer("/status/document_id").and_then(Value::as_str), Some("7001"));
    assert_eq!(
        set_result.pointer("/status/until_unix").and_then(Value::as_i64),
        Some(i64::from(until))
    );

    let (stale_id, stale_message_id, _, _) =
        prepare_and_request(&client, "set_emoji_status", json!({ "collectibleId": "8001" }))
            .await?;
    backend.set_fixture_account_emoji_status(default_status(9_999)).await;
    bot.push_mutation_decision(&stale_id, ApprovalAction::Approve, 700, 700, stale_message_id)
        .await;
    wait_for_mutation_state(&state_db_path, &stale_id, MutationApprovalState::Failed).await?;
    let stale = MutationLedger::new(state_db_path.clone())
        .get_approval(&stale_id)?
        .expect("stale emoji status approval");
    assert_eq!(stale.last_error_code.as_deref(), Some("MUTATION_PRECONDITION_FAILED"));
    assert_eq!(backend.emoji_status_set_call_count().await, 1);

    let (collectible_id, collectible_message_id, _, _) =
        prepare_and_request(&client, "set_emoji_status", json!({ "collectibleId": "8001" }))
            .await?;
    bot.push_mutation_decision(
        &collectible_id,
        ApprovalAction::Approve,
        700,
        700,
        collectible_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &collectible_id, MutationApprovalState::Applied)
        .await?;
    let collectible_result = serde_json::to_value(
        MutationLedger::new(state_db_path.clone())
            .get_approval(&collectible_id)?
            .expect("collectible approval")
            .result
            .expect("collectible receipt"),
    )?;
    assert_eq!(
        collectible_result.pointer("/status/collectible_id").and_then(Value::as_str),
        Some("8001")
    );
    assert_eq!(
        collectible_result.pointer("/status/pattern_document_id").and_then(Value::as_str),
        Some("17101")
    );

    let (clear_current_id, clear_current_message_id, _, _) =
        prepare_and_request(&client, "set_emoji_status", json!({})).await?;
    bot.push_mutation_decision(
        &clear_current_id,
        ApprovalAction::Approve,
        700,
        700,
        clear_current_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &clear_current_id, MutationApprovalState::Applied)
        .await?;
    assert_eq!(backend.emoji_status_set_call_count().await, 3);
    assert_eq!(backend.get_account_emoji_status(None).await?.kind, EmojiStatusKind::Empty);

    backend.make_next_emoji_recent_clear_outcome_uncertain().await;
    let (recent_id, recent_message_id, recent_prepared, _) =
        prepare_and_request(&client, "clear_recent_emoji_statuses", json!({})).await?;
    assert_eq!(
        recent_prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("clear_recent_emoji_statuses")
    );
    assert_eq!(
        recent_prepared
            .pointer("/data/action/before/statuses")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(backend.emoji_recent_clear_call_count().await, 0);
    let recent_card = format!("{:?}", bot.sent_mutation_cards().await.last());
    assert!(recent_card.contains("7002"));
    assert!(recent_card.contains("8001"));
    assert!(recent_card.contains("7101"));
    bot.push_mutation_decision(&recent_id, ApprovalAction::Approve, 700, 700, recent_message_id)
        .await;
    wait_for_mutation_state(&state_db_path, &recent_id, MutationApprovalState::Applied).await?;
    let recent_applied = MutationLedger::new(state_db_path.clone())
        .get_approval(&recent_id)?
        .expect("recent clear approval");
    assert_eq!(recent_applied.automated_retry_count, 1);
    assert_eq!(backend.emoji_recent_clear_call_count().await, 1);
    let recent_result = serde_json::to_value(recent_applied.result.expect("recent clear receipt"))?;
    assert_eq!(
        recent_result.pointer("/remaining/statuses").and_then(Value::as_array).map(Vec::len),
        Some(0)
    );

    backend
        .set_fixture_emoji_status_catalog(
            EmojiStatusCatalogKind::Recent,
            20_001,
            vec![default_status(7_001)],
        )
        .await;
    let (stale_recent_id, stale_recent_message_id, _, _) =
        prepare_and_request(&client, "clear_recent_emoji_statuses", json!({})).await?;
    backend
        .set_fixture_emoji_status_catalog(
            EmojiStatusCatalogKind::Recent,
            20_002,
            vec![default_status(7_002)],
        )
        .await;
    bot.push_mutation_decision(
        &stale_recent_id,
        ApprovalAction::Approve,
        700,
        700,
        stale_recent_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &stale_recent_id, MutationApprovalState::Failed)
        .await?;
    let stale_recent = MutationLedger::new(state_db_path.clone())
        .get_approval(&stale_recent_id)?
        .expect("stale recent clear approval");
    assert_eq!(stale_recent.last_error_code.as_deref(), Some("MUTATION_PRECONDITION_FAILED"));
    assert_eq!(backend.emoji_recent_clear_call_count().await, 1);

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    Ok(())
}
