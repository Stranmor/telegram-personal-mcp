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
use telegram_personal_mcp::{FakeTelegramBackend, McpServer, McpServerSettings};

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

#[tokio::test]
async fn group_calls_are_typed_filterable_paginated_and_schema_closed() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let temp = tempfile::tempdir()?;
    let server = McpServer::with_settings(
        backend,
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
    );
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let tools = client.list_all_tools().await?;
    for name in ["get_group_call", "get_group_call_participants"] {
        let tool = tools.iter().find(|tool| tool.name == name).expect("group-call tool");
        assert_eq!(tool.input_schema.get("additionalProperties"), Some(&json!(false)));
        assert!(tool.output_schema.is_some());
        assert_eq!(tool.annotations.as_ref().and_then(|value| value.read_only_hint), Some(true));
    }

    let metadata_only = client
        .call_tool(
            CallToolRequestParams::new("get_group_call")
                .with_arguments(json!({ "chat": "-100505" }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(metadata_only.is_error, Some(false));
    let metadata_only = metadata_only.structured_content.expect("group-call metadata");
    assert_eq!(metadata_only.pointer("/data/chat_id").and_then(Value::as_str), Some("-100505"));
    assert_eq!(metadata_only.pointer("/data/call/kind").and_then(Value::as_str), Some("active"));
    assert_eq!(metadata_only.pointer("/data/call/call_id").and_then(Value::as_str), Some("1001"));
    assert_eq!(
        metadata_only.pointer("/data/call/participants_count").and_then(Value::as_i64),
        Some(3)
    );
    assert_eq!(
        metadata_only.pointer("/data/call/title").and_then(Value::as_str),
        Some("Release stand-up")
    );
    assert_eq!(
        metadata_only.pointer("/data/call/record_video_active").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        metadata_only.pointer("/data/call/send_paid_messages_stars").and_then(Value::as_str),
        Some("25")
    );
    assert_eq!(
        metadata_only.pointer("/data/call/default_send_as").and_then(Value::as_str),
        Some("-100505")
    );
    assert!(metadata_only.pointer("/data/call/access_hash").is_none());
    assert_eq!(
        metadata_only.pointer("/data/participants").and_then(Value::as_array).map(Vec::len),
        Some(0)
    );
    assert!(metadata_only.pointer("/data/participants_next_offset").is_some_and(Value::is_null));

    let initial_page = client
        .call_tool(CallToolRequestParams::new("get_group_call").with_arguments(
            json!({ "chatId": "-100505", "limit": 1 }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(initial_page.is_error, Some(false));
    let initial_page = initial_page.structured_content.expect("initial participant page");
    assert_eq!(
        initial_page.pointer("/data/participants/0/peer_ref").and_then(Value::as_str),
        Some("101")
    );
    assert_eq!(
        initial_page.pointer("/data/participants/0/raise_hand_rating").and_then(Value::as_str),
        Some("42")
    );
    assert_eq!(
        initial_page.pointer("/data/participants/0/paid_stars_total").and_then(Value::as_str),
        Some("5")
    );
    assert_eq!(
        initial_page.pointer("/data/participants/0/video/endpoint").and_then(Value::as_str),
        Some("video:101")
    );
    assert_eq!(
        initial_page
            .pointer("/data/participants/0/video/source_groups/0/semantics")
            .and_then(Value::as_str),
        Some("SIM")
    );
    assert_eq!(
        initial_page.pointer("/data/participants_next_offset").and_then(Value::as_str),
        Some("1")
    );

    let participants = client
        .call_tool(
            CallToolRequestParams::new("get_group_call_participants")
                .with_arguments(json!({ "chat_id": "-100505" }).as_object().unwrap().clone()),
        )
        .await?;
    assert_eq!(participants.is_error, Some(false));
    let participants = participants.structured_content.expect("group-call participants");
    assert_eq!(participants.pointer("/data/count").and_then(Value::as_i64), Some(2));
    assert_eq!(participants.pointer("/data/version").and_then(Value::as_i64), Some(7));
    assert_eq!(
        participants.pointer("/data/participants").and_then(Value::as_array).map(Vec::len),
        Some(2)
    );
    assert_eq!(
        participants.pointer("/data/participants/1/self_participant").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        participants.pointer("/data/participants/1/presentation/paused").and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        participants
            .pointer("/data/related_users")
            .and_then(Value::as_array)
            .is_some_and(|users| users.len() >= 2)
    );

    let peer_filtered = client
        .call_tool(CallToolRequestParams::new("get_group_call_participants").with_arguments(
            json!({ "chat": "-100505", "ids": ["@alex"] }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(peer_filtered.is_error, Some(false));
    assert_eq!(
        peer_filtered
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/participants/0/peer_ref"))
            .and_then(Value::as_str),
        Some("101")
    );

    let source_filtered = client
        .call_tool(CallToolRequestParams::new("get_group_call_participants").with_arguments(
            json!({ "chatId": "-100505", "sources": [222] }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(source_filtered.is_error, Some(false));
    assert_eq!(
        source_filtered
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/participants/0/peer_ref"))
            .and_then(Value::as_str),
        Some("303")
    );

    let first_page = client
        .call_tool(CallToolRequestParams::new("get_group_call_participants").with_arguments(
            json!({ "chat": "-100505", "offset": "", "limit": 1 }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(first_page.is_error, Some(false));
    assert_eq!(
        first_page
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/next_offset"))
            .and_then(Value::as_str),
        Some("1")
    );
    let second_page = client
        .call_tool(CallToolRequestParams::new("get_group_call_participants").with_arguments(
            json!({ "chat": "-100505", "offset": "1", "limit": 1 }).as_object().unwrap().clone(),
        ))
        .await?;
    assert_eq!(second_page.is_error, Some(false));
    assert_eq!(
        second_page
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/participants/0/peer_ref"))
            .and_then(Value::as_str),
        Some("303")
    );
    assert!(
        second_page
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/next_offset"))
            .is_some_and(Value::is_null)
    );

    assert_eq!(
        rejected_code(&client, "get_group_call", json!({ "chat": "@alex" })).await?,
        "INVALID_PEER"
    );
    assert_eq!(
        rejected_code(&client, "get_group_call", json!({ "chat": "-202" })).await?,
        "INVALID_PEER"
    );
    for (tool, arguments) in [
        ("get_group_call", json!({ "chat": "", "limit": 0 })),
        ("get_group_call", json!({ "chat": "-100505", "limit": 501 })),
        ("get_group_call_participants", json!({ "chat": "-100505", "limit": 0 })),
        ("get_group_call_participants", json!({ "chat": "-100505", "limit": 501 })),
        ("get_group_call_participants", json!({ "chat": "-100505", "ids": [""] })),
        ("get_group_call_participants", json!({ "chat": "-100505", "ids": ["@alex", "@alex"] })),
        ("get_group_call_participants", json!({ "chat": "-100505", "sources": [111, 111] })),
        ("get_group_call_participants", json!({ "chat": "-100505", "offset": "x".repeat(513) })),
    ] {
        assert_eq!(rejected_code(&client, tool, arguments).await?, "INVALID_ARGUMENT");
    }

    let too_many_ids = (0..=100).map(|value| value.to_string()).collect::<Vec<_>>();
    assert_eq!(
        rejected_code(
            &client,
            "get_group_call_participants",
            json!({ "chat": "-100505", "ids": too_many_ids })
        )
        .await?,
        "INVALID_ARGUMENT"
    );
    let too_many_sources = (0..=100).collect::<Vec<_>>();
    assert_eq!(
        rejected_code(
            &client,
            "get_group_call_participants",
            json!({ "chat": "-100505", "sources": too_many_sources })
        )
        .await?,
        "INVALID_ARGUMENT"
    );

    for (tool, arguments) in [
        ("get_group_call", json!({ "chat": "-100505", "extra": true })),
        ("get_group_call_participants", json!({ "chat": "-100505", "extra": true })),
    ] {
        let rejected = client
            .call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await;
        assert!(rejected.is_err(), "{tool} must reject unknown fields at the JSON-RPC boundary");
    }

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
