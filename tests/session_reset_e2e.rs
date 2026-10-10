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
) -> anyhow::Result<(String, i64, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_terminate_all_other_sessions")
                .with_arguments(json!({}).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared session reset");
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
    let approval = requested.structured_content.expect("session reset approval");
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

#[tokio::test]
async fn all_other_sessions_reset_is_exact_current_safe_and_idempotently_recoverable()
-> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let initial_sessions = backend.get_sessions(None).await?;
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

    let closed_schema = client
        .call_tool(
            CallToolRequestParams::new("prepare_terminate_all_other_sessions").with_arguments(
                json!({ "includeCurrent": true }).as_object().expect("object").clone(),
            ),
        )
        .await;
    assert!(closed_schema.is_err(), "unknown fields must fail at the JSON-RPC boundary");

    let listed = client
        .call_tool(
            CallToolRequestParams::new("get_sessions")
                .with_arguments(json!({}).as_object().expect("object").clone()),
        )
        .await?
        .structured_content
        .expect("session list");
    assert_eq!(listed.pointer("/data/returned_count").and_then(Value::as_u64), Some(2));
    assert_eq!(
        listed
            .pointer("/data/sessions")
            .and_then(Value::as_array)
            .expect("session array")
            .iter()
            .filter(|session| session.pointer("/current").and_then(Value::as_bool) == Some(true))
            .count(),
        1
    );

    backend.make_next_session_reset_outcome_uncertain().await;
    let (approval_id, approval_message_id, prepared) = prepare_and_request(&client).await?;
    assert_eq!(
        prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("terminate_all_other_sessions")
    );
    assert_eq!(
        prepared.pointer("/data/action/before").and_then(Value::as_array).map(Vec::len),
        Some(1)
    );
    assert_eq!(
        prepared.pointer("/data/action/before/0/authorization_id").and_then(Value::as_i64),
        Some(2)
    );
    assert_eq!(
        prepared.pointer("/data/action/before/0/current").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(backend.session_reset_call_count().await, 0);
    let card_debug = format!("{:?}", bot.sent_mutation_cards().await.last());
    assert!(card_debug.contains("Fixture phone"));
    assert!(card_debug.contains("authorization_id: 2"));

    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::Applied).await?;
    let applied = MutationLedger::new(state_db_path.clone())
        .get_approval(&approval_id)?
        .expect("applied session reset");
    assert_eq!(applied.automated_retry_count, 1);
    assert_eq!(backend.session_reset_call_count().await, 1);
    let result = serde_json::to_value(applied.result.expect("session reset receipt"))?;
    assert_eq!(result.pointer("/kind").and_then(Value::as_str), Some("other_sessions_terminated"));
    assert_eq!(result.pointer("/authorization_ids/0").and_then(Value::as_i64), Some(2));
    let remaining = backend.get_sessions(None).await?;
    assert_eq!(remaining.len(), 1);
    assert!(remaining[0].current);
    assert_eq!(remaining[0].authorization_id, 1);
    assert_eq!(
        rejected_code(&client, "prepare_terminate_all_other_sessions", json!({})).await?,
        "NO_CHANGE"
    );

    backend.set_fixture_sessions(initial_sessions.clone()).await;
    let (stale_id, stale_message_id, _) = prepare_and_request(&client).await?;
    let mut changed_sessions = initial_sessions;
    changed_sessions
        .iter_mut()
        .find(|session| !session.current)
        .expect("other session")
        .device_model = "Changed phone after approval".to_string();
    backend.set_fixture_sessions(changed_sessions.clone()).await;
    bot.push_mutation_decision(&stale_id, ApprovalAction::Approve, 700, 700, stale_message_id)
        .await;
    wait_for_mutation_state(&state_db_path, &stale_id, MutationApprovalState::Failed).await?;
    let stale = MutationLedger::new(state_db_path.clone())
        .get_approval(&stale_id)?
        .expect("stale reset approval");
    assert_eq!(stale.last_error_code.as_deref(), Some("MUTATION_PRECONDITION_FAILED"));
    assert_eq!(backend.session_reset_call_count().await, 1);
    assert_eq!(backend.get_sessions(None).await?, changed_sessions);

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    Ok(())
}
