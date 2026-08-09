use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger,
};

async fn wait_for_mutation_state(
    state_db_path: &std::path::Path,
    approval_id: &str,
    wanted: MutationApprovalState,
) -> anyhow::Result<()> {
    let mut last = None;
    for _ in 0..300 {
        let approval =
            MutationLedger::new(state_db_path.to_path_buf()).get_approval(approval_id)?;
        let state = approval.as_ref().map(|approval| approval.state);
        if state == Some(wanted) {
            return Ok(());
        }
        last = approval.map(|approval| (approval.state, approval.last_error_code));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("mutation approval did not reach {wanted:?}; last={last:?}")
}

macro_rules! approve_mutation {
    ($client:expr, $bot:expr, $state_db_path:expr, $tool:expr, $arguments:expr) => {{
        let prepared = $client
            .call_tool(
                CallToolRequestParams::new($tool)
                    .with_arguments($arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(prepared.is_error, Some(false), "prepare {}", $tool);
        let data = prepared.structured_content.clone().expect("prepared data");
        let mutation_id = data
            .pointer("/data/mutation_id")
            .and_then(Value::as_str)
            .expect("mutation id")
            .to_string();
        let payload_sha256 = data
            .pointer("/data/payload_sha256")
            .and_then(Value::as_str)
            .expect("payload hash")
            .to_string();
        let requested = $client
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
        assert_eq!(requested.is_error, Some(false), "request {}", $tool);
        let requested_data = requested.structured_content.clone().expect("approval data");
        let approval_id = requested_data
            .pointer("/data/approval_id")
            .and_then(Value::as_str)
            .expect("approval id")
            .to_string();
        let message_id = requested_data
            .pointer("/data/approval_message_id")
            .and_then(Value::as_i64)
            .expect("approval message id");
        $bot
            .push_mutation_decision(
                &approval_id,
                ApprovalAction::Approve,
                700,
                700,
                message_id,
            )
            .await;
        wait_for_mutation_state(
            &$state_db_path,
            &approval_id,
            MutationApprovalState::Applied,
        )
        .await?;
        let result = MutationLedger::new($state_db_path.to_path_buf())
            .get_approval(&approval_id)?
            .expect("applied mutation")
            .result
            .map(|result| serde_json::to_value(result).expect("result json"))
            .expect("mutation result");
        (result, approval_id, message_id)
    }};
}

#[tokio::test]
async fn channel_settings_are_snapshot_bound_and_e2e_idempotent() -> anyhow::Result<()> {
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

    let initial =
        client
            .call_tool(CallToolRequestParams::new("get_channel_info").with_arguments(
                json!({ "chat_id": "-100505" }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(initial.is_error, Some(false));
    assert_eq!(
        initial
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/slow_mode_seconds"))
            .and_then(Value::as_i64),
        Some(10)
    );
    assert_eq!(
        initial
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/forum_enabled"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let checked = client
        .call_tool(
            CallToolRequestParams::new("check_username").with_arguments(
                json!({ "chat_id": "-100505", "username": "release_ops" })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(checked.is_error, Some(false));
    assert_eq!(
        checked
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let (username, username_approval, username_message) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "update_chat_username",
        json!({ "chat_id": "-100505", "username": "release_ops" })
    );
    assert_eq!(username.pointer("/kind").and_then(Value::as_str), Some("channel_settings_updated"));
    assert_eq!(
        username.pointer("/settings/details/chat/username").and_then(Value::as_str),
        Some("release_ops")
    );
    bot.push_mutation_decision(
        &username_approval,
        ApprovalAction::Approve,
        700,
        700,
        username_message,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let (slow, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "set_slow_mode",
        json!({ "chat_id": "-100505", "seconds": 30 })
    );
    assert_eq!(slow.pointer("/settings/slow_mode_seconds").and_then(Value::as_i64), Some(30));

    let (signatures, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "toggle_signatures",
        json!({ "chat_id": "-100505", "enabled": true, "profiles_enabled": true })
    );
    assert_eq!(
        signatures.pointer("/settings/signatures_enabled").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        signatures.pointer("/settings/signature_profiles_enabled").and_then(Value::as_bool),
        Some(true)
    );

    let (pre_history, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "toggle_pre_history_hidden",
        json!({ "chat_id": "-100505", "enabled": true })
    );
    assert_eq!(
        pre_history.pointer("/settings/pre_history_hidden").and_then(Value::as_bool),
        Some(true)
    );

    let (join_request, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "toggle_join_request",
        json!({ "chat_id": "-100505", "enabled": true })
    );
    assert_eq!(
        join_request.pointer("/settings/join_request_required").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(backend.channel_settings_call_counts().await, (1, 1, 1, 1, 1));

    let invalid_slow = client
        .call_tool(CallToolRequestParams::new("set_slow_mode").with_arguments(
            json!({ "chat_id": "-100505", "seconds": 7 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(invalid_slow.is_error, Some(true));

    let no_change = client
        .call_tool(CallToolRequestParams::new("toggle_join_request").with_arguments(
            json!({ "chat_id": "-100505", "enabled": true }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(no_change.is_error, Some(true));

    let approval_surface =
        format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(!approval_surface.contains("access_hash"));
    assert!(!approval_surface.contains("file_reference"));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
