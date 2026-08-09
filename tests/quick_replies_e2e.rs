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
async fn quick_replies_are_hash_aware_typed_filterable_and_schema_closed() -> anyhow::Result<()> {
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
    for name in ["get_quick_replies", "get_quick_reply_messages"] {
        let tool = tools.iter().find(|tool| tool.name == name).expect("quick-reply tool");
        assert_eq!(tool.input_schema.get("additionalProperties"), Some(&json!(false)));
        assert!(tool.output_schema.is_some());
    }

    let catalog = client.call_tool(CallToolRequestParams::new("get_quick_replies")).await?;
    assert_eq!(catalog.is_error, Some(false));
    let catalog = catalog.structured_content.expect("quick-reply catalog");
    assert_eq!(catalog.pointer("/data/not_modified").and_then(Value::as_bool), Some(false));
    assert_eq!(catalog.pointer("/data/shortcuts/0/shortcut_id").and_then(Value::as_i64), Some(7));
    assert_eq!(
        catalog.pointer("/data/shortcuts/0/shortcut").and_then(Value::as_str),
        Some("greeting")
    );
    assert_eq!(
        catalog.pointer("/data/shortcuts/0/top_message_id").and_then(Value::as_i64),
        Some(901)
    );
    assert_eq!(catalog.pointer("/data/shortcuts/0/count").and_then(Value::as_i64), Some(2));
    assert_eq!(
        catalog.pointer("/data/top_messages/0/message/text").and_then(Value::as_str),
        Some("Hello — how can I help?")
    );
    assert_eq!(
        catalog.pointer("/data/top_messages/0/quick_reply_shortcut_id").and_then(Value::as_i64),
        Some(7)
    );
    assert_eq!(
        catalog.pointer("/data/top_messages/0/message/edit_date").and_then(Value::as_i64),
        Some(1_700_002_902)
    );
    assert!(
        catalog
            .pointer("/data/related_users")
            .and_then(Value::as_array)
            .is_some_and(|users| !users.is_empty())
    );
    assert!(
        catalog
            .pointer("/data/related_chats")
            .and_then(Value::as_array)
            .is_some_and(|chats| !chats.is_empty())
    );

    let catalog_not_modified = client
        .call_tool(
            CallToolRequestParams::new("get_quick_replies")
                .with_arguments(json!({ "hash": "12001" }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(catalog_not_modified.is_error, Some(false));
    let catalog_not_modified =
        catalog_not_modified.structured_content.expect("not-modified catalog");
    assert_eq!(
        catalog_not_modified.pointer("/data/not_modified").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        catalog_not_modified.pointer("/data/shortcuts").and_then(Value::as_array).map(Vec::len),
        Some(0)
    );

    let messages = client
        .call_tool(
            CallToolRequestParams::new("get_quick_reply_messages")
                .with_arguments(json!({ "shortcut_id": 7 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(messages.is_error, Some(false));
    let messages = messages.structured_content.expect("quick-reply messages");
    assert_eq!(messages.pointer("/data/shortcut_id").and_then(Value::as_i64), Some(7));
    assert_eq!(messages.pointer("/data/total_count").and_then(Value::as_i64), Some(2));
    assert_eq!(messages.pointer("/data/messages").and_then(Value::as_array).map(Vec::len), Some(2));
    assert_eq!(
        messages.pointer("/data/messages/0/is_service").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        messages.pointer("/data/messages/1/is_service").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        messages.pointer("/data/messages/1/service_action_kind").and_then(Value::as_str),
        Some("history_clear")
    );
    assert_eq!(
        messages.pointer("/data/messages/1/message/reply_to_message_id").and_then(Value::as_i64),
        Some(901)
    );

    let filtered = client
        .call_tool(CallToolRequestParams::new("get_quick_reply_messages").with_arguments(
            json!({ "shortcutId": 7, "ids": [900] }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(filtered.is_error, Some(false));
    let filtered = filtered.structured_content.expect("filtered quick replies");
    assert_eq!(filtered.pointer("/data/total_count").and_then(Value::as_i64), Some(2));
    assert_eq!(filtered.pointer("/data/messages").and_then(Value::as_array).map(Vec::len), Some(1));
    assert_eq!(
        filtered.pointer("/data/messages/0/message/message_id").and_then(Value::as_i64),
        Some(900)
    );

    let filtered_alias = client
        .call_tool(CallToolRequestParams::new("get_quick_reply_messages").with_arguments(
            json!({ "shortcutId": 7, "messageIds": [901] }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(filtered_alias.is_error, Some(false));
    assert_eq!(
        filtered_alias
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/message/message_id"))
            .and_then(Value::as_i64),
        Some(901)
    );

    let messages_not_modified = client
        .call_tool(CallToolRequestParams::new("get_quick_reply_messages").with_arguments(
            json!({ "shortcutId": 7, "hash": "12002" }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(messages_not_modified.is_error, Some(false));
    assert_eq!(
        messages_not_modified
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/not_modified"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let zero_shortcut = client
        .call_tool(
            CallToolRequestParams::new("get_quick_reply_messages")
                .with_arguments(json!({ "shortcutId": 0 }).as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(zero_shortcut.is_error, Some(false));
    assert_eq!(
        zero_shortcut
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    for (tool, arguments) in [
        ("get_quick_replies", json!({ "hash": "012001" })),
        ("get_quick_replies", json!({ "hash": "+1" })),
        ("get_quick_replies", json!({ "hash": "9223372036854775808" })),
        ("get_quick_reply_messages", json!({ "shortcutId": -1 })),
        ("get_quick_reply_messages", json!({ "shortcutId": 7, "ids": [-1] })),
        ("get_quick_reply_messages", json!({ "shortcutId": 7, "ids": [900, 900] })),
    ] {
        assert_eq!(rejected_code(&client, tool, arguments).await?, "INVALID_ARGUMENT");
    }
    let too_many_ids = (0..=100).collect::<Vec<_>>();
    assert_eq!(
        rejected_code(
            &client,
            "get_quick_reply_messages",
            json!({ "shortcutId": 7, "ids": too_many_ids })
        )
        .await?,
        "INVALID_ARGUMENT"
    );

    for (tool, arguments) in [
        ("get_quick_replies", json!({ "extra": true })),
        ("get_quick_reply_messages", json!({ "shortcutId": 7, "extra": true })),
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
