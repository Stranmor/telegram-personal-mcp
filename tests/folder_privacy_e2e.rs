use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend,
    GlobalPrivacySettingsSummary, McpServer, McpServerSettings, MutationApprovalState,
    MutationLedger, TelegramBackend,
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
    assert_eq!(rejected.is_error, Some(true), "{tool} must reject invalid input");
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
) -> anyhow::Result<(String, i64, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new(tool)
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false), "prepare {tool}");
    let prepared = prepared.structured_content.expect("prepared mutation");
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
    assert_eq!(requested.is_error, Some(false), "request {tool}");
    let approval = requested.structured_content.expect("approval request");
    let approval_id = approval
        .pointer("/data/approval_id")
        .and_then(Value::as_str)
        .expect("approval id")
        .to_string();
    let message_id = approval
        .pointer("/data/approval_message_id")
        .and_then(Value::as_i64)
        .expect("approval message id");
    Ok((approval_id, message_id, prepared))
}

async fn approve(bot: &FakeApprovalBot, approval_id: &str, message_id: i64) {
    bot.push_mutation_decision(approval_id, ApprovalAction::Approve, 700, 700, message_id).await;
}

#[tokio::test]
async fn folder_suggestions_tags_and_global_privacy_are_exact_and_recoverable() -> anyhow::Result<()>
{
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

    let suggestions = client
        .call_tool(CallToolRequestParams::new("get_suggested_folders"))
        .await?
        .structured_content
        .expect("suggested folders");
    assert_eq!(
        suggestions.pointer("/data/suggestions/0/description").and_then(Value::as_str),
        Some("Chats with unread messages")
    );
    assert_eq!(
        suggestions.pointer("/data/suggestions/0/folder/title").and_then(Value::as_str),
        Some("Unread")
    );
    assert_eq!(
        suggestions.pointer("/data/suggestions/0/folder/exclude_read").and_then(Value::as_bool),
        Some(true)
    );

    let initial = client
        .call_tool(CallToolRequestParams::new("get_global_privacy_settings"))
        .await?
        .structured_content
        .expect("global privacy settings");
    assert_eq!(initial.pointer("/data/keep_archived_unmuted").and_then(Value::as_bool), Some(true));
    assert_eq!(
        initial.pointer("/data/noncontact_peers_paid_stars").and_then(Value::as_str),
        Some("25")
    );
    assert_eq!(
        initial
            .pointer("/data/disallowed_gifts/disallow_limited_stargifts")
            .and_then(Value::as_bool),
        Some(true)
    );

    let unknown_outer = client
        .call_tool(CallToolRequestParams::new("set_global_privacy_settings").with_arguments(
            json!({ "hideReadMarks": true, "rawTl": true }).as_object().expect("object").clone(),
        ))
        .await;
    assert!(unknown_outer.is_err(), "unknown top-level fields must fail closed");
    let unknown_nested = client
        .call_tool(
            CallToolRequestParams::new("set_global_privacy_settings").with_arguments(
                json!({
                    "disallowedGifts": {
                        "disallowUnlimitedStargifts": false,
                        "disallowLimitedStargifts": false,
                        "disallowUniqueStargifts": false,
                        "disallowPremiumGifts": false,
                        "disallowStargiftsFromChannels": false,
                        "rawTl": true
                    }
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await;
    assert!(unknown_nested.is_err(), "unknown nested fields must fail closed");
    for arguments in [
        json!({}),
        json!({ "noncontactPeersPaidStars": "025" }),
        json!({
            "noncontactPeersPaidStars": "25",
            "clearNoncontactPeersPaidStars": true
        }),
        json!({
            "disallowedGifts": {
                "disallowUnlimitedStargifts": false,
                "disallowLimitedStargifts": false,
                "disallowUniqueStargifts": false,
                "disallowPremiumGifts": false,
                "disallowStargiftsFromChannels": false
            },
            "clearDisallowedGifts": true
        }),
    ] {
        assert_eq!(
            rejected_code(&client, "set_global_privacy_settings", arguments).await?,
            "INVALID_ARGUMENT"
        );
    }

    backend.make_next_dialog_folder_tags_outcome_uncertain().await;
    let (tags_id, tags_message_id, tags_prepared) =
        prepare_and_request(&client, "toggle_folder_tags", json!({ "enabled": true })).await?;
    assert_eq!(
        tags_prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("set_dialog_folder_tags")
    );
    assert_eq!(tags_prepared.pointer("/data/action/before").and_then(Value::as_bool), Some(false));
    approve(&bot, &tags_id, tags_message_id).await;
    wait_for_mutation_state(&state_db_path, &tags_id, MutationApprovalState::Applied).await?;
    let tags_applied = MutationLedger::new(state_db_path.clone())
        .get_approval(&tags_id)?
        .expect("applied folder tags");
    assert_eq!(tags_applied.automated_retry_count, 1);
    assert_eq!(backend.folder_privacy_call_counts().await, (1, 0));
    assert_eq!(
        rejected_code(&client, "toggle_folder_tags", json!({ "enabled": true })).await?,
        "NO_CHANGE"
    );

    backend.make_next_global_privacy_outcome_uncertain().await;
    let (privacy_id, privacy_message_id, privacy_prepared) = prepare_and_request(
        &client,
        "set_global_privacy_settings",
        json!({
            "archiveAndMuteNewNoncontactPeers": true,
            "hideReadMarks": true,
            "displayGiftsButton": false,
            "noncontactPeersPaidStars": "75",
            "disallowedGifts": {
                "disallowUnlimitedStargifts": true,
                "disallowLimitedStargifts": false,
                "disallowUniqueStargifts": true,
                "disallowPremiumGifts": true,
                "disallowStargiftsFromChannels": false
            }
        }),
    )
    .await?;
    assert_eq!(
        privacy_prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("set_global_privacy_settings")
    );
    assert_eq!(
        privacy_prepared
            .pointer("/data/action/before/keep_archived_unmuted")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        privacy_prepared
            .pointer("/data/action/target/noncontact_peers_paid_stars")
            .and_then(Value::as_str),
        Some("75")
    );
    approve(&bot, &privacy_id, privacy_message_id).await;
    wait_for_mutation_state(&state_db_path, &privacy_id, MutationApprovalState::Applied).await?;
    let privacy_applied = MutationLedger::new(state_db_path.clone())
        .get_approval(&privacy_id)?
        .expect("applied global privacy");
    assert_eq!(privacy_applied.automated_retry_count, 1);
    assert_eq!(backend.folder_privacy_call_counts().await, (1, 1));
    let privacy = backend.get_global_privacy_settings(None).await?;
    assert!(privacy.archive_and_mute_new_noncontact_peers);
    assert!(privacy.keep_archived_unmuted, "omitted field must be preserved");
    assert!(!privacy.keep_archived_folders, "omitted field must be preserved");
    assert!(privacy.hide_read_marks);
    assert!(!privacy.display_gifts_button);
    assert_eq!(privacy.noncontact_peers_paid_stars.as_deref(), Some("75"));
    assert!(privacy.disallowed_gifts.as_ref().is_some_and(|gifts| gifts.disallow_unique_stargifts
        && gifts.disallow_premium_gifts
        && !gifts.disallow_stargifts_from_channels));

    let (clear_id, clear_message_id, _) = prepare_and_request(
        &client,
        "set_global_privacy_settings",
        json!({
            "clearNoncontactPeersPaidStars": true,
            "clearDisallowedGifts": true
        }),
    )
    .await?;
    approve(&bot, &clear_id, clear_message_id).await;
    wait_for_mutation_state(&state_db_path, &clear_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.folder_privacy_call_counts().await, (1, 2));
    let cleared = backend.get_global_privacy_settings(None).await?;
    assert!(cleared.noncontact_peers_paid_stars.is_none());
    assert!(cleared.disallowed_gifts.is_none());

    let (stale_id, stale_message_id, _) = prepare_and_request(
        &client,
        "set_global_privacy_settings",
        json!({ "keepArchivedFolders": true }),
    )
    .await?;
    let mut third_state = cleared.clone();
    third_state.new_noncontact_peers_require_premium = true;
    backend.set_fixture_global_privacy_settings(third_state).await;
    approve(&bot, &stale_id, stale_message_id).await;
    wait_for_mutation_state(&state_db_path, &stale_id, MutationApprovalState::Failed).await?;
    let stale = MutationLedger::new(state_db_path.clone())
        .get_approval(&stale_id)?
        .expect("stale global privacy approval");
    assert_eq!(stale.last_error_code.as_deref(), Some("MUTATION_PRECONDITION_FAILED"));
    assert_eq!(backend.folder_privacy_call_counts().await, (1, 2));

    backend.set_fixture_global_privacy_settings(cleared.clone()).await;
    backend.make_next_global_privacy_outcome_uncertain().await;
    let (changed_id, changed_message_id, _) = prepare_and_request(
        &client,
        "set_global_privacy_settings",
        json!({ "keepArchivedFolders": true }),
    )
    .await?;
    approve(&bot, &changed_id, changed_message_id).await;
    wait_for_mutation_state(&state_db_path, &changed_id, MutationApprovalState::OutcomeUnknown)
        .await?;
    let mut changed_after_uncertain: GlobalPrivacySettingsSummary = cleared;
    changed_after_uncertain.keep_archived_folders = true;
    changed_after_uncertain.display_gifts_button = true;
    backend.set_fixture_global_privacy_settings(changed_after_uncertain).await;
    wait_for_error_code(&state_db_path, &changed_id, "MUTATION_OUTCOME_UNKNOWN_NO_RETRY").await?;
    assert_eq!(backend.folder_privacy_call_counts().await, (1, 3));

    let approval_surface =
        format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(approval_surface.contains("noncontact_peers_paid_stars: Some(\"75\")"));
    assert!(approval_surface.contains("disallow_stargifts_from_channels: false"));
    assert!(approval_surface.contains("SetDialogFolderTags"));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
