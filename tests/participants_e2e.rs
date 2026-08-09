use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use telegram_personal_mcp::{FakeTelegramBackend, McpServer, McpServerSettings};

async fn list_participants(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    arguments: Value,
) -> anyhow::Result<rmcp::model::CallToolResult> {
    Ok(client
        .call_tool(
            CallToolRequestParams::new("list_participants")
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?)
}

#[tokio::test]
async fn participant_inventory_is_detailed_filterable_searchable_and_pageable() -> anyhow::Result<()>
{
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

    let first = list_participants(&client, json!({ "chat_id": "-100505", "limit": 2 })).await?;
    assert_eq!(first.is_error, Some(false));
    let first = first.structured_content.expect("first participant page");
    assert_eq!(first.pointer("/data/chat_id").and_then(Value::as_str), Some("-100505"));
    assert_eq!(first.pointer("/data/filter").and_then(Value::as_str), Some("recent"));
    assert_eq!(first.pointer("/data/offset").and_then(Value::as_u64), Some(0));
    assert_eq!(first.pointer("/data/limit").and_then(Value::as_u64), Some(2));
    assert_eq!(first.pointer("/data/total_count").and_then(Value::as_u64), Some(5));
    assert_eq!(first.pointer("/data/returned_count").and_then(Value::as_u64), Some(2));
    assert_eq!(first.pointer("/data/next_offset").and_then(Value::as_u64), Some(2));
    assert_eq!(first.pointer("/data/has_more").and_then(Value::as_bool), Some(true));
    assert_eq!(first.pointer("/data/completeness").and_then(Value::as_str), Some("page"));
    assert_eq!(first.pointer("/data/participants/0/role").and_then(Value::as_str), Some("creator"));
    assert_eq!(
        first.pointer("/data/participants/1/user/user_id").and_then(Value::as_i64),
        Some(101)
    );

    let second =
        list_participants(&client, json!({ "chat_id": "-100505", "offset": 2, "limit": 2 }))
            .await?;
    assert_eq!(second.is_error, Some(false));
    let second = second.structured_content.expect("second participant page");
    assert_eq!(
        second.pointer("/data/participants/0/user/user_id").and_then(Value::as_i64),
        Some(303)
    );
    assert_eq!(
        second
            .pointer("/data/participants/0/admin_rights/delete_messages")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        second.pointer("/data/participants/1/role").and_then(Value::as_str),
        Some("restricted")
    );
    assert_eq!(second.pointer("/data/next_offset").and_then(Value::as_u64), Some(4));

    let searched_admin = list_participants(
        &client,
        json!({ "chat_id": "-100505", "filter": "admins", "query": "MIRA" }),
    )
    .await?;
    assert_eq!(searched_admin.is_error, Some(false));
    let searched_admin = searched_admin.structured_content.expect("admin search");
    assert_eq!(searched_admin.pointer("/data/total_count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        searched_admin.pointer("/data/participants/0/user/username").and_then(Value::as_str),
        Some("mira_ops")
    );
    assert_eq!(
        searched_admin.pointer("/data/participants/0/rank").and_then(Value::as_str),
        Some("Ops")
    );

    let searched_bot = list_participants(
        &client,
        json!({ "chat_id": "-100505", "filter": "bots", "search": "inline" }),
    )
    .await?;
    assert_eq!(searched_bot.is_error, Some(false));
    assert_eq!(
        searched_bot
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/participants/0/user/user_id"))
            .and_then(Value::as_i64),
        Some(909)
    );

    let contacts =
        list_participants(&client, json!({ "chat_id": "-100505", "filter": "contacts" })).await?;
    assert_eq!(contacts.is_error, Some(false));
    let contacts = contacts.structured_content.expect("contacts page");
    assert_eq!(contacts.pointer("/data/total_count").and_then(Value::as_u64), Some(2));
    assert_eq!(
        contacts.pointer("/data/participants/0/user/user_id").and_then(Value::as_i64),
        Some(101)
    );
    assert_eq!(
        contacts.pointer("/data/participants/1/user/user_id").and_then(Value::as_i64),
        Some(303)
    );

    let kicked =
        list_participants(&client, json!({ "chat_id": "-100505", "filter": "kicked" })).await?;
    assert_eq!(kicked.is_error, Some(false));
    let kicked = kicked.structured_content.expect("kicked page");
    assert_eq!(kicked.pointer("/data/total_count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        kicked.pointer("/data/participants/0/user/user_id").and_then(Value::as_i64),
        Some(707)
    );
    assert_eq!(kicked.pointer("/data/participants/0/role").and_then(Value::as_str), Some("banned"));
    assert_eq!(kicked.pointer("/data/participants/0/left").and_then(Value::as_bool), Some(true));
    assert_eq!(
        kicked.pointer("/data/participants/0/banned_rights/view_messages").and_then(Value::as_bool),
        Some(true)
    );

    let banned = list_participants(
        &client,
        json!({ "chat_id": "-100505", "filter": "banned", "query": "rin" }),
    )
    .await?;
    assert_eq!(banned.is_error, Some(false));
    let banned = banned.structured_content.expect("banned page");
    assert_eq!(banned.pointer("/data/total_count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        banned.pointer("/data/participants/0/user/user_id").and_then(Value::as_i64),
        Some(808)
    );
    assert_eq!(
        banned.pointer("/data/participants/0/role").and_then(Value::as_str),
        Some("restricted")
    );
    assert_eq!(
        banned.pointer("/data/participants/0/banned_rights/send_docs").and_then(Value::as_bool),
        Some(true)
    );

    let empty =
        list_participants(&client, json!({ "chat_id": "-100505", "offset": 99, "limit": 10 }))
            .await?;
    assert_eq!(empty.is_error, Some(false));
    let empty = empty.structured_content.expect("empty page");
    assert_eq!(empty.pointer("/data/total_count").and_then(Value::as_u64), Some(5));
    assert_eq!(empty.pointer("/data/returned_count").and_then(Value::as_u64), Some(0));
    assert_eq!(empty.pointer("/data/next_offset"), Some(&Value::Null));
    assert_eq!(empty.pointer("/data/has_more").and_then(Value::as_bool), Some(false));
    assert_eq!(empty.pointer("/data/completeness").and_then(Value::as_str), Some("complete"));

    let basic_admins =
        list_participants(&client, json!({ "chat_id": "-202", "filter": "admins" })).await?;
    assert_eq!(basic_admins.is_error, Some(false));
    let basic_admins = basic_admins.structured_content.expect("basic group admins");
    assert_eq!(
        basic_admins.pointer("/data/participants/0/role").and_then(Value::as_str),
        Some("creator")
    );
    assert_eq!(basic_admins.pointer("/data/total_count").and_then(Value::as_u64), Some(1));

    let basic_kicked =
        list_participants(&client, json!({ "chat_id": "-202", "filter": "kicked" })).await?;
    assert_eq!(basic_kicked.is_error, Some(false));
    assert_eq!(
        basic_kicked
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/total_count"))
            .and_then(Value::as_u64),
        Some(0)
    );

    for arguments in [
        json!({ "chat_id": "-100505", "limit": 0 }),
        json!({ "chat_id": "-100505", "limit": 201 }),
        json!({ "chat_id": "-100505", "offset": 2_147_483_648_u64 }),
        json!({ "chat_id": "-100505", "query": "   " }),
        json!({ "chat_id": "-100505", "query": "x".repeat(257) }),
        json!({ "chat_id": "-100505", "filter": "owners" }),
        json!({ "chat_id": "-100505", "cursor": 1 }),
        json!({ "chat_id": "@alex" }),
    ] {
        let result = client
            .call_tool(
                CallToolRequestParams::new("list_participants")
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await;
        if let Ok(rejected) = result {
            assert_eq!(rejected.is_error, Some(true));
        }
    }

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
