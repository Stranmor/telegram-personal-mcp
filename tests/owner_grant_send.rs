#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::json;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use telegram_personal_mcp::{
    ApprovalPolicy, FakeApprovalBot, FakeTelegramBackend, McpServer, McpServerSettings,
    PreparedSendState, SendApprovalState, SendLedger,
};
use telegram_personal_mcp::domain::{
    ContactSummary, PreparedNonTelegramSource, PreparedOutboundSource, PreparedSendTarget,
};
use telegram_personal_mcp::telegram::DialogSummary;

fn grant_policy(grant_path: std::path::PathBuf) -> ApprovalPolicy {
    ApprovalPolicy {
        approval_chat_id: 700,
        approval_user_id: 700,
        ttl_seconds: 3_600,
        poll_timeout_seconds: 1,
        owner_grant_file: Some(grant_path),
    }
}

fn write_grant(path: &std::path::Path, remaining: u32) -> anyhow::Result<()> {
    let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
    let grant = serde_json::json!({
        "granted_by": 700,
        "granted_at_unix": now,
        "expires_at_unix": now + 3_600,
        "remaining_messages": remaining,
        "allow_peers": ["@alex", "alex", "Alex", "101"],
        "reason": "owner-grant send contract test",
    });
    std::fs::write(path, serde_json::to_vec_pretty(&grant)?)?;
    Ok(())
}

fn grant_remaining(path: &std::path::Path) -> anyhow::Result<u32> {
    let raw = std::fs::read_to_string(path)?;
    let grant: serde_json::Value = serde_json::from_str(&raw)?;
    Ok(grant
        .get("remaining_messages")
        .and_then(serde_json::Value::as_u64)
        .expect("remaining_messages") as u32)
}

#[tokio::test]
async fn owner_grant_send_reaches_sent() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let grant_path = temp.path().join("grant.json");
    write_grant(&grant_path, 10)?;
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: state_db_path.clone(),
        },
        grant_policy(grant_path.clone()),
        bot.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Owner-grant contract: first send",
                    "source": { "kind": "self_originated" },
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
        .expect("payload hash")
        .to_string();

    let sent = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval").with_arguments(
                json!({
                    "draft_id": draft_id,
                    "approved_payload_sha256": payload_sha256,
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(sent.is_error, Some(false));
    let state = sent
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/state"))
        .and_then(|value| value.as_str())
        .expect("approval state");
    assert_eq!(state, "sent");
    let approval_id = sent
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("approval id")
        .to_string();
    let target_message_id = sent
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/target_message_id"))
        .and_then(|value| value.as_i64())
        .expect("target message id");

    assert_eq!(backend.sent_messages().await.len(), 1);
    assert!(bot.sent_cards().await.is_empty());
    assert_eq!(grant_remaining(&grant_path)?, 9);

    let ledger = SendLedger::new(state_db_path);
    let approval = ledger.get_approval(&approval_id)?.expect("approval record");
    assert_eq!(approval.state, SendApprovalState::Sent);
    assert_eq!(approval.target_message_id, Some(target_message_id as i32));
    let prepared_record = ledger.get(&draft_id)?.expect("prepared record");
    assert_eq!(prepared_record.public.state, PreparedSendState::Sent);
    assert_eq!(prepared_record.public.message_id, Some(target_message_id as i32));

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn owner_grant_repeat_returns_same_receipt_without_second_send() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let grant_path = temp.path().join("grant.json");
    write_grant(&grant_path, 10)?;
    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: state_db_path.clone(),
        },
        grant_policy(grant_path.clone()),
        bot.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Owner-grant contract: idempotent repeat",
                    "source": { "kind": "self_originated" },
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
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
        .expect("payload hash")
        .to_string();
    let arguments = json!({
        "draft_id": draft_id,
        "approved_payload_sha256": payload_sha256,
    })
    .as_object()
    .expect("object")
    .clone();

    let first = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval")
                .with_arguments(arguments.clone()),
        )
        .await?;
    assert_eq!(first.is_error, Some(false));
    let first_approval_id = first
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("first approval id")
        .to_string();
    let first_target = first
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/target_message_id"))
        .and_then(|value| value.as_i64())
        .expect("first target");
    assert_eq!(backend.sent_messages().await.len(), 1);
    assert_eq!(grant_remaining(&grant_path)?, 9);

    let second = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval")
                .with_arguments(arguments.clone()),
        )
        .await?;
    assert_eq!(second.is_error, Some(false));
    let second_approval_id = second
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/approval_id"))
        .and_then(|value| value.as_str())
        .expect("second approval id");
    let second_target = second
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/target_message_id"))
        .and_then(|value| value.as_i64())
        .expect("second target");
    let second_state = second
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/data/state"))
        .and_then(|value| value.as_str())
        .expect("second state");
    assert_eq!(second_approval_id, first_approval_id);
    assert_eq!(second_target, first_target);
    assert_eq!(second_state, "sent");
    assert_eq!(backend.sent_messages().await.len(), 1);
    assert!(bot.sent_cards().await.is_empty());
    assert_eq!(
        grant_remaining(&grant_path)?,
        9,
        "the repeat must not consume a second grant message"
    );

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn approval_state_write_failure_is_unrecorded_not_retryable() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let state_db_path = temp.path().join("state.sqlite");
    let grant_path = temp.path().join("grant.json");
    write_grant(&grant_path, 10)?;

    SendLedger::new(state_db_path.clone()).get("init")?;
    {
        let connection = rusqlite::Connection::open(&state_db_path)?;
        connection.execute_batch(
            "CREATE TRIGGER block_approval_sent
             BEFORE UPDATE OF state ON send_approvals
             WHEN NEW.state = 'sent'
             BEGIN
                 SELECT RAISE(ABORT, 'injected approval sent failure');
             END;",
        )?;
    }

    let server = McpServer::with_approval(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: state_db_path.clone(),
        },
        grant_policy(grant_path),
        bot.clone(),
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@alex",
                    "text": "Owner-grant contract: unrecorded state",
                    "source": { "kind": "self_originated" },
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
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
        .expect("payload hash")
        .to_string();

    let failed = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval").with_arguments(
                json!({
                    "draft_id": draft_id,
                    "approved_payload_sha256": payload_sha256,
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        )
        .await?;
    assert_eq!(failed.is_error, Some(true));
    let error = failed.structured_content.as_ref().expect("error payload");
    assert_eq!(
        error.pointer("/error/code").and_then(|value| value.as_str()),
        Some("DELIVERY_STATE_UNRECORDED")
    );
    assert_eq!(
        error.pointer("/error/retryable").and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        error
            .pointer("/error/delivery_confirmed")
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    let message =
        error.pointer("/error/message").and_then(|value| value.as_str()).unwrap_or("");
    assert!(message.contains("was sent"), "message must confirm delivery: {message}");
    assert!(
        message.contains("do not resend"),
        "message must forbid a retry that would duplicate: {message}"
    );

    assert_eq!(
        backend.sent_messages().await.len(),
        1,
        "the Telegram send happened once even though the receipt write failed"
    );
    let ledger = SendLedger::new(state_db_path);
    let prepared_record = ledger.get(&draft_id)?.expect("prepared record");
    assert_eq!(prepared_record.public.state, PreparedSendState::Sent);
    assert_eq!(
        prepared_record.public.message_id,
        backend.sent_messages().await.first().map(|sent| sent.message_id)
    );

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[test]
fn approval_transition_refusal_names_states() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ledger = SendLedger::new(temp.path().join("state.sqlite"));
    let prepared = ledger
        .prepare(
            7,
            "101".to_string(),
            PreparedSendTarget {
                display_name: "Alex".to_string(),
                username: Some("alex".to_string()),
                kind: "user".to_string(),
            },
            PreparedOutboundSource::NonTelegram(PreparedNonTelegramSource::SelfOriginated),
            "Hello".to_string(),
            None,
            "abc".to_string(),
        )
        .expect("prepare");
    let approval = ledger
        .create_or_get_approval(&prepared.public.draft_id, "abc", 700, 700, 3_600)
        .expect("approval")
        .public;
    assert_eq!(approval.state, SendApprovalState::DispatchingCard);
    let error = ledger
        .mark_approval_sent(&approval.approval_id, 42)
        .expect_err("dispatching_card must not transition straight to sent");
    let text = error.to_string();
    assert!(text.contains("dispatching_card"), "names the source state: {text}");
    assert!(text.contains("sent"), "names the target state: {text}");
    assert!(
        !text.contains("Conversion error"),
        "must not look like a type conversion: {text}"
    );
}

fn write_grant_with_peers(
    path: &std::path::Path,
    remaining: u32,
    peers: &[&str],
) -> anyhow::Result<()> {
    let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
    let grant = serde_json::json!({
        "granted_by": 700,
        "granted_at_unix": now,
        "expires_at_unix": now + 3_600,
        "remaining_messages": remaining,
        "allow_peers": peers,
        "reason": "owner-grant contract test",
    });
    std::fs::write(path, serde_json::to_vec_pretty(&grant)?)?;
    Ok(())
}

fn botfather_dialog() -> DialogSummary {
    DialogSummary {
        chat_id: 93_372_553,
        canonical_chat_id: "93372553".to_string(),
        chat_ref: "@BotFather".to_string(),
        chat_kind: "user".to_string(),
        title: "BotFather".to_string(),
        username: Some("BotFather".to_string()),
        last_message_id: Some(1),
        last_message_text: Some("/start".to_string()),
        last_message_outgoing: Some(false),
        last_message_date: Some(1_700_000_001),
        pinned: false,
        unread_mark: false,
        unread_count: 0,
        archived: false,
        folder_id: Some(0),
    }
}

fn botfather_contact() -> ContactSummary {
    ContactSummary {
        user_id: 93_372_553,
        first_name: "BotFather".to_string(),
        last_name: None,
        display_name: "BotFather".to_string(),
        username: Some("BotFather".to_string()),
        phone: None,
        is_contact: false,
        mutual_contact: false,
        bot: true,
        verified: true,
        premium: false,
        scam: false,
        restricted: false,
    }
}

async fn start_grant_server(
    backend: Arc<FakeTelegramBackend>,
    bot: Arc<FakeApprovalBot>,
    temp: &tempfile::TempDir,
    grant_path: std::path::PathBuf,
) -> anyhow::Result<(
    rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tokio::task::JoinHandle<anyhow::Result<()>>,
)> {
    let server = McpServer::with_approval(
        backend,
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
        grant_policy(grant_path),
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

/// A grant that names @BotFather — even written directly into the file — must
/// never let a send reach BotFather without the owner's approval card.
#[tokio::test]
async fn owner_grant_never_covers_botfather() -> anyhow::Result<()> {
    let backend = Arc::new(FakeTelegramBackend::fixture());
    backend.add_fixture_dialog(botfather_dialog()).await;
    backend.add_fixture_contact(botfather_contact()).await;
    let bot = Arc::new(FakeApprovalBot::default());
    let temp = tempfile::tempdir()?;
    let grant_path = temp.path().join("grant.json");
    write_grant_with_peers(&grant_path, 10, &["@BotFather", "93372553", "botfather"])?;
    let (client, server_task) =
        start_grant_server(backend.clone(), bot.clone(), &temp, grant_path.clone()).await?;

    let prepared = client
        .call_tool(
            CallToolRequestParams::new("prepare_message").with_arguments(
                json!({
                    "chat_id": "@BotFather",
                    "text": "Owner-grant contract: BotFather stays on the card",
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

    let requested = client
        .call_tool(
            CallToolRequestParams::new("request_send_approval").with_arguments(
                json!({ "draft_id": draft_id, "approved_payload_sha256": payload_sha256 })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(requested.is_error, Some(false));
    let requested = requested.structured_content.expect("send approval");
    let state = requested
        .pointer("/data/state")
        .and_then(serde_json::Value::as_str)
        .expect("state");
    assert_ne!(state, "sent", "a BotFather send must never be auto-sent");
    assert_eq!(state, "pending", "the approval card is the only path");
    assert_eq!(backend.sent_messages().await.len(), 0, "nothing reached Telegram");
    assert_eq!(bot.sent_cards().await.len(), 1, "the owner got the card");
    assert_eq!(grant_remaining(&grant_path)?, 10, "a refused grant is not consumed");

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

/// A mutation grant is checked against the exact target peer: a grant for
/// another chat, or one whose name merely occurs inside the action JSON (the
/// button label "Approve"), must fall back to a card, while a grant naming the
/// target chat id applies.
#[tokio::test]
async fn owner_grant_mutation_matches_the_target_peer_not_a_substring() -> anyhow::Result<()> {
    // press_button on the callback button of message 11 in @alex (chat 101).
    for (peers, expect_card) in
        [(vec!["-202"], true), (vec!["Approve"], true), (vec!["101"], false)]
    {
        let backend = Arc::new(FakeTelegramBackend::fixture());
        let bot = Arc::new(FakeApprovalBot::default());
        let temp = tempfile::tempdir()?;
        let grant_path = temp.path().join("grant.json");
        write_grant_with_peers(&grant_path, 10, &peers)?;
        let (client, server_task) =
            start_grant_server(backend.clone(), bot.clone(), &temp, grant_path.clone()).await?;

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
                    json!({ "mutation_id": mutation_id, "approved_payload_sha256": payload_sha256 })
                        .as_object()
                        .expect("object")
                        .clone(),
                ),
            )
            .await?;
        assert_eq!(requested.is_error, Some(false));
        let requested = requested.structured_content.expect("mutation approval");
        let state = requested
            .pointer("/data/state")
            .and_then(serde_json::Value::as_str)
            .expect("state");
        if expect_card {
            assert_eq!(state, "pending", "grant {peers:?} must not authorize the mutation");
            assert_eq!(
                bot.sent_mutation_cards().await.len(),
                1,
                "the owner got the card for {peers:?}"
            );
            assert_eq!(
                backend.pressed_callback_buttons().await.len(),
                0,
                "no button was pressed for {peers:?}"
            );
            assert_eq!(grant_remaining(&grant_path)?, 10, "a refused grant is not consumed");
        } else {
            assert_eq!(state, "applied", "an exact target grant applies");
            assert_eq!(bot.sent_mutation_cards().await.len(), 0, "no card when covered");
            assert_eq!(
                backend.pressed_callback_buttons().await.len(),
                1,
                "the button was pressed once"
            );
            assert_eq!(grant_remaining(&grant_path)?, 9, "the grant was consumed once");
        }

        client.cancel().await?;
        server_task.await??;
    }
    Ok(())
}
