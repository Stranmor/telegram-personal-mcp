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
    for _ in 0..2_000 {
        let approval =
            MutationLedger::new(state_db_path.to_path_buf()).get_approval(approval_id)?;
        if approval.as_ref().map(|approval| approval.state) == Some(wanted) {
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
    anyhow::bail!("Business settings mutation did not reach {wanted:?}; last={last:?}")
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
    assert_eq!(rejected.is_error, Some(true), "{tool} must reject the input");
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
    assert_eq!(prepared.is_error, Some(false), "prepare {tool}: {:?}", prepared.structured_content);
    let prepared = prepared.structured_content.expect("prepared Business settings mutation");
    let mutation_id =
        prepared.pointer("/data/mutation_id").and_then(Value::as_str).expect("mutation id");
    let payload_sha256 =
        prepared.pointer("/data/payload_sha256").and_then(Value::as_str).expect("payload hash");
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
    assert_eq!(requested.is_error, Some(false), "request {tool}");
    let requested = requested.structured_content.expect("Business settings approval request");
    Ok((
        requested
            .pointer("/data/approval_id")
            .and_then(Value::as_str)
            .expect("approval id")
            .to_string(),
        requested
            .pointer("/data/approval_message_id")
            .and_then(Value::as_i64)
            .expect("approval message id"),
        prepared,
    ))
}

async fn approve(bot: &FakeApprovalBot, approval_id: &str, message_id: i64) {
    bot.push_mutation_decision(approval_id, ApprovalAction::Approve, 700, 700, message_id).await;
}

async fn account_profile(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
) -> anyhow::Result<Value> {
    let response = client.call_tool(CallToolRequestParams::new("get_account_profile")).await?;
    assert_eq!(response.is_error, Some(false));
    Ok(response.structured_content.expect("account profile"))
}

#[tokio::test]
async fn business_settings_are_exact_private_and_recover_without_duplicate_actuation()
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

    let tools = client.list_all_tools().await?;
    for name in [
        "set_business_hours",
        "set_business_location",
        "set_business_greeting",
        "set_business_away",
        "set_business_intro",
    ] {
        let tool = tools.iter().find(|tool| tool.name == name).expect("Business settings tool");
        assert_eq!(tool.input_schema.get("additionalProperties"), Some(&json!(false)));
        assert!(tool.output_schema.is_some());
        assert_eq!(tool.annotations.as_ref().and_then(|value| value.read_only_hint), Some(false));
        assert_eq!(tool.annotations.as_ref().and_then(|value| value.idempotent_hint), Some(true));
        assert_eq!(tool.annotations.as_ref().and_then(|value| value.destructive_hint), Some(false));
    }

    let initial = account_profile(&client).await?;
    assert_eq!(
        initial.pointer("/data/business/work_hours/config/timezone_id").and_then(Value::as_str),
        Some("Europe/Moscow")
    );
    assert_eq!(
        initial
            .pointer("/data/business/work_hours/config/weekly_open/0/start_minute")
            .and_then(Value::as_i64),
        Some(540)
    );
    assert_eq!(
        initial.pointer("/data/business/work_hours/open_now").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        initial.pointer("/data/business/location/address").and_then(Value::as_str),
        Some("Fixture office")
    );
    assert_eq!(
        initial.pointer("/data/business/greeting/no_activity_days").and_then(Value::as_i64),
        Some(7)
    );
    assert!(initial.pointer("/data/business/away").is_some_and(Value::is_null));
    assert_eq!(
        initial.pointer("/data/business/intro/title").and_then(Value::as_str),
        Some("Fixture support")
    );

    for (tool, arguments, expected) in [
        (
            "set_business_hours",
            json!({
                "timezone": "Europe/Moscow",
                "openNow": false,
                "schedule": [{ "day": "mon", "openFrom": "09:00", "openTo": "18:00" }]
            }),
            "INVALID_ARGUMENT",
        ),
        (
            "set_business_hours",
            json!({
                "timezone": "Europe/Moscow",
                "schedule": [{ "day": "mon", "open_from": "9:00", "open_to": "18:00" }]
            }),
            "INVALID_ARGUMENT",
        ),
        ("set_business_location", json!({ "address": "x".repeat(97) }), "INVALID_ARGUMENT"),
        (
            "set_business_location",
            json!({ "address": "Office", "latitude": 55.75 }),
            "INVALID_ARGUMENT",
        ),
        (
            "set_business_greeting",
            json!({ "shortcutId": 7, "noActivityDays": 1 }),
            "INVALID_ARGUMENT",
        ),
        (
            "set_business_greeting",
            json!({ "shortcutId": 7, "includeUsers": ["@alex"], "excludeUsers": ["@alex"] }),
            "INVALID_ARGUMENT",
        ),
        (
            "set_business_greeting",
            json!({ "shortcutId": 999, "noActivityDays": 7 }),
            "QUICK_REPLY_SHORTCUT_NOT_FOUND",
        ),
        (
            "set_business_away",
            json!({ "shortcutId": 7, "schedule": "custom", "customFrom": 1_800_000_000 }),
            "INVALID_ARGUMENT",
        ),
        (
            "set_business_away",
            json!({
                "shortcutId": 7,
                "schedule": "custom",
                "customFrom": 1_800_000_000,
                "customTo": 1_799_999_999
            }),
            "INVALID_ARGUMENT",
        ),
        (
            "set_business_intro",
            json!({
                "title": "Hello",
                "description": "Tell us what you need",
                "stickerHandle": "sticker_invalid"
            }),
            "INVALID_ARGUMENT",
        ),
        (
            "set_business_intro",
            json!({
                "title": "Hello",
                "description": "Tell us what you need",
                "stickerSetShortName": "fixture_reactions",
                "stickerHandle": "sticker_00000000000000000000000000000000"
            }),
            "STICKER_NOT_FOUND",
        ),
    ] {
        assert_eq!(rejected_code(&client, tool, arguments).await?, expected);
    }
    let raw_document_credentials = client
        .call_tool(
            CallToolRequestParams::new("set_business_intro").with_arguments(
                json!({
                    "title": "Hello",
                    "description": "Tell us what you need",
                    "document_id": "9101",
                    "access_hash": "secret-like"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await;
    assert!(raw_document_credentials.is_err());

    let sticker_set = client
        .call_tool(CallToolRequestParams::new("get_sticker_set").with_arguments(
            json!({ "short_name": "fixture_reactions" }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(sticker_set.is_error, Some(false));
    let sticker_handle = sticker_set
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/stickers/0/sticker_handle"))
        .and_then(Value::as_str)
        .expect("opaque sticker handle")
        .to_string();
    assert!(sticker_handle.starts_with("sticker_"));

    let (hours_id, hours_message_id, hours_prepared) = prepare_and_request(
        &client,
        "set_business_hours",
        json!({
            "timezone": "Europe/Moscow",
            "schedule": [
                { "day": "mon", "openFrom": "09:00", "openTo": "12:00" },
                { "day": "mon", "openFrom": "12:00", "openTo": "18:00" },
                { "day": "sun", "openFrom": "22:00", "openTo": "02:00" }
            ]
        }),
    )
    .await?;
    assert_eq!(
        hours_prepared.pointer("/data/action/kind").and_then(Value::as_str),
        Some("set_business_work_hours")
    );
    assert_eq!(
        hours_prepared
            .pointer("/data/action/target/weekly_open")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        hours_prepared
            .pointer("/data/action/target/weekly_open/1/end_minute")
            .and_then(Value::as_i64),
        Some(10_200)
    );
    assert!(hours_prepared.pointer("/data/action/target/open_now").is_none());
    let hours_card = bot.sent_mutation_cards().await.last().expect("hours card").clone();
    assert!(hours_card.rendered_text.contains("РАБОЧИХ ЧАСОВ TELEGRAM BUSINESS"));
    assert!(hours_card.rendered_text.contains("open_now не принимается"));
    approve(&bot, &hours_id, hours_message_id).await;
    wait_for_mutation_state(&state_db_path, &hours_id, MutationApprovalState::Applied).await?;

    let (location_id, location_message_id, location_prepared) = prepare_and_request(
        &client,
        "set_business_location",
        json!({
            "address": "Release office",
            "latitude": 55.755826,
            "longitude": 37.6173
        }),
    )
    .await?;
    assert_eq!(
        location_prepared.pointer("/data/action/target/point/latitude_e7").and_then(Value::as_i64),
        Some(557_558_260)
    );
    assert_eq!(
        location_prepared.pointer("/data/action/target/point/longitude_e7").and_then(Value::as_i64),
        Some(376_173_000)
    );
    let location_card = bot.sent_mutation_cards().await.last().expect("location card").clone();
    assert!(location_card.rendered_text.contains("Release office"));
    assert!(location_card.rendered_text.contains("latitude_e7=557558260"));
    approve(&bot, &location_id, location_message_id).await;
    wait_for_mutation_state(&state_db_path, &location_id, MutationApprovalState::Applied).await?;

    let (greeting_id, greeting_message_id, greeting_prepared) = prepare_and_request(
        &client,
        "set_business_greeting",
        json!({ "shortcutId": 7, "audience": "all_new", "noActivityDays": 14 }),
    )
    .await?;
    assert_eq!(
        greeting_prepared
            .pointer("/data/action/target/recipients/new_chats")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        greeting_prepared
            .pointer("/data/action/target/recipients/contacts")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        greeting_prepared
            .pointer("/data/action/target/recipients/non_contacts")
            .and_then(Value::as_bool),
        Some(false)
    );
    let greeting_card = bot.sent_mutation_cards().await.last().expect("greeting card").clone();
    assert!(greeting_card.rendered_text.contains("ПРИВЕТСТВИЯ TELEGRAM BUSINESS"));
    assert!(greeting_card.rendered_text.contains("Без активности: 14 дней"));
    approve(&bot, &greeting_id, greeting_message_id).await;
    wait_for_mutation_state(&state_db_path, &greeting_id, MutationApprovalState::Applied).await?;

    let (away_id, away_message_id, away_prepared) = prepare_and_request(
        &client,
        "set_business_away",
        json!({
            "shortcutId": 7,
            "schedule": "custom",
            "customFrom": 1_800_000_000,
            "customTo": 1_800_003_600,
            "offlineOnly": false,
            "audience": "non_contacts"
        }),
    )
    .await?;
    assert_eq!(
        away_prepared.pointer("/data/action/target/schedule/kind").and_then(Value::as_str),
        Some("custom")
    );
    assert_eq!(
        away_prepared
            .pointer("/data/action/target/recipients/non_contacts")
            .and_then(Value::as_bool),
        Some(true)
    );
    let away_card = bot.sent_mutation_cards().await.last().expect("away card").clone();
    assert!(away_card.rendered_text.contains("AWAY-СООБЩЕНИЯ TELEGRAM BUSINESS"));
    assert!(away_card.rendered_text.contains("Unix 1800000000..1800003600"));
    approve(&bot, &away_id, away_message_id).await;
    wait_for_mutation_state(&state_db_path, &away_id, MutationApprovalState::Applied).await?;

    let (intro_id, intro_message_id, intro_prepared) = prepare_and_request(
        &client,
        "set_business_intro",
        json!({
            "title": "Release support",
            "description": "Choose the request type",
            "stickerSetShortName": "fixture_reactions",
            "stickerHandle": sticker_handle
        }),
    )
    .await?;
    assert_eq!(
        intro_prepared
            .pointer("/data/action/target/sticker_set_short_name")
            .and_then(Value::as_str),
        Some("fixture_reactions")
    );
    assert!(intro_prepared.pointer("/data/action/target/access_hash").is_none());
    assert!(intro_prepared.pointer("/data/action/target/file_reference").is_none());
    let intro_card = bot.sent_mutation_cards().await.last().expect("intro card").clone();
    assert!(intro_card.rendered_text.contains("INTRO TELEGRAM BUSINESS"));
    assert!(
        intro_card
            .rendered_text
            .contains("Raw document_id, access_hash и file_reference не принимаются")
    );
    approve(&bot, &intro_id, intro_message_id).await;
    wait_for_mutation_state(&state_db_path, &intro_id, MutationApprovalState::Applied).await?;

    assert_eq!(backend.business_settings_call_counts().await, (1, 1, 1, 1, 1));
    let configured = account_profile(&client).await?;
    assert_eq!(
        configured
            .pointer("/data/business/work_hours/config/weekly_open/1/end_minute")
            .and_then(Value::as_i64),
        Some(10_200)
    );
    assert_eq!(
        configured.pointer("/data/business/location/address").and_then(Value::as_str),
        Some("Release office")
    );
    assert_eq!(
        configured.pointer("/data/business/greeting/no_activity_days").and_then(Value::as_i64),
        Some(14)
    );
    assert_eq!(
        configured.pointer("/data/business/away/schedule/kind").and_then(Value::as_str),
        Some("custom")
    );
    assert_eq!(
        configured.pointer("/data/business/intro/title").and_then(Value::as_str),
        Some("Release support")
    );

    backend.make_next_business_work_hours_outcome_uncertain().await;
    let (clear_hours_id, clear_hours_message_id, _) =
        prepare_and_request(&client, "set_business_hours", json!({ "clear": true })).await?;
    approve(&bot, &clear_hours_id, clear_hours_message_id).await;

    backend.make_next_business_location_outcome_uncertain().await;
    let (clear_location_id, clear_location_message_id, _) =
        prepare_and_request(&client, "set_business_location", json!({ "clear": true })).await?;
    approve(&bot, &clear_location_id, clear_location_message_id).await;

    backend.make_next_business_greeting_outcome_uncertain().await;
    let (clear_greeting_id, clear_greeting_message_id, _) =
        prepare_and_request(&client, "set_business_greeting", json!({ "clear": true })).await?;
    approve(&bot, &clear_greeting_id, clear_greeting_message_id).await;

    backend.make_next_business_away_outcome_uncertain().await;
    let (clear_away_id, clear_away_message_id, _) =
        prepare_and_request(&client, "set_business_away", json!({ "clear": true })).await?;
    approve(&bot, &clear_away_id, clear_away_message_id).await;

    backend.make_next_business_intro_outcome_uncertain().await;
    let (clear_intro_id, clear_intro_message_id, _) =
        prepare_and_request(&client, "set_business_intro", json!({ "clear": true })).await?;
    approve(&bot, &clear_intro_id, clear_intro_message_id).await;

    for approval_id in
        [&clear_hours_id, &clear_location_id, &clear_greeting_id, &clear_away_id, &clear_intro_id]
    {
        wait_for_mutation_state(&state_db_path, approval_id, MutationApprovalState::Applied)
            .await?;
        let recovered = MutationLedger::new(state_db_path.clone())
            .get_approval(approval_id)?
            .expect("recovered Business setting approval");
        assert_eq!(recovered.automated_retry_count, 1);
    }
    assert_eq!(backend.business_settings_call_counts().await, (2, 2, 2, 2, 2));

    let cleared = account_profile(&client).await?;
    for field in ["work_hours", "location", "greeting", "away", "intro"] {
        assert!(
            cleared.pointer(&format!("/data/business/{field}")).is_some_and(Value::is_null),
            "{field} must be cleared"
        );
    }

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
