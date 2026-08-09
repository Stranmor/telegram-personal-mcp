use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use telegram_personal_mcp::{FakeTelegramBackend, McpServer, McpServerSettings};

#[tokio::test]
async fn admin_log_is_typed_filterable_and_cursor_bounded() -> anyhow::Result<()> {
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

    let first = client
        .call_tool(CallToolRequestParams::new("get_admin_log").with_arguments(
            json!({ "chat_id": "-100505", "limit": 2 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(first.is_error, Some(false));
    let first = first.structured_content.expect("admin log page");
    assert_eq!(first.pointer("/data/chat_id").and_then(Value::as_str), Some("-100505"));
    assert_eq!(first.pointer("/data/returned_count").and_then(Value::as_u64), Some(2));
    assert_eq!(
        first.pointer("/data/events/0/action").and_then(Value::as_str),
        Some("change_title")
    );
    assert_eq!(
        first.pointer("/data/events/0/actor/username").and_then(Value::as_str),
        Some("fixture_account")
    );
    assert_eq!(
        first.pointer("/data/events/1/target_user/username").and_then(Value::as_str),
        Some("alex")
    );
    assert_eq!(first.pointer("/data/next_max_id").and_then(Value::as_i64), Some(9_003));
    assert_eq!(first.pointer("/data/completeness").and_then(Value::as_str), Some("page"));

    let second = client
        .call_tool(
            CallToolRequestParams::new("get_admin_log").with_arguments(
                json!({ "chat_id": "-100505", "max_id": 9003, "limit": 1 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(second.is_error, Some(false));
    assert_eq!(
        second
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/events/0/event_id"))
            .and_then(Value::as_i64),
        Some(9_002)
    );

    let forums = client
        .call_tool(
            CallToolRequestParams::new("get_admin_log").with_arguments(
                json!({
                    "chat_id": "-100505",
                    "events": { "forums": true }
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(forums.is_error, Some(false));
    assert_eq!(
        forums
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/events/0/action"))
            .and_then(Value::as_str),
        Some("create_topic")
    );
    assert_eq!(
        forums
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/events/0/topic_id"))
            .and_then(Value::as_i64),
        Some(44)
    );

    let mira = client
        .call_tool(
            CallToolRequestParams::new("get_admin_log").with_arguments(
                json!({ "chat_id": "-100505", "admins": ["@mira_ops"] })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(mira.is_error, Some(false));
    assert_eq!(
        mira.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/returned_count"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert!(
        mira.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/events"))
            .and_then(Value::as_array)
            .is_some_and(|events| events.iter().all(|event| {
                event.pointer("/actor_user_id").and_then(Value::as_i64) == Some(303)
            }))
    );

    let queried = client
        .call_tool(
            CallToolRequestParams::new("get_admin_log").with_arguments(
                json!({ "chat_id": "-100505", "query": "Release operations" })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(queried.is_error, Some(false));
    assert_eq!(
        queried
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/events/0/event_id"))
            .and_then(Value::as_i64),
        Some(9_004)
    );

    for arguments in [
        json!({ "chat_id": "-100505", "limit": 101 }),
        json!({ "chat_id": "-100505", "max_id": -1 }),
        json!({ "chat_id": "-100505", "min_id": 10, "max_id": 10 }),
        json!({ "chat_id": "-100505", "admins": ["@mira_ops", "mira_ops"] }),
        json!({ "chat_id": "@alex" }),
        json!({ "chat_id": "-100505", "query": "x".repeat(257) }),
    ] {
        let rejected = client
            .call_tool(
                CallToolRequestParams::new("get_admin_log")
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(rejected.is_error, Some(true));
    }

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
