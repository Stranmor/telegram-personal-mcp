use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::json;
use std::sync::Arc;
use telegram_personal_mcp::{FakeTelegramBackend, McpServer, McpServerSettings};

#[tokio::test]
async fn oversized_download_stops_at_the_streaming_limit_and_removes_partial_file()
-> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let attachment_cache_dir = temp.path().join("attachments");
    let backend = Arc::new(FakeTelegramBackend::fixture());
    backend.set_fixture_attachment_size_hint(Some(5)).await;
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

    let result = client
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
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        result
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(|value| value.as_str()),
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
    assert_eq!(regular_files, 0, "partial file must be removed");

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
