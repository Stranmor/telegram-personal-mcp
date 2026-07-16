use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn binary_demo_mode_serves_mcp_over_real_stdio() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let binary = env!("CARGO_BIN_EXE_telegram-personal-mcp");
    let transport =
        TokioChildProcess::new(tokio::process::Command::new(binary).configure(|command| {
            command
                .arg("--demo")
                .arg("--attachment-cache-dir")
                .arg(temp.path().join("attachments"))
                .arg("--state-db-path")
                .arg(temp.path().join("state.sqlite"))
                .env_remove("TELEGRAM_API_ID")
                .env_remove("TELEGRAM_API_HASH")
                .env_remove("TELEGRAM_MCP_SESSION_PATH")
                .env_remove("TELEGRAM_MCP_SESSION_CACHE_DIR");
        }))?;

    let client = ().serve(transport).await?;
    let tools = client.list_all_tools().await?;
    assert_eq!(tools.len(), 14);
    assert!(tools.iter().all(|tool| tool.output_schema.is_some()));

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

    let attachment = client
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
    assert_eq!(attachment.is_error, Some(false));
    let local_path = attachment
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/local_path"))
        .and_then(|value| value.as_str())
        .expect("downloaded path");
    let downloaded_sha256 = attachment
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/sha256"))
        .and_then(|value| value.as_str())
        .expect("downloaded sha256");
    assert!(std::path::Path::new(local_path).is_file());

    let mut modified_bytes = std::fs::read(local_path)?;
    modified_bytes.extend_from_slice(b"% modified by stdio demo\n");
    let modified_path = std::path::Path::new(local_path)
        .parent()
        .expect("attachment directory")
        .join("contract-updated.pdf");
    std::fs::write(&modified_path, &modified_bytes)?;
    let modified_sha256 = format!("{:x}", Sha256::digest(&modified_bytes));

    let sent = client
        .call_tool(
            CallToolRequestParams::new("send_file").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "file_path": modified_path,
                    "caption": "Updated contract",
                    "reply_to_message_id": 11,
                    "mode": "document",
                    "approved_sha256": modified_sha256
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(sent.is_error, Some(false));
    assert_eq!(
        sent.structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/sha256"))
            .and_then(|value| value.as_str()),
        Some(modified_sha256.as_str())
    );
    println!(
        "demo-proof tools={} message_id=11 downloaded_sha256={} file_reply_sha256={}",
        tools.len(),
        downloaded_sha256,
        modified_sha256
    );

    client.cancel().await?;
    Ok(())
}

#[tokio::test]
async fn binary_does_not_negotiate_unimplemented_2026_07_28_protocol() -> anyhow::Result<()> {
    let binary = env!("CARGO_BIN_EXE_telegram-personal-mcp");
    let mut child = tokio::process::Command::new(binary)
        .arg("--demo")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_remove("TELEGRAM_API_ID")
        .env_remove("TELEGRAM_API_HASH")
        .env_remove("TELEGRAM_MCP_SESSION_PATH")
        .env_remove("TELEGRAM_MCP_SESSION_CACHE_DIR")
        .spawn()?;
    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2026-07-28",
            "capabilities": {},
            "clientInfo": {
                "name": "protocol-regression-test",
                "version": "1.0.0"
            }
        }
    });
    stdin.write_all(format!("{request}\n").as_bytes()).await?;
    stdin.flush().await?;

    let mut response_line = String::new();
    BufReader::new(stdout).read_line(&mut response_line).await?;
    let response: serde_json::Value = serde_json::from_str(&response_line)?;
    let negotiated = response
        .pointer("/result/protocolVersion")
        .and_then(serde_json::Value::as_str)
        .expect("negotiated protocol version");
    assert_eq!(negotiated, "2025-11-25");
    println!("protocol-proof requested=2026-07-28 negotiated={negotiated}");

    child.kill().await?;
    child.wait().await?;
    Ok(())
}
