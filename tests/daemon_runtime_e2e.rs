#![cfg(unix)]

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use telegram_personal_mcp::telegram::MessageSummary;
use telegram_personal_mcp::{
    AccountMetadata, CacheSyncPhase, CachedMessageInput, GlobalUpdateCursor, MessageCache,
    MessageSource,
};

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}

const SIGTERM: i32 = 15;

struct DaemonCleanup {
    lock_path: Option<std::path::PathBuf>,
}

impl DaemonCleanup {
    fn new(lock_path: std::path::PathBuf) -> Self {
        Self { lock_path: Some(lock_path) }
    }

    fn disarm(&mut self) {
        self.lock_path = None;
    }
}

impl Drop for DaemonCleanup {
    fn drop(&mut self) {
        let Some(lock_path) = &self.lock_path else {
            return;
        };
        let Ok(pid) = std::fs::read_to_string(lock_path) else {
            return;
        };
        let Ok(pid) = pid.trim().parse::<i32>() else {
            return;
        };
        if pid > 1 {
            // SAFETY: cleanup is restricted to the PID written into this
            // test's isolated owner-only singleton lock.
            unsafe {
                kill(pid, SIGTERM);
            }
        }
    }
}

fn transport(
    binary: &str,
    daemon_dir: &Path,
    state_db: &Path,
    message_cache_db: &Path,
    message_cache_max_age_days: Option<u32>,
) -> anyhow::Result<TokioChildProcess> {
    Ok(TokioChildProcess::new(tokio::process::Command::new(binary).configure(|command| {
        command
            .arg("--demo")
            .arg("--state-db-path")
            .arg(state_db)
            .arg("--message-cache-db-path")
            .arg(message_cache_db)
            .env("TELEGRAM_MCP_DAEMON_DIR", daemon_dir)
            .env_remove("TELEGRAM_API_ID")
            .env_remove("TELEGRAM_API_HASH")
            .env_remove("TELEGRAM_MCP_SESSION_PATH")
            .env_remove("TELEGRAM_MCP_SESSION_CACHE_DIR");
        if let Some(max_age_days) = message_cache_max_age_days {
            command.arg("--message-cache-max-age-days").arg(max_age_days.to_string());
        }
    }))?)
}

async fn search_contract(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
) -> anyhow::Result<Value> {
    let result = client
        .call_tool(
            CallToolRequestParams::new("search_regex").with_arguments(
                json!({ "pattern": "CONTRACT", "chat_id": "@alex" })
                    .as_object()
                    .expect("object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(result.is_error, Some(false));
    Ok(result.structured_content.expect("structured cache result"))
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: &'static str,
    arguments: Value,
) -> anyhow::Result<Value> {
    let result = client
        .call_tool(
            CallToolRequestParams::new(tool)
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(result.is_error, Some(false), "{tool} should succeed");
    Ok(result.structured_content.expect("structured result"))
}

async fn wait_for_background_cache(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
) -> anyhow::Result<Value> {
    for _ in 0..100 {
        let result = search_contract(client).await?;
        if result.pointer("/data/returned_count").and_then(Value::as_u64).unwrap_or(0) >= 2 {
            return Ok(result);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("background cache did not become queryable before the E2E deadline")
}

async fn wait_for_file(path: &Path) -> anyhow::Result<()> {
    for _ in 0..100 {
        if path.exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("{} did not appear before the E2E deadline", path.display())
}

async fn wait_for_removal(path: &Path) -> anyhow::Result<()> {
    for _ in 0..100 {
        if !path.exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("{} was not removed before the E2E deadline", path.display())
}

#[tokio::test]
async fn stdio_proxies_share_one_persistent_daemon_cache_and_shutdown_cleanly() -> anyhow::Result<()>
{
    let temp = tempfile::tempdir()?;
    let binary = env!("CARGO_BIN_EXE_telegram-personal-mcp");
    let daemon_dir = temp.path().join("daemon");
    let state_db = temp.path().join("state.sqlite");
    let message_cache_db = temp.path().join("message-cache.sqlite");
    let socket = daemon_dir.join("daemon.sock");
    let lock = daemon_dir.join("daemon.lock");
    let log = daemon_dir.join("daemon.log");
    let identity = daemon_dir.join("daemon.identity");
    let mut cleanup = DaemonCleanup::new(lock.clone());

    let cache = MessageCache::open(&message_cache_db)?;
    cache.bind_account(&AccountMetadata {
        account_user_id: 7,
        display_name: Some("Fixture User".to_string()),
        username: Some("fixture_user".to_string()),
        observed_at_unix: 1_700_003_998,
    })?;
    cache.set_global_update_cursor(
        7,
        &GlobalUpdateCursor {
            pts: 118,
            qts: 7,
            date: 1_700_003_998,
            seq: 8,
            unread_count: Some(3),
            phase: CacheSyncPhase::Current,
            observed_at_unix: 1_700_003_998,
            last_error_code: None,
        },
    )?;
    let event_cursor = cache.incoming_event_window(7, None, 1)?.cursor.token;
    drop(cache);

    let first =
        ().serve(transport(binary, &daemon_dir, &state_db, &message_cache_db, None)?).await?;
    let first_cache = wait_for_background_cache(&first).await?;
    assert_eq!(
        first_cache.pointer("/data/coverage/account_user_id").and_then(Value::as_i64),
        Some(7)
    );
    wait_for_file(&lock).await?;
    wait_for_file(&socket).await?;
    wait_for_file(&identity).await?;
    let first_pid: i32 = std::fs::read_to_string(&lock)?.trim().parse()?;
    assert!(first_pid > 1);
    assert_eq!(std::fs::metadata(&daemon_dir)?.permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(&lock)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(&socket)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(&log)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(&identity)?.permissions().mode() & 0o777, 0o600);
    let identity_text = std::fs::read_to_string(&identity)?;
    assert!(identity_text.starts_with("telegram-personal-mcp-daemon-identity-v1\n"));
    assert!(!identity_text.contains(&state_db.display().to_string()));
    assert!(!identity_text.contains(&message_cache_db.display().to_string()));
    let incoming = call(
        &first,
        "wait_for_new_message",
        json!({ "cursor": event_cursor.clone(), "timeout": 2.0 }),
    )
    .await?;
    assert_eq!(incoming.pointer("/data/state").and_then(Value::as_str), Some("event"));
    assert_eq!(
        incoming.pointer("/data/pending_chats/0/last_message_id").and_then(Value::as_i64),
        Some(11)
    );
    first.cancel().await?;

    let second =
        ().serve(transport(binary, &daemon_dir, &state_db, &message_cache_db, None)?).await?;
    let second_cache = search_contract(&second).await?;
    assert_eq!(second_cache.pointer("/data/returned_count").and_then(Value::as_u64), Some(2));
    let second_pid: i32 = std::fs::read_to_string(&lock)?.trim().parse()?;
    assert_eq!(second_pid, first_pid, "the second proxy must reuse the existing daemon");
    let settled = call(
        &second,
        "wait_for_settled_message",
        json!({ "cursor": event_cursor, "settle_ms": 1, "max_wait_ms": 500 }),
    )
    .await?;
    assert_eq!(settled.pointer("/data/state").and_then(Value::as_str), Some("event"));
    assert_eq!(settled.pointer("/data/chat_id").and_then(Value::as_str), Some("101"));
    assert_eq!(settled.pointer("/data/message_count").and_then(Value::as_u64), Some(1));
    second.cancel().await?;

    // SAFETY: the PID was read from the owner-only singleton lock created in
    // this isolated temporary daemon directory, and SIGTERM is the daemon's
    // documented graceful shutdown signal.
    let killed = unsafe { kill(first_pid, SIGTERM) };
    assert_eq!(killed, 0, "failed to signal isolated daemon PID {first_pid}");
    wait_for_removal(&socket).await?;
    wait_for_removal(&identity).await?;
    cleanup.disarm();
    let log_text = std::fs::read_to_string(&log)?;
    assert!(!log_text.contains("\"jsonrpc\""));
    assert!(!log_text.contains("CONTRACT"));
    assert!(message_cache_db.exists());
    Ok(())
}

#[tokio::test]
async fn live_daemon_rejects_a_proxy_with_different_effective_retention_config()
-> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let binary = env!("CARGO_BIN_EXE_telegram-personal-mcp");
    let daemon_dir = temp.path().join("daemon-config-identity");
    let state_db = temp.path().join("state-config-identity.sqlite");
    let message_cache_db = temp.path().join("message-cache-config-identity.sqlite");
    let socket = daemon_dir.join("daemon.sock");
    let lock = daemon_dir.join("daemon.lock");
    let identity = daemon_dir.join("daemon.identity");
    let mut cleanup = DaemonCleanup::new(lock.clone());

    let cache = MessageCache::open(&message_cache_db)?;
    cache.bind_account(&AccountMetadata {
        account_user_id: 7,
        display_name: Some("Fixture User".to_string()),
        username: Some("fixture_user".to_string()),
        observed_at_unix: 1,
    })?;
    cache.upsert_messages(
        7,
        &[CachedMessageInput {
            chat_id: "101".to_string(),
            message: MessageSummary {
                message_id: 990,
                chat_ref: Some("101".to_string()),
                sender_ref: Some("101".to_string()),
                outgoing: false,
                pinned: false,
                silent: false,
                noforwards: false,
                text: Some("losing-daemon-retention-sentinel".to_string()),
                date: 1,
                edit_date: None,
                reply_to_message_id: None,
                views: None,
                forwards: None,
                reply_count: None,
                media_type: None,
                forward_origin: None,
            },
            observed_at_unix: 1,
            source: MessageSource::Import,
        }],
    )?;
    drop(cache);

    let first =
        ().serve(transport(binary, &daemon_dir, &state_db, &message_cache_db, None)?).await?;
    wait_for_file(&socket).await?;
    wait_for_file(&identity).await?;
    let first_pid: i32 = std::fs::read_to_string(&lock)?.trim().parse()?;

    let output = tokio::process::Command::new(binary)
        .arg("--demo")
        .arg("--state-db-path")
        .arg(&state_db)
        .arg("--message-cache-db-path")
        .arg(&message_cache_db)
        .arg("--message-cache-max-age-days")
        .arg("30")
        .env("TELEGRAM_MCP_DAEMON_DIR", &daemon_dir)
        .env_remove("TELEGRAM_API_ID")
        .env_remove("TELEGRAM_API_HASH")
        .env_remove("TELEGRAM_MCP_SESSION_PATH")
        .env_remove("TELEGRAM_MCP_SESSION_CACHE_DIR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    assert!(!output.status.success(), "mismatched proxy must fail closed");
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("running daemon configuration differs"), "stderr was: {stderr}");
    assert_eq!(
        std::fs::read_to_string(&lock)?.trim().parse::<i32>()?,
        first_pid,
        "a mismatch must not replace or restart the live daemon"
    );
    assert!(socket.exists());

    let losing_daemon = tokio::process::Command::new(binary)
        .arg("--demo")
        .arg("--state-db-path")
        .arg(&state_db)
        .arg("--message-cache-db-path")
        .arg(&message_cache_db)
        .arg("--message-cache-max-age-days")
        .arg("1")
        .arg("daemon")
        .env("TELEGRAM_MCP_DAEMON_DIR", &daemon_dir)
        .env_remove("TELEGRAM_API_ID")
        .env_remove("TELEGRAM_API_HASH")
        .env_remove("TELEGRAM_MCP_SESSION_PATH")
        .env_remove("TELEGRAM_MCP_SESSION_CACHE_DIR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    assert!(!losing_daemon.status.success());
    let losing_stderr = String::from_utf8(losing_daemon.stderr)?;
    assert!(losing_stderr.contains("daemon is already running"));
    assert!(
        MessageCache::open(&message_cache_db)?.exact_get(7, "101", 990)?.is_some(),
        "a daemon that loses the singleton lease must not run retention first"
    );

    first.cancel().await?;
    // SAFETY: the PID belongs to this test's isolated owner-only daemon lock.
    assert_eq!(unsafe { kill(first_pid, SIGTERM) }, 0);
    wait_for_removal(&socket).await?;
    wait_for_removal(&identity).await?;
    cleanup.disarm();
    Ok(())
}

#[tokio::test]
async fn relative_daemon_paths_are_bound_to_the_starting_working_directory() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let binary = env!("CARGO_BIN_EXE_telegram-personal-mcp");
    let daemon_dir = temp.path().join("daemon-relative-paths");
    let first_cwd = temp.path().join("first-cwd");
    let second_cwd = temp.path().join("second-cwd");
    std::fs::create_dir_all(&first_cwd)?;
    std::fs::create_dir_all(&second_cwd)?;
    let socket = daemon_dir.join("daemon.sock");
    let lock = daemon_dir.join("daemon.lock");
    let identity = daemon_dir.join("daemon.identity");
    let mut cleanup = DaemonCleanup::new(lock.clone());

    let first_transport =
        TokioChildProcess::new(tokio::process::Command::new(binary).configure(|command| {
            command
                .current_dir(&first_cwd)
                .arg("--demo")
                .arg("--state-db-path")
                .arg("state.sqlite")
                .arg("--message-cache-db-path")
                .arg("message-cache.sqlite")
                .env("TELEGRAM_MCP_DAEMON_DIR", &daemon_dir)
                .env_remove("TELEGRAM_API_ID")
                .env_remove("TELEGRAM_API_HASH")
                .env_remove("TELEGRAM_MCP_SESSION_PATH")
                .env_remove("TELEGRAM_MCP_SESSION_CACHE_DIR");
        }))?;
    let first = ().serve(first_transport).await?;
    wait_for_file(&socket).await?;
    wait_for_file(&identity).await?;
    let first_pid: i32 = std::fs::read_to_string(&lock)?.trim().parse()?;

    let output = tokio::process::Command::new(binary)
        .current_dir(&second_cwd)
        .arg("--demo")
        .arg("--state-db-path")
        .arg("state.sqlite")
        .arg("--message-cache-db-path")
        .arg("message-cache.sqlite")
        .env("TELEGRAM_MCP_DAEMON_DIR", &daemon_dir)
        .env_remove("TELEGRAM_API_ID")
        .env_remove("TELEGRAM_API_HASH")
        .env_remove("TELEGRAM_MCP_SESSION_PATH")
        .env_remove("TELEGRAM_MCP_SESSION_CACHE_DIR")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await?;
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("running daemon configuration differs"), "stderr was: {stderr}");
    assert_eq!(std::fs::read_to_string(&lock)?.trim().parse::<i32>()?, first_pid);
    assert!(first_cwd.join("message-cache.sqlite").exists());
    assert!(!second_cwd.join("message-cache.sqlite").exists());

    first.cancel().await?;
    // SAFETY: the PID belongs to this test's isolated owner-only daemon lock.
    assert_eq!(unsafe { kill(first_pid, SIGTERM) }, 0);
    wait_for_removal(&socket).await?;
    wait_for_removal(&identity).await?;
    cleanup.disarm();
    Ok(())
}

#[tokio::test]
async fn daemon_cli_retention_prunes_existing_and_future_expired_cache_rows() -> anyhow::Result<()>
{
    let temp = tempfile::tempdir()?;
    let binary = env!("CARGO_BIN_EXE_telegram-personal-mcp");
    let daemon_dir = temp.path().join("daemon-retention");
    let state_db = temp.path().join("state-retention.sqlite");
    let message_cache_db = temp.path().join("message-cache-retention.sqlite");
    let socket = daemon_dir.join("daemon.sock");
    let lock = daemon_dir.join("daemon.lock");
    let mut cleanup = DaemonCleanup::new(lock.clone());
    let cache = MessageCache::open(&message_cache_db)?;
    cache.bind_account(&AccountMetadata {
        account_user_id: 7,
        display_name: Some("Fixture User".to_string()),
        username: Some("fixture_user".to_string()),
        observed_at_unix: 1,
    })?;
    cache.set_global_update_cursor(
        7,
        &GlobalUpdateCursor {
            pts: 120,
            qts: 7,
            date: 1_700_004_000,
            seq: 10,
            unread_count: Some(2),
            phase: CacheSyncPhase::Current,
            observed_at_unix: 1_700_004_000,
            last_error_code: None,
        },
    )?;
    let now_unix = chrono::Utc::now().timestamp();
    let message = |message_id: i32, text: &str, date: i64| CachedMessageInput {
        chat_id: "101".to_string(),
        message: MessageSummary {
            message_id,
            chat_ref: Some("101".to_string()),
            sender_ref: Some("101".to_string()),
            outgoing: false,
            pinned: false,
            silent: false,
            noforwards: false,
            text: Some(text.to_string()),
            date: i32::try_from(date).expect("test timestamp fits Telegram i32"),
            edit_date: None,
            reply_to_message_id: None,
            views: None,
            forwards: None,
            reply_count: None,
            media_type: None,
            forward_origin: None,
        },
        observed_at_unix: now_unix,
        source: MessageSource::Import,
    };
    cache.upsert_messages(
        7,
        &[
            message(901, "retention-marker expired", now_unix - 31 * 24 * 60 * 60),
            message(902, "retention-marker recent", now_unix - 24 * 60 * 60),
        ],
    )?;
    drop(cache);

    let client =
        ().serve(transport(binary, &daemon_dir, &state_db, &message_cache_db, Some(30))?).await?;
    let retained = call(
        &client,
        "search_regex",
        json!({ "pattern": "retention-marker", "chat_id": "101", "limit": 10 }),
    )
    .await?;
    assert_eq!(retained.pointer("/data/returned_count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        retained.pointer("/data/messages/0/message/message_id").and_then(Value::as_i64),
        Some(902)
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    let reopened = MessageCache::open(&message_cache_db)?;
    let stats = reopened.account_stats(7)?.expect("retained account");
    assert_eq!(stats.message_count, 1);
    assert!(reopened.exact_get(7, "101", 901)?.is_none());
    assert!(reopened.exact_get(7, "101", 902)?.is_some());
    drop(reopened);
    client.cancel().await?;
    let pid: i32 = std::fs::read_to_string(&lock)?.trim().parse()?;
    // SAFETY: the PID belongs to this test's isolated owner-only daemon lock.
    assert_eq!(unsafe { kill(pid, SIGTERM) }, 0);
    wait_for_removal(&socket).await?;
    cleanup.disarm();
    Ok(())
}
