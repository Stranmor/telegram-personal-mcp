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
async fn chat_lifecycle_and_settings_are_e2e_exact_and_idempotent() -> anyhow::Result<()> {
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

    let initial =
        client
            .call_tool(CallToolRequestParams::new("get_chat_details").with_arguments(
                json!({ "chat_id": "-100505" }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(initial.is_error, Some(false));
    let initial = initial.structured_content.expect("chat details");
    assert_eq!(initial.pointer("/data/in_dialogs").and_then(Value::as_bool), Some(true));
    assert_eq!(
        initial.pointer("/data/details/about").and_then(Value::as_str),
        Some("Fixture operations supergroup")
    );

    let (title, title_approval_id, title_message_id) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "update_chat_title",
        json!({ "chat_id": "-100505", "title": "  Release Ops  " })
    );
    assert_eq!(title.pointer("/chat/chat/title").and_then(Value::as_str), Some("Release Ops"));
    bot.push_mutation_decision(
        &title_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        title_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.chat_lifecycle_call_counts().await.4, 1);

    let (about, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "update_chat_about",
        json!({ "chat_id": "-100505", "about": "  Exact release coordination  " })
    );
    assert_eq!(
        about.pointer("/chat/about").and_then(Value::as_str),
        Some("Exact release coordination")
    );

    let (group, group_approval_id, group_message_id) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "create_group",
        json!({ "title": "Client Room", "user_refs": ["@alex", "303"] })
    );
    assert_eq!(group.pointer("/chat/chat/kind").and_then(Value::as_str), Some("group"));
    let group_id =
        group.pointer("/chat/chat/chat_id").and_then(Value::as_str).expect("group id").to_string();
    bot.push_mutation_decision(
        &group_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        group_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.chat_lifecycle_call_counts().await.0, 1);

    let (channel, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "create_channel",
        json!({
            "title": "Release Community",
            "about": "Deterministic lifecycle fixture",
            "broadcast": false
        })
    );
    assert_eq!(channel.pointer("/chat/chat/kind").and_then(Value::as_str), Some("supergroup"));
    let channel_id = channel
        .pointer("/chat/chat/chat_id")
        .and_then(Value::as_str)
        .expect("channel id")
        .to_string();

    let (left, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "leave_chat",
        json!({ "chat_id": channel_id })
    );
    assert_eq!(left.pointer("/state/in_dialogs").and_then(Value::as_bool), Some(false));

    let after_leave =
        client
            .call_tool(CallToolRequestParams::new("get_chat_details").with_arguments(
                json!({ "chat_id": channel_id }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(after_leave.is_error, Some(false));
    assert_eq!(
        after_leave
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/in_dialogs"))
            .and_then(Value::as_bool),
        Some(false)
    );

    let (joined, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "join_chat",
        json!({ "chat_id": channel_id })
    );
    assert_eq!(joined.pointer("/state/in_dialogs").and_then(Value::as_bool), Some(true));

    let (deleted, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "delete_chat",
        json!({ "chat_id": group_id })
    );
    assert_eq!(deleted.pointer("/state/in_dialogs").and_then(Value::as_bool), Some(false));
    let deleted_readback =
        client
            .call_tool(CallToolRequestParams::new("get_chat_details").with_arguments(
                json!({ "chat_id": group_id }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(deleted_readback.is_error, Some(true));

    let duplicate_member = client
        .call_tool(
            CallToolRequestParams::new("create_group").with_arguments(
                json!({ "title": "Bad", "user_refs": ["@alex", "101"] })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(duplicate_member.is_error, Some(true));

    let same_title = client
        .call_tool(
            CallToolRequestParams::new("update_chat_title").with_arguments(
                json!({ "chat_id": "-100505", "title": "Release Ops" })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(same_title.is_error, Some(true));
    assert_eq!(backend.chat_lifecycle_call_counts().await, (2, 1, 1, 1, 1, 1));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    Ok(())
}
