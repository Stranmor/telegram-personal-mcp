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
use serde_json::{Value, json};
use std::sync::Arc;
use telegram_personal_mcp::telegram::MessageSummary;
use telegram_personal_mcp::{
    AccountMetadata, CacheSyncPhase, CacheUpdateBatch, CachedMessageInput, DialogCheckpoint,
    DialogMetadata, FakeTelegramBackend, GlobalUpdateCursor, IncomingMessageEventInput, McpServer,
    McpServerSettings, MessageCache, MessageSource,
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
    assert_eq!(result.is_error, Some(false), "{tool} should succeed");
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
async fn persistent_cache_tools_are_account_scoped_bounded_and_restart_safe() -> anyhow::Result<()>
{
    let temp = tempfile::tempdir()?;
    let cache_path = temp.path().join("message-cache.sqlite");
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let first_server = server(backend.clone(), &temp, &cache_path);
    let first_task = tokio::spawn(async move {
        first_server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let first = ().serve(client_transport).await?;

    let tools = first.list_all_tools().await?;
    for name in [
        "search_regex",
        "export_chat",
        "clear_cache",
        "sync_messages",
        "chat_analytics",
        "message_timeline",
        "today_messages",
        "export_cached_messages",
    ] {
        let tool = tools.iter().find(|tool| tool.name == name).expect("cache tool");
        assert_eq!(tool.input_schema.get("additionalProperties"), Some(&json!(false)));
        assert!(tool.output_schema.is_some());
    }
    let clear = tools.iter().find(|tool| tool.name == "clear_cache").expect("clear tool");
    assert_eq!(clear.annotations.as_ref().and_then(|value| value.destructive_hint), Some(true));
    assert_eq!(clear.annotations.as_ref().and_then(|value| value.idempotent_hint), Some(false));

    let read = call(&first, "read_chat", json!({ "chat_id": "@alex", "limit": 2 })).await?;
    assert_eq!(read.pointer("/data/returned_count").and_then(Value::as_u64), Some(2));

    let passive =
        call(&first, "search_regex", json!({ "pattern": "CONTRACT", "chat_id": "@alex" })).await?;
    assert_eq!(passive.pointer("/data/returned_count").and_then(Value::as_u64), Some(2));
    assert_eq!(
        passive.pointer("/data/messages/0/message/message_id").and_then(Value::as_i64),
        Some(11)
    );

    let exported = call(&first, "export_chat", json!({ "chat_id": "@alex", "limit": 3 })).await?;
    assert_eq!(exported.pointer("/data/returned_count").and_then(Value::as_u64), Some(3));
    assert_eq!(exported.pointer("/data/cache/message_count").and_then(Value::as_u64), Some(3));
    assert!(cache_path.exists());

    first.cancel().await?;
    first_task.await??;

    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let second_server = server(backend, &temp, &cache_path);
    let second_task = tokio::spawn(async move {
        second_server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let second = ().serve(client_transport).await?;

    let persisted = call(
        &second,
        "search_regex",
        json!({
            "pattern": "contract\\.pdf",
            "chat_id": "101",
            "after": "2023-11-14T22:13:31Z",
            "before": "2023-11-14T22:13:31+00:00"
        }),
    )
    .await?;
    assert_eq!(persisted.pointer("/data/returned_count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        persisted.pointer("/data/messages/0/message/message_id").and_then(Value::as_i64),
        Some(11)
    );

    let analytics =
        call(&second, "chat_analytics", json!({ "chat_id": "@alex", "limit": 20 })).await?;
    assert_eq!(analytics.pointer("/data/total_senders").and_then(Value::as_u64), Some(2));
    assert_eq!(
        analytics.pointer("/data/top_senders/0/sender_name").and_then(Value::as_str),
        Some("101")
    );
    assert_eq!(analytics.pointer("/data/top_senders/0/msg_count").and_then(Value::as_u64), Some(2));

    let timeline =
        call(&second, "message_timeline", json!({ "chat_id": "@alex", "granularity": "day" }))
            .await?;
    assert_eq!(
        timeline.pointer("/data/timeline/0/period").and_then(Value::as_str),
        Some("2023-11-14")
    );
    assert_eq!(timeline.pointer("/data/timeline/0/msg_count").and_then(Value::as_u64), Some(3));

    let json_export =
        call(&second, "export_cached_messages", json!({ "chat_id": "@alex", "format": "json" }))
            .await?;
    assert_eq!(json_export.pointer("/data/count").and_then(Value::as_u64), Some(3));
    let ids = json_export
        .pointer("/data/messages")
        .and_then(Value::as_array)
        .expect("JSON messages")
        .iter()
        .map(|value| value.pointer("/message/message_id").and_then(Value::as_i64).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![12, 10, 11]);
    assert!(json_export.to_string().find("raw_json").is_none());

    let csv_export =
        call(&second, "export_cached_messages", json!({ "chat_id": "@alex", "format": "csv" }))
            .await?;
    assert_eq!(csv_export.pointer("/data/count").and_then(Value::as_u64), Some(3));
    let csv = csv_export.pointer("/data/data").and_then(Value::as_str).expect("CSV body");
    assert!(csv.starts_with("account_user_id,chat_id,message_id,"));
    assert!(!csv.contains("raw_json"));

    let today = call(&second, "today_messages", json!({ "chat_id": "@alex" })).await?;
    assert_eq!(today.pointer("/data/timezone").and_then(Value::as_str), Some("UTC"));
    assert_eq!(today.pointer("/data/returned_count").and_then(Value::as_u64), Some(0));

    let synced =
        call(&second, "sync_messages", json!({ "chat_id": "@alex", "limit": 1000 })).await?;
    assert_eq!(synced.pointer("/data/scope").and_then(Value::as_str), Some("chat"));
    assert_eq!(synced.pointer("/data/detail/messages_synced").and_then(Value::as_u64), Some(0));
    assert_eq!(
        synced.pointer("/data/detail/complete_within_requested_window").and_then(Value::as_bool),
        Some(true)
    );

    assert_eq!(
        rejected_code(&second, "search_regex", json!({ "pattern": "[" })).await?,
        "INVALID_ARGUMENT"
    );
    assert_eq!(
        rejected_code(
            &second,
            "message_timeline",
            json!({ "after": "2026-07-24", "before": "2026-07-23" }),
        )
        .await?,
        "INVALID_ARGUMENT"
    );

    let preview = call(&second, "clear_cache", json!({})).await?;
    assert_eq!(
        preview.pointer("/data/status").and_then(Value::as_str),
        Some("confirmation_required")
    );
    assert_eq!(preview.pointer("/data/message_count_before").and_then(Value::as_u64), Some(3));
    assert_eq!(preview.pointer("/data/message_count_after").and_then(Value::as_u64), Some(3));

    let cleared = call(&second, "clear_cache", json!({ "confirm": true })).await?;
    assert_eq!(cleared.pointer("/data/status").and_then(Value::as_str), Some("cache_cleared"));
    assert_eq!(cleared.pointer("/data/message_count_before").and_then(Value::as_u64), Some(3));
    assert_eq!(cleared.pointer("/data/message_count_after").and_then(Value::as_u64), Some(0));
    assert_eq!(cleared.pointer("/data/generation").and_then(Value::as_i64), Some(1));

    let empty_csv =
        call(&second, "export_cached_messages", json!({ "chat_id": "@alex", "format": "csv" }))
            .await?;
    assert_eq!(empty_csv.pointer("/data/count").and_then(Value::as_u64), Some(0));
    assert_eq!(empty_csv.pointer("/data/data").and_then(Value::as_str), Some(""));

    second.cancel().await?;
    second_task.await??;
    Ok(())
}

#[tokio::test]
async fn retention_preserves_the_observed_frontier_without_refetching_expired_history()
-> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let cache_path = temp.path().join("retention-frontier.sqlite");
    let cache = MessageCache::open(&cache_path)?;
    cache.bind_account(&AccountMetadata {
        account_user_id: 7,
        display_name: Some("Fixture User".to_string()),
        username: Some("fixture_user".to_string()),
        observed_at_unix: 1_700_000_100,
    })?;
    cache.upsert_dialog(
        7,
        &DialogMetadata {
            chat_id: "101".to_string(),
            chat_ref: Some("@alex".to_string()),
            chat_kind: Some("user".to_string()),
            title: Some("Alex".to_string()),
            username: Some("alex".to_string()),
            observed_at_unix: 1_700_000_100,
        },
    )?;
    cache.upsert_messages(
        7,
        &[10, 11, 12]
            .into_iter()
            .map(|message_id| CachedMessageInput {
                chat_id: "101".to_string(),
                message: MessageSummary {
                    message_id,
                    chat_ref: Some("101".to_string()),
                    sender_ref: Some("101".to_string()),
                    outgoing: false,
                    pinned: false,
                    silent: false,
                    noforwards: false,
                    text: Some(format!("expired {message_id}")),
                    date: 1_700_000_000 + message_id,
                    edit_date: None,
                    reply_to_message_id: None,
                    views: None,
                    forwards: None,
                    reply_count: None,
                    media_type: None,
                    forward_origin: None,
                },
                observed_at_unix: 1_700_000_100,
                source: MessageSource::Import,
            })
            .collect::<Vec<_>>(),
    )?;
    cache.upsert_dialog_checkpoint(
        7,
        &DialogCheckpoint {
            chat_id: "101".to_string(),
            frontier_message_id: Some(12),
            oldest_message_id: Some(10),
            history_complete: true,
            observed_at_unix: 1_700_000_100,
        },
    )?;
    drop(cache);

    let backend = Arc::new(FakeTelegramBackend::fixture());
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let server = server(backend.clone(), &temp, &cache_path).with_message_cache_max_age_days(1)?;
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let synced =
        call(&client, "sync_messages", json!({ "chat_id": "@alex", "limit": 200 })).await?;
    assert_eq!(synced.pointer("/data/detail/messages_synced").and_then(Value::as_u64), Some(0));
    assert_eq!(backend.history_observation().await, (1, 0));

    let reopened = MessageCache::open(&cache_path)?;
    assert_eq!(reopened.account_stats(7)?.expect("account").message_count, 0);
    let checkpoint = reopened.dialog_checkpoint(7, "101")?.expect("retained partial checkpoint");
    assert_eq!(checkpoint.frontier_message_id, Some(12));
    assert!(!checkpoint.history_complete);

    client.cancel().await?;
    server_task.await??;
    Ok(())
}

#[tokio::test]
async fn first_retention_read_sweeps_every_bound_account_before_concurrent_reads()
-> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let cache_path = temp.path().join("retention-all-accounts.sqlite");
    let cache = MessageCache::open(&cache_path)?;
    let now_unix = chrono::Utc::now().timestamp();
    for account_user_id in [7, 8] {
        cache.bind_account(&AccountMetadata {
            account_user_id,
            display_name: Some(format!("Fixture {account_user_id}")),
            username: Some(format!("fixture_{account_user_id}")),
            observed_at_unix: now_unix,
        })?;
    }
    cache.clear_account(7, now_unix)?;
    let seeded = |chat_id: &str, message_id: i32, text: &str, date: i64| CachedMessageInput {
        chat_id: chat_id.to_string(),
        message: MessageSummary {
            message_id,
            chat_ref: Some(chat_id.to_string()),
            sender_ref: Some(chat_id.to_string()),
            outgoing: false,
            pinned: false,
            silent: false,
            noforwards: false,
            text: Some(text.to_string()),
            date: i32::try_from(date).expect("test date fits Telegram i32"),
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
    for (account_user_id, chat_id, base_id) in [(7, "701", 700), (8, "801", 800)] {
        cache.upsert_messages(
            account_user_id,
            &[
                seeded(
                    chat_id,
                    base_id + 1,
                    "retention-barrier expired",
                    now_unix - 2 * 24 * 60 * 60,
                ),
                seeded(chat_id, base_id + 2, "retention-barrier recent", now_unix - 60),
            ],
        )?;
    }
    let event_cursor = cache.incoming_event_window(7, None, now_unix * 1_000)?.cursor.token;
    cache.apply_update_batch(
        7,
        &CacheUpdateBatch {
            dialogs: Vec::new(),
            messages: Vec::new(),
            incoming_events: vec![IncomingMessageEventInput {
                chat_id: "701".to_string(),
                sender_user_id: 701,
                sender_name: "Legacy sender".to_string(),
                sender_username: None,
                message_id: 799,
                message_date_unix: now_unix - 2 * 24 * 60 * 60,
                observed_at_unix_ms: now_unix * 1_000,
            }],
            channel_tombstones: Vec::new(),
            global_deleted_message_ids: Vec::new(),
            cursor: GlobalUpdateCursor {
                pts: 1,
                qts: 0,
                date: i32::try_from(now_unix).expect("test date fits Telegram i32"),
                seq: 1,
                unread_count: Some(1),
                phase: CacheSyncPhase::Current,
                observed_at_unix: now_unix,
                last_error_code: None,
            },
        },
    )?;
    drop(cache);

    let backend = Arc::new(FakeTelegramBackend::fixture());
    backend.set_fixture_account_user_id(8).await;
    let (server_transport, client_transport) = tokio::io::duplex(128 * 1024);
    let server = server(backend, &temp, &cache_path).with_message_cache_max_age_days(1)?;
    let server_task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;

    let first =
        call(&client, "search_regex", json!({ "pattern": "retention-barrier", "limit": 10 }));
    let second =
        call(&client, "search_regex", json!({ "pattern": "retention-barrier", "limit": 10 }));
    let (first, second) = tokio::join!(first, second);
    for result in [first?, second?] {
        assert_eq!(result.pointer("/data/returned_count").and_then(Value::as_u64), Some(1));
        assert_eq!(
            result.pointer("/data/messages/0/message/message_id").and_then(Value::as_i64),
            Some(802)
        );
    }

    let reopened = MessageCache::open(&cache_path)?;
    assert_eq!(reopened.bound_account_generations()?, vec![(7, 1), (8, 0)]);
    for (account_user_id, chat_id, expired_id, recent_id) in
        [(7, "701", 701, 702), (8, "801", 801, 802)]
    {
        assert!(reopened.exact_get(account_user_id, chat_id, expired_id)?.is_none());
        assert!(reopened.exact_get(account_user_id, chat_id, recent_id)?.is_some());
    }
    assert_eq!(
        reopened.incoming_event_window(7, Some(&event_cursor), now_unix * 1_000 + 1)?.events.len(),
        1,
        "retention must preserve the event journal and its checkpoint"
    );

    client.cancel().await?;
    server_task.await??;
    Ok(())
}
