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
async fn update_cursors_cover_empty_complete_slice_too_long_and_channel_bounds()
-> anyhow::Result<()> {
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
    for name in ["get_updates_state", "get_updates", "get_channel_updates"] {
        let tool = tools.iter().find(|tool| tool.name == name).expect("updates tool");
        assert_eq!(tool.input_schema.get("additionalProperties"), Some(&json!(false)));
        assert!(tool.output_schema.is_some());
        assert_eq!(tool.annotations.as_ref().and_then(|value| value.read_only_hint), Some(true));
    }

    let state = client.call_tool(CallToolRequestParams::new("get_updates_state")).await?;
    assert_eq!(state.is_error, Some(false));
    let state = state.structured_content.expect("updates state");
    assert_eq!(state.pointer("/data/pts").and_then(Value::as_i64), Some(120));
    assert_eq!(state.pointer("/data/qts").and_then(Value::as_i64), Some(7));
    assert_eq!(state.pointer("/data/date").and_then(Value::as_i64), Some(1_700_004_000));
    assert_eq!(state.pointer("/data/seq").and_then(Value::as_i64), Some(10));
    assert_eq!(state.pointer("/data/unread_count").and_then(Value::as_i64), Some(2));

    let complete = client
        .call_tool(
            CallToolRequestParams::new("get_updates").with_arguments(
                json!({
                    "pts": 119,
                    "qts": 7,
                    "date": 1_700_003_999,
                    "ptsLimit": 50,
                    "ptsTotalLimit": 1000,
                    "qtsLimit": 25
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(complete.is_error, Some(false));
    let complete = complete.structured_content.expect("complete difference");
    assert_eq!(complete.pointer("/data/kind").and_then(Value::as_str), Some("complete"));
    assert_eq!(complete.pointer("/data/is_final").and_then(Value::as_bool), Some(true));
    assert_eq!(complete.pointer("/data/request/pts_limit").and_then(Value::as_i64), Some(50));
    assert_eq!(complete.pointer("/data/request/qts_limit").and_then(Value::as_i64), Some(25));
    assert_eq!(complete.pointer("/data/next_cursor/pts").and_then(Value::as_i64), Some(120));
    assert_eq!(complete.pointer("/data/next_cursor/unread_count").and_then(Value::as_i64), Some(2));
    assert_eq!(
        complete.pointer("/data/new_messages/0/message_id").and_then(Value::as_i64),
        Some(11)
    );
    assert_eq!(complete.pointer("/data/encrypted_message_count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        complete.pointer("/data/deleted_messages/0/scope").and_then(Value::as_str),
        Some("global")
    );
    assert_eq!(
        complete.pointer("/data/deleted_messages/1/chat_id").and_then(Value::as_str),
        Some("-100505")
    );
    assert_eq!(
        complete.pointer("/data/other_updates/0/kind").and_then(Value::as_str),
        Some("user_status")
    );
    assert!(
        complete
            .pointer("/data/related_chats")
            .and_then(Value::as_array)
            .is_some_and(|chats| !chats.is_empty())
    );

    let slice = client
        .call_tool(
            CallToolRequestParams::new("get_updates").with_arguments(
                json!({ "pts": 118, "qts": 7, "date": 1_700_003_998 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(slice.is_error, Some(false));
    let slice = slice.structured_content.expect("slice difference");
    assert_eq!(slice.pointer("/data/kind").and_then(Value::as_str), Some("slice"));
    assert_eq!(slice.pointer("/data/continuation_required").and_then(Value::as_bool), Some(true));
    assert_eq!(slice.pointer("/data/next_cursor/pts").and_then(Value::as_i64), Some(119));
    assert_eq!(slice.pointer("/data/request/pts_limit").and_then(Value::as_i64), Some(100));
    assert_eq!(slice.pointer("/data/request/pts_total_limit").and_then(Value::as_i64), Some(1_000));

    let empty = client
        .call_tool(
            CallToolRequestParams::new("get_updates").with_arguments(
                json!({ "pts": 120, "qts": 7, "date": 1_700_004_000 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(empty.is_error, Some(false));
    let empty = empty.structured_content.expect("empty difference");
    assert_eq!(empty.pointer("/data/kind").and_then(Value::as_str), Some("empty"));
    assert!(empty.pointer("/data/next_cursor/unread_count").is_some_and(Value::is_null));

    let too_long = client
        .call_tool(CallToolRequestParams::new("get_updates").with_arguments(
            json!({ "pts": 0, "qts": 0, "date": 1 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(too_long.is_error, Some(false));
    let too_long = too_long.structured_content.expect("too-long difference");
    assert_eq!(too_long.pointer("/data/kind").and_then(Value::as_str), Some("too_long"));
    assert_eq!(too_long.pointer("/data/requires_resync").and_then(Value::as_bool), Some(true));
    assert_eq!(too_long.pointer("/data/resync_pts").and_then(Value::as_i64), Some(120));
    assert!(too_long.pointer("/data/next_cursor").is_some_and(Value::is_null));

    let channel = client
        .call_tool(
            CallToolRequestParams::new("get_channel_updates").with_arguments(
                json!({ "chatId": "-100505", "pts": 49, "limit": 100, "force": true })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(channel.is_error, Some(false));
    let channel = channel.structured_content.expect("channel difference");
    assert_eq!(channel.pointer("/data/kind").and_then(Value::as_str), Some("difference"));
    assert_eq!(channel.pointer("/data/request/chat_id").and_then(Value::as_str), Some("-100505"));
    assert_eq!(channel.pointer("/data/request/force").and_then(Value::as_bool), Some(true));
    assert_eq!(channel.pointer("/data/next_pts").and_then(Value::as_i64), Some(50));
    assert_eq!(
        channel.pointer("/data/new_messages/0/message_id").and_then(Value::as_i64),
        Some(50)
    );
    assert_eq!(
        channel.pointer("/data/deleted_messages/0/scope").and_then(Value::as_str),
        Some("channel")
    );

    let channel_slice = client
        .call_tool(
            CallToolRequestParams::new("get_channel_updates").with_arguments(
                json!({ "chat": "-100505", "pts": 48, "limit": 10 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(channel_slice.is_error, Some(false));
    let channel_slice = channel_slice.structured_content.expect("channel continuation");
    assert_eq!(channel_slice.pointer("/data/is_final").and_then(Value::as_bool), Some(false));
    assert_eq!(
        channel_slice.pointer("/data/continuation_required").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(channel_slice.pointer("/data/next_pts").and_then(Value::as_i64), Some(49));

    let channel_empty = client
        .call_tool(CallToolRequestParams::new("get_channel_updates").with_arguments(
            json!({ "chat_id": "-100505", "pts": 50 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(channel_empty.is_error, Some(false));
    assert_eq!(
        channel_empty
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/kind"))
            .and_then(Value::as_str),
        Some("empty")
    );

    let channel_too_long = client
        .call_tool(
            CallToolRequestParams::new("get_channel_updates").with_arguments(
                json!({ "chat_id": "-100505", "pts": 0, "limit": 10 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(channel_too_long.is_error, Some(false));
    let channel_too_long = channel_too_long.structured_content.expect("channel too-long");
    assert_eq!(channel_too_long.pointer("/data/kind").and_then(Value::as_str), Some("too_long"));
    assert_eq!(channel_too_long.pointer("/data/resync_pts").and_then(Value::as_i64), Some(50));
    assert_eq!(
        channel_too_long.pointer("/data/snapshot_messages").and_then(Value::as_array).map(Vec::len),
        Some(1)
    );

    for (tool, arguments, expected_code) in [
        ("get_updates", json!({ "pts": -1, "qts": 0, "date": 1 }), "INVALID_ARGUMENT"),
        ("get_updates", json!({ "pts": 0, "qts": -1, "date": 1 }), "INVALID_ARGUMENT"),
        ("get_updates", json!({ "pts": 0, "qts": 0, "date": 0 }), "INVALID_ARGUMENT"),
        (
            "get_updates",
            json!({ "pts": 0, "qts": 0, "date": 1, "ptsLimit": 0 }),
            "INVALID_ARGUMENT",
        ),
        (
            "get_updates",
            json!({ "pts": 0, "qts": 0, "date": 1, "ptsTotalLimit": 1001 }),
            "INVALID_ARGUMENT",
        ),
        (
            "get_updates",
            json!({ "pts": 0, "qts": 0, "date": 1, "qtsLimit": 1001 }),
            "INVALID_ARGUMENT",
        ),
        ("get_channel_updates", json!({ "chat_id": "-100505", "pts": -1 }), "INVALID_ARGUMENT"),
        (
            "get_channel_updates",
            json!({ "chat_id": "-100505", "pts": 0, "limit": 9 }),
            "INVALID_ARGUMENT",
        ),
        (
            "get_channel_updates",
            json!({ "chat_id": "-100505", "pts": 0, "limit": 101 }),
            "INVALID_ARGUMENT",
        ),
        ("get_channel_updates", json!({ "chat_id": "-202", "pts": 0 }), "INVALID_PEER"),
    ] {
        assert_eq!(rejected_code(&client, tool, arguments).await?, expected_code);
    }

    for (tool, arguments) in [
        ("get_updates_state", json!({ "extra": true })),
        ("get_updates", json!({ "pts": 0, "qts": 0, "date": 1, "extra": true })),
        ("get_channel_updates", json!({ "chat_id": "-100505", "pts": 0, "extra": true })),
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
