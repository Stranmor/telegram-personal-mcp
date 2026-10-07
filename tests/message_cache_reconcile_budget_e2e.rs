//! Reconcile-budget regression for the daemon message-cache worker.
//!
//! The worker used to re-fetch every visible dialog's history on each
//! reconciliation pass (60 s interval, 50 dialogs => up to 3 000 history
//! requests per idle hour). Now the pass runs once per 30 minutes by default
//! and issues a per-chat history request only for dialogs whose visible top
//! message advanced past the cached frontier.
//!
//! The scaled-interval tests compress one 30-minute window into 1 500 ms and
//! assert the steady-state budget: two windows (one hour) cost at most two
//! `list_dialogs` calls and zero history requests, while a single changed chat
//! costs exactly one history call, for that chat only.
//!
//! `idle_hour_stays_within_reconcile_budget` replays the frozen-dialogs
//! scenario with the production defaults (2 s poll, 30 min reconcile) in real
//! time; it is `#[ignore]`d because it runs for 65 minutes:
//!
//! ```text
//! cargo test --locked --test message_cache_reconcile_budget_e2e -- \
//!     --ignored --nocapture idle_hour_stays_within_reconcile_budget
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};
use telegram_personal_mcp::{
    AccountMetadata, CacheSyncPhase, DEFAULT_MESSAGE_CACHE_POLL_INTERVAL,
    DEFAULT_MESSAGE_CACHE_RECONCILE_INTERVAL, FakeTelegramBackend, GlobalUpdateCursor, McpServer,
    McpServerSettings, MessageCache, MessageSource,
};

/// One 30-minute reconcile window under the scaled test clock.
const SCALED_RECONCILE: Duration = Duration::from_millis(1_500);
const SCALED_POLL: Duration = Duration::from_millis(25);

type Stand = (tempfile::TempDir, MessageCache, Arc<FakeTelegramBackend>, McpServer);

/// Build a stand with a bound account and a `Current` cursor so only
/// reconciliation (never update-difference catch-up) produces Telegram calls.
fn open_stand() -> anyhow::Result<Stand> {
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
            pts: 120,
            qts: 7,
            date: 1_700_004_000,
            seq: 10,
            unread_count: Some(2),
            phase: CacheSyncPhase::Current,
            observed_at_unix: 1_700_003_998,
            last_error_code: None,
        },
    )?;
    let backend = Arc::new(FakeTelegramBackend::fixture());
    let server = McpServer::with_settings(
        backend.clone(),
        McpServerSettings {
            attachment_cache_dir: temp.path().join("attachments"),
            max_download_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            state_db_path: temp.path().join("state.sqlite"),
        },
    )
    .with_message_cache_path(cache_path.clone());
    Ok((temp, cache, backend, server))
}

/// Wait until the initial catch-up pass has clearly finished: at least one
/// dialog-list call is visible and both counters stayed quiet for 200 ms.
async fn wait_for_warm_up(backend: &FakeTelegramBackend) {
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut previous = None;
    let mut quiet = 0_u32;
    loop {
        let read = tokio::time::timeout(Duration::from_secs(5), async {
            (backend.list_dialogs_call_count().await, backend.history_observation().await.0)
        })
        .await;
        let counters = read.unwrap_or((usize::MAX, usize::MAX));
        if previous == Some(counters) && counters.0 >= 1 && counters.1 >= 1 {
            quiet += 1;
            if quiet >= 8 {
                return;
            }
        } else {
            quiet = 0;
        }
        previous = Some(counters);
        assert!(Instant::now() < deadline, "reconcile warm-up never settled: {counters:?}");
        tokio::time::sleep(SCALED_POLL).await;
    }
}

#[tokio::test]
async fn frozen_dialogs_cost_no_history_and_two_lists_per_hour() -> anyhow::Result<()> {
    let (_temp, _cache, backend, server) = open_stand()?;
    let worker = server.start_message_cache_worker_with_intervals(SCALED_POLL, SCALED_RECONCILE);
    wait_for_warm_up(&backend).await;

    let baseline_dialogs = backend.list_dialogs_call_count().await;
    let baseline_history = backend.history_observation().await.0;
    // Two reconcile windows (a scaled hour) plus scheduling slack.
    tokio::time::sleep(SCALED_RECONCILE * 2 + Duration::from_millis(400)).await;
    let dialogs = backend.list_dialogs_call_count().await - baseline_dialogs;
    let history = backend.history_observation().await.0 - baseline_history;
    worker.shutdown().await;

    println!("frozen hour: list_dialogs={dialogs} history={history}");
    assert!(dialogs >= 1, "the reconcile loop did not run at all");
    assert!(dialogs <= 2, "a frozen hour must cost at most two dialog-list calls, got {dialogs}");
    assert_eq!(history, 0, "frozen dialogs must not issue history requests");
    Ok(())
}

#[tokio::test]
async fn a_single_changed_chat_costs_one_history_call() -> anyhow::Result<()> {
    let (_temp, cache, backend, server) = open_stand()?;
    let worker = server.start_message_cache_worker_with_intervals(SCALED_POLL, SCALED_RECONCILE);
    wait_for_warm_up(&backend).await;

    let baseline_dialogs = backend.list_dialogs_call_count().await;
    let baseline_history = backend.history_observation().await.0;
    let new_message_id =
        backend.push_fixture_message(101, "Status report is ready").await.expect("fixture dialog");
    assert_eq!(new_message_id, 13);

    // Exactly one reconcile window.
    tokio::time::sleep(SCALED_RECONCILE + Duration::from_millis(500)).await;
    let dialogs = backend.list_dialogs_call_count().await - baseline_dialogs;
    let history = backend.history_observation().await.0 - baseline_history;
    worker.shutdown().await;

    println!("changed chat: list_dialogs={dialogs} history={history}");
    assert_eq!(dialogs, 1, "one window must cost exactly one dialog-list call");
    assert_eq!(history, 1, "only the changed chat may pay a history call");
    let cached = cache.exact_get(7, "101", new_message_id)?.expect("reconciled message");
    assert_eq!(cached.message.text.as_deref(), Some("Status report is ready"));
    assert_eq!(cached.source, MessageSource::Reconciliation);
    Ok(())
}

#[tokio::test]
#[ignore = "65-minute acceptance probe; run explicitly with --ignored --nocapture"]
async fn idle_hour_stays_within_reconcile_budget() -> anyhow::Result<()> {
    let (_temp, _cache, backend, server) = open_stand()?;
    let worker = server.start_message_cache_worker_with_intervals(
        DEFAULT_MESSAGE_CACHE_POLL_INTERVAL,
        DEFAULT_MESSAGE_CACHE_RECONCILE_INTERVAL,
    );
    wait_for_warm_up(&backend).await;

    let baseline_dialogs = backend.list_dialogs_call_count().await;
    let baseline_history = backend.history_observation().await.0;
    let start = Instant::now();
    let mut events: Vec<Duration> = Vec::new();
    let mut observed = baseline_dialogs;
    println!(
        "probe start: poll={:?} reconcile={:?} warm-up list_dialogs={baseline_dialogs} history={baseline_history}",
        DEFAULT_MESSAGE_CACHE_POLL_INTERVAL, DEFAULT_MESSAGE_CACHE_RECONCILE_INTERVAL
    );
    while start.elapsed() < Duration::from_secs(65 * 60) {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let dialogs = backend.list_dialogs_call_count().await;
        if dialogs > observed {
            for _ in observed..dialogs {
                events.push(start.elapsed());
            }
            observed = dialogs;
            println!(
                "probe event: t={:?} list_dialogs={dialogs} history={}",
                start.elapsed(),
                backend.history_observation().await.0
            );
        }
    }
    let history_delta = backend.history_observation().await.0 - baseline_history;
    worker.shutdown().await;

    let max_in_window = events
        .iter()
        .map(|reference| {
            events
                .iter()
                .filter(|event| {
                    **event >= *reference
                        && event.saturating_sub(*reference) <= Duration::from_secs(3600)
                })
                .count()
        })
        .max()
        .unwrap_or(0);
    let min_gap = events
        .windows(2)
        .map(|pair| pair[1].saturating_sub(pair[0]))
        .min()
        .unwrap_or(Duration::from_secs(3600));
    println!(
        "probe end: events={events:?} min_gap={min_gap:?} max_per_60min_window={max_in_window} history_delta={history_delta}"
    );
    assert_eq!(history_delta, 0, "frozen dialogs must not issue history requests");
    assert!(
        min_gap >= Duration::from_secs(1790),
        "reconciliations must stay at least 30 minutes apart, saw {min_gap:?}"
    );
    assert!(max_in_window <= 2, "one hour must not exceed two reconciliations, got {max_in_window}");
    Ok(())
}
