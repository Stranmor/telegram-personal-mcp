use anyhow::{Context, ensure};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    ApprovalAction, ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer,
    McpServerSettings, SendApprovalState, SendLedger, SendLedgerReader,
};

const APPROVAL_CHAT_ID: i64 = 700;
const APPROVAL_USER_ID: i64 = 700;
const DEMO_CHAT_ID: i64 = 202;
const DEMO_CHAT_REF: &str = "-202";
const SOURCE_MESSAGE_ID: i64 = 22;
const SOURCE_MESSAGE_ID_I32: i32 = 22;
const CONTEXT_MESSAGE: &str = "Release candidate passed all health checks.";
const SOURCE_MESSAGE: &str = "Production deploy is ready. Should I start now?";
const REPLY_TEXT: &str = "Start at 18:00 UTC. Roll back immediately if any health check fails.";

fn tool_data(result: &rmcp::model::CallToolResult) -> anyhow::Result<&Value> {
    ensure!(result.is_error != Some(true), "MCP tool returned an error: {result:?}");
    result
        .structured_content
        .as_ref()
        .and_then(|value| value.get("data"))
        .context("MCP tool result did not contain structured data")
}

fn required_str<'a>(value: &'a Value, pointer: &str) -> anyhow::Result<&'a str> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .with_context(|| format!("missing string at {pointer}"))
}

fn required_i64(value: &Value, pointer: &str) -> anyhow::Result<i64> {
    value
        .pointer(pointer)
        .and_then(Value::as_i64)
        .with_context(|| format!("missing integer at {pointer}"))
}

/// Open the reusable state reader, retrying while a busy host holds the write lock.
async fn open_state_reader(state_db_path: &std::path::Path) -> anyhow::Result<SendLedgerReader> {
    const ATTEMPTS: usize = 10;
    for attempt in 1..=ATTEMPTS {
        match SendLedger::new(state_db_path.to_path_buf()).reader() {
            Ok(reader) => return Ok(reader),
            Err(error) if attempt < ATTEMPTS && is_database_busy(&error) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(error) => return Err(error).context("open the approval state reader"),
        }
    }
    unreachable!("the retry loop returns on its last attempt")
}

fn is_database_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::DatabaseBusy
    )
}

/// How long the demo waits for the approval worker to move the card to `sent`.
const SENT_WAIT: Duration = Duration::from_secs(60);

/// Call one MCP tool, retrying the state store's bounded retryable failures.
///
/// A store write that loses the race for SQLite's write lock on a host stalled
/// on I/O comes back as `STATE_STORE_ERROR` with `retryable: true`; the example
/// is a client of that contract, so it retries instead of failing on host load.
async fn call_tool(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &'static str,
    arguments: &Value,
) -> anyhow::Result<rmcp::model::CallToolResult> {
    const ATTEMPTS: usize = 6;
    for attempt in 1..=ATTEMPTS {
        let result = client
            .call_tool(
                CallToolRequestParams::new(name).with_arguments(
                    arguments
                        .as_object()
                        .with_context(|| format!("{name} arguments"))?
                        .clone(),
                ),
            )
            .await?;
        if result.is_error != Some(true) || attempt == ATTEMPTS || !is_retryable(&result) {
            return Ok(result);
        }
        tokio::time::sleep(Duration::from_millis(200 * attempt as u64)).await;
    }
    unreachable!("the retry loop returns on its last attempt")
}

fn is_retryable(result: &rmcp::model::CallToolResult) -> bool {
    result
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/error/retryable"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

async fn wait_for_sent(state_db_path: &std::path::Path, approval_id: &str) -> anyhow::Result<()> {
    // One SQLite connection for the whole poll cycle, and a wall-clock deadline
    // instead of a tick count: a host stalled on I/O delays the approval worker,
    // and the demo must not turn that delay into a failure.
    let reader = open_state_reader(state_db_path).await?;
    let deadline = tokio::time::Instant::now() + SENT_WAIT;
    while tokio::time::Instant::now() < deadline {
        if reader
            .get_approval(approval_id)?
            .is_some_and(|approval| approval.state == SendApprovalState::Sent)
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    anyhow::bail!("approval did not reach sent state within {SENT_WAIT:?}")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    ensure!(
        backend
            .set_fixture_message_text(DEMO_CHAT_ID, 21, Some(CONTEXT_MESSAGE.to_string()), None,)
            .await,
        "fixture context message was unavailable"
    );
    ensure!(
        backend
            .set_fixture_message_text(
                DEMO_CHAT_ID,
                SOURCE_MESSAGE_ID_I32,
                Some(SOURCE_MESSAGE.to_string()),
                None,
            )
            .await,
        "fixture source message was unavailable"
    );
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: state_db_path.clone(),
        },
        ApprovalPolicy {
            approval_chat_id: APPROVAL_CHAT_ID,
            approval_user_id: APPROVAL_USER_ID,
            ttl_seconds: 3_600,
            poll_timeout_seconds: 1,
            owner_grant_file: None,
            owner_permission_file: None,
        },
        bot.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let worker = server.start_approval_worker()?.context("approval worker was not started")?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let dialogs = call_tool(
        &client,
        "search_dialogs",
        &json!({
            "query": "Project Room",
            "limit": 1,
            "scan_limit": 10
        }),
    )
    .await?;
    let dialogs_data = tool_data(&dialogs)?;
    ensure!(required_str(dialogs_data, "/matches/0/dialog/title")? == "Project Room");
    let dialog_chat_ref = required_str(dialogs_data, "/matches/0/dialog/chat_ref")?.to_string();
    let dialog_canonical_chat_id =
        required_str(dialogs_data, "/matches/0/dialog/canonical_chat_id")?.to_string();
    ensure!(dialog_chat_ref == DEMO_CHAT_REF, "fixture dialog reference changed");
    ensure!(dialog_canonical_chat_id == DEMO_CHAT_REF, "fixture canonical dialog identity changed");
    let dialog_scanned_count = required_i64(dialogs_data, "/scanned_count")?;

    let search = call_tool(
        &client,
        "search_messages",
        &json!({ "chat_id": dialog_chat_ref, "query": "deploy", "limit": 10 }),
    )
    .await?;
    let search_data = tool_data(&search)?;
    let source_message_id = required_i64(search_data, "/messages/0/message_id")?;
    ensure!(source_message_id == SOURCE_MESSAGE_ID, "fixture source message changed");

    let context = call_tool(
        &client,
        "read_context",
        &json!({
            "chat_id": DEMO_CHAT_REF,
            "message_id": source_message_id,
            "before": 2,
            "after": 2
        }),
    )
    .await?;
    let context_data = tool_data(&context)?;
    ensure!(
        required_i64(context_data, "/target/message_id")? == source_message_id,
        "context was not bound to the searched message"
    );
    ensure!(required_str(context_data, "/target/text")? == SOURCE_MESSAGE);
    ensure!(required_str(context_data, "/target/sender_ref")? == "101");
    ensure!(required_str(context_data, "/older/0/text")? == CONTEXT_MESSAGE);

    let prepared = call_tool(
        &client,
        "prepare_message",
        &json!({
            "chat_id": DEMO_CHAT_REF,
            "text": REPLY_TEXT,
            "source": {
                "kind": "telegram_message",
                "relation": "reply_context",
                "chat_id": DEMO_CHAT_REF,
                "message_id": SOURCE_MESSAGE_ID_I32
            },
            "reply_to_message_id": source_message_id
        }),
    )
    .await?;
    let prepared_data = tool_data(&prepared)?;
    let draft_id = required_str(prepared_data, "/draft_id")?.to_string();
    let payload_sha256 = required_str(prepared_data, "/payload_sha256")?.to_string();

    let bypass = call_tool(
        &client,
        "send_prepared",
        &json!({
            "draft_id": draft_id,
            "approved_payload_sha256": payload_sha256
        }),
    )
    .await?;
    ensure!(bypass.is_error == Some(true), "direct send unexpectedly succeeded");
    ensure!(
        bypass
            .structured_content
            .as_ref()
            .and_then(|value| value.pointer("/error/code"))
            .and_then(Value::as_str)
            == Some("APPROVAL_REQUIRED"),
        "direct send failed for an unexpected reason"
    );

    let requested = call_tool(
        &client,
        "request_send_approval",
        &json!({
            "draft_id": draft_id,
            "approved_payload_sha256": payload_sha256
        }),
    )
    .await?;
    let requested_data = tool_data(&requested)?;
    ensure!(required_str(requested_data, "/state")? == "pending");
    let approval_id = required_str(requested_data, "/approval_id")?.to_string();
    let approval_message_id = required_i64(requested_data, "/approval_message_id")?;

    let cards = bot.sent_cards().await;
    let card = cards.first().context("approval bot did not receive a card")?;
    ensure!(cards.len() == 1, "approval request created duplicate cards");
    ensure!(card.card.approval.chat_id == DEMO_CHAT_REF);
    ensure!(card.card.approval.target.display_name == "Project Room");
    let source_message_id_i32 = i32::try_from(source_message_id)
        .context("source message id does not fit Telegram's i32 reply field")?;
    ensure!(card.card.approval.reply_to_message_id == Some(source_message_id_i32));
    ensure!(card.card.approval.text == REPLY_TEXT);
    ensure!(card.card.approval.payload_sha256 == payload_sha256);
    ensure!(backend.sent_messages().await.is_empty());

    bot.push_decision(
        &approval_id,
        ApprovalAction::Approve,
        APPROVAL_USER_ID,
        APPROVAL_CHAT_ID,
        approval_message_id,
    )
    .await;
    wait_for_sent(&state_db_path, &approval_id).await?;

    bot.push_decision(
        &approval_id,
        ApprovalAction::Approve,
        APPROVAL_USER_ID,
        APPROVAL_CHAT_ID,
        approval_message_id,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(80)).await;
    let delivery_count = backend.sent_messages().await.len();
    ensure!(delivery_count == 1, "duplicate callback created another delivery");

    let status =
        call_tool(&client, "get_send_approval", &json!({ "approval_id": approval_id })).await?;
    let status_data = tool_data(&status)?;
    ensure!(required_str(status_data, "/state")? == "sent");
    let target_message_id = required_i64(status_data, "/target_message_id")?;

    let readback = call_tool(
        &client,
        "read_context",
        &json!({
            "chat_id": DEMO_CHAT_REF,
            "message_id": target_message_id,
            "before": 1,
            "after": 1
        }),
    )
    .await?;
    let readback_data = tool_data(&readback)?;
    ensure!(required_i64(readback_data, "/target/message_id")? == target_message_id);
    ensure!(required_str(readback_data, "/target/text")? == REPLY_TEXT);

    println!(
        "{}",
        serde_json::to_string(&json!({
            "proof": "telegram-personal-mcp-agent-workspace-v4",
            "fixture": "deterministic_no_credentials",
            "mcp_calls": [
                "search_dialogs",
                "search_messages",
                "read_context",
                "prepare_message",
                "send_prepared",
                "request_send_approval",
                "get_send_approval",
                "read_context"
            ],
            "dialog_title": "Project Room",
            "dialog_chat_ref": dialog_chat_ref,
            "dialog_canonical_chat_id": dialog_canonical_chat_id,
            "dialog_scanned_count": dialog_scanned_count,
            "chat_id": DEMO_CHAT_REF,
            "canonical_chat_id": card.card.approval.chat_id,
            "source_message_id": source_message_id,
            "source_message": SOURCE_MESSAGE,
            "context_message": CONTEXT_MESSAGE,
            "reply_text": REPLY_TEXT,
            "payload_sha256": payload_sha256,
            "direct_bypass": "blocked",
            "approval_card_count": cards.len(),
            "approval_state": "sent",
            "target_message_id": target_message_id,
            "delivery_count": delivery_count,
            "backend_readback": "confirmed"
        }))?
    );

    client.cancel().await?;
    server_task.await??;
    worker.abort();
    let _ = worker.await;
    Ok(())
}
