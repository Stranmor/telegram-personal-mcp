use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::json;
use std::sync::Arc;
use telegram_personal_mcp::{FakeTelegramBackend, McpServer, McpServerSettings};

fn resolve_local_ref<'a>(
    root: &'a serde_json::Value,
    value: &'a serde_json::Value,
) -> &'a serde_json::Value {
    value
        .get("$ref")
        .and_then(serde_json::Value::as_str)
        .and_then(|reference| reference.strip_prefix('#'))
        .and_then(|pointer| root.pointer(pointer))
        .unwrap_or(value)
}

fn contains_maximum(value: &serde_json::Value, maximum: i64) -> bool {
    match value {
        serde_json::Value::Object(object) => {
            object.get("maximum") == Some(&json!(maximum))
                || object.values().any(|nested| contains_maximum(nested, maximum))
        },
        serde_json::Value::Array(items) => {
            items.iter().any(|nested| contains_maximum(nested, maximum))
        },
        _ => false,
    }
}

#[tokio::test]
async fn generic_media_discovery_and_context_contract_are_advertised() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let cache = tempfile::tempdir()?;
    let server = McpServer::with_settings(
        backend,
        McpServerSettings {
            attachment_cache_dir: cache.path().to_path_buf(),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: cache.path().join("state.sqlite"),
        },
    );
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let tools = client.list_all_tools().await?;
    let context = tools
        .iter()
        .find(|tool| tool.name == "read_context")
        .expect("read_context must be advertised");
    let context_schema = serde_json::Value::Object(context.input_schema.as_ref().clone());
    let chat_id = context_schema.pointer("/properties/chat_id").expect("chat_id schema");
    let chat_id = resolve_local_ref(&context_schema, chat_id);
    assert_eq!(chat_id.get("type"), Some(&json!("string")));
    assert!(
        chat_id
            .get("description")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|description| description.contains("Quote numeric IDs"))
    );
    assert!(contains_maximum(
        context_schema.pointer("/properties/before").expect("before schema"),
        25
    ));
    assert!(contains_maximum(
        context_schema.pointer("/properties/after").expect("after schema"),
        25
    ));

    let discovered = client
        .call_tool(
            CallToolRequestParams::new("search_tools").with_arguments(
                json!({"query": "attachment download media photo file", "limit": 20})
                    .as_object()
                    .expect("search arguments")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(discovered.is_error, Some(false));
    let names = discovered
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/tools"))
        .and_then(serde_json::Value::as_array)
        .expect("typed tool-search results")
        .iter()
        .filter_map(|tool| tool.get("name").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();
    assert!(names.contains(&"list_attachments"));
    assert!(names.contains(&"download_attachment"));

    let numeric_chat_id_error = client
        .call_tool(
            CallToolRequestParams::new("read_context").with_arguments(
                json!({"chat_id": -1001234567890_i64, "message_id": 1})
                    .as_object()
                    .expect("numeric contract probe")
                    .clone(),
            ),
        )
        .await
        .expect_err("numeric chat_id must be rejected by typed MCP deserialization");
    let numeric_chat_id_error = numeric_chat_id_error.to_string();
    assert!(numeric_chat_id_error.contains("-32602"));
    assert!(numeric_chat_id_error.contains("expected a string"));

    drop(client);
    server_task.abort();
    Ok(())
}
