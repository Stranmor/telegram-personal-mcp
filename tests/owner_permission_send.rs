#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

//! Live-path acceptance for the durable owner-permission consumer (#2369):
//! the real server object, a real orch-format mirror file and the real
//! writer-shared sidecar lock — no network, no approval card unless the
//! permission itself fails to cover the action.

use fs2::FileExt;
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use telegram_personal_mcp::{
    ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer, McpServerSettings,
};

fn now_unix() -> i64 {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_secs())
        .expect("timestamp")
}

fn permission_policy(permission_path: PathBuf) -> ApprovalPolicy {
    ApprovalPolicy {
        approval_chat_id: 700,
        approval_user_id: 700,
        ttl_seconds: 3_600,
        poll_timeout_seconds: 1,
        owner_grant_file: None,
        owner_permission_file: Some(permission_path),
    }
}

/// Write the orch-exported mirror in the exact writer format: the consumer
/// trusts only a fresh `generated_at_unix`, the owner's evidence, a named
/// peer and a mandatory `expires_at_unix`.
fn write_permission_mirror(
    path: &std::path::Path,
    generated_at_unix: i64,
    expires_at_unix: Option<i64>,
    peers: &[&str],
    actions: &[&str],
) -> anyhow::Result<()> {
    let mirror = json!({
        "v": 1,
        "generated_at_unix": generated_at_unix,
        "permissions": [{
            "id": 7,
            "subject": "e2e owner permission",
            "title": "Владелец разрешил отправку без карточки",
            "peers": peers,
            "actions": actions,
            "evidence": {
                "by": "owner",
                "msg_id": 1107,
                "quote": "отправляй без карточки",
            },
            "expires_at_unix": expires_at_unix,
            "max_uses": 5,
        }],
    });
    std::fs::write(path, serde_json::to_vec_pretty(&mirror)?)?;
    Ok(())
}

type Client = rmcp::service::RunningService<rmcp::RoleClient, ()>;

async fn start_permission_server(
    backend: Arc<FakeTelegramBackend>,
    bot: Arc<FakeApprovalBot>,
    temp: &tempfile::TempDir,
    permission_path: PathBuf,
) -> anyhow::Result<(Client, tokio::task::JoinHandle<anyhow::Result<()>>)> {
    let server = McpServer::with_approval(
        backend,
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
        permission_policy(permission_path),
        bot,
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    Ok((client, server_task))
}

async fn prepare_and_request_send(
    client: &Client,
    text: &str,
) -> anyhow::Result<serde_json::Value> {
    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": text,
                    "source": { "kind": "self_originated" },
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared send");
    let draft_id = prepared
        .pointer("/data/draft_id")
        .and_then(serde_json::Value::as_str)
        .expect("draft id")
        .to_string();
    let payload_sha256 = prepared
        .pointer("/data/payload_sha256")
        .and_then(serde_json::Value::as_str)
        .expect("payload hash")
        .to_string();
    let sent = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval").with_arguments(
                json!({ "draft_id": draft_id, "approved_payload_sha256": payload_sha256 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(sent.is_error, Some(false));
    Ok(sent.structured_content.expect("send approval"))
}

#[tokio::test]
async fn owner_permission_covers_a_send_and_journals_the_use() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let permission_path = temp.path().join("owner-permissions.json");
    let now = now_unix();
    write_permission_mirror(
        &permission_path,
        now,
        Some(now + 3_600),
        &["@alex", "alex", "Alex", "101"],
        &["send_message"],
    )?;
    let (client, server_task) =
        start_permission_server(backend.clone(), bot.clone(), &temp, permission_path.clone()).await?;

    let approval =
        prepare_and_request_send(&client, "Owner-permission contract: covered send").await?;
    assert_eq!(
        approval.pointer("/data/state").and_then(serde_json::Value::as_str),
        Some("sent")
    );
    assert_eq!(backend.sent_messages().await.len(), 1, "the covered send reached Telegram");
    assert!(bot.sent_cards().await.is_empty(), "no card was needed");
    let journal = std::fs::read_to_string(permission_path.with_extension("json.log"))?;
    assert_eq!(journal.lines().count(), 1, "one use, one journal line");
    // After the send the writer-shared lock is free again: the orch writer can
    // export the next mirror without waiting.
    let lock_path = permission_path.with_extension("json.lock");
    let probe = std::fs::OpenOptions::new().read(true).write(true).open(&lock_path)?;
    probe.try_lock_exclusive().expect("the lease must be released after the send");

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn stale_mirror_leaves_the_approval_card_mandatory() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let permission_path = temp.path().join("owner-permissions.json");
    let now = now_unix();
    // The exporter stopped refreshing (orch down): older than ten writer ticks
    // means deny — an unexpired record in an outdated snapshot covers nothing.
    write_permission_mirror(
        &permission_path,
        now - 3_600,
        Some(now + 3_600),
        &["@alex", "alex", "Alex", "101"],
        &["send_message"],
    )?;
    let (client, server_task) =
        start_permission_server(backend.clone(), bot.clone(), &temp, permission_path.clone()).await?;

    let approval =
        prepare_and_request_send(&client, "Owner-permission contract: stale mirror").await?;
    assert_eq!(
        approval.pointer("/data/state").and_then(serde_json::Value::as_str),
        Some("pending"),
        "the approval card is the only path for a stale mirror"
    );
    assert_eq!(backend.sent_messages().await.len(), 0, "nothing reached Telegram");
    assert_eq!(bot.sent_cards().await.len(), 1, "the owner got the card");
    assert!(
        !permission_path.with_extension("json.log").exists(),
        "a denied check journals nothing"
    );

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn record_without_expiry_leaves_the_approval_card_mandatory() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let permission_path = temp.path().join("owner-permissions.json");
    write_permission_mirror(
        &permission_path,
        now_unix(),
        None,
        &["@alex", "alex", "Alex", "101"],
        &["send_message"],
    )?;
    let (client, server_task) =
        start_permission_server(backend.clone(), bot.clone(), &temp, permission_path.clone()).await?;

    let approval =
        prepare_and_request_send(&client, "Owner-permission contract: no expiry").await?;
    assert_eq!(
        approval.pointer("/data/state").and_then(serde_json::Value::as_str),
        Some("pending"),
        "an expiry-less record must never be unbounded"
    );
    assert_eq!(backend.sent_messages().await.len(), 0, "nothing reached Telegram");
    assert_eq!(bot.sent_cards().await.len(), 1, "the owner got the card");

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn owner_permission_covers_a_mutation() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let permission_path = temp.path().join("owner-permissions.json");
    let now = now_unix();
    write_permission_mirror(
        &permission_path,
        now,
        Some(now + 3_600),
        &["101"],
        &["press_callback_button"],
    )?;
    let (client, server_task) =
        start_permission_server(backend.clone(), bot.clone(), &temp, permission_path.clone()).await?;

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("press_button").with_arguments(
                json!({ "chatId": "@alex", "messageId": 11, "row": 0, "column": 0 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(prepared.is_error, Some(false));
    let prepared = prepared.structured_content.expect("prepared mutation");
    let mutation_id = prepared
        .pointer("/data/mutation_id")
        .and_then(serde_json::Value::as_str)
        .expect("mutation id")
        .to_string();
    let payload_sha256 = prepared
        .pointer("/data/payload_sha256")
        .and_then(serde_json::Value::as_str)
        .expect("payload hash")
        .to_string();
    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_mutation_approval").with_arguments(
                json!({
                    "purpose": "тест: разрешение владельца на нажатие",
                    "mutation_id": mutation_id,
                    "approved_payload_sha256": payload_sha256,
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(requested.is_error, Some(false));
    let requested = requested.structured_content.expect("mutation approval");
    assert_eq!(
        requested.pointer("/data/state").and_then(serde_json::Value::as_str),
        Some("applied"),
        "an exact target permission applies the mutation without a card"
    );
    assert_eq!(bot.sent_mutation_cards().await.len(), 0, "no card when covered");
    assert_eq!(backend.pressed_callback_buttons().await.len(), 1, "the button was pressed once");
    let journal = std::fs::read_to_string(permission_path.with_extension("json.log"))?;
    assert_eq!(journal.lines().count(), 1, "one use, one journal line");

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
