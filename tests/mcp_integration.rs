use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use telegram_personal_mcp::{FakeTelegramBackend, McpServer, McpServerSettings};

#[tokio::test]
async fn official_mcp_transport_exposes_structured_tools_and_typed_errors() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let cache = tempfile::tempdir()?;
    let server = McpServer::with_settings(
        backend.clone(),
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
    assert_eq!(tools.len(), 14);
    assert!(tools.iter().all(|tool| tool.output_schema.is_some()));
    assert!(
        tools
            .iter()
            .all(|tool| tool.input_schema.get("additionalProperties") == Some(&json!(false)))
    );
    assert!(tools.iter().any(|tool| tool.name == "search_dialogs"));
    assert!(tools.iter().any(|tool| tool.name == "search_messages"));

    let health = client.call_tool(CallToolRequestParams::new("health")).await?;
    assert_eq!(health.is_error, Some(false));
    assert_eq!(
        health
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/username"))
            .and_then(|value| value.as_str()),
        Some("fixture_account")
    );
    assert!(
        health
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/session_path"))
            .is_none()
    );

    let unknown_send_argument = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Must not be sent",
                    "reply_to_message_id": 11
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await
        .expect_err("unknown send arguments must be rejected at the MCP boundary");
    assert!(unknown_send_argument.to_string().contains("unknown field"));
    assert!(backend.sent_messages().await.is_empty());

    let search = client
        .call_tool(
            CallToolRequestParams::new("search_messages").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "query": "contract",
                    "limit": 10
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(search.is_error, Some(false));
    assert_eq!(
        search
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/message_id"))
            .and_then(|value| value.as_i64()),
        Some(11)
    );

    let unsupported_global_page = client
        .call_tool(
            CallToolRequestParams::new("search_messages").with_arguments(
                json!({
                    "query": "contract",
                    "limit": 10,
                    "offset_id": 11
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(unsupported_global_page.is_error, Some(true));
    assert_eq!(
        unsupported_global_page
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("INVALID_ARGUMENT")
    );

    let global_group_match = client
        .call_tool(
            CallToolRequestParams::new("search_messages").with_arguments(
                json!({
                    "query": "Release",
                    "limit": 10
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(
        global_group_match
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/messages/0/chat_ref"))
            .and_then(|value| value.as_str()),
        Some("-202")
    );
    let round_trip_group_message = client
        .call_tool(
            CallToolRequestParams::new("get_message").with_arguments(
                json!({
                    "chat_id": "-202",
                    "message_id": 21
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(round_trip_group_message.is_error, Some(false));

    let context = client
        .call_tool(
            CallToolRequestParams::new("read_context").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 11,
                    "before": 5,
                    "after": 5
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(context.is_error, Some(false));
    assert_eq!(
        context
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/target/message_id"))
            .and_then(|value| value.as_i64()),
        Some(11)
    );
    assert_eq!(
        context
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/older/0/message_id"))
            .and_then(|value| value.as_i64()),
        Some(10)
    );

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Prepared exact reply",
                    "reply_to_message_id": 11
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let draft_id = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/draft_id"))
        .and_then(|value| value.as_str())
        .expect("draft id")
        .to_string();
    let payload_sha256 = prepared
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/payload_sha256"))
        .and_then(|value| value.as_str())
        .expect("payload sha256")
        .to_string();

    let prepared_readback =
        client
            .call_tool(CallToolRequestParams::new("get_prepared").with_arguments(
                json!({ "draft_id": draft_id }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(prepared_readback.is_error, Some(false));
    assert_eq!(
        prepared_readback
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/payload_sha256"))
            .and_then(|value| value.as_str()),
        Some(payload_sha256.as_str())
    );

    let mismatched_approval = client
        .call_tool(
            CallToolRequestParams::new("send_prepared").with_arguments(
                json!({
                    "draft_id": draft_id,
                    "approved_payload_sha256": "not-the-approved-hash"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(mismatched_approval.is_error, Some(true));
    assert_eq!(
        mismatched_approval
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("PAYLOAD_HASH_MISMATCH")
    );
    assert!(backend.sent_messages().await.is_empty());

    backend.set_fixture_account_user_id(8).await;
    let wrong_account = client
        .call_tool(
            CallToolRequestParams::new("send_prepared").with_arguments(
                json!({
                    "draft_id": draft_id,
                    "approved_payload_sha256": payload_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(wrong_account.is_error, Some(true));
    assert_eq!(
        wrong_account
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("ACCOUNT_MISMATCH")
    );
    assert!(backend.sent_messages().await.is_empty());
    backend.set_fixture_account_user_id(7).await;

    let send_prepared_args = json!({
        "draft_id": draft_id,
        "approved_payload_sha256": payload_sha256
    })
    .as_object()
    .expect("object")
    .clone();
    let prepared_sent = client
        .call_tool(
            CallToolRequestParams::new("send_prepared").with_arguments(send_prepared_args.clone()),
        )
        .await?;
    assert_eq!(prepared_sent.is_error, Some(false));
    let first_prepared_message_id = prepared_sent
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/message_id"))
        .and_then(|value| value.as_i64());
    let prepared_replayed = client
        .call_tool(CallToolRequestParams::new("send_prepared").with_arguments(send_prepared_args))
        .await?;
    assert_eq!(prepared_replayed.is_error, Some(false));
    assert_eq!(
        prepared_replayed
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/message_id"))
            .and_then(|value| value.as_i64()),
        first_prepared_message_id
    );
    assert_eq!(backend.sent_messages().await.len(), 1);

    let downloaded = client
        .call_tool(
            CallToolRequestParams::new("download_attachment").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 11
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(downloaded.is_error, Some(false));
    let downloaded_path = downloaded
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/local_path"))
        .and_then(|value| value.as_str())
        .expect("downloaded path");
    let downloaded_sha256 = downloaded
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/sha256"))
        .and_then(|value| value.as_str())
        .expect("downloaded sha256")
        .to_string();
    assert!(std::path::Path::new(downloaded_path).is_file());
    #[cfg(unix)]
    {
        let downloaded_path = std::path::Path::new(downloaded_path);
        assert_eq!(std::fs::metadata(downloaded_path)?.permissions().mode() & 0o777, 0o400);
        let attachment_dir = downloaded_path.parent().expect("attachment directory");
        let account_dir = attachment_dir.parent().expect("account directory");
        assert_eq!(std::fs::metadata(attachment_dir)?.permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(account_dir)?.permissions().mode() & 0o777, 0o700);
    }
    assert_eq!(
        downloaded
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/bytes_written"))
            .and_then(|value| value.as_u64()),
        Some(26)
    );

    let downloaded_again = client
        .call_tool(
            CallToolRequestParams::new("download_attachment").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "message_id": 11
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(downloaded_again.is_error, Some(false));
    assert_eq!(
        downloaded_again
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/local_path"))
            .and_then(|value| value.as_str()),
        Some(downloaded_path)
    );
    assert_eq!(
        downloaded_again
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/sha256"))
            .and_then(|value| value.as_str()),
        Some(downloaded_sha256.as_str())
    );

    let mut updated_bytes = std::fs::read(downloaded_path)?;
    updated_bytes.extend_from_slice(b"% deterministic portfolio edit\n");
    let updated_path = std::path::Path::new(downloaded_path)
        .parent()
        .expect("attachment directory")
        .join("contract-updated.pdf");
    std::fs::write(&updated_path, &updated_bytes)?;
    let updated_sha256 = format!("{:x}", Sha256::digest(&updated_bytes));

    backend.set_fixture_account_user_id(8).await;
    let cross_account_file = client
        .call_tool(
            CallToolRequestParams::new("send_file").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": updated_path,
                    "caption": "Must stay bound to the account that downloaded it",
                    "reply_to_message_id": 11,
                    "mode": "document",
                    "approved_sha256": updated_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(cross_account_file.is_error, Some(true));
    assert_eq!(
        cross_account_file
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("FILE_OUTSIDE_UPLOAD_ROOT")
    );
    assert_eq!(backend.sent_messages().await.len(), 1);
    backend.set_fixture_account_user_id(7).await;

    let wrong_file_hash = client
        .call_tool(
            CallToolRequestParams::new("send_file").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": updated_path,
                    "caption": "Updated contract",
                    "reply_to_message_id": 11,
                    "mode": "document",
                    "approved_sha256": downloaded_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(wrong_file_hash.is_error, Some(true));
    assert_eq!(
        wrong_file_hash
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("FILE_HASH_MISMATCH")
    );
    assert_eq!(backend.sent_messages().await.len(), 1);

    let sent_file = client
        .call_tool(
            CallToolRequestParams::new("send_file").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": updated_path,
                    "caption": "Updated contract",
                    "reply_to_message_id": 11,
                    "mode": "document",
                    "approved_sha256": updated_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(sent_file.is_error, Some(false));
    assert_eq!(
        sent_file
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/sha256"))
            .and_then(|value| value.as_str()),
        Some(updated_sha256.as_str())
    );
    let sent_messages = backend.sent_messages().await;
    assert_eq!(sent_messages.len(), 2);
    let file_message = sent_messages.last().expect("file send receipt");
    assert_eq!(file_message.reply_to_message_id, Some(11));
    assert_eq!(file_message.file_mode, Some(telegram_personal_mcp::FileSendMode::Document));
    assert!(
        file_message
            .file_name
            .as_deref()
            .is_some_and(|name| name.ends_with("contract-updated.pdf"))
    );

    let outside_upload_root = tempfile::tempdir()?;
    let outside_file = outside_upload_root.path().join("not-managed.txt");
    std::fs::write(&outside_file, b"must not be exfiltrated")?;
    let outside_sha256 = format!("{:x}", Sha256::digest(b"must not be exfiltrated"));
    let rejected_file = client
        .call_tool(
            CallToolRequestParams::new("send_file").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": outside_file,
                    "caption": "Should not leave the machine",
                    "mode": "document",
                    "approved_sha256": outside_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(rejected_file.is_error, Some(true));
    assert_eq!(
        rejected_file
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("FILE_OUTSIDE_UPLOAD_ROOT")
    );
    assert_eq!(backend.sent_messages().await.len(), 2);

    let sent = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Updated contract attached"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(sent.is_error, Some(false));
    assert_eq!(backend.sent_messages().await.len(), 3);

    let rejected = client
        .call_tool(
            CallToolRequestParams::new("send_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "   "
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(rejected.is_error, Some(true));
    assert_eq!(
        rejected
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
        Some("INVALID_ARGUMENT")
    );
    assert_eq!(backend.sent_messages().await.len(), 3);

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
