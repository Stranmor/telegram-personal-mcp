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
    anyhow::bail!("boost mutation did not become applied; last={last:?}")
}

#[tokio::test]
async fn channel_stats_and_boosts_are_typed_bounded_and_cursor_safe() -> anyhow::Result<()> {
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

    let broadcast = client
        .call_tool(
            CallToolRequestParams::new("get_channel_stats")
                .with_arguments(json!({"chat_id": "-100505"}).as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(broadcast.is_error, Some(false));
    assert_eq!(
        broadcast
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/kind"))
            .and_then(Value::as_str),
        Some("broadcast")
    );
    assert_eq!(
        broadcast
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/followers/current"))
            .and_then(Value::as_f64),
        Some(1200.0)
    );
    assert_eq!(
        broadcast
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/graphs/0/state"))
            .and_then(Value::as_str),
        Some("loaded")
    );

    let megagroup = client
        .call_tool(
            CallToolRequestParams::new("get_channel_stats")
                .with_arguments(json!({"chat_id": "-202"}).as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(megagroup.is_error, Some(false));
    assert_eq!(
        megagroup
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/kind"))
            .and_then(Value::as_str),
        Some("megagroup")
    );
    assert_eq!(
        megagroup
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/top_admins/0/user_id"))
            .and_then(Value::as_i64),
        Some(7)
    );

    let mine = client.call_tool(CallToolRequestParams::new("get_my_boosts")).await?;
    assert_eq!(mine.is_error, Some(false));
    assert_eq!(
        mine.structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/returned_count"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        mine.structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/slots/0/chat_id"))
            .and_then(Value::as_str),
        Some("-100505")
    );

    let status = client
        .call_tool(
            CallToolRequestParams::new("get_boost_status")
                .with_arguments(json!({"chat_id": "-100505"}).as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(status.is_error, Some(false));
    assert_eq!(
        status
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/boosts"))
            .and_then(Value::as_i64),
        Some(8)
    );
    assert_eq!(
        status
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/premium_audience/percentage"))
            .and_then(Value::as_f64),
        Some(56.25)
    );

    let first =
        client
            .call_tool(CallToolRequestParams::new("list_boosts").with_arguments(
                json!({"chat_id": "-100505", "limit": 1}).as_object().unwrap().clone(),
            ))
            .await?;
    assert_eq!(first.is_error, Some(false));
    assert_eq!(
        first
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/returned_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        first
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/next_offset"))
            .and_then(Value::as_str),
        Some("1")
    );
    assert_eq!(
        first
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/boosts/0/gift"))
            .and_then(Value::as_bool),
        Some(false)
    );

    let second = client
        .call_tool(CallToolRequestParams::new("list_boosts").with_arguments(
            json!({"chat_id": "-100505", "gifts_only": true}).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(second.is_error, Some(false));
    assert_eq!(
        second
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/boosts/0/boost_id"))
            .and_then(Value::as_str),
        Some("boost-fixture-gift")
    );
    assert_eq!(
        second
            .structured_content
            .as_ref()
            .and_then(|v| v.pointer("/data/boosts/0/gift"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let rejected = client
        .call_tool(CallToolRequestParams::new("list_boosts").with_arguments(
            json!({"chat_id": "-100505", "limit": 101}).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(rejected.is_error, Some(true));

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn boost_slots_are_approval_bound_read_back_and_recovered_without_duplicate_application()
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

    for arguments in [
        json!({ "chat_id": "-100505", "slots": [] }),
        json!({ "chat_id": "-100505", "slots": [2, 2] }),
        json!({ "chat_id": "-100505", "slots": [999] }),
        json!({ "chat_id": "-100505", "slots": [1] }),
    ] {
        let rejected = client
            .call_tool(
                CallToolRequestParams::new("apply_boost")
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(rejected.is_error, Some(true));
    }
    assert_eq!(backend.boost_apply_call_count().await, 0);

    backend.make_next_boost_apply_outcome_uncertain().await;
    let prepared = client
        .call_tool(CallToolRequestParams::new("apply_boost").with_arguments(
            json!({ "chat_id": "-100505", "slots": [2] }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared boost mutation");
    assert_eq!(prepared.pointer("/data/action/kind").and_then(Value::as_str), Some("apply_boost"));
    assert_eq!(
        prepared.pointer("/data/action/before/slots/1/chat_id").and_then(Value::as_str),
        None
    );
    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
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
    let requested = requested.structured_content.expect("boost approval");
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

    let approval = MutationLedger::new(state_db_path.clone())
        .get_approval(&approval_id)?
        .expect("applied boost approval");
    let result = serde_json::to_value(approval.result.expect("boost receipt"))?;
    assert_eq!(result.pointer("/kind").and_then(Value::as_str), Some("boost_applied"));
    assert_eq!(result.pointer("/chat_id").and_then(Value::as_str), Some("-100505"));
    assert_eq!(result.pointer("/boosts/slots/1/chat_id").and_then(Value::as_str), Some("-100505"));
    assert_eq!(backend.boost_apply_call_count().await, 1);

    let readback = client.call_tool(CallToolRequestParams::new("get_my_boosts")).await?;
    assert_eq!(
        readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/slots/1/chat_id"))
            .and_then(Value::as_str),
        Some("-100505")
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
    assert_eq!(backend.boost_apply_call_count().await, 1);
    let cards = format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(cards.contains("ApplyBoost"));
    assert!(cards.contains("slots: [2]"));

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}
