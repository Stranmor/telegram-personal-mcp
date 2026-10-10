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
async fn saved_messages_reads_and_mutations_are_typed_and_recover_from_lost_receipts()
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

    let dialogs = client
        .call_tool(
            CallToolRequestParams::new("get_saved_dialogs")
                .with_arguments(json!({ "limit": 10 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(dialogs.is_error, Some(false));
    let dialogs = dialogs.structured_content.expect("saved dialogs");
    assert_eq!(dialogs.pointer("/data/dialogs/0/peer_ref").and_then(Value::as_str), Some("101"));
    assert_eq!(dialogs.pointer("/data/dialogs/0/pinned").and_then(Value::as_bool), Some(true));
    assert_eq!(
        dialogs.pointer("/data/dialogs/1/title").and_then(Value::as_str),
        Some("Project Room")
    );

    let history = client
        .call_tool(CallToolRequestParams::new("get_saved_history").with_arguments(
            json!({ "peer": "@alex", "limit": 10 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(history.is_error, Some(false));
    let history = history.structured_content.expect("saved history");
    assert_eq!(history.pointer("/data/peer_ref").and_then(Value::as_str), Some("101"));
    assert_eq!(history.pointer("/data/total_count").and_then(Value::as_i64), Some(2));
    assert_eq!(
        history.pointer("/data/messages/0/text").and_then(Value::as_str),
        Some("Saved contract research notes")
    );

    let search = client
        .call_tool(
            CallToolRequestParams::new("search_saved_messages").with_arguments(
                json!({
                    "query": "release",
                    "tagCustomEmojiIds": ["7001"],
                    "savedPeer": "-202",
                    "limit": 10
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(search.is_error, Some(false));
    assert_eq!(
        search
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/message_id"))
            .and_then(Value::as_i64),
        Some(702)
    );

    let tags = client
        .call_tool(
            CallToolRequestParams::new("get_saved_reaction_tags")
                .with_arguments(json!({}).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(tags.is_error, Some(false));
    let tags = tags.structured_content.expect("saved reaction tags");
    assert_eq!(tags.pointer("/data/hash").and_then(Value::as_str), Some("7007"));
    assert_eq!(tags.pointer("/data/tags/0/title").and_then(Value::as_str), Some("Research"));

    let defaults = client
        .call_tool(
            CallToolRequestParams::new("get_default_tag_reactions")
                .with_arguments(json!({}).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(defaults.is_error, Some(false));
    assert_eq!(
        defaults
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/reactions/0/emoji"))
            .and_then(Value::as_str),
        Some("🧠")
    );

    for (tool, arguments) in [
        ("search_saved_messages", json!({ "minDate": 20, "maxDate": 10 })),
        ("search_saved_messages", json!({ "tagCustomEmojiIds": ["01"] })),
        (
            "update_saved_reaction_tag",
            json!({ "emoji": "🧠", "customEmojiId": "7001", "title": "Bad" }),
        ),
        ("get_saved_history", json!({ "peer": "@alex", "offsetId": 0 })),
    ] {
        let rejected = client
            .call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(rejected.is_error, Some(true), "{tool} should reject invalid input");
    }

    backend.make_next_saved_reaction_tag_update_outcome_uncertain().await;
    let (tag_approval_id, tag_message_id, prepared_tag) = request_mutation!(
        client,
        "update_saved_reaction_tag",
        json!({ "emoji": "🧠", "title": "Deep research" })
    );
    assert_eq!(
        prepared_tag.pointer("/data/action/kind").and_then(Value::as_str),
        Some("update_saved_reaction_tag")
    );
    assert_eq!(backend.saved_messages_mutation_call_counts().await, (0, 0, 0));
    bot.push_mutation_decision(&tag_approval_id, ApprovalAction::Approve, 700, 700, tag_message_id)
        .await;
    wait_for_mutation_state(&state_db_path, &tag_approval_id, MutationApprovalState::Applied)
        .await?;
    assert_eq!(backend.saved_messages_mutation_call_counts().await, (0, 0, 1));

    let renamed = client
        .call_tool(
            CallToolRequestParams::new("get_saved_reaction_tags")
                .with_arguments(json!({}).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(renamed.is_error, Some(false));
    assert_eq!(
        renamed
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/tags/0/title"))
            .and_then(Value::as_str),
        Some("Deep research")
    );

    backend.make_next_saved_dialog_pin_outcome_uncertain().await;
    let (pin_approval_id, pin_message_id, prepared_pin) = request_mutation!(
        client,
        "toggle_saved_dialog_pin",
        json!({ "peer": "-202", "pinned": true })
    );
    assert_eq!(
        prepared_pin.pointer("/data/action/before/peer_ref").and_then(Value::as_str),
        Some("-202")
    );
    bot.push_mutation_decision(&pin_approval_id, ApprovalAction::Approve, 700, 700, pin_message_id)
        .await;
    wait_for_mutation_state(&state_db_path, &pin_approval_id, MutationApprovalState::Applied)
        .await?;
    assert_eq!(backend.saved_messages_mutation_call_counts().await, (0, 1, 1));

    let pinned = client
        .call_tool(
            CallToolRequestParams::new("get_saved_dialogs")
                .with_arguments(json!({ "limit": 10 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(pinned.is_error, Some(false));
    let pinned = pinned.structured_content.expect("pinned dialogs");
    let project_pinned = pinned
        .pointer("/data/dialogs")
        .and_then(Value::as_array)
        .and_then(|dialogs| {
            dialogs
                .iter()
                .find(|dialog| dialog.get("peer_ref").and_then(Value::as_str) == Some("-202"))
        })
        .and_then(|dialog| dialog.get("pinned"))
        .and_then(Value::as_bool);
    assert_eq!(project_pinned, Some(true));

    backend.make_next_saved_history_delete_outcome_uncertain().await;
    let (delete_approval_id, delete_message_id, prepared_delete) =
        request_mutation!(client, "delete_saved_history", json!({ "peer": "@alex" }));
    assert_eq!(
        prepared_delete.pointer("/data/action/kind").and_then(Value::as_str),
        Some("delete_saved_history")
    );
    assert_eq!(
        prepared_delete.pointer("/data/action/before/observed_total_count").and_then(Value::as_i64),
        Some(2)
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
    assert_eq!(backend.saved_messages_mutation_call_counts().await, (1, 1, 1));

    let deleted = client
        .call_tool(CallToolRequestParams::new("get_saved_history").with_arguments(
            json!({ "peer": "@alex", "limit": 10 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(deleted.is_error, Some(false));
    assert_eq!(
        deleted
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/total_count"))
            .and_then(Value::as_i64),
        Some(0)
    );

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    Ok(())
}
