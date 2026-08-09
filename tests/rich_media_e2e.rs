use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, MutationApproval, MutationApprovalState, MutationLedger,
};

async fn wait_for_applied(
    state_db_path: &std::path::Path,
    approval_id: &str,
) -> anyhow::Result<MutationApproval> {
    let ledger = MutationLedger::new(state_db_path.to_path_buf());
    let mut last = None;
    for _ in 0..900 {
        let approval = ledger.get_approval(approval_id)?;
        if approval
            .as_ref()
            .is_some_and(|approval| approval.state == MutationApprovalState::Applied)
        {
            return Ok(approval.expect("applied approval"));
        }
        last = approval.map(|approval| {
            (approval.state, approval.last_error_code, approval.automated_retry_count)
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    anyhow::bail!("rich-media mutation did not become applied; last={last:?}")
}

async fn prepare_and_approve(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    bot: &FakeApprovalBot,
    state_db_path: &std::path::Path,
    tool_name: &'static str,
    arguments: Value,
) -> anyhow::Result<(Value, String, i64, Value)> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new(tool_name)
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false), "prepare {tool_name}");
    let prepared = prepared.structured_content.expect("prepared rich-media mutation");
    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "mutation_id": prepared
                        .pointer("/data/mutation_id")
                        .and_then(Value::as_str)
                        .expect("mutation id"),
                    "approved_payload_sha256": prepared
                        .pointer("/data/payload_sha256")
                        .and_then(Value::as_str)
                        .expect("payload hash")
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(requested.is_error, Some(false), "approval request {tool_name}");
    let requested = requested.structured_content.expect("approval request");
    let approval_id = requested
        .pointer("/data/approval_id")
        .and_then(Value::as_str)
        .expect("approval id")
        .to_string();
    let approval_message_id = requested
        .pointer("/data/approval_message_id")
        .and_then(Value::as_i64)
        .expect("approval message id");
    bot.push_mutation_decision(
        &approval_id,
        ApprovalAction::Approve,
        700,
        700,
        approval_message_id,
    )
    .await;
    let approval = wait_for_applied(state_db_path, &approval_id).await?;
    let result = serde_json::to_value(approval.result.expect("rich-media result"))?;
    Ok((prepared, approval_id, approval_message_id, result))
}

#[tokio::test]
async fn rich_media_catalog_validation_approval_readback_and_recovery_are_e2e() -> anyhow::Result<()>
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

    let catalog = client.call_tool(CallToolRequestParams::new("get_dice_emojis")).await?;
    assert_eq!(catalog.is_error, Some(false));
    let catalog = catalog.structured_content.expect("dice catalog");
    assert_eq!(catalog.pointer("/data/not_modified").and_then(Value::as_bool), Some(false));
    assert_eq!(catalog.pointer("/data/hash").and_then(Value::as_i64), Some(12_345));
    assert_eq!(catalog.pointer("/data/returned_count").and_then(Value::as_u64), Some(6));
    assert_eq!(catalog.pointer("/data/emojis/0").and_then(Value::as_str), Some("🎲"));
    let unchanged = client
        .call_tool(
            CallToolRequestParams::new("get_dice_emojis")
                .with_arguments(json!({ "hash": 12_345 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(
        unchanged
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/not_modified"))
            .and_then(Value::as_bool),
        Some(true)
    );

    for (tool_name, arguments) in [
        ("send_location", json!({ "chat_id": "@alex", "latitude": 91.0, "longitude": 37.0 })),
        (
            "send_venue",
            json!({
                "chat_id": "@alex",
                "latitude": 55.7,
                "longitude": 37.6,
                "title": "Office",
                "address": "Moscow",
                "provider": "",
                "venue_id": "office-1",
                "venue_type": "office"
            }),
        ),
        (
            "send_contact",
            json!({
                "chat_id": "@alex",
                "phone_number": "+12abc",
                "first_name": "Alex"
            }),
        ),
        ("send_dice", json!({ "chat_id": "@alex", "emoji": "🧨" })),
    ] {
        let invalid = client
            .call_tool(
                CallToolRequestParams::new(tool_name)
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(invalid.is_error, Some(true), "invalid {tool_name}");
    }

    let (location, location_approval, location_card, location_result) = prepare_and_approve(
        &client,
        &bot,
        &state_db_path,
        "send_location",
        json!({
            "chat_id": "@alex",
            "latitude": 55.7558264,
            "longitude": 37.6172999,
            "accuracy_radius_m": 25,
            "caption": "Exact meeting point",
            "reply_to_message_id": 11,
            "silent": true,
            "noforwards": true
        }),
    )
    .await?;
    assert_eq!(
        location.pointer("/data/action/kind").and_then(Value::as_str),
        Some("send_rich_media")
    );
    assert_eq!(
        location.pointer("/data/action/media/kind").and_then(Value::as_str),
        Some("location")
    );
    assert_eq!(
        location.pointer("/data/action/media/point/latitude_e7").and_then(Value::as_i64),
        Some(557_558_264)
    );
    assert_eq!(
        location.pointer("/data/action/media/point/longitude_e7").and_then(Value::as_i64),
        Some(376_172_999)
    );
    assert_eq!(location_result.pointer("/kind").and_then(Value::as_str), Some("rich_media_sent"));
    assert_eq!(
        location_result.pointer("/media/media/kind").and_then(Value::as_str),
        Some("location")
    );

    let (_, _, _, venue_result) = prepare_and_approve(
        &client,
        &bot,
        &state_db_path,
        "send_venue",
        json!({
            "chat_id": "@alex",
            "latitude": 55.751244,
            "longitude": 37.618423,
            "title": "Release room",
            "address": "Red Square 1",
            "provider": "foursquare",
            "venue_id": "release-room-1",
            "venue_type": "office",
            "caption": "Venue payload"
        }),
    )
    .await?;
    assert_eq!(venue_result.pointer("/media/media/kind").and_then(Value::as_str), Some("venue"));
    assert_eq!(
        venue_result.pointer("/media/media/provider").and_then(Value::as_str),
        Some("foursquare")
    );

    let (_, _, _, contact_result) = prepare_and_approve(
        &client,
        &bot,
        &state_db_path,
        "send_contact",
        json!({
            "chat_id": "@alex",
            "phone_number": "+12025550123",
            "first_name": "Alex",
            "last_name": "Fixture",
            "vcard": "BEGIN:VCARD\nVERSION:3.0\nFN:Alex Fixture\nEND:VCARD",
            "caption": "Exact contact"
        }),
    )
    .await?;
    assert_eq!(
        contact_result.pointer("/media/media/phone_number").and_then(Value::as_str),
        Some("+12025550123")
    );

    let (_, _, _, dice_result) = prepare_and_approve(
        &client,
        &bot,
        &state_db_path,
        "send_dice",
        json!({
            "chat_id": "@alex",
            "emoji": "🎯",
            "reply_to_message_id": 11,
            "silent": true,
            "noforwards": true
        }),
    )
    .await?;
    assert_eq!(dice_result.pointer("/media/media/kind").and_then(Value::as_str), Some("dice"));
    assert_eq!(dice_result.pointer("/media/dice_value").and_then(Value::as_i64), Some(4));
    assert_eq!(backend.rich_media_send_call_count().await, 4);

    backend.make_next_rich_media_send_outcome_uncertain().await;
    let (_, _, _, recovered) = prepare_and_approve(
        &client,
        &bot,
        &state_db_path,
        "send_location",
        json!({
            "chat_id": "@alex",
            "latitude": 40.7128,
            "longitude": -74.006,
            "live_period_seconds": 600,
            "heading_degrees": 270,
            "proximity_notification_radius_m": 150,
            "caption": "Recovered without duplicate"
        }),
    )
    .await?;
    assert_eq!(recovered.pointer("/media/media/kind").and_then(Value::as_str), Some("location"));
    assert_eq!(
        recovered.pointer("/media/media/live_period_seconds").and_then(Value::as_i64),
        Some(600)
    );
    assert_eq!(backend.rich_media_send_call_count().await, 5);
    assert_eq!(backend.sent_messages().await.len(), 5);

    bot.push_mutation_decision(
        &location_approval,
        ApprovalAction::Approve,
        700,
        700,
        location_card,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(backend.rich_media_send_call_count().await, 5);

    let location_message_id = location_result
        .pointer("/message_id")
        .and_then(Value::as_i64)
        .expect("location message id");
    let location_message = client
        .call_tool(
            CallToolRequestParams::new("get_message").with_arguments(
                json!({ "chat_id": "@alex", "message_id": location_message_id })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(
        location_message
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/message/media_type"))
            .and_then(Value::as_str),
        Some("geo")
    );

    let cards = format!("{:?}\n{:?}", bot.sent_mutation_cards().await, bot.mutation_edits().await);
    assert!(cards.contains("557558264"));
    assert!(cards.contains("foursquare"));
    assert!(cards.contains("+12025550123"));
    assert!(cards.contains("🎯"));
    assert!(!cards.contains("access_hash"));
    assert!(!cards.contains("file_reference"));

    client.cancel().await?;
    server_task.abort();
    worker.abort();
    Ok(())
}
