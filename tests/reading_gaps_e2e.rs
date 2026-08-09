use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use telegram_personal_mcp::{FakeTelegramBackend, McpServer, McpServerSettings};

#[tokio::test]
async fn unread_topic_receipt_and_self_role_surfaces_are_e2e_typed() -> anyhow::Result<()> {
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

    let mentions = client
        .call_tool(CallToolRequestParams::new("get_unread_mentions").with_arguments(
            json!({ "chat_id": "@alex", "limit": 10 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(mentions.is_error, Some(false));
    assert_eq!(
        mentions
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/message_id"))
            .and_then(Value::as_i64),
        Some(11)
    );
    assert_eq!(
        mentions
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/completeness"))
            .and_then(Value::as_str),
        Some("complete")
    );

    let mention_page_2 = client
        .call_tool(
            CallToolRequestParams::new("get_unread_mentions").with_arguments(
                json!({ "chat_id": "@alex", "offset_id": 11, "limit": 10 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(mention_page_2.is_error, Some(false));
    assert_eq!(
        mention_page_2
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    let reactions = client
        .call_tool(
            CallToolRequestParams::new("get_unread_reactions").with_arguments(
                json!({ "chat_id": "-202", "saved_peer_id": "@alex", "limit": 10 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(reactions.is_error, Some(false));
    assert_eq!(
        reactions
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/message_id"))
            .and_then(Value::as_i64),
        Some(21)
    );

    let poll_votes = client
        .call_tool(CallToolRequestParams::new("get_unread_poll_votes").with_arguments(
            json!({ "chat_id": "-202", "limit": 10 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(poll_votes.is_error, Some(false));
    assert_eq!(
        poll_votes
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/media_type"))
            .and_then(Value::as_str),
        Some("poll")
    );

    for (tool, arguments) in [
        ("get_unread_mentions", json!({ "chat_id": "@alex", "saved_peer_id": "-202" })),
        ("get_unread_reactions", json!({ "chat_id": "@alex", "min_id": 20, "max_id": 10 })),
        ("get_unread_poll_votes", json!({ "chat_id": "-202", "add_offset": 101 })),
    ] {
        let rejected = client
            .call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(arguments.as_object().expect("object").clone()),
            )
            .await?;
        assert_eq!(rejected.is_error, Some(true));
    }

    let topic = client
        .call_tool(
            CallToolRequestParams::new("read_topic_messages").with_arguments(
                json!({ "chat_id": "-202", "topic_id": 44, "limit": 10 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(topic.is_error, Some(false));
    assert_eq!(
        topic
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/message_id"))
            .and_then(Value::as_i64),
        Some(22)
    );
    assert_eq!(
        topic
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/reply_to_message_id"))
            .and_then(Value::as_i64),
        Some(21)
    );

    let read_date = client
        .call_tool(CallToolRequestParams::new("get_outbox_read_date").with_arguments(
            json!({ "chat_id": "@alex", "message_id": 10 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(read_date.is_error, Some(false));
    assert_eq!(
        read_date
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/read_at_unix"))
            .and_then(Value::as_i64),
        Some(1_700_000_020)
    );
    let invalid_read_date = client
        .call_tool(CallToolRequestParams::new("get_outbox_read_date").with_arguments(
            json!({ "chat_id": "-202", "message_id": 21 }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(invalid_read_date.is_error, Some(true));

    let role =
        client
            .call_tool(CallToolRequestParams::new("get_my_role").with_arguments(
                json!({ "chat_id": "-100505" }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(role.is_error, Some(false));
    assert_eq!(
        role.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/member/role"))
            .and_then(Value::as_str),
        Some("creator")
    );
    assert_eq!(
        role.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/member/is_self"))
            .and_then(Value::as_bool),
        Some(true)
    );

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
