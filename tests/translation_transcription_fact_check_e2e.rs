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

macro_rules! request_mutation {
    ($client:expr, $tool:expr, $arguments:expr) => {{
        let prepared = $client
            .call_tool(
                CallToolRequestParams::new($tool)
                    .with_arguments($arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(prepared.is_error, Some(false), "prepare {}", $tool);
        let prepared_data = prepared.structured_content.expect("prepared mutation");
        let mutation_id = prepared_data
            .pointer("/data/mutation_id")
            .and_then(Value::as_str)
            .expect("mutation id")
            .to_string();
        let payload_sha256 = prepared_data
            .pointer("/data/payload_sha256")
            .and_then(Value::as_str)
            .expect("payload sha256")
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
        let approval = requested.structured_content.expect("approval");
        let approval_id = approval
            .pointer("/data/approval_id")
            .and_then(Value::as_str)
            .expect("approval id")
            .to_string();
        let approval_message_id = approval
            .pointer("/data/approval_message_id")
            .and_then(Value::as_i64)
            .expect("approval message id");
        (approval_id, approval_message_id, prepared_data)
    }};
}

#[tokio::test]
async fn translation_transcription_and_fact_checks_are_approval_bound_and_recover_safely()
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

    let unapproved_poll = client
        .call_tool(CallToolRequestParams::new("get_transcription").with_arguments(
            json!({ "chat_id": "@alex", "message_id": 12 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(unapproved_poll.is_error, Some(true));
    assert_eq!(backend.transcription_call_counts().await, (0, 0, 0));

    let fact_checks = client
        .call_tool(
            CallToolRequestParams::new("get_fact_check").with_arguments(
                json!({ "chat_id": "-100505", "message_ids": [50] })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(fact_checks.is_error, Some(false));
    assert_eq!(
        fact_checks
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/fact_checks/0/text"))
            .and_then(Value::as_str),
        Some("Verified fixture release announcement")
    );
    assert_eq!(
        fact_checks
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/fact_checks/0/hash"))
            .and_then(Value::as_str),
        Some("5001")
    );

    for (tool, arguments) in [
        (
            "translate_message",
            json!({ "chat_id": "@alex", "message_ids": [10, 10], "to_language": "es" }),
        ),
        (
            "translate_message",
            json!({ "chat_id": "@alex", "message_ids": [10], "to_language": "bad tag!" }),
        ),
        (
            "translate_message",
            json!({ "chat_id": "@alex", "message_ids": [12], "to_language": "es" }),
        ),
        ("transcribe_audio", json!({ "chat_id": "@alex", "message_id": 10 })),
        ("get_fact_check", json!({ "chat_id": "-100505", "message_ids": [50, 50] })),
        ("edit_fact_check", json!({ "chat_id": "-100505", "message_id": 50, "text": "" })),
    ] {
        let rejected = client
            .call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(rejected.is_error, Some(true), "{tool} should reject invalid input");
    }

    let (translation_approval_id, translation_message_id, prepared_translation) = request_mutation!(
        client,
        "translate_message",
        json!({
            "chat_id": "@alex",
            "message_ids": [10, 11],
            "to_language": "es"
        })
    );
    assert_eq!(
        prepared_translation.pointer("/data/action/kind").and_then(Value::as_str),
        Some("translate_messages")
    );
    assert_eq!(
        prepared_translation.pointer("/data/action/messages/0/text").and_then(Value::as_str),
        Some("Here is the latest contract")
    );
    bot.push_mutation_decision(
        &translation_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        translation_message_id,
    )
    .await;
    wait_for_mutation_state(
        &state_db_path,
        &translation_approval_id,
        MutationApprovalState::Applied,
    )
    .await?;
    let translation = MutationLedger::new(state_db_path.clone())
        .get_approval(&translation_approval_id)?
        .and_then(|approval| approval.result)
        .map(serde_json::to_value)
        .transpose()?
        .expect("translation result");
    assert_eq!(
        translation.pointer("/translation/translations/0/text").and_then(Value::as_str),
        Some("Aquí está el contrato más reciente")
    );
    assert_eq!(backend.translation_call_count().await, 1);

    backend.make_next_translation_outcome_uncertain().await;
    let (uncertain_translation_id, uncertain_translation_message_id, _) = request_mutation!(
        client,
        "translate_message",
        json!({ "chat_id": "@alex", "message_ids": [11], "to_language": "de" })
    );
    bot.push_mutation_decision(
        &uncertain_translation_id,
        ApprovalAction::Approve,
        700,
        700,
        uncertain_translation_message_id,
    )
    .await;
    wait_for_mutation_state(
        &state_db_path,
        &uncertain_translation_id,
        MutationApprovalState::OutcomeUnknown,
    )
    .await?;
    let uncertain = MutationLedger::new(state_db_path.clone())
        .get_approval(&uncertain_translation_id)?
        .expect("uncertain translation");
    assert_eq!(uncertain.last_error_code.as_deref(), Some("QUOTA_OUTCOME_UNKNOWN"));
    assert_eq!(uncertain.next_retry_at_unix, None);
    assert_eq!(backend.translation_call_count().await, 2);
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(backend.translation_call_count().await, 2);

    backend.make_next_transcription_outcome_uncertain().await;
    let (transcription_approval_id, transcription_message_id, _) = request_mutation!(
        client,
        "transcribe_audio",
        json!({ "chat_id": "@alex", "message_id": 12 })
    );
    bot.push_mutation_decision(
        &transcription_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        transcription_message_id,
    )
    .await;
    wait_for_mutation_state(
        &state_db_path,
        &transcription_approval_id,
        MutationApprovalState::Applied,
    )
    .await?;
    let (starts, polls, ratings) = backend.transcription_call_counts().await;
    assert_eq!((starts, polls, ratings), (1, 2, 0));

    let transcription = client
        .call_tool(CallToolRequestParams::new("get_transcription").with_arguments(
            json!({ "chat_id": "@alex", "message_id": 12 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(transcription.is_error, Some(false));
    let transcription = transcription.structured_content.expect("transcription");
    assert_eq!(transcription.pointer("/data/pending").and_then(Value::as_bool), Some(false));
    assert_eq!(
        transcription.pointer("/data/text").and_then(Value::as_str),
        Some("Fixture voice transcription complete")
    );
    let transcription_id = transcription
        .pointer("/data/transcription_id")
        .and_then(Value::as_str)
        .expect("transcription id")
        .to_string();

    let duplicate = client
        .call_tool(CallToolRequestParams::new("transcribe_audio").with_arguments(
            json!({ "chat_id": "@alex", "message_id": 12 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(duplicate.is_error, Some(true));

    let (rating_approval_id, rating_message_id, _) = request_mutation!(
        client,
        "rate_transcription",
        json!({
            "chat_id": "@alex",
            "message_id": 12,
            "transcription_id": transcription_id,
            "good": true
        })
    );
    bot.push_mutation_decision(
        &rating_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        rating_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &rating_approval_id, MutationApprovalState::Applied)
        .await?;
    assert_eq!(backend.transcription_rating(101, 12, &transcription_id).await, Some(true));

    backend.make_next_fact_check_edit_outcome_uncertain().await;
    let (edit_approval_id, edit_message_id, _) = request_mutation!(
        client,
        "edit_fact_check",
        json!({
            "chat_id": "-100505",
            "message_id": 50,
            "text": "Verified after independent source review"
        })
    );
    bot.push_mutation_decision(
        &edit_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        edit_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &edit_approval_id, MutationApprovalState::Applied)
        .await?;
    assert_eq!(backend.fact_check_mutation_call_counts().await, (1, 0));

    let edited = client
        .call_tool(
            CallToolRequestParams::new("get_fact_check").with_arguments(
                json!({ "chat_id": "-100505", "message_ids": [50] })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(edited.is_error, Some(false));
    assert_eq!(
        edited
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/fact_checks/0/text"))
            .and_then(Value::as_str),
        Some("Verified after independent source review")
    );

    let (delete_approval_id, delete_message_id, _) = request_mutation!(
        client,
        "delete_fact_check",
        json!({ "chat_id": "-100505", "message_id": 50 })
    );
    bot.push_mutation_decision(
        &delete_approval_id,
        ApprovalAction::Approve,
        700,
        700,
        delete_message_id,
    )
    .await;
    wait_for_mutation_state(&state_db_path, &delete_approval_id, MutationApprovalState::Applied)
        .await?;
    assert_eq!(backend.fact_check_mutation_call_counts().await, (1, 1));
    let deleted = client
        .call_tool(
            CallToolRequestParams::new("get_fact_check").with_arguments(
                json!({ "chat_id": "-100505", "message_ids": [50] })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(deleted.is_error, Some(false));
    assert_eq!(
        deleted
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/fact_checks/0/text"))
            .and_then(Value::as_str),
        None
    );

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    Ok(())
}
