use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger, TypingAction,
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

async fn prepare_and_request(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    arguments: Value,
) -> anyhow::Result<(String, i64, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("send_typing")
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared typing action");
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
    let approval = requested.structured_content.expect("typing approval");
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
async fn typing_actions_are_approval_bound_and_never_replayed_after_uncertain_delivery()
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

    for invalid in [
        json!({ "chatId": "@alex", "action": "record_audio" }),
        json!({ "chatId": "@alex", "action": "typing", "progress": 10 }),
    ] {
        let rejected = client
            .call_tool(
                CallToolRequestParams::new("send_typing")
                    .with_arguments(invalid.as_object().expect("object").clone()),
            )
            .await;
        assert!(rejected.is_err(), "closed input schema must reject {invalid}");
    }

    let cases = [
        (json!({ "chatId": "@alex" }), "typing", TypingAction::Typing),
        (
            json!({ "chatId": "@alex", "action": "upload_photo" }),
            "upload_photo",
            TypingAction::UploadPhoto,
        ),
        (
            json!({ "chatId": "@alex", "action": "upload_document" }),
            "upload_document",
            TypingAction::UploadDocument,
        ),
        (json!({ "chatId": "@alex", "action": "cancel" }), "cancel", TypingAction::Cancel),
    ];

    for (index, (arguments, action_name, _)) in cases.iter().enumerate() {
        let (approval_id, approval_message_id, prepared) =
            prepare_and_request(&client, arguments.clone()).await?;
        assert_eq!(
            prepared.pointer("/data/action/kind").and_then(Value::as_str),
            Some("send_typing_action")
        );
        assert_eq!(prepared.pointer("/data/action/chat_id").and_then(Value::as_str), Some("101"));
        assert_eq!(
            prepared.pointer("/data/action/action").and_then(Value::as_str),
            Some(*action_name)
        );
        assert_eq!(backend.typing_action_call_count().await, index);
        bot.push_mutation_decision(
            &approval_id,
            ApprovalAction::Approve,
            700,
            700,
            approval_message_id,
        )
        .await;
        wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::Applied)
            .await?;
        let applied = MutationLedger::new(state_db_path.clone())
            .get_approval(&approval_id)?
            .expect("applied typing approval");
        let result = serde_json::to_value(applied.result.expect("typing result"))?;
        assert_eq!(result.pointer("/kind").and_then(Value::as_str), Some("typing_action_sent"));
        assert_eq!(result.pointer("/chat_id").and_then(Value::as_str), Some("101"));
        assert_eq!(result.pointer("/action").and_then(Value::as_str), Some(*action_name));
    }

    assert_eq!(
        backend.typing_actions().await,
        cases.iter().map(|(_, _, action)| ("101".to_string(), *action)).collect::<Vec<_>>()
    );

    backend.make_next_typing_action_outcome_uncertain().await;
    let (approval_id, approval_message_id, _) =
        prepare_and_request(&client, json!({ "chatId": "-202", "action": "typing" })).await?;
    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::OutcomeUnknown)
        .await?;
    assert_eq!(backend.typing_action_call_count().await, 5);
    tokio::time::sleep(Duration::from_millis(1_250)).await;
    assert_eq!(backend.typing_action_call_count().await, 5);
    let uncertain = MutationLedger::new(state_db_path.clone())
        .get_approval(&approval_id)?
        .expect("uncertain typing approval");
    assert_eq!(uncertain.state, MutationApprovalState::OutcomeUnknown);
    assert_eq!(uncertain.automated_retry_count, 1);
    assert_eq!(uncertain.next_retry_at_unix, None);
    assert_eq!(uncertain.last_error_code.as_deref(), Some("MUTATION_OUTCOME_UNKNOWN_NO_RETRY"));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    Ok(())
}
