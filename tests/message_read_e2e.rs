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
async fn discussion_and_read_receipts_are_e2e_bounded_and_idempotent() -> anyhow::Result<()> {
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

    let discussion = client
        .call_tool(CallToolRequestParams::new("get_discussion_message").with_arguments(
            json!({ "chat_id": "@alex", "message_id": 10 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(discussion.is_error, Some(false));
    let discussion = discussion.structured_content.expect("discussion data");
    assert_eq!(discussion.pointer("/data/source_chat_id").and_then(Value::as_str), Some("101"));
    assert_eq!(
        discussion.pointer("/data/messages").and_then(Value::as_array).map(Vec::len),
        Some(2)
    );
    assert_eq!(
        discussion.pointer("/data/messages/1/reply_to_message_id").and_then(Value::as_i64),
        Some(10)
    );

    let invalid_discussion = client
        .call_tool(CallToolRequestParams::new("get_discussion_message").with_arguments(
            json!({ "chat_id": "@alex", "message_id": 0 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(invalid_discussion.is_error, Some(true));

    let (partial, partial_approval_id, partial_message_id) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "mark_as_read",
        json!({ "chat_id": "@alex", "max_id": 10 })
    );
    assert_eq!(partial.pointer("/kind").and_then(Value::as_str), Some("messages_marked_read"));
    assert_eq!(partial.pointer("/max_id").and_then(Value::as_i64), Some(10));
    assert_eq!(partial.pointer("/state/read_inbox_max_id").and_then(Value::as_i64), Some(10));
    assert_eq!(partial.pointer("/state/unread_count").and_then(Value::as_i64), Some(1));
    assert_eq!(partial.pointer("/state/unread_mark").and_then(Value::as_bool), Some(true));

    bot.push_mutation_decision(
        &partial_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        partial_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.message_read_call_counts().await.0, 1);

    let (all_current, _, _) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "mark_as_read",
        json!({ "chat_id": "@alex" })
    );
    assert_eq!(all_current.pointer("/max_id").and_then(Value::as_i64), Some(11));
    assert_eq!(all_current.pointer("/state/read_inbox_max_id").and_then(Value::as_i64), Some(11));
    assert_eq!(all_current.pointer("/state/unread_count").and_then(Value::as_i64), Some(0));
    assert_eq!(all_current.pointer("/state/unread_mark").and_then(Value::as_bool), Some(false));

    for arguments in
        [json!({ "chat_id": "@alex", "max_id": 11 }), json!({ "chat_id": "@alex", "max_id": 12 })]
    {
        let rejected = client
            .call_tool(
                CallToolRequestParams::new("mark_as_read")
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(rejected.is_error, Some(true));
    }

    let (reactions, reactions_approval_id, reactions_message_id) = approve_mutation!(
        client,
        bot,
        state_db_path,
        "mark_reactions_read",
        json!({ "chat_id": "@alex" })
    );
    assert_eq!(
        reactions.pointer("/kind").and_then(Value::as_str),
        Some("message_reactions_marked_read")
    );
    assert_eq!(reactions.pointer("/unread_reactions_count").and_then(Value::as_i64), Some(0));
    assert_eq!(reactions.pointer("/server_receipt/pts_count").and_then(Value::as_i64), Some(1));

    bot.push_mutation_decision(
        &reactions_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        reactions_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(backend.message_read_call_counts().await, (2, 1));

    let no_reactions = client
        .call_tool(
            CallToolRequestParams::new("mark_reactions_read")
                .with_arguments(json!({ "chat_id": "@alex" }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(no_reactions.is_error, Some(true));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    Ok(())
}
