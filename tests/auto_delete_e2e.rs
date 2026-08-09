use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger, TelegramBackend,
};

async fn wait_for_mutation_state(
    state_db_path: &std::path::Path,
    approval_id: &str,
    wanted: MutationApprovalState,
) -> anyhow::Result<()> {
    let mut last = None;
    for _ in 0..1_500 {
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

async fn wait_for_error_code(
    state_db_path: &std::path::Path,
    approval_id: &str,
    wanted: &str,
) -> anyhow::Result<()> {
    let mut last = None;
    for _ in 0..1_500 {
        let approval =
            MutationLedger::new(state_db_path.to_path_buf()).get_approval(approval_id)?;
        let code = approval.as_ref().and_then(|approval| approval.last_error_code.as_deref());
        if code == Some(wanted) {
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
    anyhow::bail!("mutation approval did not reach error {wanted}; last={last:?}")
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
    arguments: Value,
) -> anyhow::Result<(String, i64, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("set_auto_delete")
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared auto-delete mutation");
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
    let approval = requested.structured_content.expect("auto-delete approval");
    let approval_id = approval
        .pointer("/data/approval_id")
        .and_then(Value::as_str)
        .expect("approval id")
        .to_string();
    let approval_message_id = approval
        .pointer("/data/approval_message_id")
        .and_then(Value::as_i64)
        .expect("approval message id");
    Ok((approval_id, approval_message_id, prepared))
}

async fn approve(bot: &FakeApprovalBot, approval_id: &str, approval_message_id: i64) {
    bot.push_mutation_decision(approval_id, ApprovalAction::Approve, 700, 700, approval_message_id)
        .await;
}

#[tokio::test]
async fn chat_auto_delete_is_canonical_approval_bound_and_recoverable_for_every_peer_kind()
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

    let closed_schema = client
        .call_tool(CallToolRequestParams::new("get_auto_delete").with_arguments(
            json!({ "chatId": "@alex", "includeRawTl": true }).as_object().expect("object").clone(),
        ))
        .await;
    assert!(closed_schema.is_err(), "unknown fields must fail at the JSON-RPC boundary");

    for (source, canonical) in [("@alex", "101"), ("-202", "-202"), ("-100505", "-100505")] {
        let readback =
            client
                .call_tool(CallToolRequestParams::new("get_auto_delete").with_arguments(
                    json!({ "chatId": source }).as_object().expect("object").clone(),
                ))
                .await?;
        assert_eq!(readback.is_error, Some(false));
        let readback = readback.structured_content.expect("auto-delete readback");
        assert_eq!(readback.pointer("/data/chat_id").and_then(Value::as_str), Some(canonical));
        assert_eq!(readback.pointer("/data/period_seconds").and_then(Value::as_i64), Some(0));
    }
    assert_eq!(
        rejected_code(&client, "set_auto_delete", json!({ "chatId": "@alex", "period": -1 }))
            .await?,
        "INVALID_ARGUMENT"
    );

    backend.make_next_chat_auto_delete_outcome_uncertain().await;
    let (private_id, private_message_id, prepared) =
        prepare_and_request(&client, json!({ "chatId": "@alex", "period": 86_400 })).await?;
    assert_eq!(
        prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("set_chat_auto_delete")
    );
    assert_eq!(
        prepared.pointer("/data/action/before/chat_id").and_then(Value::as_str),
        Some("101")
    );
    assert_eq!(
        prepared.pointer("/data/action/before/period_seconds").and_then(Value::as_i64),
        Some(0)
    );
    assert_eq!(
        prepared.pointer("/data/action/period_seconds").and_then(Value::as_i64),
        Some(86_400)
    );
    assert_eq!(backend.chat_auto_delete_call_count().await, 0);
    let card_debug = format!("{:?}", bot.sent_mutation_cards().await.last());
    assert!(card_debug.contains("101"));
    assert!(card_debug.contains("86400"));
    approve(&bot, &private_id, private_message_id).await;
    wait_for_mutation_state(&state_db_path, &private_id, MutationApprovalState::Applied).await?;
    let private = MutationLedger::new(state_db_path.clone())
        .get_approval(&private_id)?
        .expect("applied private-chat auto-delete");
    assert_eq!(private.automated_retry_count, 1);
    assert_eq!(backend.chat_auto_delete_call_count().await, 1);
    let private_result = serde_json::to_value(private.result.expect("private receipt"))?;
    assert_eq!(
        private_result.pointer("/kind").and_then(Value::as_str),
        Some("chat_auto_delete_updated")
    );
    assert_eq!(private_result.pointer("/state/chat_id").and_then(Value::as_str), Some("101"));
    assert_eq!(
        private_result.pointer("/state/period_seconds").and_then(Value::as_i64),
        Some(86_400)
    );
    assert_eq!(
        rejected_code(
            &client,
            "set_auto_delete",
            json!({ "chatId": "101", "periodSeconds": 86_400 })
        )
        .await?,
        "NO_CHANGE"
    );

    let (group_id, group_message_id, _) =
        prepare_and_request(&client, json!({ "chatId": "-202", "periodSeconds": 3_600 })).await?;
    approve(&bot, &group_id, group_message_id).await;
    wait_for_mutation_state(&state_db_path, &group_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.chat_auto_delete_call_count().await, 2);
    assert_eq!(backend.get_chat_auto_delete(None, "-202").await?.period_seconds, 3_600);

    let (channel_id, channel_message_id, _) =
        prepare_and_request(&client, json!({ "chatId": "-100505", "periodSeconds": 604_800 }))
            .await?;
    approve(&bot, &channel_id, channel_message_id).await;
    wait_for_mutation_state(&state_db_path, &channel_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.chat_auto_delete_call_count().await, 3);
    assert_eq!(backend.get_chat_auto_delete(None, "-100505").await?.period_seconds, 604_800);

    let (disable_id, disable_message_id, _) =
        prepare_and_request(&client, json!({ "chatId": "-100505", "periodSeconds": 0 })).await?;
    approve(&bot, &disable_id, disable_message_id).await;
    wait_for_mutation_state(&state_db_path, &disable_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.chat_auto_delete_call_count().await, 4);
    assert_eq!(backend.get_chat_auto_delete(None, "-100505").await?.period_seconds, 0);

    let (stale_id, stale_message_id, _) =
        prepare_and_request(&client, json!({ "chatId": "-202", "periodSeconds": 7_200 })).await?;
    backend.set_fixture_chat_auto_delete("-202", 1_800).await;
    approve(&bot, &stale_id, stale_message_id).await;
    wait_for_mutation_state(&state_db_path, &stale_id, MutationApprovalState::Failed).await?;
    let stale = MutationLedger::new(state_db_path.clone())
        .get_approval(&stale_id)?
        .expect("stale auto-delete approval");
    assert_eq!(stale.last_error_code.as_deref(), Some("MUTATION_PRECONDITION_FAILED"));
    assert_eq!(backend.chat_auto_delete_call_count().await, 4);
    assert_eq!(backend.get_chat_auto_delete(None, "-202").await?.period_seconds, 1_800);

    backend.make_next_chat_auto_delete_outcome_uncertain().await;
    let (changed_id, changed_message_id, _) =
        prepare_and_request(&client, json!({ "chatId": "101", "periodSeconds": 172_800 })).await?;
    approve(&bot, &changed_id, changed_message_id).await;
    wait_for_mutation_state(&state_db_path, &changed_id, MutationApprovalState::OutcomeUnknown)
        .await?;
    assert_eq!(backend.chat_auto_delete_call_count().await, 5);
    backend.set_fixture_chat_auto_delete("101", 42).await;
    wait_for_error_code(&state_db_path, &changed_id, "MUTATION_OUTCOME_UNKNOWN_NO_RETRY").await?;
    let changed = MutationLedger::new(state_db_path.clone())
        .get_approval(&changed_id)?
        .expect("changed recovery approval");
    assert_eq!(changed.state, MutationApprovalState::OutcomeUnknown);
    assert_eq!(changed.automated_retry_count, 2);
    assert_eq!(changed.next_retry_at_unix, None);
    assert_eq!(backend.chat_auto_delete_call_count().await, 5);
    assert_eq!(backend.get_chat_auto_delete(None, "101").await?.period_seconds, 42);

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    Ok(())
}
