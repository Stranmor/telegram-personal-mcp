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
    ApprovalAction, ApprovalPolicy, ChatSummary, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApprovalState, MutationLedger, PeerColorSummary,
    PersonalChannelSummary, TelegramBackend,
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

fn collectible(id: i64) -> PeerColorSummary {
    PeerColorSummary::Collectible {
        collectible_id: id.to_string(),
        gift_emoji_id: (id + 1).to_string(),
        background_emoji_id: (id + 2).to_string(),
        accent_color: 0x11_22_33,
        colors: vec![0x11_22_33, 0x44_55_66],
        dark_accent_color: Some(0x01_02_03),
        dark_colors: Some(vec![0x01_02_03, 0x04_05_06]),
    }
}

fn personal_channel(chat_id: &str, title: &str, message_id: i32) -> PersonalChannelSummary {
    PersonalChannelSummary {
        chat: ChatSummary {
            chat_id: chat_id.to_string(),
            title: title.to_string(),
            kind: "channel".to_string(),
            username: None,
            participants_count: Some(1),
            creator: true,
            verified: false,
            restricted: false,
        },
        message_id,
    }
}

#[tokio::test]
async fn profile_colors_and_personal_channel_are_exact_readback_bound_and_recoverable()
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

    let initial = client
        .call_tool(CallToolRequestParams::new("get_account_profile"))
        .await?
        .structured_content
        .expect("initial account profile");
    assert_eq!(initial.pointer("/data/name_color/kind").and_then(Value::as_str), Some("simple"));
    assert_eq!(initial.pointer("/data/name_color/color").and_then(Value::as_i64), Some(3));
    assert!(initial.pointer("/data/profile_color").is_none_or(Value::is_null));
    assert!(initial.pointer("/data/personal_channel").is_none_or(Value::is_null));

    let closed_schema = client
        .call_tool(
            CallToolRequestParams::new("set_profile_color").with_arguments(
                json!({ "forProfile": false, "color": 4, "rawTl": true })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await;
    assert!(closed_schema.is_err(), "unknown fields must fail at the JSON-RPC boundary");
    for arguments in
        [json!({ "color": -1 }), json!({ "color": 21 }), json!({ "backgroundEmojiId": "001" })]
    {
        assert_eq!(
            rejected_code(&client, "set_profile_color", arguments).await?,
            "INVALID_ARGUMENT"
        );
    }
    for arguments in [
        json!({}),
        json!({ "channelId": "-100505", "clear": true }),
        json!({ "channelId": "-202" }),
    ] {
        assert_eq!(
            rejected_code(&client, "set_personal_channel", arguments).await?,
            "INVALID_ARGUMENT"
        );
    }

    backend.make_next_account_color_outcome_uncertain().await;
    let (name_id, name_message_id, name_prepared) = prepare_and_request(
        &client,
        "set_profile_color",
        json!({ "forProfile": false, "color": 7, "backgroundEmojiId": "9001" }),
    )
    .await?;
    assert_eq!(
        name_prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("set_account_peer_color")
    );
    assert_eq!(
        name_prepared.pointer("/data/action/before/kind").and_then(Value::as_str),
        Some("simple")
    );
    assert_eq!(
        name_prepared.pointer("/data/action/background_emoji_id").and_then(Value::as_str),
        Some("9001")
    );
    approve(&bot, &name_id, name_message_id).await;
    wait_for_mutation_state(&state_db_path, &name_id, MutationApprovalState::Applied).await?;
    let applied = MutationLedger::new(state_db_path.clone())
        .get_approval(&name_id)?
        .expect("applied name color");
    assert_eq!(applied.automated_retry_count, 1);
    assert_eq!(backend.account_identity_call_counts().await, (1, 0));
    let name_result = serde_json::to_value(applied.result.expect("name color receipt"))?;
    assert_eq!(
        name_result.pointer("/kind").and_then(Value::as_str),
        Some("account_peer_color_updated")
    );
    assert_eq!(
        name_result.pointer("/profile/name_color/background_emoji_id").and_then(Value::as_str),
        Some("9001")
    );
    assert_eq!(
        rejected_code(
            &client,
            "set_profile_color",
            json!({ "color": 7, "backgroundEmojiId": "9001" })
        )
        .await?,
        "NO_CHANGE"
    );

    let (profile_id, profile_message_id, _) = prepare_and_request(
        &client,
        "set_profile_color",
        json!({ "forProfile": true, "color": 12, "backgroundEmojiId": "9012" }),
    )
    .await?;
    approve(&bot, &profile_id, profile_message_id).await;
    wait_for_mutation_state(&state_db_path, &profile_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.account_identity_call_counts().await, (2, 0));

    for arguments in [json!({}), json!({ "forProfile": true })] {
        let (approval_id, message_id, _) =
            prepare_and_request(&client, "set_profile_color", arguments).await?;
        approve(&bot, &approval_id, message_id).await;
        wait_for_mutation_state(&state_db_path, &approval_id, MutationApprovalState::Applied)
            .await?;
    }
    assert_eq!(backend.account_identity_call_counts().await, (4, 0));
    let cleared = backend.get_account_profile(None).await?;
    assert!(cleared.name_color.is_none());
    assert!(cleared.profile_color.is_none());

    backend.set_fixture_account_peer_color(true, Some(collectible(9_100))).await;
    let (stale_color_id, stale_color_message_id, stale_color_prepared) =
        prepare_and_request(&client, "set_profile_color", json!({ "forProfile": true })).await?;
    assert_eq!(
        stale_color_prepared.pointer("/data/action/before/kind").and_then(Value::as_str),
        Some("collectible")
    );
    assert_eq!(
        stale_color_prepared.pointer("/data/action/before/dark_colors/1").and_then(Value::as_i64),
        Some(0x04_05_06)
    );
    backend.set_fixture_account_peer_color(true, Some(collectible(9_200))).await;
    approve(&bot, &stale_color_id, stale_color_message_id).await;
    wait_for_mutation_state(&state_db_path, &stale_color_id, MutationApprovalState::Failed).await?;
    let stale_color = MutationLedger::new(state_db_path.clone())
        .get_approval(&stale_color_id)?
        .expect("stale color approval");
    assert_eq!(stale_color.last_error_code.as_deref(), Some("MUTATION_PRECONDITION_FAILED"));
    assert_eq!(backend.account_identity_call_counts().await, (4, 0));

    backend.set_fixture_account_peer_color(false, None).await;
    backend.make_next_account_color_outcome_uncertain().await;
    let (changed_color_id, changed_color_message_id, _) =
        prepare_and_request(&client, "set_profile_color", json!({ "color": 9 })).await?;
    approve(&bot, &changed_color_id, changed_color_message_id).await;
    wait_for_mutation_state(
        &state_db_path,
        &changed_color_id,
        MutationApprovalState::OutcomeUnknown,
    )
    .await?;
    backend.set_fixture_account_peer_color(false, Some(collectible(9_300))).await;
    wait_for_error_code(&state_db_path, &changed_color_id, "MUTATION_OUTCOME_UNKNOWN_NO_RETRY")
        .await?;
    assert_eq!(backend.account_identity_call_counts().await, (5, 0));

    backend.make_next_personal_channel_outcome_uncertain().await;
    let (channel_id, channel_message_id, channel_prepared) =
        prepare_and_request(&client, "set_personal_channel", json!({ "channelId": "-100505" }))
            .await?;
    assert_eq!(
        channel_prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("set_account_personal_channel")
    );
    assert_eq!(
        channel_prepared.pointer("/data/action/channel/chat_id").and_then(Value::as_str),
        Some("-100505")
    );
    approve(&bot, &channel_id, channel_message_id).await;
    wait_for_mutation_state(&state_db_path, &channel_id, MutationApprovalState::Applied).await?;
    let channel_applied = MutationLedger::new(state_db_path.clone())
        .get_approval(&channel_id)?
        .expect("applied personal channel");
    assert_eq!(channel_applied.automated_retry_count, 1);
    assert_eq!(backend.account_identity_call_counts().await, (5, 1));
    let channel_result = serde_json::to_value(channel_applied.result.expect("channel receipt"))?;
    assert_eq!(
        channel_result.pointer("/profile/personal_channel/chat/chat_id").and_then(Value::as_str),
        Some("-100505")
    );
    assert_eq!(
        channel_result.pointer("/profile/personal_channel/message_id").and_then(Value::as_i64),
        Some(50)
    );
    assert_eq!(
        rejected_code(&client, "set_personal_channel", json!({ "channelId": "-100505" })).await?,
        "NO_CHANGE"
    );

    let (clear_id, clear_message_id, _) =
        prepare_and_request(&client, "set_personal_channel", json!({ "clear": true })).await?;
    approve(&bot, &clear_id, clear_message_id).await;
    wait_for_mutation_state(&state_db_path, &clear_id, MutationApprovalState::Applied).await?;
    assert_eq!(backend.account_identity_call_counts().await, (5, 2));
    assert!(backend.get_account_profile(None).await?.personal_channel.is_none());

    let (stale_channel_id, stale_channel_message_id, _) =
        prepare_and_request(&client, "set_personal_channel", json!({ "channelId": "-100505" }))
            .await?;
    backend
        .set_fixture_personal_channel(Some(personal_channel(
            "-100999",
            "Changed after preparation",
            77,
        )))
        .await;
    approve(&bot, &stale_channel_id, stale_channel_message_id).await;
    wait_for_mutation_state(&state_db_path, &stale_channel_id, MutationApprovalState::Failed)
        .await?;
    let stale_channel = MutationLedger::new(state_db_path.clone())
        .get_approval(&stale_channel_id)?
        .expect("stale personal-channel approval");
    assert_eq!(stale_channel.last_error_code.as_deref(), Some("MUTATION_PRECONDITION_FAILED"));
    assert_eq!(backend.account_identity_call_counts().await, (5, 2));

    backend.set_fixture_personal_channel(None).await;
    backend.make_next_personal_channel_outcome_uncertain().await;
    let (changed_channel_id, changed_channel_message_id, _) =
        prepare_and_request(&client, "set_personal_channel", json!({ "channelId": "-100505" }))
            .await?;
    approve(&bot, &changed_channel_id, changed_channel_message_id).await;
    wait_for_mutation_state(
        &state_db_path,
        &changed_channel_id,
        MutationApprovalState::OutcomeUnknown,
    )
    .await?;
    backend
        .set_fixture_personal_channel(Some(personal_channel(
            "-100998",
            "Created after uncertain receipt",
            88,
        )))
        .await;
    wait_for_error_code(&state_db_path, &changed_channel_id, "MUTATION_OUTCOME_UNKNOWN_NO_RETRY")
        .await?;
    assert_eq!(backend.account_identity_call_counts().await, (5, 3));

    let approval_surface =
        format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(approval_surface.contains("9100"));
    assert!(approval_surface.contains("Ops Supergroup"));
    assert!(approval_surface.contains("9001"));

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
