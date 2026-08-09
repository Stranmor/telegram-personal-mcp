use std::sync::Arc;
use std::time::Duration;
use telegram_personal_mcp::{
    AccountMetadata, CacheSyncPhase, FakeTelegramBackend, GlobalUpdateCursor, McpServer,
    McpServerSettings, MessageCache, MessageSource,
};

#[tokio::test]
async fn background_worker_replays_slices_atomically_and_persists_its_cursor() -> anyhow::Result<()>
{
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

    let server = McpServer::with_settings(
        Arc::new(FakeTelegramBackend::fixture()),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
    )
    .with_message_cache_path(cache_path.clone());
    let worker = server.start_message_cache_worker_with_intervals(
        Duration::from_millis(10),
        Duration::from_secs(3600),
    );

    let mut reached_current = false;
    for _ in 0..200 {
        let cursor = cache.global_update_cursor(7)?;
        let stats = cache.account_stats(7)?.expect("bound account");
        if cursor.as_ref().is_some_and(|cursor| {
            cursor.pts == 120
                && cursor.qts == 7
                && cursor.seq == 10
                && cursor.phase == CacheSyncPhase::Current
                && cursor.last_error_code.is_none()
        }) && stats.global_tombstone_count == 2
        {
            reached_current = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(reached_current, "worker did not drain slice plus complete difference");
    worker.shutdown().await;

    let reopened = MessageCache::open(&cache_path)?;
    let cursor = reopened.global_update_cursor(7)?.expect("durable cursor");
    assert_eq!(cursor.pts, 120);
    assert_eq!(cursor.phase, CacheSyncPhase::Current);
    let message = reopened.exact_get(7, "101", 11)?.expect("live update message");
    assert_eq!(message.message.text.as_deref(), Some("Please check contract.pdf"));
    assert_eq!(message.source, MessageSource::LiveUpdate);
    let stats = reopened.account_stats(7)?.expect("account stats");
    assert_eq!(stats.global_tombstone_count, 2);
    Ok(())
}
