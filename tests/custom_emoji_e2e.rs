use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use telegram_personal_mcp::{FakeTelegramBackend, McpServer, McpServerSettings};

fn assert_opaque_sticker_handle(handle: &str) {
    let suffix = handle.strip_prefix("sticker_").expect("opaque sticker prefix");
    assert_eq!(suffix.len(), 32);
    assert!(
        suffix.as_bytes().iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    );
}

#[tokio::test]
async fn custom_emoji_lookup_and_download_are_typed_private_and_content_addressed()
-> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let temp = tempfile::tempdir()?;
    let attachment_cache_dir = temp.path().join("attachments");
    let server = McpServer::with_settings(
        backend,
        McpServerSettings {
            attachment_cache_dir: attachment_cache_dir.clone(),
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

    let lookup = client
        .call_tool(CallToolRequestParams::new("get_custom_emoji_documents").with_arguments(
            json!({ "document_ids": [8_001, 99_999, 7_001] }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(lookup.is_error, Some(false));
    let lookup_data = lookup.structured_content.as_ref().expect("custom emoji lookup");
    assert_eq!(lookup_data.pointer("/data/requested_count").and_then(Value::as_u64), Some(3));
    assert_eq!(lookup_data.pointer("/data/returned_count").and_then(Value::as_u64), Some(2));
    assert_eq!(
        lookup_data.pointer("/data/missing_document_ids/0").and_then(Value::as_i64),
        Some(99_999)
    );
    assert_eq!(
        lookup_data.pointer("/data/documents/0/document_id").and_then(Value::as_i64),
        Some(8_001)
    );
    assert_eq!(
        lookup_data.pointer("/data/documents/0/sticker/custom_emoji").and_then(Value::as_bool),
        Some(true)
    );
    let handle = lookup_data
        .pointer("/data/documents/0/sticker/sticker_handle")
        .and_then(Value::as_str)
        .expect("opaque custom emoji handle");
    assert_opaque_sticker_handle(handle);
    let serialized_lookup = serde_json::to_string(lookup_data)?;
    assert!(!serialized_lookup.contains("access_hash"));
    assert!(!serialized_lookup.contains("file_reference"));

    let duplicate = client
        .call_tool(CallToolRequestParams::new("get_custom_emoji_documents").with_arguments(
            json!({ "document_ids": [7_001, 7_001] }).as_object().expect("object").clone(),
        ))
        .await?;
    assert_eq!(duplicate.is_error, Some(true));
    assert_eq!(
        duplicate
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(Value::as_str),
        Some("INVALID_ARGUMENT")
    );

    let downloaded =
        client
            .call_tool(CallToolRequestParams::new("download_custom_emoji").with_arguments(
                json!({ "document_id": 8_001 }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(downloaded.is_error, Some(false));
    let downloaded_data = downloaded.structured_content.as_ref().expect("downloaded custom emoji");
    let local_path =
        downloaded_data.pointer("/data/local_path").and_then(Value::as_str).expect("download path");
    let expected_bytes = b"fixture custom emoji 8001 webm\n";
    let expected_sha256 = format!("{:x}", Sha256::digest(expected_bytes));
    assert_eq!(std::fs::read(local_path)?, expected_bytes);
    assert_eq!(
        downloaded_data.pointer("/data/sha256").and_then(Value::as_str),
        Some(expected_sha256.as_str())
    );
    assert_eq!(
        downloaded_data.pointer("/data/bytes_written").and_then(Value::as_u64),
        Some(u64::try_from(expected_bytes.len())?)
    );
    assert!(local_path.ends_with(&format!("{expected_sha256}-custom-emoji.webm")));
    assert!(!local_path.contains("8001"));
    #[cfg(unix)]
    {
        let path = std::path::Path::new(local_path);
        assert_eq!(std::fs::metadata(path)?.permissions().mode() & 0o777, 0o400);
        assert_eq!(
            std::fs::metadata(path.parent().expect("custom emoji cache directory"))?
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    let repeated =
        client
            .call_tool(CallToolRequestParams::new("download_custom_emoji").with_arguments(
                json!({ "document_id": 8_001 }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(repeated.is_error, Some(false));
    assert_eq!(
        repeated
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/local_path"))
            .and_then(Value::as_str),
        Some(local_path)
    );

    let missing =
        client
            .call_tool(CallToolRequestParams::new("download_custom_emoji").with_arguments(
                json!({ "document_id": 99_999 }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(missing.is_error, Some(true));
    assert_eq!(
        missing
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(Value::as_str),
        Some("CUSTOM_EMOJI_NOT_FOUND")
    );

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn custom_emoji_stream_limit_removes_partial_file() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    backend.set_fixture_custom_emoji_size_hint(Some(5)).await;
    let temp = tempfile::tempdir()?;
    let attachment_cache_dir = temp.path().join("attachments");
    let server = McpServer::with_settings(
        backend,
        McpServerSettings {
            attachment_cache_dir: attachment_cache_dir.clone(),
            max_download_bytes: 10,
            max_upload_bytes: 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
    );
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let result =
        client
            .call_tool(CallToolRequestParams::new("download_custom_emoji").with_arguments(
                json!({ "document_id": 8_001 }).as_object().expect("object").clone(),
            ))
            .await?;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        result
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(Value::as_str),
        Some("ATTACHMENT_TOO_LARGE")
    );

    let mut directories = vec![attachment_cache_dir];
    let mut regular_files = 0_usize;
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                directories.push(path);
            } else {
                regular_files = regular_files.saturating_add(1);
            }
        }
    }
    assert_eq!(regular_files, 0, "partial custom emoji file must be removed");

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
