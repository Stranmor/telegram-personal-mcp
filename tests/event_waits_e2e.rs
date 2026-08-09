use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::telegram::MessageSummary;
use telegram_personal_mcp::{
    AccountMetadata, CacheSyncPhase, CachedMessageInput, DialogCheckpoint, DialogMetadata,
    FakeTelegramBackend, GlobalUpdateCursor, McpServer, McpServerSettings, MessageCache,
    MessageSource,
};

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
    assert_eq!(result.is_error, Some(false), "{tool} should succeed: {result:?}");
    Ok(result.structured_content.expect("structured tool result"))
}

async fn rejected_code(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tool: &'static str,
    arguments: Value,
) -> anyhow::Result<String> {
    let result = client
        .call_tool(
            CallToolRequestParams::new(tool)
                .with_arguments(arguments.as_object().expect("object").clone()),
        )
        .await?;
    assert_eq!(result.is_error, Some(true), "{tool} should reject invalid input");
    Ok(result
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/error/code"))
        .and_then(Value::as_str)
        .expect("typed error code")
        .to_string())
}

fn server(
    backend: Arc<FakeTelegramBackend>,
    temp: &tempfile::TempDir,
    cache_path: &std::path::Path,
) -> McpServer {
    McpServer::with_settings(
        backend,
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
    )
    .with_message_cache_path(cache_path.to_path_buf())
}

#[tokio::test]
async fn event_waits_wake_from_one_worker_replay_per_cursor_and_survive_restart()
-> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let cache_path = temp.path().join("message-cache.sqlite");
    let cache = MessageCache::open(&cache_path)?;
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
    let initial_cursor = cache.incoming_event_window(7, None, 1)?.cursor.token;

    let backend = Arc::new(FakeTelegramBackend::fixture());
    let first_server = server(backend.clone(), &temp, &cache_path);
    let worker_server = first_server.clone();
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let first_task = tokio::spawn(async move {
        first_server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let first = ().serve(client_transport).await?;

    let wait = call(
        &first,
        "wait_for_new_message",
        json!({ "cursor": initial_cursor.clone(), "timeout": 2.0 }),
    );
    let start_worker = async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        worker_server.start_message_cache_worker_with_intervals(
            Duration::from_millis(10),
            Duration::from_secs(3600),
        )
    };
    let (first_event, worker) = tokio::join!(wait, start_worker);
    let first_event = first_event?;
    assert_eq!(first_event.pointer("/data/state").and_then(Value::as_str), Some("event"));
    assert_eq!(first_event.pointer("/data/event").and_then(Value::as_bool), Some(true));
    assert_eq!(
        first_event.pointer("/data/pending_chats/0/chat_id").and_then(Value::as_str),
        Some("101")
    );
    assert_eq!(
        first_event.pointer("/data/pending_chats/0/count").and_then(Value::as_u64),
        Some(1),
        "the outgoing fixture message must not enter the incoming-event journal"
    );
    assert_eq!(
        first_event.pointer("/data/pending_chats/0/last_message_id").and_then(Value::as_i64),
        Some(11)
    );
    let after_new = first_event
        .pointer("/data/cursor")
        .and_then(Value::as_str)
        .expect("successor cursor")
        .to_string();
    worker.shutdown().await;

    let second_server = worker_server.clone();
    let (second_server_transport, second_client_transport) = tokio::io::duplex(128 * 1024);
    let second_task = tokio::spawn(async move {
        second_server.serve(second_server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let second = ().serve(second_client_transport).await?;
    let replay = call(
        &second,
        "wait_for_new_message",
        json!({ "cursor": initial_cursor.clone(), "timeout": 0.1 }),
    )
    .await?;
    assert_eq!(
        replay.pointer("/data/pending_chats/0/last_message_id").and_then(Value::as_i64),
        Some(11)
    );

    let settled = call(
        &first,
        "wait_for_settled_message",
        json!({ "cursor": initial_cursor.clone(), "settle_ms": 1, "max_wait_ms": 500 }),
    )
    .await?;
    assert_eq!(settled.pointer("/data/state").and_then(Value::as_str), Some("event"));
    assert_eq!(settled.pointer("/data/chat_id").and_then(Value::as_str), Some("101"));
    assert_eq!(settled.pointer("/data/message_count").and_then(Value::as_u64), Some(1));
    let after_settled = settled
        .pointer("/data/cursor")
        .and_then(Value::as_str)
        .expect("settled successor")
        .to_string();
    let settled_timeout = call(
        &first,
        "wait_for_settled_message",
        json!({ "cursor": after_settled.clone(), "settle_ms": 1, "max_wait_ms": 20 }),
    )
    .await?;
    assert_eq!(settled_timeout.pointer("/data/state").and_then(Value::as_str), Some("timeout"));
    assert_eq!(
        settled_timeout.pointer("/data/cursor").and_then(Value::as_str),
        Some(after_settled.as_str())
    );

    let new_timeout = call(
        &first,
        "wait_for_new_message",
        json!({ "cursor": after_new.clone(), "timeout": 0.02 }),
    )
    .await?;
    assert_eq!(new_timeout.pointer("/data/state").and_then(Value::as_str), Some("timeout"));
    assert_eq!(
        new_timeout.pointer("/data/cursor").and_then(Value::as_str),
        Some(after_new.as_str())
    );
    assert_eq!(
        rejected_code(&first, "wait_for_new_message", json!({ "timeout": 0 })).await?,
        "INVALID_ARGUMENT"
    );
    assert_eq!(
        rejected_code(
            &first,
            "wait_for_settled_message",
            json!({ "settle_ms": 0, "max_wait_ms": 20 }),
        )
        .await?,
        "INVALID_ARGUMENT"
    );
    assert_eq!(
        rejected_code(
            &first,
            "wait_for_settled_message",
            json!({ "cursor": "not-a-real-cursor", "settle_ms": 1, "max_wait_ms": 20 }),
        )
        .await?,
        "INVALID_ARGUMENT"
    );

    first.cancel().await?;
    first_task.await??;
    second.cancel().await?;
    second_task.await??;

    let restarted_server = server(backend, &temp, &cache_path);
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let restarted_task = tokio::spawn(async move {
        restarted_server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let restarted = ().serve(client_transport).await?;
    let restart_replay = call(
        &restarted,
        "wait_for_new_message",
        json!({ "cursor": initial_cursor.clone(), "timeout": 0.1 }),
    )
    .await?;
    assert_eq!(
        restart_replay.pointer("/data/pending_chats/0/last_message_id").and_then(Value::as_i64),
        Some(11)
    );
    restarted.cancel().await?;
    restarted_task.await??;
    Ok(())
}

#[tokio::test]
async fn clear_invalidates_and_wakes_an_active_wait_without_waiting_for_its_deadline()
-> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let cache_path = temp.path().join("message-cache.sqlite");
    let cache = MessageCache::open(&cache_path)?;
    cache.bind_account(&AccountMetadata {
        account_user_id: 7,
        display_name: Some("Fixture User".to_string()),
        username: Some("fixture_user".to_string()),
        observed_at_unix: 1_700_004_000,
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
    let cursor = cache.incoming_event_window(7, None, 1)?.cursor.token;

    let server = server(Arc::new(FakeTelegramBackend::fixture()), &temp, &cache_path);
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let waiting = rejected_code(
        &client,
        "wait_for_new_message",
        json!({ "cursor": cursor, "timeout": 30.0 }),
    );
    let clearing = async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        call(&client, "clear_cache", json!({ "confirm": true })).await
    };
    let (wait_code, clear_result) =
        tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(waiting, clearing) })
            .await
            .expect("cursor invalidation must wake well before the 30-second wait deadline");
    assert_eq!(wait_code?, "CACHE_EVENT_CURSOR_STALE");
    assert_eq!(
        clear_result?.pointer("/data/status").and_then(Value::as_str),
        Some("cache_cleared")
    );
    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn too_long_difference_invalidates_pre_gap_event_cursors_instead_of_timing_out()
-> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let cache_path = temp.path().join("message-cache.sqlite");
    let cache = MessageCache::open(&cache_path)?;
    cache.bind_account(&AccountMetadata {
        account_user_id: 7,
        display_name: Some("Fixture User".to_string()),
        username: Some("fixture_user".to_string()),
        observed_at_unix: 1_700_003_997,
    })?;
    cache.set_global_update_cursor(
        7,
        &GlobalUpdateCursor {
            pts: 117,
            qts: 7,
            date: 1_700_003_997,
            seq: 7,
            unread_count: Some(3),
            phase: CacheSyncPhase::Current,
            observed_at_unix: 1_700_003_997,
            last_error_code: None,
        },
    )?;
    cache.upsert_dialog(
        7,
        &DialogMetadata {
            chat_id: "101".to_string(),
            chat_ref: Some("@alex".to_string()),
            chat_kind: Some("user".to_string()),
            title: Some("Alex".to_string()),
            username: Some("alex".to_string()),
            observed_at_unix: 1_700_003_997,
        },
    )?;
    let stale_message = |message_id: i32, text: &str| CachedMessageInput {
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
            date: 1_700_000_000 + message_id,
            edit_date: None,
            reply_to_message_id: None,
            views: None,
            forwards: None,
            reply_count: None,
            media_type: None,
            forward_origin: None,
        },
        observed_at_unix: 1_700_003_997,
        source: MessageSource::Import,
    };
    cache.upsert_messages(
        7,
        &[stale_message(10, "deleted during gap"), stale_message(11, "before gap edit")],
    )?;
    cache.upsert_dialog_checkpoint(
        7,
        &DialogCheckpoint {
            chat_id: "101".to_string(),
            frontier_message_id: Some(11),
            oldest_message_id: Some(10),
            history_complete: false,
            observed_at_unix: 1_700_003_997,
        },
    )?;
    let cursor = cache.incoming_event_window(7, None, 1)?.cursor.token;

    let backend = Arc::new(FakeTelegramBackend::fixture());
    assert!(
        backend
            .set_fixture_message_text(101, 11, Some("edited during gap".to_string()), Some(31))
            .await
    );
    assert!(backend.remove_fixture_message(101, 10).await);
    let server = server(backend, &temp, &cache_path);
    let worker_server = server.clone();
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let waiting =
        rejected_code(&client, "wait_for_new_message", json!({ "cursor": cursor, "timeout": 5.0 }));
    let start_worker = async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        worker_server.start_message_cache_worker_with_intervals(
            Duration::from_millis(10),
            Duration::from_secs(3600),
        )
    };
    let (wait_code, worker) = tokio::join!(waiting, start_worker);
    assert_eq!(wait_code?, "CACHE_EVENT_CURSOR_STALE");
    worker.shutdown().await;
    let cursor = cache.global_update_cursor(7)?.expect("rebased durable cursor");
    assert_eq!(cursor.pts, 120);
    assert_ne!(cursor.phase, CacheSyncPhase::ResyncRequired);
    assert_eq!(cache.account_generation(7)?, Some(1));
    assert!(cache.exact_get(7, "101", 10)?.is_none());
    let edited = cache.exact_get(7, "101", 11)?.expect("authoritative resync replacement");
    assert_eq!(edited.message.text.as_deref(), Some("edited during gap"));
    assert_eq!(edited.message.edit_date, Some(31));
    client.cancel().await?;
    server_task.await??;
    Ok(())
}
