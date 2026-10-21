#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

//! Приёмка машинного гейта наружу (задача #3620): отправка клиенту с личного
//! аккаунта проходит только с независимой квитанцией (медиа) или вердиктом
//! `orch outbound check` (текст). Проверяется путь потребителя: отказ ДО сети
//! (счётчик мутаций фейкового бэкенда остаётся нулевым), проход с годной
//! квитанцией, исключение для собственного чата и fail-closed на нечитаемом
//! реестре адресатов.

use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use telegram_personal_mcp::{
    ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer, McpServerSettings,
};

const CLIENT_CHAT: &str = "-100777001";
/// Чат, который знает фикстура бэкенда: положительный путь проверяется на реальном адресате.
const FIXTURE_CLIENT_CHAT: &str = "-202";

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs() as i64)
        .unwrap_or(0)
}

fn write_targets(dir: &Path, entries: &[&str]) {
    let targets: Vec<Value> = entries
        .iter()
        .map(|chat_id| json!({"chat_id": chat_id, "kind": "chat", "source": "test"}))
        .collect();
    std::fs::write(
        dir.join("client-targets.json"),
        json!({
            "schema": "telegram-client-targets/v1",
            "generated_at": "2026-10-21T00:00:00Z",
            "targets": targets,
        })
        .to_string(),
    )
    .expect("client targets registry");
}

fn canonical(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn text_sha256(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical(text).as_bytes());
    format!("{:x}", hasher.finalize())
}

fn write_verdict(dir: &Path, text: &str) -> PathBuf {
    let path = dir.join("outbound-verdict.json");
    std::fs::write(
        &path,
        json!({
            "schema": "orch-outbound-verdict/v1",
            "ok": true,
            "addressee": "client",
            "route": "model",
            "model": "deepseek-v4.1-flash",
            "text_sha256": text_sha256(text),
            "at_unix": now_unix(),
            "independent": true,
            "gate_rev": 3,
            "reason": null,
        })
        .to_string(),
    )
    .expect("verdict");
    path
}

fn write_receipt(dir: &Path, sha256: &str, bytes: u64) -> PathBuf {
    let path = dir.join("review-receipt.json");
    std::fs::write(
        &path,
        json!({
            "schema": "independent-review-receipt/v1",
            "verdict": "pass",
            "reviewer": {"agent": "critic", "model": "deepseek-v4.1-flash", "route": "agm"},
            "checks": {"text_readback": true, "defects": []},
            "files": [{"sha256": sha256, "bytes": bytes}],
            "at": now_unix() - 30,
        })
        .to_string(),
    )
    .expect("receipt");
    path
}

fn error_code(result: &rmcp::model::CallToolResult) -> Option<&str> {
    result
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/error/code"))
        .and_then(Value::as_str)
}

#[tokio::test]
async fn client_text_without_verdict_is_refused_before_network() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    write_targets(temp.path(), &[CLIENT_CHAT]);
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
        ApprovalPolicy {
            approval_chat_id: 700,
            approval_user_id: 700,
            ttl_seconds: 3_600,
            poll_timeout_seconds: 1,
            owner_grant_file: None,
            owner_permission_file: None,
        },
        bot,
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let refused = client
        .call_tool(CallToolRequestParams::new("prepare_message").with_arguments(
            json!({
                "chat_id": CLIENT_CHAT,
                "text": "Публикация в канал клиента без независимого вердикта",
                "source": {"kind": "self_originated"}
            })
            .as_object()
            .expect("object")
            .clone(),
        ))
        .await?;
    assert_eq!(refused.is_error, Some(true));
    assert_eq!(error_code(&refused), Some("OUTBOUND_VERDICT_MISSING"));
    assert_eq!(backend.mutation_call_counts().await, (0, 0));

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn client_text_with_bound_verdict_is_prepared() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    // Класс «клиент» проверяем на чате, который бэкенд знает: отказ гейта и отказ резолвера
    // различаются кодом, поэтому положительный путь требует реального адресата фикстуры.
    write_targets(temp.path(), &[FIXTURE_CLIENT_CHAT]);
    let text = "Текст поста, прошедший независимый проход";
    let verdict = write_verdict(temp.path(), text);
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
        ApprovalPolicy {
            approval_chat_id: 700,
            approval_user_id: 700,
            ttl_seconds: 3_600,
            poll_timeout_seconds: 1,
            owner_grant_file: None,
            owner_permission_file: None,
        },
        bot,
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let prepared = client
        .call_tool(CallToolRequestParams::new("prepare_message").with_arguments(
            json!({
                "chat_id": FIXTURE_CLIENT_CHAT,
                "text": text,
                "source": {"kind": "self_originated"},
                "outbound_verdict": verdict,
            })
            .as_object()
            .expect("object")
            .clone(),
        ))
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    assert!(
        prepared
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/data/draft_id"))
            .and_then(Value::as_str)
            .is_some(),
        "prepare_message must return a draft id"
    );
    assert_eq!(backend.mutation_call_counts().await, (0, 0));

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn client_media_without_receipt_is_refused_before_network() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    write_targets(temp.path(), &[CLIENT_CHAT]);
    let artifact = temp.path().join("frame.png");
    let bytes = b"\x89PNG\r\n\x1a\nnot-really-a-frame";
    std::fs::write(&artifact, bytes)?;
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
        ApprovalPolicy {
            approval_chat_id: 700,
            approval_user_id: 700,
            ttl_seconds: 3_600,
            poll_timeout_seconds: 1,
            owner_grant_file: None,
            owner_permission_file: None,
        },
        bot,
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let refused = client
        .call_tool(CallToolRequestParams::new("prepare_file_message").with_arguments(
            json!({
                "chat_id": CLIENT_CHAT,
                "file_path": artifact,
                "caption": "Кадр без независимого критика",
                "mode": "photo",
                "approved_sha256": sha256,
            })
            .as_object()
            .expect("object")
            .clone(),
        ))
        .await?;
    assert_eq!(refused.is_error, Some(true));
    assert_eq!(error_code(&refused), Some("MEDIA_REVIEW_RECEIPT_MISSING"));
    assert_eq!(backend.mutation_call_counts().await, (0, 0));

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn client_media_with_bound_receipt_is_prepared() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    write_targets(temp.path(), &[FIXTURE_CLIENT_CHAT]);
    // Файл обязан лежать в управляемом кеше вложений: берём каталог через download_attachment
    // (тот же путь, что у живого потребителя), иначе отказ придёт не от гейта.
    let downloaded = client
        .call_tool(CallToolRequestParams::new("download_attachment").with_arguments(
            json!({"chat_id": "@alex", "message_id": 11}).as_object().expect("object").clone(),
        ))
        .await?;
    let downloaded_path = downloaded
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/local_path"))
        .and_then(Value::as_str)
        .expect("download path");
    let artifact = Path::new(downloaded_path)
        .parent()
        .expect("download directory")
        .join("approved-frame.png");
    let bytes = b"\x89PNG\r\n\x1a\napproved-frame";
    std::fs::write(&artifact, bytes)?;
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    let receipt = write_receipt(temp.path(), &sha256, bytes.len() as u64);
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
        ApprovalPolicy {
            approval_chat_id: 700,
            approval_user_id: 700,
            ttl_seconds: 3_600,
            poll_timeout_seconds: 1,
            owner_grant_file: None,
            owner_permission_file: None,
        },
        bot,
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let prepared = client
        .call_tool(CallToolRequestParams::new("prepare_file_message").with_arguments(
            json!({
                "chat_id": FIXTURE_CLIENT_CHAT,
                "file_path": artifact,
                "caption": "Кадр, прошедший независимого критика",
                "mode": "photo",
                "approved_sha256": sha256,
                "review_receipt": receipt,
            })
            .as_object()
            .expect("object")
            .clone(),
        ))
        .await?;
    assert_eq!(
        prepared.is_error,
        Some(false),
        "prepare_file_message rejected: {:?}",
        prepared.structured_content
    );
    assert_eq!(backend.mutation_call_counts().await, (0, 0));

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn owner_chat_stays_exempt_from_both_gates() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    write_targets(temp.path(), &[CLIENT_CHAT]);
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
        ApprovalPolicy {
            approval_chat_id: 700,
            approval_user_id: 700,
            ttl_seconds: 3_600,
            poll_timeout_seconds: 1,
            owner_grant_file: None,
            owner_permission_file: None,
        },
        bot,
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let prepared = client
        .call_tool(CallToolRequestParams::new("prepare_message").with_arguments(
            json!({
                "chat_id": "me",
                "text": "Заметка себе: гейт клиентских адресатов меня не касается",
                "source": {"kind": "self_originated"}
            })
            .as_object()
            .expect("object")
            .clone(),
        ))
        .await?;
    assert_eq!(prepared.is_error, Some(false));

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn unreadable_targets_registry_fails_closed() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    std::fs::write(temp.path().join("client-targets.json"), "{not json")?;
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
        ApprovalPolicy {
            approval_chat_id: 700,
            approval_user_id: 700,
            ttl_seconds: 3_600,
            poll_timeout_seconds: 1,
            owner_grant_file: None,
            owner_permission_file: None,
        },
        bot,
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let refused = client
        .call_tool(CallToolRequestParams::new("prepare_message").with_arguments(
            json!({
                "chat_id": "-100999000",
                "text": "Любая отправка при сломанном реестре адресатов",
                "source": {"kind": "self_originated"}
            })
            .as_object()
            .expect("object")
            .clone(),
        ))
        .await?;
    assert_eq!(refused.is_error, Some(true));
    assert_eq!(error_code(&refused), Some("CLIENT_TARGETS_REGISTRY_UNREADABLE"));
    assert_eq!(backend.mutation_call_counts().await, (0, 0));

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
