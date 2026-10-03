#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::unwrap_used,
    )
)]

use crate::state_db::{SynchronousMode, open_private_wal, restrict_private_sqlite_files};
use crate::telegram::MessageSummary;
use chrono::{DateTime, Utc};
use regex::RegexBuilder;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

pub const MESSAGE_CACHE_SCHEMA_VERSION: i64 = 5;
pub const MAX_MESSAGE_QUERY_ROWS: usize = 1_000;
pub const MAX_REGEX_PATTERN_BYTES: usize = 512;
pub const MAX_REGEX_SCAN_ROWS: usize = 10_000;
pub const MAX_SOURCE_ROWS: usize = 50_000;
pub const MAX_INCOMING_EVENT_ROWS_PER_ACCOUNT: usize = 100_000;
pub const MAX_MESSAGE_CACHE_AGE_DAYS: u32 = 36_500;
const MAX_EVENT_CHECKPOINT_ROWS_PER_ACCOUNT: usize = 100_000;
const MAX_EVENT_CURSOR_TOMBSTONES_PER_ACCOUNT: usize = 100_000;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

const MESSAGE_CACHE_APPLICATION_ID: i64 = 0x544d_4348; // "TMCH"
const MESSAGE_CACHE_KIND: &str = "telegram_message_cache";
const CSV_HEADER: &str = "account_user_id,chat_id,message_id,sender_ref,text,date_unix,edit_date,observed_at_unix,source,message_json\r\n";

#[derive(Debug, Error)]
pub enum MessageCacheError {
    #[error("SQLite message-cache failure: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("message-cache serialization failure: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid case-insensitive regex: {0}")]
    InvalidRegex(#[source] regex::Error),
    #[error("regex pattern is {actual} bytes; maximum is {maximum}")]
    RegexPatternTooLong { actual: usize, maximum: usize },
    #[error("unsupported message-cache schema version {found}; maximum supported is {supported}")]
    UnsupportedSchema { found: i64, supported: i64 },
    #[error("the SQLite file is not a standalone Telegram message cache: {0}")]
    IncompatibleDatabase(String),
    #[error("corrupt message-cache state: {0}")]
    CorruptData(String),
    #[error("account {account_user_id} has not been bound to this message cache")]
    AccountNotBound { account_user_id: i64 },
    #[error("requested row limit {requested} is outside 1..={maximum}")]
    InvalidLimit { requested: usize, maximum: usize },
    #[error("inclusive lower timestamp {after_unix} is later than upper timestamp {before_unix}")]
    InvalidTimeRange { after_unix: i64, before_unix: i64 },
    #[error("message-cache retention {requested} days is outside 1..={maximum}")]
    InvalidRetentionDays { requested: u32, maximum: u32 },
    #[error("cache generation changed: expected {expected}, current {current}")]
    StaleGeneration { expected: u64, current: u64 },
    #[error("global update cursor changed while a Telegram difference was in flight")]
    StaleUpdateCursor,
    #[error("dialog cache cursor changed while history for chat {chat_id:?} was in flight")]
    StaleDialogCursor { chat_id: String },
    #[error("dialog batch is bound to chat {expected:?}, but contains chat {actual:?}")]
    DialogBatchChatMismatch { expected: String, actual: String },
    #[error("unknown persisted message source {0:?}")]
    UnknownSource(String),
    #[error("unknown persisted cache sync phase {0:?}")]
    UnknownSyncPhase(String),
    #[error("incoming-message cursor {0:?} is invalid or belongs to another account")]
    InvalidEventCursor(String),
    #[error(
        "incoming-message cursor {cursor:?} is no longer continuous with the retained event stream; start a new wait without a cursor"
    )]
    StaleEventCursor { cursor: String },
    #[error(
        "incoming-message waits are temporarily unavailable while the Telegram update stream is being rebased"
    )]
    EventStreamResyncRequired,
}

pub type MessageCacheResult<T> = Result<T, MessageCacheError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MessageSource {
    DialogHistory,
    Search,
    LiveUpdate,
    ExactLookup,
    Import,
    Reconciliation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheSyncPhase {
    Uninitialized,
    CatchingUp,
    Current,
    ResyncRequired,
    Degraded,
}

impl CacheSyncPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::CatchingUp => "catching_up",
            Self::Current => "current",
            Self::ResyncRequired => "resync_required",
            Self::Degraded => "degraded",
        }
    }

    fn parse(value: String) -> MessageCacheResult<Self> {
        match value.as_str() {
            "uninitialized" => Ok(Self::Uninitialized),
            "catching_up" => Ok(Self::CatchingUp),
            "current" => Ok(Self::Current),
            "resync_required" => Ok(Self::ResyncRequired),
            "degraded" => Ok(Self::Degraded),
            _ => Err(MessageCacheError::UnknownSyncPhase(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalUpdateCursor {
    pub pts: i32,
    pub qts: i32,
    pub date: i32,
    pub seq: i32,
    pub unread_count: Option<i32>,
    pub phase: CacheSyncPhase,
    pub observed_at_unix: i64,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CacheUpdateBatch {
    pub dialogs: Vec<DialogMetadata>,
    pub messages: Vec<CachedMessageInput>,
    pub incoming_events: Vec<IncomingMessageEventInput>,
    pub channel_tombstones: Vec<MessageTombstone>,
    pub global_deleted_message_ids: Vec<i32>,
    pub cursor: GlobalUpdateCursor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingMessageEventInput {
    pub chat_id: String,
    pub sender_user_id: i64,
    pub sender_name: String,
    pub sender_username: Option<String>,
    pub message_id: i32,
    pub message_date_unix: i64,
    pub observed_at_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingMessageEvent {
    pub event_id: i64,
    pub chat_id: String,
    pub sender_user_id: i64,
    pub sender_name: String,
    pub sender_username: Option<String>,
    pub message_id: i32,
    pub message_date_unix: i64,
    pub observed_at_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingEventCursor {
    pub token: String,
    baseline_event_id: i64,
    consumed_through_by_chat: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingEventWindow {
    pub cursor: IncomingEventCursor,
    pub events: Vec<IncomingMessageEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateBatchApplyResult {
    pub inserted_incoming_events: usize,
}

impl MessageSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DialogHistory => "dialog_history",
            Self::Search => "search",
            Self::LiveUpdate => "live_update",
            Self::ExactLookup => "exact_lookup",
            Self::Import => "import",
            Self::Reconciliation => "reconciliation",
        }
    }

    fn parse(value: String) -> MessageCacheResult<Self> {
        match value.as_str() {
            "dialog_history" => Ok(Self::DialogHistory),
            "search" => Ok(Self::Search),
            "live_update" => Ok(Self::LiveUpdate),
            "exact_lookup" => Ok(Self::ExactLookup),
            "import" => Ok(Self::Import),
            "reconciliation" => Ok(Self::Reconciliation),
            _ => Err(MessageCacheError::UnknownSource(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountMetadata {
    pub account_user_id: i64,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub observed_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialogMetadata {
    pub chat_id: String,
    pub chat_ref: Option<String>,
    pub chat_kind: Option<String>,
    pub title: Option<String>,
    pub username: Option<String>,
    pub observed_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialogCheckpoint {
    pub chat_id: String,
    pub frontier_message_id: Option<i32>,
    pub oldest_message_id: Option<i32>,
    pub history_complete: bool,
    pub observed_at_unix: i64,
}

#[derive(Debug, Clone)]
pub struct CachedMessageInput {
    pub chat_id: String,
    pub message: MessageSummary,
    pub observed_at_unix: i64,
    pub source: MessageSource,
}

#[derive(Debug, Clone)]
pub struct CachedMessage {
    pub account_user_id: i64,
    pub chat_id: String,
    pub message: MessageSummary,
    pub observed_at_unix: i64,
    pub source: MessageSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageTombstone {
    pub chat_id: String,
    pub message_id: i32,
    pub deleted_at_unix: i64,
    pub source: MessageSource,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageWindow {
    pub chat_id: Option<String>,
    pub after_unix: Option<i64>,
    pub before_unix: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct MessagePage {
    pub generation: u64,
    pub messages: Vec<CachedMessage>,
}

#[derive(Debug, Clone)]
pub struct RegexSearchRequest {
    pub pattern: String,
    pub window: MessageWindow,
    pub max_results: usize,
    pub max_scan_rows: usize,
}

#[derive(Debug, Clone)]
pub struct RegexSearchPage {
    pub generation: u64,
    pub messages: Vec<CachedMessage>,
    pub scanned_rows: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderCount {
    pub sender_ref: String,
    pub message_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineGranularity {
    Hour,
    Day,
}

impl TimelineGranularity {
    const fn seconds(self) -> i64 {
        match self {
            Self::Hour => 60 * 60,
            Self::Day => 24 * 60 * 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelinePoint {
    pub start_unix: i64,
    pub message_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageSourceRow {
    pub account_user_id: i64,
    pub chat_id: String,
    pub message_id: i32,
    pub sender_ref: Option<String>,
    pub text: Option<String>,
    pub date_unix: i64,
    pub edit_date: Option<i64>,
    pub observed_at_unix: i64,
    pub source: MessageSource,
    pub message: MessageSummary,
}

#[derive(Debug, Clone)]
pub struct SourceRowsPage {
    pub generation: u64,
    pub rows: Vec<MessageSourceRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceExport {
    pub generation: u64,
    pub row_count: usize,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatCacheStats {
    pub chat_id: String,
    pub message_count: u64,
    pub tombstone_count: u64,
    pub min_message_id: Option<i32>,
    pub max_message_id: Option<i32>,
    pub oldest_date_unix: Option<i64>,
    pub newest_date_unix: Option<i64>,
    pub frontier_message_id: Option<i32>,
    pub history_complete: bool,
    pub checkpoint_observed_at_unix: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountCacheStats {
    pub account_user_id: i64,
    pub generation: u64,
    pub dialog_count: u64,
    pub message_count: u64,
    pub sync_state_count: u64,
    pub complete_dialog_count: u64,
    pub tombstone_count: u64,
    pub global_tombstone_count: u64,
    pub incoming_event_count: u64,
    pub event_checkpoint_count: u64,
    pub oldest_date_unix: Option<i64>,
    pub newest_date_unix: Option<i64>,
    pub last_checkpoint_unix: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClearAccountResult {
    pub account_user_id: i64,
    pub previous_generation: u64,
    pub generation: u64,
    pub deleted_dialogs: u64,
    pub deleted_messages: u64,
    pub deleted_sync_states: u64,
    pub deleted_tombstones: u64,
    pub deleted_global_tombstones: u64,
    pub deleted_update_states: u64,
    pub deleted_incoming_events: u64,
    pub deleted_event_checkpoints: u64,
    pub deleted_event_stream_states: u64,
    pub deleted_chat_revisions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageRetentionResult {
    pub account_user_id: i64,
    pub cutoff_unix: i64,
    pub removed_messages: u64,
    pub affected_chats: Vec<String>,
}

/// A private, account-scoped SQLite read-model. Connections are opened per
/// synchronous operation, so the cloneable handle owns only the database path
/// and its immutable runtime retention policy.
#[derive(Debug, Clone)]
pub struct MessageCache {
    path: PathBuf,
    max_age_days: Option<u32>,
}

impl MessageCache {
    pub fn new(path: PathBuf) -> Self {
        Self { path, max_age_days: None }
    }

    pub fn open(path: impl Into<PathBuf>) -> MessageCacheResult<Self> {
        let cache = Self::new(path.into());
        cache.initialize()?;
        Ok(cache)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn max_age_days(&self) -> Option<u32> {
        self.max_age_days
    }

    pub fn with_path(mut self, path: PathBuf) -> Self {
        self.path = path;
        self
    }

    pub fn with_max_age_days(mut self, max_age_days: u32) -> MessageCacheResult<Self> {
        validate_retention_days(max_age_days)?;
        self.max_age_days = Some(max_age_days);
        Ok(self)
    }

    pub fn initialize(&self) -> MessageCacheResult<()> {
        drop(self.connection()?);
        Ok(())
    }

    pub fn bind_account(&self, metadata: &AccountMetadata) -> MessageCacheResult<u64> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO cache_accounts (
                 account_user_id, display_name, username, observed_at_unix, generation
             ) VALUES (?1, ?2, ?3, ?4, 0)
             ON CONFLICT(account_user_id) DO UPDATE SET
                 display_name = excluded.display_name,
                 username = excluded.username,
                 observed_at_unix = excluded.observed_at_unix",
            params![
                metadata.account_user_id,
                metadata.display_name,
                metadata.username,
                metadata.observed_at_unix,
            ],
        )?;
        required_generation(&connection, metadata.account_user_id)
    }

    pub fn account_generation(&self, account_user_id: i64) -> MessageCacheResult<Option<u64>> {
        let connection = self.connection()?;
        optional_generation(&connection, account_user_id)
    }

    /// Return every account currently bound to this cache together with the
    /// generation that scopes account-local maintenance transactions.
    pub fn bound_account_generations(&self) -> MessageCacheResult<Vec<(i64, u64)>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT account_user_id, generation
             FROM cache_accounts
             ORDER BY account_user_id ASC",
        )?;
        statement
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
            .map(|row| {
                let (account_user_id, generation) = row?;
                Ok((account_user_id, generation_to_u64(generation)?))
            })
            .collect()
    }

    pub fn global_update_state(
        &self,
        account_user_id: i64,
    ) -> MessageCacheResult<(u64, Option<GlobalUpdateCursor>)> {
        let connection = self.connection()?;
        let generation = required_generation(&connection, account_user_id)?;
        let cursor = load_global_update_cursor(&connection, account_user_id)?;
        Ok((generation, cursor))
    }

    pub fn dialog_sync_state(
        &self,
        account_user_id: i64,
        chat_id: &str,
    ) -> MessageCacheResult<(u64, u64, Option<i32>, Option<DialogCheckpoint>)> {
        let connection = self.connection()?;
        let generation = required_generation(&connection, account_user_id)?;
        let revision = chat_revision_in_connection(&connection, account_user_id, chat_id)?;
        let frontier_message_id =
            max_message_id_in_connection(&connection, account_user_id, chat_id)?;
        let checkpoint = dialog_checkpoint_in_connection(&connection, account_user_id, chat_id)?;
        Ok((generation, revision, frontier_message_id, checkpoint))
    }

    pub fn chat_revision(&self, account_user_id: i64, chat_id: &str) -> MessageCacheResult<u64> {
        let connection = self.connection()?;
        require_account(&connection, account_user_id)?;
        chat_revision_in_connection(&connection, account_user_id, chat_id)
    }

    pub fn require_generation(
        &self,
        account_user_id: i64,
        expected: u64,
    ) -> MessageCacheResult<()> {
        let connection = self.connection()?;
        let current = required_generation(&connection, account_user_id)?;
        if current == expected {
            Ok(())
        } else {
            Err(MessageCacheError::StaleGeneration { expected, current })
        }
    }

    /// Delete message-history rows older than the configured UTC age while
    /// preserving dialogs, delete tombstones, incoming events, and the live
    /// Telegram update cursor. Any affected dialog checkpoint becomes partial
    /// because the cache no longer contains its complete historical prefix.
    pub fn prune_configured_messages_if_generation(
        &self,
        account_user_id: i64,
        expected_generation: u64,
        now_unix: i64,
    ) -> MessageCacheResult<Option<MessageRetentionResult>> {
        let Some(cutoff_unix) = self.retention_cutoff(now_unix)? else {
            return Ok(None);
        };
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_expected_generation(&transaction, account_user_id, expected_generation)?;
        let result = prune_messages_before_in_transaction(
            &transaction,
            account_user_id,
            cutoff_unix,
            now_unix,
        )?;
        bump_chat_revisions(
            &transaction,
            account_user_id,
            &result.affected_chats.iter().cloned().collect(),
        )?;
        transaction.commit()?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(Some(result))
    }

    fn retention_cutoff(&self, now_unix: i64) -> MessageCacheResult<Option<i64>> {
        self.max_age_days
            .map(|days| {
                validate_retention_days(days)?;
                let retention_seconds =
                    i64::from(days).checked_mul(SECONDS_PER_DAY).ok_or_else(|| {
                        MessageCacheError::CorruptData(
                            "message-cache retention duration overflowed".to_string(),
                        )
                    })?;
                now_unix.checked_sub(retention_seconds).ok_or_else(|| {
                    MessageCacheError::CorruptData(
                        "message-cache retention cutoff overflowed".to_string(),
                    )
                })
            })
            .transpose()
    }

    fn enforce_configured_retention(
        &self,
        connection: &Connection,
        account_user_id: i64,
    ) -> MessageCacheResult<Option<MessageRetentionResult>> {
        let now_unix = Utc::now().timestamp();
        let Some(cutoff_unix) = self.retention_cutoff(now_unix)? else {
            return Ok(None);
        };
        prune_messages_before_in_transaction(connection, account_user_id, cutoff_unix, now_unix)
            .map(Some)
    }

    pub fn global_update_cursor(
        &self,
        account_user_id: i64,
    ) -> MessageCacheResult<Option<GlobalUpdateCursor>> {
        let connection = self.connection()?;
        require_account(&connection, account_user_id)?;
        load_global_update_cursor(&connection, account_user_id)
    }

    pub fn set_global_update_cursor(
        &self,
        account_user_id: i64,
        cursor: &GlobalUpdateCursor,
    ) -> MessageCacheResult<()> {
        self.set_global_update_cursor_inner(account_user_id, None, None, cursor)
    }

    pub fn set_global_update_cursor_if_state(
        &self,
        account_user_id: i64,
        expected_generation: u64,
        expected_cursor: Option<&GlobalUpdateCursor>,
        cursor: &GlobalUpdateCursor,
    ) -> MessageCacheResult<()> {
        self.set_global_update_cursor_inner(
            account_user_id,
            Some(expected_generation),
            Some(expected_cursor),
            cursor,
        )
    }

    fn set_global_update_cursor_inner(
        &self,
        account_user_id: i64,
        expected_generation: Option<u64>,
        expected_cursor: Option<Option<&GlobalUpdateCursor>>,
        cursor: &GlobalUpdateCursor,
    ) -> MessageCacheResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(expected_generation) = expected_generation {
            require_expected_generation(&transaction, account_user_id, expected_generation)?;
        } else {
            require_account(&transaction, account_user_id)?;
        }
        if let Some(expected_cursor) = expected_cursor
            && load_global_update_cursor(&transaction, account_user_id)?.as_ref() != expected_cursor
        {
            return Err(MessageCacheError::StaleUpdateCursor);
        }
        if cursor.phase == CacheSyncPhase::ResyncRequired {
            invalidate_incoming_event_stream(
                &transaction,
                account_user_id,
                "telegram_resync",
                Utc::now().timestamp_millis(),
            )?;
        }
        upsert_global_update_cursor(&transaction, account_user_id, cursor)?;
        transaction.commit()?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(())
    }

    pub fn mark_global_update_degraded_if_generation(
        &self,
        account_user_id: i64,
        expected_generation: u64,
        error_code: &str,
    ) -> MessageCacheResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_expected_generation(&transaction, account_user_id, expected_generation)?;
        if let Some(mut cursor) = load_global_update_cursor(&transaction, account_user_id)?
            && cursor.phase != CacheSyncPhase::ResyncRequired
        {
            cursor.phase = CacheSyncPhase::Degraded;
            cursor.last_error_code = Some(error_code.to_string());
            upsert_global_update_cursor(&transaction, account_user_id, &cursor)?;
        }
        transaction.commit()?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(())
    }

    /// Resolve an existing opaque event cursor, or create a restart-safe
    /// checkpoint at the current journal head when no cursor is supplied.
    pub fn incoming_event_window(
        &self,
        account_user_id: i64,
        cursor: Option<&str>,
        now_unix_ms: i64,
    ) -> MessageCacheResult<IncomingEventWindow> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_account(&transaction, account_user_id)?;
        let cursor = match cursor {
            Some(token) => {
                let cursor = resolve_event_cursor(&transaction, account_user_id, token)?;
                ensure_event_stream_available(&transaction, account_user_id)?;
                cursor
            },
            None => {
                ensure_event_stream_available(&transaction, account_user_id)?;
                let baseline_event_id = latest_incoming_event_id(&transaction, account_user_id)?;
                persist_event_cursor(
                    &transaction,
                    account_user_id,
                    baseline_event_id,
                    &BTreeMap::new(),
                    now_unix_ms,
                )?
            },
        };
        ensure_event_cursor_retained(&transaction, account_user_id, &cursor)?;
        let events = load_unconsumed_incoming_events(&transaction, account_user_id, &cursor)?;
        transaction.commit()?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(IncomingEventWindow { cursor, events })
    }

    /// Advance past every event through the supplied journal identity. Events
    /// committed later remain visible from the returned immutable cursor.
    pub fn advance_incoming_event_cursor_all(
        &self,
        account_user_id: i64,
        cursor: &IncomingEventCursor,
        through_event_id: i64,
        now_unix_ms: i64,
    ) -> MessageCacheResult<String> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_account(&transaction, account_user_id)?;
        let cursor = resolve_event_cursor(&transaction, account_user_id, &cursor.token)?;
        ensure_event_stream_available(&transaction, account_user_id)?;
        ensure_event_cursor_retained(&transaction, account_user_id, &cursor)?;
        require_incoming_event(&transaction, account_user_id, None, through_event_id)?;
        if through_event_id < cursor.baseline_event_id {
            return Err(MessageCacheError::CorruptData(format!(
                "event cursor cannot move backward from {} to {through_event_id}",
                cursor.baseline_event_id
            )));
        }
        let mut consumed = cursor.consumed_through_by_chat.clone();
        for event in load_unconsumed_incoming_events(&transaction, account_user_id, &cursor)? {
            if event.event_id > through_event_id {
                break;
            }
            consumed
                .entry(event.chat_id)
                .and_modify(|current| *current = (*current).max(event.event_id))
                .or_insert(event.event_id);
        }
        let successor = compact_and_persist_event_cursor(
            &transaction,
            account_user_id,
            &cursor,
            consumed,
            now_unix_ms,
        )?;
        transaction.commit()?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(successor.token)
    }

    /// Mark one chat's settled burst as observed without consuming interleaved
    /// events from other chats. The persisted cursor is compacted whenever a
    /// fully consumed prefix can be represented by one global baseline.
    pub fn advance_incoming_event_cursor_chat(
        &self,
        account_user_id: i64,
        cursor: &IncomingEventCursor,
        chat_id: &str,
        through_event_id: i64,
        now_unix_ms: i64,
    ) -> MessageCacheResult<String> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_account(&transaction, account_user_id)?;
        let cursor = resolve_event_cursor(&transaction, account_user_id, &cursor.token)?;
        ensure_event_stream_available(&transaction, account_user_id)?;
        ensure_event_cursor_retained(&transaction, account_user_id, &cursor)?;
        require_incoming_event(&transaction, account_user_id, Some(chat_id), through_event_id)?;

        let mut consumed = cursor.consumed_through_by_chat.clone();
        consumed
            .entry(chat_id.to_string())
            .and_modify(|current| *current = (*current).max(through_event_id))
            .or_insert(through_event_id);

        let successor = compact_and_persist_event_cursor(
            &transaction,
            account_user_id,
            &cursor,
            consumed,
            now_unix_ms,
        )?;
        transaction.commit()?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(successor.token)
    }

    pub fn upsert_dialog(
        &self,
        account_user_id: i64,
        dialog: &DialogMetadata,
    ) -> MessageCacheResult<()> {
        let connection = self.connection()?;
        require_account(&connection, account_user_id)?;
        connection.execute(
            "INSERT INTO cache_dialogs (
                 account_user_id, chat_id, chat_ref, chat_kind, title, username,
                 observed_at_unix
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(account_user_id, chat_id) DO UPDATE SET
                 chat_ref = excluded.chat_ref,
                 chat_kind = COALESCE(excluded.chat_kind, cache_dialogs.chat_kind),
                 title = excluded.title,
                 username = excluded.username,
                 observed_at_unix = excluded.observed_at_unix",
            params![
                account_user_id,
                dialog.chat_id,
                dialog.chat_ref,
                dialog.chat_kind,
                dialog.title,
                dialog.username,
                dialog.observed_at_unix,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_dialog_checkpoint(
        &self,
        account_user_id: i64,
        checkpoint: &DialogCheckpoint,
    ) -> MessageCacheResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_account(&transaction, account_user_id)?;
        transaction.execute(
            "INSERT INTO cache_sync_state (
                 account_user_id, chat_id, frontier_message_id, oldest_message_id,
                 history_complete, observed_at_unix
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(account_user_id, chat_id) DO UPDATE SET
                 frontier_message_id = excluded.frontier_message_id,
                 oldest_message_id = excluded.oldest_message_id,
                 history_complete = excluded.history_complete,
                 observed_at_unix = excluded.observed_at_unix",
            params![
                account_user_id,
                checkpoint.chat_id,
                checkpoint.frontier_message_id,
                checkpoint.oldest_message_id,
                i64::from(checkpoint.history_complete),
                checkpoint.observed_at_unix,
            ],
        )?;
        bump_chat_revisions(
            &transaction,
            account_user_id,
            &BTreeSet::from([checkpoint.chat_id.clone()]),
        )?;
        transaction.commit()?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(())
    }

    pub fn dialog_checkpoint(
        &self,
        account_user_id: i64,
        chat_id: &str,
    ) -> MessageCacheResult<Option<DialogCheckpoint>> {
        let connection = self.connection()?;
        require_account(&connection, account_user_id)?;
        connection
            .query_row(
                "SELECT chat_id, frontier_message_id, oldest_message_id,
                        history_complete, observed_at_unix
                 FROM cache_sync_state
                 WHERE account_user_id = ?1 AND chat_id = ?2",
                params![account_user_id, chat_id],
                |row| {
                    Ok(DialogCheckpoint {
                        chat_id: row.get(0)?,
                        frontier_message_id: row.get(1)?,
                        oldest_message_id: row.get(2)?,
                        history_complete: row.get::<_, i64>(3)? != 0,
                        observed_at_unix: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn upsert_message(
        &self,
        account_user_id: i64,
        message: &CachedMessageInput,
    ) -> MessageCacheResult<()> {
        self.upsert_messages(account_user_id, std::slice::from_ref(message))
    }

    pub fn upsert_messages(
        &self,
        account_user_id: i64,
        messages: &[CachedMessageInput],
    ) -> MessageCacheResult<()> {
        self.upsert_messages_inner(account_user_id, None, None, messages)
    }

    pub fn upsert_messages_if_generation(
        &self,
        account_user_id: i64,
        expected_generation: u64,
        messages: &[CachedMessageInput],
    ) -> MessageCacheResult<()> {
        self.upsert_messages_inner(account_user_id, Some(expected_generation), None, messages)
    }

    pub fn upsert_messages_if_state(
        &self,
        account_user_id: i64,
        expected_generation: u64,
        chat_id: &str,
        expected_chat_revision: u64,
        messages: &[CachedMessageInput],
    ) -> MessageCacheResult<()> {
        self.upsert_messages_inner(
            account_user_id,
            Some(expected_generation),
            Some((chat_id.to_string(), expected_chat_revision)),
            messages,
        )
    }

    fn upsert_messages_inner(
        &self,
        account_user_id: i64,
        expected_generation: Option<u64>,
        expected_chat_state: Option<(String, u64)>,
        messages: &[CachedMessageInput],
    ) -> MessageCacheResult<()> {
        if let Some((chat_id, _)) = &expected_chat_state
            && messages.iter().any(|message| message.chat_id != *chat_id)
        {
            let actual = messages
                .iter()
                .find(|message| message.chat_id != *chat_id)
                .map(|message| message.chat_id.clone())
                .unwrap_or_default();
            return Err(MessageCacheError::DialogBatchChatMismatch {
                expected: chat_id.clone(),
                actual,
            });
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(expected_generation) = expected_generation {
            require_expected_generation(&transaction, account_user_id, expected_generation)?;
        } else {
            require_account(&transaction, account_user_id)?;
        }
        if let Some((chat_id, expected_revision)) = &expected_chat_state
            && chat_revision_in_connection(&transaction, account_user_id, chat_id)?
                != *expected_revision
        {
            return Err(MessageCacheError::StaleDialogCursor { chat_id: chat_id.clone() });
        }
        let mut affected_chats = BTreeSet::new();
        {
            let mut upsert = transaction.prepare(
                "INSERT INTO cache_messages (
                     account_user_id, chat_id, message_id, message_json, sender_ref,
                     text, date_unix, edit_date, observed_at_unix, source
                 )
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10
                 WHERE NOT EXISTS (
                     SELECT 1 FROM cache_tombstones
                     WHERE account_user_id = ?1 AND chat_id = ?2 AND message_id = ?3
                 )
                   AND NOT EXISTS (
                     SELECT 1 FROM cache_global_tombstones
                     WHERE account_user_id = ?1 AND message_id = ?3
                       AND NOT EXISTS (
                           SELECT 1 FROM cache_dialogs
                           WHERE account_user_id = ?1 AND chat_id = ?2
                             AND chat_kind IN ('channel', 'supergroup', 'gigagroup')
                       )
                 )
                 ON CONFLICT(account_user_id, chat_id, message_id) DO UPDATE SET
                     message_json = excluded.message_json,
                     sender_ref = excluded.sender_ref,
                     text = excluded.text,
                     date_unix = excluded.date_unix,
                     edit_date = excluded.edit_date,
                     observed_at_unix = excluded.observed_at_unix,
                     source = excluded.source",
            )?;
            for cached in messages {
                let message_json = serde_json::to_string(&cached.message)?;
                let changed = upsert.execute(params![
                    account_user_id,
                    cached.chat_id,
                    cached.message.message_id,
                    message_json,
                    cached.message.sender_ref,
                    cached.message.text,
                    i64::from(cached.message.date),
                    cached.message.edit_date.map(i64::from),
                    cached.observed_at_unix,
                    cached.source.as_str(),
                ])?;
                if changed > 0 {
                    affected_chats.insert(cached.chat_id.clone());
                }
            }
        }
        if let Some(retention) = self.enforce_configured_retention(&transaction, account_user_id)? {
            affected_chats.extend(retention.affected_chats);
        }
        bump_chat_revisions(&transaction, account_user_id, &affected_chats)?;
        transaction.commit()?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(())
    }

    /// Atomically advances one dialog read-model: metadata and message rows are
    /// committed before the checkpoint becomes visible in the same transaction.
    pub fn apply_dialog_batch(
        &self,
        account_user_id: i64,
        dialog: Option<&DialogMetadata>,
        messages: &[CachedMessageInput],
        checkpoint: &DialogCheckpoint,
    ) -> MessageCacheResult<()> {
        self.apply_dialog_batch_inner(
            account_user_id,
            None,
            None,
            None,
            None,
            dialog,
            messages,
            checkpoint,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_dialog_batch_if_state(
        &self,
        account_user_id: i64,
        expected_generation: u64,
        expected_chat_revision: u64,
        expected_frontier_message_id: Option<i32>,
        expected_checkpoint: Option<&DialogCheckpoint>,
        dialog: Option<&DialogMetadata>,
        messages: &[CachedMessageInput],
        checkpoint: &DialogCheckpoint,
    ) -> MessageCacheResult<()> {
        self.apply_dialog_batch_inner(
            account_user_id,
            Some(expected_generation),
            Some(expected_chat_revision),
            Some(expected_frontier_message_id),
            Some(expected_checkpoint),
            dialog,
            messages,
            checkpoint,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_dialog_batch_inner(
        &self,
        account_user_id: i64,
        expected_generation: Option<u64>,
        expected_chat_revision: Option<u64>,
        expected_frontier_message_id: Option<Option<i32>>,
        expected_checkpoint: Option<Option<&DialogCheckpoint>>,
        dialog: Option<&DialogMetadata>,
        messages: &[CachedMessageInput],
        checkpoint: &DialogCheckpoint,
    ) -> MessageCacheResult<()> {
        if let Some(dialog) = dialog
            && dialog.chat_id != checkpoint.chat_id
        {
            return Err(MessageCacheError::DialogBatchChatMismatch {
                expected: checkpoint.chat_id.clone(),
                actual: dialog.chat_id.clone(),
            });
        }
        if let Some(message) = messages.iter().find(|message| message.chat_id != checkpoint.chat_id)
        {
            return Err(MessageCacheError::DialogBatchChatMismatch {
                expected: checkpoint.chat_id.clone(),
                actual: message.chat_id.clone(),
            });
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(expected_generation) = expected_generation {
            require_expected_generation(&transaction, account_user_id, expected_generation)?;
        } else {
            require_account(&transaction, account_user_id)?;
        }
        if let Some(expected_chat_revision) = expected_chat_revision
            && chat_revision_in_connection(&transaction, account_user_id, &checkpoint.chat_id)?
                != expected_chat_revision
        {
            return Err(MessageCacheError::StaleDialogCursor {
                chat_id: checkpoint.chat_id.clone(),
            });
        }
        if let Some(expected_frontier_message_id) = expected_frontier_message_id {
            let current_frontier_message_id =
                max_message_id_in_connection(&transaction, account_user_id, &checkpoint.chat_id)?;
            let current_checkpoint = dialog_checkpoint_in_connection(
                &transaction,
                account_user_id,
                &checkpoint.chat_id,
            )?;
            if current_frontier_message_id != expected_frontier_message_id
                || current_checkpoint.as_ref() != expected_checkpoint.flatten()
            {
                return Err(MessageCacheError::StaleDialogCursor {
                    chat_id: checkpoint.chat_id.clone(),
                });
            }
        }
        if let Some(dialog) = dialog {
            transaction.execute(
                "INSERT INTO cache_dialogs (
                     account_user_id, chat_id, chat_ref, chat_kind, title, username,
                     observed_at_unix
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(account_user_id, chat_id) DO UPDATE SET
                     chat_ref = excluded.chat_ref,
                     chat_kind = COALESCE(excluded.chat_kind, cache_dialogs.chat_kind),
                     title = excluded.title,
                     username = excluded.username,
                     observed_at_unix = excluded.observed_at_unix",
                params![
                    account_user_id,
                    dialog.chat_id,
                    dialog.chat_ref,
                    dialog.chat_kind,
                    dialog.title,
                    dialog.username,
                    dialog.observed_at_unix,
                ],
            )?;
        }
        {
            let mut upsert = transaction.prepare(
                "INSERT INTO cache_messages (
                     account_user_id, chat_id, message_id, message_json, sender_ref,
                     text, date_unix, edit_date, observed_at_unix, source
                 )
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10
                 WHERE NOT EXISTS (
                     SELECT 1 FROM cache_tombstones
                     WHERE account_user_id = ?1 AND chat_id = ?2 AND message_id = ?3
                 )
                   AND NOT EXISTS (
                     SELECT 1 FROM cache_global_tombstones
                     WHERE account_user_id = ?1 AND message_id = ?3
                       AND NOT EXISTS (
                           SELECT 1 FROM cache_dialogs
                           WHERE account_user_id = ?1 AND chat_id = ?2
                             AND chat_kind IN ('channel', 'supergroup', 'gigagroup')
                       )
                 )
                 ON CONFLICT(account_user_id, chat_id, message_id) DO UPDATE SET
                     message_json = excluded.message_json,
                     sender_ref = excluded.sender_ref,
                     text = excluded.text,
                     date_unix = excluded.date_unix,
                     edit_date = excluded.edit_date,
                     observed_at_unix = excluded.observed_at_unix,
                     source = excluded.source",
            )?;
            for cached in messages {
                let message_json = serde_json::to_string(&cached.message)?;
                upsert.execute(params![
                    account_user_id,
                    cached.chat_id,
                    cached.message.message_id,
                    message_json,
                    cached.message.sender_ref,
                    cached.message.text,
                    i64::from(cached.message.date),
                    cached.message.edit_date.map(i64::from),
                    cached.observed_at_unix,
                    cached.source.as_str(),
                ])?;
            }
        }
        transaction.execute(
            "INSERT INTO cache_sync_state (
                 account_user_id, chat_id, frontier_message_id, oldest_message_id,
                 history_complete, observed_at_unix
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(account_user_id, chat_id) DO UPDATE SET
                 frontier_message_id = excluded.frontier_message_id,
                 oldest_message_id = excluded.oldest_message_id,
                 history_complete = excluded.history_complete,
                 observed_at_unix = excluded.observed_at_unix",
            params![
                account_user_id,
                checkpoint.chat_id,
                checkpoint.frontier_message_id,
                checkpoint.oldest_message_id,
                i64::from(checkpoint.history_complete),
                checkpoint.observed_at_unix,
            ],
        )?;
        let mut affected_chats = BTreeSet::from([checkpoint.chat_id.clone()]);
        if let Some(retention) = self.enforce_configured_retention(&transaction, account_user_id)? {
            affected_chats.extend(retention.affected_chats);
        }
        bump_chat_revisions(&transaction, account_user_id, &affected_chats)?;
        transaction.commit()?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(())
    }

    /// Atomically applies a global Telegram update difference and advances its
    /// cursor only after every typed cache row in that difference is durable.
    pub fn apply_update_batch(
        &self,
        account_user_id: i64,
        batch: &CacheUpdateBatch,
    ) -> MessageCacheResult<UpdateBatchApplyResult> {
        self.apply_update_batch_inner(account_user_id, None, None, batch)
    }

    pub fn apply_update_batch_if_state(
        &self,
        account_user_id: i64,
        expected_generation: u64,
        expected_cursor: &GlobalUpdateCursor,
        batch: &CacheUpdateBatch,
    ) -> MessageCacheResult<UpdateBatchApplyResult> {
        self.apply_update_batch_inner(
            account_user_id,
            Some(expected_generation),
            Some(expected_cursor),
            batch,
        )
    }

    fn apply_update_batch_inner(
        &self,
        account_user_id: i64,
        expected_generation: Option<u64>,
        expected_cursor: Option<&GlobalUpdateCursor>,
        batch: &CacheUpdateBatch,
    ) -> MessageCacheResult<UpdateBatchApplyResult> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(expected_generation) = expected_generation {
            require_expected_generation(&transaction, account_user_id, expected_generation)?;
        } else {
            require_account(&transaction, account_user_id)?;
        }
        if let Some(expected_cursor) = expected_cursor
            && load_global_update_cursor(&transaction, account_user_id)?.as_ref()
                != Some(expected_cursor)
        {
            return Err(MessageCacheError::StaleUpdateCursor);
        }
        let mut affected_chats = BTreeSet::new();

        {
            let mut upsert_dialog = transaction.prepare(
                "INSERT INTO cache_dialogs (
                     account_user_id, chat_id, chat_ref, chat_kind, title, username,
                     observed_at_unix
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(account_user_id, chat_id) DO UPDATE SET
                     chat_ref = excluded.chat_ref,
                     chat_kind = COALESCE(excluded.chat_kind, cache_dialogs.chat_kind),
                     title = excluded.title,
                     username = excluded.username,
                     observed_at_unix = excluded.observed_at_unix",
            )?;
            for dialog in &batch.dialogs {
                upsert_dialog.execute(params![
                    account_user_id,
                    dialog.chat_id,
                    dialog.chat_ref,
                    dialog.chat_kind,
                    dialog.title,
                    dialog.username,
                    dialog.observed_at_unix,
                ])?;
            }
        }

        let mut inserted_incoming_events = 0_usize;
        if !batch.incoming_events.is_empty() {
            transaction.execute(
                "INSERT INTO cache_event_stream_state (
                     account_user_id, pruned_through_event_id
                 ) VALUES (?1, 0)
                 ON CONFLICT(account_user_id) DO NOTHING",
                [account_user_id],
            )?;
            {
                let mut insert_event = transaction.prepare(
                    "INSERT OR IGNORE INTO cache_incoming_events (
                         account_user_id, chat_id, sender_user_id, sender_name,
                         sender_username, message_id, message_date_unix,
                         observed_at_unix_ms
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                )?;
                for event in &batch.incoming_events {
                    inserted_incoming_events =
                        inserted_incoming_events.saturating_add(insert_event.execute(params![
                            account_user_id,
                            event.chat_id,
                            event.sender_user_id,
                            event.sender_name,
                            event.sender_username,
                            event.message_id,
                            event.message_date_unix,
                            event.observed_at_unix_ms,
                        ])?);
                }
            }
            prune_incoming_events(&transaction, account_user_id)?;
        }

        {
            let mut upsert_message = transaction.prepare(
                "INSERT INTO cache_messages (
                     account_user_id, chat_id, message_id, message_json, sender_ref,
                     text, date_unix, edit_date, observed_at_unix, source
                 )
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10
                 WHERE NOT EXISTS (
                     SELECT 1 FROM cache_global_tombstones
                     WHERE account_user_id = ?1 AND message_id = ?3
                       AND NOT EXISTS (
                           SELECT 1 FROM cache_dialogs
                           WHERE account_user_id = ?1 AND chat_id = ?2
                             AND chat_kind IN ('channel', 'supergroup', 'gigagroup')
                       )
                 )
                 ON CONFLICT(account_user_id, chat_id, message_id) DO UPDATE SET
                     message_json = excluded.message_json,
                     sender_ref = excluded.sender_ref,
                     text = excluded.text,
                     date_unix = excluded.date_unix,
                     edit_date = excluded.edit_date,
                     observed_at_unix = excluded.observed_at_unix,
                     source = excluded.source",
            )?;
            let mut remove_channel_tombstone = transaction.prepare(
                "DELETE FROM cache_tombstones
                 WHERE account_user_id = ?1 AND chat_id = ?2 AND message_id = ?3",
            )?;
            for cached in &batch.messages {
                let message_json = serde_json::to_string(&cached.message)?;
                let changed = upsert_message.execute(params![
                    account_user_id,
                    cached.chat_id,
                    cached.message.message_id,
                    message_json,
                    cached.message.sender_ref,
                    cached.message.text,
                    i64::from(cached.message.date),
                    cached.message.edit_date.map(i64::from),
                    cached.observed_at_unix,
                    cached.source.as_str(),
                ])?;
                if changed > 0 {
                    remove_channel_tombstone.execute(params![
                        account_user_id,
                        cached.chat_id,
                        cached.message.message_id,
                    ])?;
                    affected_chats.insert(cached.chat_id.clone());
                }
            }
        }

        {
            let mut remove_message = transaction.prepare(
                "DELETE FROM cache_messages
                 WHERE account_user_id = ?1 AND chat_id = ?2 AND message_id = ?3",
            )?;
            let mut upsert_tombstone = transaction.prepare(
                "INSERT INTO cache_tombstones (
                     account_user_id, chat_id, message_id, deleted_at_unix, source
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(account_user_id, chat_id, message_id) DO UPDATE SET
                     deleted_at_unix = excluded.deleted_at_unix,
                     source = excluded.source",
            )?;
            for tombstone in &batch.channel_tombstones {
                remove_message.execute(params![
                    account_user_id,
                    tombstone.chat_id,
                    tombstone.message_id,
                ])?;
                upsert_tombstone.execute(params![
                    account_user_id,
                    tombstone.chat_id,
                    tombstone.message_id,
                    tombstone.deleted_at_unix,
                    tombstone.source.as_str(),
                ])?;
                affected_chats.insert(tombstone.chat_id.clone());
            }
        }

        {
            let mut matching_chats = transaction.prepare(
                "SELECT DISTINCT messages.chat_id
                 FROM cache_messages AS messages
                 LEFT JOIN cache_dialogs AS dialogs
                   ON dialogs.account_user_id = messages.account_user_id
                  AND dialogs.chat_id = messages.chat_id
                 WHERE messages.account_user_id = ?1 AND messages.message_id = ?2
                   AND (dialogs.chat_kind IS NULL OR dialogs.chat_kind NOT IN (
                       'channel', 'supergroup', 'gigagroup'
                   ))
                 ORDER BY messages.chat_id ASC",
            )?;
            let mut remove_messages = transaction.prepare(
                "DELETE FROM cache_messages
                 WHERE account_user_id = ?1 AND message_id = ?2
                   AND chat_id NOT IN (
                       SELECT chat_id FROM cache_dialogs
                       WHERE account_user_id = ?1
                         AND chat_kind IN ('channel', 'supergroup', 'gigagroup')
                   )",
            )?;
            let mut upsert_global_tombstone = transaction.prepare(
                "INSERT INTO cache_global_tombstones (
                     account_user_id, message_id, deleted_at_unix
                 ) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_user_id, message_id) DO UPDATE SET
                     deleted_at_unix = excluded.deleted_at_unix",
            )?;
            for message_id in &batch.global_deleted_message_ids {
                let chats = matching_chats
                    .query_map(params![account_user_id, message_id], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                affected_chats.extend(chats);
                remove_messages.execute(params![account_user_id, message_id])?;
                upsert_global_tombstone.execute(params![
                    account_user_id,
                    message_id,
                    batch.cursor.observed_at_unix,
                ])?;
            }
        }

        upsert_global_update_cursor(&transaction, account_user_id, &batch.cursor)?;
        if let Some(retention) = self.enforce_configured_retention(&transaction, account_user_id)? {
            affected_chats.extend(retention.affected_chats);
        }
        bump_chat_revisions(&transaction, account_user_id, &affected_chats)?;
        transaction.commit()?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(UpdateBatchApplyResult { inserted_incoming_events })
    }

    pub fn tombstone_message(
        &self,
        account_user_id: i64,
        tombstone: &MessageTombstone,
    ) -> MessageCacheResult<()> {
        self.tombstone_messages(account_user_id, std::slice::from_ref(tombstone))
    }

    pub fn tombstone_messages(
        &self,
        account_user_id: i64,
        tombstones: &[MessageTombstone],
    ) -> MessageCacheResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_account(&transaction, account_user_id)?;
        let mut affected_chats = BTreeSet::new();
        {
            let mut remove_message = transaction.prepare(
                "DELETE FROM cache_messages
                 WHERE account_user_id = ?1 AND chat_id = ?2 AND message_id = ?3",
            )?;
            let mut upsert_tombstone = transaction.prepare(
                "INSERT INTO cache_tombstones (
                     account_user_id, chat_id, message_id, deleted_at_unix, source
                 ) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(account_user_id, chat_id, message_id) DO UPDATE SET
                     deleted_at_unix = excluded.deleted_at_unix,
                     source = excluded.source",
            )?;
            for tombstone in tombstones {
                remove_message.execute(params![
                    account_user_id,
                    tombstone.chat_id,
                    tombstone.message_id,
                ])?;
                upsert_tombstone.execute(params![
                    account_user_id,
                    tombstone.chat_id,
                    tombstone.message_id,
                    tombstone.deleted_at_unix,
                    tombstone.source.as_str(),
                ])?;
                affected_chats.insert(tombstone.chat_id.clone());
            }
        }
        bump_chat_revisions(&transaction, account_user_id, &affected_chats)?;
        transaction.commit()?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(())
    }

    pub fn exact_get(
        &self,
        account_user_id: i64,
        chat_id: &str,
        message_id: i32,
    ) -> MessageCacheResult<Option<CachedMessage>> {
        let connection = self.connection()?;
        require_account(&connection, account_user_id)?;
        let raw = connection
            .query_row(
                &format!(
                    "{MESSAGE_COLUMNS}
                     WHERE account_user_id = ?1 AND chat_id = ?2 AND message_id = ?3"
                ),
                params![account_user_id, chat_id, message_id],
                map_raw_message,
            )
            .optional()?;
        raw.map(decode_message).transpose()
    }

    pub fn query_newest(
        &self,
        account_user_id: i64,
        window: &MessageWindow,
        limit: usize,
    ) -> MessageCacheResult<MessagePage> {
        validate_window(window)?;
        validate_limit(limit, MAX_MESSAGE_QUERY_ROWS)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let generation = required_generation(&transaction, account_user_id)?;
        let messages = load_messages(
            &transaction,
            account_user_id,
            window,
            None,
            false,
            limit,
            MessageOrder::NewestFirst,
        )?;
        transaction.commit()?;
        Ok(MessagePage { generation, messages })
    }

    pub fn regex_search(
        &self,
        account_user_id: i64,
        request: &RegexSearchRequest,
    ) -> MessageCacheResult<RegexSearchPage> {
        validate_window(&request.window)?;
        validate_limit(request.max_results, MAX_MESSAGE_QUERY_ROWS)?;
        validate_limit(request.max_scan_rows, MAX_REGEX_SCAN_ROWS)?;
        if request.pattern.len() > MAX_REGEX_PATTERN_BYTES {
            return Err(MessageCacheError::RegexPatternTooLong {
                actual: request.pattern.len(),
                maximum: MAX_REGEX_PATTERN_BYTES,
            });
        }
        let regex = RegexBuilder::new(&request.pattern)
            .case_insensitive(true)
            .build()
            .map_err(MessageCacheError::InvalidRegex)?;

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let generation = required_generation(&transaction, account_user_id)?;
        let fetch_limit = request.max_scan_rows.saturating_add(1);
        let mut raw = load_raw_messages(
            &transaction,
            account_user_id,
            &request.window,
            None,
            true,
            fetch_limit,
            MessageOrder::NewestFirst,
        )?;
        let more_candidates = raw.len() > request.max_scan_rows;
        raw.truncate(request.max_scan_rows);
        let scanned_rows = raw.len();
        let mut messages = Vec::new();
        let mut more_matches = false;
        for row in raw {
            let cached = decode_message(row)?;
            if regex.is_match(cached.message.text.as_deref().unwrap_or_default()) {
                if messages.len() < request.max_results {
                    messages.push(cached);
                } else {
                    more_matches = true;
                }
            }
        }
        transaction.commit()?;
        Ok(RegexSearchPage {
            generation,
            messages,
            scanned_rows,
            truncated: more_candidates || more_matches,
        })
    }

    pub fn top_senders(
        &self,
        account_user_id: i64,
        window: &MessageWindow,
        limit: usize,
    ) -> MessageCacheResult<Vec<SenderCount>> {
        validate_window(window)?;
        validate_limit(limit, MAX_MESSAGE_QUERY_ROWS)?;
        let connection = self.connection()?;
        require_account(&connection, account_user_id)?;
        let mut statement = connection.prepare(
            "SELECT sender_ref, COUNT(*)
             FROM cache_messages
             WHERE account_user_id = ?1
               AND sender_ref IS NOT NULL
               AND (?2 IS NULL OR chat_id = ?2)
               AND (?3 IS NULL OR date_unix >= ?3)
               AND (?4 IS NULL OR date_unix <= ?4)
             GROUP BY sender_ref
             ORDER BY COUNT(*) DESC, sender_ref ASC
             LIMIT ?5",
        )?;
        let mut rows = statement.query(params![
            account_user_id,
            window.chat_id.as_deref(),
            window.after_unix,
            window.before_unix,
            usize_to_i64(limit)?,
        ])?;
        let mut senders = Vec::new();
        while let Some(row) = rows.next()? {
            senders.push(SenderCount {
                sender_ref: row.get(0)?,
                message_count: count_to_u64(row.get(1)?)?,
            });
        }
        Ok(senders)
    }

    pub fn timeline(
        &self,
        account_user_id: i64,
        window: &MessageWindow,
        granularity: TimelineGranularity,
    ) -> MessageCacheResult<Vec<TimelinePoint>> {
        validate_window(window)?;
        let connection = self.connection()?;
        require_account(&connection, account_user_id)?;
        let seconds = granularity.seconds();
        let mut statement = connection.prepare(
            "SELECT date_unix - (((date_unix % ?5) + ?5) % ?5) AS bucket_start,
                    COUNT(*)
             FROM cache_messages
             WHERE account_user_id = ?1
               AND (?2 IS NULL OR chat_id = ?2)
               AND (?3 IS NULL OR date_unix >= ?3)
               AND (?4 IS NULL OR date_unix <= ?4)
             GROUP BY bucket_start
             ORDER BY bucket_start ASC",
        )?;
        let mut rows = statement.query(params![
            account_user_id,
            window.chat_id.as_deref(),
            window.after_unix,
            window.before_unix,
            seconds,
        ])?;
        let mut timeline = Vec::new();
        while let Some(row) = rows.next()? {
            timeline.push(TimelinePoint {
                start_unix: row.get(0)?,
                message_count: count_to_u64(row.get(1)?)?,
            });
        }
        Ok(timeline)
    }

    pub fn utc_today(
        &self,
        account_user_id: i64,
        chat_id: Option<&str>,
        now: DateTime<Utc>,
        limit: usize,
    ) -> MessageCacheResult<MessagePage> {
        validate_limit(limit, MAX_MESSAGE_QUERY_ROWS)?;
        let start_unix = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .expect("midnight is always a valid naive time")
            .and_utc()
            .timestamp();
        let end_unix = start_unix.checked_add(24 * 60 * 60).ok_or_else(|| {
            MessageCacheError::CorruptData("UTC day upper bound overflowed".to_string())
        })?;
        let window = MessageWindow {
            chat_id: chat_id.map(str::to_owned),
            after_unix: Some(start_unix),
            before_unix: None,
        };
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let generation = required_generation(&transaction, account_user_id)?;
        let messages = load_messages(
            &transaction,
            account_user_id,
            &window,
            Some(end_unix),
            false,
            limit,
            MessageOrder::Chronological,
        )?;
        transaction.commit()?;
        Ok(MessagePage { generation, messages })
    }

    pub fn chronological_source_rows(
        &self,
        account_user_id: i64,
        window: &MessageWindow,
        limit: usize,
    ) -> MessageCacheResult<SourceRowsPage> {
        validate_window(window)?;
        validate_limit(limit, MAX_SOURCE_ROWS)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let generation = required_generation(&transaction, account_user_id)?;
        let messages = load_messages(
            &transaction,
            account_user_id,
            window,
            None,
            false,
            limit,
            MessageOrder::Chronological,
        )?;
        let rows = messages.into_iter().map(MessageSourceRow::from).collect();
        transaction.commit()?;
        Ok(SourceRowsPage { generation, rows })
    }

    pub fn source_rows_json(
        &self,
        account_user_id: i64,
        window: &MessageWindow,
        limit: usize,
    ) -> MessageCacheResult<SourceExport> {
        let page = self.chronological_source_rows(account_user_id, window, limit)?;
        let body = serde_json::to_string(&page.rows)?;
        Ok(SourceExport { generation: page.generation, row_count: page.rows.len(), body })
    }

    pub fn source_rows_csv(
        &self,
        account_user_id: i64,
        window: &MessageWindow,
        limit: usize,
    ) -> MessageCacheResult<SourceExport> {
        let page = self.chronological_source_rows(account_user_id, window, limit)?;
        if page.rows.is_empty() {
            return Ok(SourceExport {
                generation: page.generation,
                row_count: 0,
                body: String::new(),
            });
        }
        let mut body = String::from(CSV_HEADER);
        for row in &page.rows {
            let message_json = serde_json::to_string(&row.message)?;
            let fields = [
                row.account_user_id.to_string(),
                row.chat_id.clone(),
                row.message_id.to_string(),
                row.sender_ref.clone().unwrap_or_default(),
                row.text.clone().unwrap_or_default(),
                row.date_unix.to_string(),
                row.edit_date.map(|value| value.to_string()).unwrap_or_default(),
                row.observed_at_unix.to_string(),
                row.source.as_str().to_string(),
                message_json,
            ];
            for (index, field) in fields.iter().enumerate() {
                if index > 0 {
                    body.push(',');
                }
                push_csv_field(&mut body, field);
            }
            body.push_str("\r\n");
        }
        Ok(SourceExport { generation: page.generation, row_count: page.rows.len(), body })
    }

    pub fn max_message_id(
        &self,
        account_user_id: i64,
        chat_id: &str,
    ) -> MessageCacheResult<Option<i32>> {
        let connection = self.connection()?;
        require_account(&connection, account_user_id)?;
        connection
            .query_row(
                "SELECT MAX(message_id) FROM cache_messages
                 WHERE account_user_id = ?1 AND chat_id = ?2",
                params![account_user_id, chat_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn chat_stats(
        &self,
        account_user_id: i64,
        chat_id: &str,
    ) -> MessageCacheResult<Option<ChatCacheStats>> {
        let connection = self.connection()?;
        require_account(&connection, account_user_id)?;
        let mut stats = load_chat_stats(&connection, account_user_id, Some(chat_id))?;
        Ok(stats.pop())
    }

    pub fn per_chat_stats(&self, account_user_id: i64) -> MessageCacheResult<Vec<ChatCacheStats>> {
        let connection = self.connection()?;
        require_account(&connection, account_user_id)?;
        load_chat_stats(&connection, account_user_id, None)
    }

    pub fn account_stats(
        &self,
        account_user_id: i64,
    ) -> MessageCacheResult<Option<AccountCacheStats>> {
        let connection = self.connection()?;
        let raw = connection
            .query_row(
                "SELECT a.account_user_id, a.generation,
                        (SELECT COUNT(*) FROM cache_dialogs d
                         WHERE d.account_user_id = a.account_user_id),
                        (SELECT COUNT(*) FROM cache_messages m
                         WHERE m.account_user_id = a.account_user_id),
                        (SELECT COUNT(*) FROM cache_sync_state s
                         WHERE s.account_user_id = a.account_user_id),
                        (SELECT COUNT(*) FROM cache_sync_state s
                         WHERE s.account_user_id = a.account_user_id
                           AND s.history_complete = 1),
                        (SELECT COUNT(*) FROM cache_tombstones t
                         WHERE t.account_user_id = a.account_user_id),
                        (SELECT COUNT(*) FROM cache_global_tombstones t
                         WHERE t.account_user_id = a.account_user_id),
                        (SELECT COUNT(*) FROM cache_incoming_events e
                         WHERE e.account_user_id = a.account_user_id),
                        (SELECT COUNT(*) FROM cache_event_checkpoints c
                         WHERE c.account_user_id = a.account_user_id),
                        (SELECT MIN(date_unix) FROM cache_messages m
                         WHERE m.account_user_id = a.account_user_id),
                        (SELECT MAX(date_unix) FROM cache_messages m
                         WHERE m.account_user_id = a.account_user_id),
                        (SELECT MAX(observed_at_unix)
                         FROM (
                             SELECT observed_at_unix FROM cache_sync_state s
                             WHERE s.account_user_id = a.account_user_id
                             UNION ALL
                             SELECT observed_at_unix FROM cache_update_state u
                             WHERE u.account_user_id = a.account_user_id
                         ))
                 FROM cache_accounts a
                 WHERE a.account_user_id = ?1",
                [account_user_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, i64>(9)?,
                        row.get::<_, Option<i64>>(10)?,
                        row.get::<_, Option<i64>>(11)?,
                        row.get::<_, Option<i64>>(12)?,
                    ))
                },
            )
            .optional()?;
        raw.map(|raw| {
            Ok(AccountCacheStats {
                account_user_id: raw.0,
                generation: generation_to_u64(raw.1)?,
                dialog_count: count_to_u64(raw.2)?,
                message_count: count_to_u64(raw.3)?,
                sync_state_count: count_to_u64(raw.4)?,
                complete_dialog_count: count_to_u64(raw.5)?,
                tombstone_count: count_to_u64(raw.6)?,
                global_tombstone_count: count_to_u64(raw.7)?,
                incoming_event_count: count_to_u64(raw.8)?,
                event_checkpoint_count: count_to_u64(raw.9)?,
                oldest_date_unix: raw.10,
                newest_date_unix: raw.11,
                last_checkpoint_unix: raw.12,
            })
        })
        .transpose()
    }

    pub fn clear_account(
        &self,
        account_user_id: i64,
        cleared_at_unix: i64,
    ) -> MessageCacheResult<ClearAccountResult> {
        self.clear_account_with_cursor(account_user_id, cleared_at_unix, None)
    }

    pub fn clear_account_with_cursor(
        &self,
        account_user_id: i64,
        cleared_at_unix: i64,
        successor_cursor: Option<&GlobalUpdateCursor>,
    ) -> MessageCacheResult<ClearAccountResult> {
        self.clear_account_with_cursor_inner(
            account_user_id,
            None,
            None,
            cleared_at_unix,
            successor_cursor,
        )
    }

    pub fn clear_account_with_cursor_if_generation(
        &self,
        account_user_id: i64,
        expected_generation: u64,
        cleared_at_unix: i64,
        successor_cursor: Option<&GlobalUpdateCursor>,
    ) -> MessageCacheResult<ClearAccountResult> {
        self.clear_account_with_cursor_inner(
            account_user_id,
            Some(expected_generation),
            None,
            cleared_at_unix,
            successor_cursor,
        )
    }

    pub fn clear_account_with_cursor_if_state(
        &self,
        account_user_id: i64,
        expected_generation: u64,
        expected_cursor: Option<&GlobalUpdateCursor>,
        cleared_at_unix: i64,
        successor_cursor: Option<&GlobalUpdateCursor>,
    ) -> MessageCacheResult<ClearAccountResult> {
        self.clear_account_with_cursor_inner(
            account_user_id,
            Some(expected_generation),
            Some(expected_cursor),
            cleared_at_unix,
            successor_cursor,
        )
    }

    fn clear_account_with_cursor_inner(
        &self,
        account_user_id: i64,
        expected_generation: Option<u64>,
        expected_cursor: Option<Option<&GlobalUpdateCursor>>,
        cleared_at_unix: i64,
        successor_cursor: Option<&GlobalUpdateCursor>,
    ) -> MessageCacheResult<ClearAccountResult> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous_generation = required_generation(&transaction, account_user_id)?;
        if let Some(expected_generation) = expected_generation
            && previous_generation != expected_generation
        {
            return Err(MessageCacheError::StaleGeneration {
                expected: expected_generation,
                current: previous_generation,
            });
        }
        if let Some(expected_cursor) = expected_cursor
            && load_global_update_cursor(&transaction, account_user_id)?.as_ref() != expected_cursor
        {
            return Err(MessageCacheError::StaleUpdateCursor);
        }
        let generation = previous_generation.checked_add(1).ok_or_else(|| {
            MessageCacheError::CorruptData(format!(
                "generation overflow for account {account_user_id}"
            ))
        })?;
        let deleted_messages = transaction
            .execute("DELETE FROM cache_messages WHERE account_user_id = ?1", [account_user_id])?;
        let deleted_sync_states = transaction.execute(
            "DELETE FROM cache_sync_state WHERE account_user_id = ?1",
            [account_user_id],
        )?;
        let deleted_tombstones = transaction.execute(
            "DELETE FROM cache_tombstones WHERE account_user_id = ?1",
            [account_user_id],
        )?;
        let deleted_global_tombstones = transaction.execute(
            "DELETE FROM cache_global_tombstones WHERE account_user_id = ?1",
            [account_user_id],
        )?;
        let deleted_update_states = transaction.execute(
            "DELETE FROM cache_update_state WHERE account_user_id = ?1",
            [account_user_id],
        )?;
        let deleted_chat_revisions = transaction.execute(
            "DELETE FROM cache_chat_revisions WHERE account_user_id = ?1",
            [account_user_id],
        )?;
        tombstone_event_cursors(
            &transaction,
            account_user_id,
            "cache_clear",
            Utc::now().timestamp_millis(),
        )?;
        let deleted_event_checkpoints = transaction.execute(
            "DELETE FROM cache_event_checkpoints WHERE account_user_id = ?1",
            [account_user_id],
        )?;
        let deleted_incoming_events = transaction.execute(
            "DELETE FROM cache_incoming_events WHERE account_user_id = ?1",
            [account_user_id],
        )?;
        let deleted_event_stream_states = transaction.execute(
            "DELETE FROM cache_event_stream_state WHERE account_user_id = ?1",
            [account_user_id],
        )?;
        let deleted_dialogs = transaction
            .execute("DELETE FROM cache_dialogs WHERE account_user_id = ?1", [account_user_id])?;
        transaction.execute(
            "UPDATE cache_accounts
             SET generation = ?2, observed_at_unix = ?3
             WHERE account_user_id = ?1",
            params![account_user_id, u64_to_i64(generation)?, cleared_at_unix],
        )?;
        if let Some(cursor) = successor_cursor {
            upsert_global_update_cursor(&transaction, account_user_id, cursor)?;
        }
        prune_event_cursor_tombstones(&transaction, account_user_id)?;
        transaction.commit()?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(ClearAccountResult {
            account_user_id,
            previous_generation,
            generation,
            deleted_dialogs: usize_count_to_u64(deleted_dialogs)?,
            deleted_messages: usize_count_to_u64(deleted_messages)?,
            deleted_sync_states: usize_count_to_u64(deleted_sync_states)?,
            deleted_tombstones: usize_count_to_u64(deleted_tombstones)?,
            deleted_global_tombstones: usize_count_to_u64(deleted_global_tombstones)?,
            deleted_update_states: usize_count_to_u64(deleted_update_states)?,
            deleted_incoming_events: usize_count_to_u64(deleted_incoming_events)?,
            deleted_event_checkpoints: usize_count_to_u64(deleted_event_checkpoints)?,
            deleted_event_stream_states: usize_count_to_u64(deleted_event_stream_states)?,
            deleted_chat_revisions: usize_count_to_u64(deleted_chat_revisions)?,
        })
    }

    fn connection(&self) -> MessageCacheResult<Connection> {
        let mut connection = open_private_wal(&self.path, SynchronousMode::Normal)?;
        initialize_or_validate_schema(&mut connection)?;
        restrict_private_sqlite_files(&self.path)?;
        Ok(connection)
    }
}

impl From<CachedMessage> for MessageSourceRow {
    fn from(cached: CachedMessage) -> Self {
        Self {
            account_user_id: cached.account_user_id,
            chat_id: cached.chat_id,
            message_id: cached.message.message_id,
            sender_ref: cached.message.sender_ref.clone(),
            text: cached.message.text.clone(),
            date_unix: i64::from(cached.message.date),
            edit_date: cached.message.edit_date.map(i64::from),
            observed_at_unix: cached.observed_at_unix,
            source: cached.source,
            message: cached.message,
        }
    }
}

#[derive(Debug)]
struct RawMessage {
    account_user_id: i64,
    chat_id: String,
    message_id: i32,
    message_json: String,
    sender_ref: Option<String>,
    text: Option<String>,
    date_unix: i64,
    edit_date: Option<i64>,
    observed_at_unix: i64,
    source: String,
}

#[derive(Debug, Clone, Copy)]
enum MessageOrder {
    NewestFirst,
    Chronological,
}

const MESSAGE_COLUMNS: &str =
    "SELECT account_user_id, chat_id, message_id, message_json, sender_ref, text,
            date_unix, edit_date, observed_at_unix, source
     FROM cache_messages";

fn load_messages(
    connection: &Connection,
    account_user_id: i64,
    window: &MessageWindow,
    upper_exclusive: Option<i64>,
    text_only: bool,
    limit: usize,
    order: MessageOrder,
) -> MessageCacheResult<Vec<CachedMessage>> {
    load_raw_messages(
        connection,
        account_user_id,
        window,
        upper_exclusive,
        text_only,
        limit,
        order,
    )?
    .into_iter()
    .map(decode_message)
    .collect()
}

fn load_raw_messages(
    connection: &Connection,
    account_user_id: i64,
    window: &MessageWindow,
    upper_exclusive: Option<i64>,
    text_only: bool,
    limit: usize,
    order: MessageOrder,
) -> MessageCacheResult<Vec<RawMessage>> {
    let order_sql = match order {
        MessageOrder::NewestFirst => "date_unix DESC, message_id DESC, chat_id ASC",
        MessageOrder::Chronological => "date_unix ASC, message_id ASC, chat_id ASC",
    };
    let sql = format!(
        "{MESSAGE_COLUMNS}
         WHERE account_user_id = ?1
           AND (?2 IS NULL OR chat_id = ?2)
           AND (?3 IS NULL OR date_unix >= ?3)
           AND (?4 IS NULL OR date_unix <= ?4)
           AND (?5 IS NULL OR date_unix < ?5)
           AND (?6 = 0 OR text IS NOT NULL)
         ORDER BY {order_sql}
         LIMIT ?7"
    );
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement.query(params![
        account_user_id,
        window.chat_id.as_deref(),
        window.after_unix,
        window.before_unix,
        upper_exclusive,
        i64::from(text_only),
        usize_to_i64(limit)?,
    ])?;
    let mut messages = Vec::new();
    while let Some(row) = rows.next()? {
        messages.push(map_raw_message(row)?);
    }
    Ok(messages)
}

fn map_raw_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawMessage> {
    Ok(RawMessage {
        account_user_id: row.get(0)?,
        chat_id: row.get(1)?,
        message_id: row.get(2)?,
        message_json: row.get(3)?,
        sender_ref: row.get(4)?,
        text: row.get(5)?,
        date_unix: row.get(6)?,
        edit_date: row.get(7)?,
        observed_at_unix: row.get(8)?,
        source: row.get(9)?,
    })
}

fn decode_message(raw: RawMessage) -> MessageCacheResult<CachedMessage> {
    let message: MessageSummary = serde_json::from_str(&raw.message_json)?;
    if message.message_id != raw.message_id
        || message.sender_ref.as_deref() != raw.sender_ref.as_deref()
        || message.text.as_deref() != raw.text.as_deref()
        || i64::from(message.date) != raw.date_unix
        || message.edit_date.map(i64::from) != raw.edit_date
    {
        return Err(MessageCacheError::CorruptData(format!(
            "indexed fields disagree with message_json for account {}, chat {:?}, message {}",
            raw.account_user_id, raw.chat_id, raw.message_id
        )));
    }
    Ok(CachedMessage {
        account_user_id: raw.account_user_id,
        chat_id: raw.chat_id,
        message,
        observed_at_unix: raw.observed_at_unix,
        source: MessageSource::parse(raw.source)?,
    })
}

fn load_chat_stats(
    connection: &Connection,
    account_user_id: i64,
    chat_id: Option<&str>,
) -> MessageCacheResult<Vec<ChatCacheStats>> {
    let mut statement = connection.prepare(
        "WITH chats(chat_id) AS (
             SELECT chat_id FROM cache_dialogs WHERE account_user_id = ?1
             UNION SELECT chat_id FROM cache_messages WHERE account_user_id = ?1
             UNION SELECT chat_id FROM cache_sync_state WHERE account_user_id = ?1
             UNION SELECT chat_id FROM cache_tombstones WHERE account_user_id = ?1
         )
         SELECT c.chat_id,
                COUNT(m.message_id),
                (SELECT COUNT(*) FROM cache_tombstones t
                 WHERE t.account_user_id = ?1 AND t.chat_id = c.chat_id),
                MIN(m.message_id), MAX(m.message_id),
                MIN(m.date_unix), MAX(m.date_unix),
                s.frontier_message_id,
                COALESCE(s.history_complete, 0),
                s.observed_at_unix
         FROM chats c
         LEFT JOIN cache_messages m
           ON m.account_user_id = ?1 AND m.chat_id = c.chat_id
         LEFT JOIN cache_sync_state s
           ON s.account_user_id = ?1 AND s.chat_id = c.chat_id
         WHERE (?2 IS NULL OR c.chat_id = ?2)
         GROUP BY c.chat_id, s.frontier_message_id, s.history_complete, s.observed_at_unix
         ORDER BY c.chat_id ASC",
    )?;
    let mut rows = statement.query(params![account_user_id, chat_id])?;
    let mut stats = Vec::new();
    while let Some(row) = rows.next()? {
        stats.push(ChatCacheStats {
            chat_id: row.get(0)?,
            message_count: count_to_u64(row.get(1)?)?,
            tombstone_count: count_to_u64(row.get(2)?)?,
            min_message_id: row.get(3)?,
            max_message_id: row.get(4)?,
            oldest_date_unix: row.get(5)?,
            newest_date_unix: row.get(6)?,
            frontier_message_id: row.get(7)?,
            history_complete: row.get::<_, i64>(8)? != 0,
            checkpoint_observed_at_unix: row.get(9)?,
        });
    }
    Ok(stats)
}

fn initialize_or_validate_schema(connection: &mut Connection) -> MessageCacheResult<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let application_id: i64 =
        transaction.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let user_version: i64 =
        transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let tables = user_tables(&transaction)?;

    if application_id == 0 && user_version == 0 && tables.is_empty() {
        transaction.execute_batch(SCHEMA_V1)?;
        transaction.execute_batch(SCHEMA_V2)?;
        transaction.execute_batch(SCHEMA_V3)?;
        transaction.execute_batch(SCHEMA_V4)?;
        transaction.execute_batch(SCHEMA_V5)?;
        transaction.execute(
            "INSERT INTO cache_meta (singleton, database_kind, schema_version)
             VALUES (1, ?1, ?2)",
            params![MESSAGE_CACHE_KIND, MESSAGE_CACHE_SCHEMA_VERSION],
        )?;
        transaction.pragma_update(None, "application_id", MESSAGE_CACHE_APPLICATION_ID)?;
        transaction.pragma_update(None, "user_version", MESSAGE_CACHE_SCHEMA_VERSION)?;
        transaction.commit()?;
        return Ok(());
    }

    if application_id != MESSAGE_CACHE_APPLICATION_ID {
        return Err(MessageCacheError::IncompatibleDatabase(format!(
            "application_id is {application_id}, expected {MESSAGE_CACHE_APPLICATION_ID}"
        )));
    }
    if user_version > MESSAGE_CACHE_SCHEMA_VERSION {
        return Err(MessageCacheError::UnsupportedSchema {
            found: user_version,
            supported: MESSAGE_CACHE_SCHEMA_VERSION,
        });
    }
    let metadata = transaction
        .query_row(
            "SELECT database_kind, schema_version FROM cache_meta WHERE singleton = 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    let Some((database_kind, metadata_version)) = metadata else {
        return Err(MessageCacheError::CorruptData(
            "cache_meta identity row is missing".to_string(),
        ));
    };
    if database_kind != MESSAGE_CACHE_KIND {
        return Err(MessageCacheError::IncompatibleDatabase(format!(
            "database kind is {database_kind:?}"
        )));
    }
    if metadata_version > MESSAGE_CACHE_SCHEMA_VERSION {
        return Err(MessageCacheError::UnsupportedSchema {
            found: metadata_version,
            supported: MESSAGE_CACHE_SCHEMA_VERSION,
        });
    }
    if metadata_version != user_version {
        return Err(MessageCacheError::CorruptData(format!(
            "cache_meta version {metadata_version} disagrees with user_version {user_version}"
        )));
    }

    let expected_before_migration = expected_tables(user_version);
    if tables != expected_before_migration {
        let extra = tables.difference(&expected_before_migration).cloned().collect::<Vec<_>>();
        let missing = expected_before_migration.difference(&tables).cloned().collect::<Vec<_>>();
        return Err(MessageCacheError::IncompatibleDatabase(format!(
            "table set mismatch for schema v{user_version}; extra={extra:?}, missing={missing:?}"
        )));
    }

    if user_version < MESSAGE_CACHE_SCHEMA_VERSION {
        if user_version == 1 {
            transaction.execute_batch(SCHEMA_V2)?;
        }
        if user_version <= 2 {
            transaction.execute_batch(SCHEMA_V3)?;
        }
        if user_version <= 3 {
            transaction.execute_batch(SCHEMA_V4)?;
        }
        if user_version <= 4 {
            transaction.execute_batch(SCHEMA_V5)?;
        }
        transaction.execute(
            "UPDATE cache_meta SET schema_version = ?1 WHERE singleton = 1",
            [MESSAGE_CACHE_SCHEMA_VERSION],
        )?;
        transaction.pragma_update(None, "user_version", MESSAGE_CACHE_SCHEMA_VERSION)?;
    } else if user_version != MESSAGE_CACHE_SCHEMA_VERSION {
        return Err(MessageCacheError::CorruptData(format!(
            "schema version is {user_version}, expected 1..={MESSAGE_CACHE_SCHEMA_VERSION}"
        )));
    }

    let migrated_tables = user_tables(&transaction)?;
    let expected = expected_tables(MESSAGE_CACHE_SCHEMA_VERSION);
    if migrated_tables != expected {
        let extra = migrated_tables.difference(&expected).cloned().collect::<Vec<_>>();
        let missing = expected.difference(&migrated_tables).cloned().collect::<Vec<_>>();
        return Err(MessageCacheError::IncompatibleDatabase(format!(
            "table set mismatch after migration; extra={extra:?}, missing={missing:?}"
        )));
    }
    transaction.commit()?;
    Ok(())
}

fn user_tables(connection: &Connection) -> MessageCacheResult<BTreeSet<String>> {
    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_schema
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
    )?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
    Ok(rows)
}

fn expected_tables(schema_version: i64) -> BTreeSet<String> {
    let mut tables = [
        "cache_accounts",
        "cache_dialogs",
        "cache_messages",
        "cache_meta",
        "cache_sync_state",
        "cache_tombstones",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();
    if schema_version >= 2 {
        tables.insert("cache_global_tombstones".to_string());
        tables.insert("cache_update_state".to_string());
    }
    if schema_version >= 3 {
        tables.insert("cache_event_checkpoints".to_string());
        tables.insert("cache_event_stream_state".to_string());
        tables.insert("cache_incoming_events".to_string());
    }
    if schema_version >= 4 {
        tables.insert("cache_event_cursor_tombstones".to_string());
    }
    if schema_version >= 5 {
        tables.insert("cache_chat_revisions".to_string());
    }
    tables
}

fn resolve_event_cursor(
    connection: &Connection,
    account_user_id: i64,
    token: &str,
) -> MessageCacheResult<IncomingEventCursor> {
    if token.is_empty() || token.len() > 128 {
        return Err(MessageCacheError::InvalidEventCursor(token.to_string()));
    }
    let raw = connection
        .query_row(
            "SELECT baseline_event_id, consumed_through_json
             FROM cache_event_checkpoints
             WHERE account_user_id = ?1 AND cursor_token = ?2",
            params![account_user_id, token],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((baseline_event_id, consumed_json)) = raw else {
        let invalidated = connection
            .query_row(
                "SELECT 1
                 FROM cache_event_cursor_tombstones
                 WHERE account_user_id = ?1 AND cursor_token = ?2",
                params![account_user_id, token],
                |_| Ok(()),
            )
            .optional()?;
        return if invalidated.is_some() {
            Err(MessageCacheError::StaleEventCursor { cursor: token.to_string() })
        } else {
            Err(MessageCacheError::InvalidEventCursor(token.to_string()))
        };
    };
    if baseline_event_id < 0 {
        return Err(MessageCacheError::CorruptData(format!(
            "negative incoming-message cursor baseline {baseline_event_id}"
        )));
    }
    let consumed_through_by_chat: BTreeMap<String, i64> = serde_json::from_str(&consumed_json)?;
    if consumed_through_by_chat
        .iter()
        .any(|(chat_id, through)| chat_id.is_empty() || *through <= baseline_event_id)
    {
        return Err(MessageCacheError::CorruptData(format!(
            "incoming-message cursor {token:?} contains an invalid per-chat frontier"
        )));
    }
    Ok(IncomingEventCursor {
        token: token.to_string(),
        baseline_event_id,
        consumed_through_by_chat,
    })
}

fn persist_event_cursor(
    connection: &Connection,
    account_user_id: i64,
    baseline_event_id: i64,
    consumed_through_by_chat: &BTreeMap<String, i64>,
    now_unix_ms: i64,
) -> MessageCacheResult<IncomingEventCursor> {
    if baseline_event_id < 0 || now_unix_ms < 0 {
        return Err(MessageCacheError::CorruptData(
            "incoming-message cursor timestamps and baselines must be non-negative".to_string(),
        ));
    }
    if consumed_through_by_chat
        .iter()
        .any(|(chat_id, through)| chat_id.is_empty() || *through <= baseline_event_id)
    {
        return Err(MessageCacheError::CorruptData(
            "incoming-message cursor contains an invalid per-chat frontier".to_string(),
        ));
    }
    let consumed_json = serde_json::to_string(consumed_through_by_chat)?;
    let existing = connection
        .query_row(
            "SELECT cursor_token
             FROM cache_event_checkpoints
             WHERE account_user_id = ?1
               AND baseline_event_id = ?2
               AND consumed_through_json = ?3",
            params![account_user_id, baseline_event_id, consumed_json],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let token = match existing {
        Some(token) => token,
        None => {
            let token = Uuid::new_v4().to_string();
            connection.execute(
                "INSERT INTO cache_event_checkpoints (
                     cursor_token, account_user_id, baseline_event_id,
                     consumed_through_json, created_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![token, account_user_id, baseline_event_id, consumed_json, now_unix_ms,],
            )?;
            token
        },
    };
    prune_event_checkpoints(connection, account_user_id, &token, now_unix_ms)?;
    Ok(IncomingEventCursor {
        token,
        baseline_event_id,
        consumed_through_by_chat: consumed_through_by_chat.clone(),
    })
}

fn ensure_event_cursor_retained(
    connection: &Connection,
    account_user_id: i64,
    cursor: &IncomingEventCursor,
) -> MessageCacheResult<()> {
    let pruned_through = connection
        .query_row(
            "SELECT pruned_through_event_id
             FROM cache_event_stream_state
             WHERE account_user_id = ?1",
            [account_user_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    if cursor.baseline_event_id < pruned_through {
        Err(MessageCacheError::StaleEventCursor { cursor: cursor.token.clone() })
    } else {
        Ok(())
    }
}

fn ensure_event_stream_available(
    connection: &Connection,
    account_user_id: i64,
) -> MessageCacheResult<()> {
    let phase = connection
        .query_row(
            "SELECT phase
             FROM cache_update_state
             WHERE account_user_id = ?1",
            [account_user_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if phase.as_deref() == Some(CacheSyncPhase::ResyncRequired.as_str()) {
        Err(MessageCacheError::EventStreamResyncRequired)
    } else {
        Ok(())
    }
}

fn compact_and_persist_event_cursor(
    connection: &Connection,
    account_user_id: i64,
    cursor: &IncomingEventCursor,
    mut consumed: BTreeMap<String, i64>,
    now_unix_ms: i64,
) -> MessageCacheResult<IncomingEventCursor> {
    let all_events =
        load_incoming_events_after(connection, account_user_id, cursor.baseline_event_id)?;
    let first_unconsumed = all_events.iter().find(|event| {
        event.event_id > consumed.get(&event.chat_id).copied().unwrap_or(cursor.baseline_event_id)
    });
    let baseline_event_id = match first_unconsumed {
        Some(event) => event.event_id.saturating_sub(1),
        None => latest_incoming_event_id(connection, account_user_id)?,
    };
    consumed.retain(|_, through| *through > baseline_event_id);
    persist_event_cursor(connection, account_user_id, baseline_event_id, &consumed, now_unix_ms)
}

fn latest_incoming_event_id(
    connection: &Connection,
    account_user_id: i64,
) -> MessageCacheResult<i64> {
    connection
        .query_row(
            "SELECT MAX(event_id)
             FROM cache_incoming_events
             WHERE account_user_id = ?1",
            [account_user_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map(|value| value.unwrap_or(0))
        .map_err(Into::into)
}

fn load_incoming_events_after(
    connection: &Connection,
    account_user_id: i64,
    after_event_id: i64,
) -> MessageCacheResult<Vec<IncomingMessageEvent>> {
    let mut statement = connection.prepare(
        "SELECT event_id, chat_id, sender_user_id, sender_name, sender_username,
                message_id, message_date_unix, observed_at_unix_ms
         FROM cache_incoming_events
         WHERE account_user_id = ?1 AND event_id > ?2
         ORDER BY event_id ASC",
    )?;
    let events = statement
        .query_map(params![account_user_id, after_event_id], |row| {
            Ok(IncomingMessageEvent {
                event_id: row.get(0)?,
                chat_id: row.get(1)?,
                sender_user_id: row.get(2)?,
                sender_name: row.get(3)?,
                sender_username: row.get(4)?,
                message_id: row.get(5)?,
                message_date_unix: row.get(6)?,
                observed_at_unix_ms: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(events)
}

fn load_unconsumed_incoming_events(
    connection: &Connection,
    account_user_id: i64,
    cursor: &IncomingEventCursor,
) -> MessageCacheResult<Vec<IncomingMessageEvent>> {
    let events = load_incoming_events_after(connection, account_user_id, cursor.baseline_event_id)?;
    Ok(events
        .into_iter()
        .filter(|event| {
            event.event_id
                > cursor
                    .consumed_through_by_chat
                    .get(&event.chat_id)
                    .copied()
                    .unwrap_or(cursor.baseline_event_id)
        })
        .collect())
}

fn require_incoming_event(
    connection: &Connection,
    account_user_id: i64,
    chat_id: Option<&str>,
    event_id: i64,
) -> MessageCacheResult<()> {
    let exists = connection
        .query_row(
            "SELECT 1
             FROM cache_incoming_events
             WHERE account_user_id = ?1
               AND event_id = ?2
               AND (?3 IS NULL OR chat_id = ?3)",
            params![account_user_id, event_id, chat_id],
            |_| Ok(()),
        )
        .optional()?;
    exists.ok_or_else(|| MessageCacheError::InvalidEventCursor(format!("event:{event_id}")))
}

fn prune_incoming_events(connection: &Connection, account_user_id: i64) -> MessageCacheResult<()> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM cache_incoming_events WHERE account_user_id = ?1",
        [account_user_id],
        |row| row.get(0),
    )?;
    let maximum = usize_to_i64(MAX_INCOMING_EVENT_ROWS_PER_ACCOUNT)?;
    if count <= maximum {
        return Ok(());
    }
    let cutoff_event_id: i64 = connection.query_row(
        "SELECT event_id
         FROM cache_incoming_events
         WHERE account_user_id = ?1
         ORDER BY event_id DESC
         LIMIT 1 OFFSET ?2",
        params![account_user_id, maximum.saturating_sub(1)],
        |row| row.get(0),
    )?;
    let deleted_through = connection
        .query_row(
            "SELECT MAX(event_id)
             FROM cache_incoming_events
             WHERE account_user_id = ?1 AND event_id < ?2",
            params![account_user_id, cutoff_event_id],
            |row| row.get::<_, Option<i64>>(0),
        )?
        .unwrap_or(0);
    connection.execute(
        "DELETE FROM cache_incoming_events
         WHERE account_user_id = ?1 AND event_id < ?2",
        params![account_user_id, cutoff_event_id],
    )?;
    connection.execute(
        "INSERT INTO cache_event_stream_state (
             account_user_id, pruned_through_event_id
         ) VALUES (?1, ?2)
         ON CONFLICT(account_user_id) DO UPDATE SET
             pruned_through_event_id = MAX(
                 cache_event_stream_state.pruned_through_event_id,
                 excluded.pruned_through_event_id
             )",
        params![account_user_id, deleted_through],
    )?;
    Ok(())
}

fn prune_event_checkpoints(
    connection: &Connection,
    account_user_id: i64,
    protected_token: &str,
    now_unix_ms: i64,
) -> MessageCacheResult<()> {
    prune_event_checkpoints_with_limit(
        connection,
        account_user_id,
        protected_token,
        now_unix_ms,
        MAX_EVENT_CHECKPOINT_ROWS_PER_ACCOUNT,
    )
}

fn prune_event_checkpoints_with_limit(
    connection: &Connection,
    account_user_id: i64,
    protected_token: &str,
    now_unix_ms: i64,
    maximum: usize,
) -> MessageCacheResult<()> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM cache_event_checkpoints WHERE account_user_id = ?1",
        [account_user_id],
        |row| row.get(0),
    )?;
    let maximum = usize_to_i64(maximum)?;
    if count <= maximum {
        return Ok(());
    }
    let mut statement = connection.prepare(
        "SELECT cursor_token
         FROM cache_event_checkpoints
         WHERE account_user_id = ?1 AND cursor_token <> ?2
         ORDER BY created_at_unix_ms ASC, cursor_token ASC
         LIMIT ?3",
    )?;
    let tokens = statement
        .query_map(
            params![account_user_id, protected_token, count.saturating_sub(maximum)],
            |row| row.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    for token in tokens {
        tombstone_event_cursor(
            connection,
            account_user_id,
            &token,
            "checkpoint_retention",
            now_unix_ms,
        )?;
        connection.execute(
            "DELETE FROM cache_event_checkpoints
             WHERE account_user_id = ?1 AND cursor_token = ?2",
            params![account_user_id, token],
        )?;
    }
    prune_event_cursor_tombstones(connection, account_user_id)?;
    Ok(())
}

fn tombstone_event_cursor(
    connection: &Connection,
    account_user_id: i64,
    cursor_token: &str,
    reason: &str,
    invalidated_at_unix_ms: i64,
) -> MessageCacheResult<()> {
    let invalidated_at_unix_ms = next_event_cursor_invalidation_timestamp(
        connection,
        account_user_id,
        invalidated_at_unix_ms,
    )?;
    connection.execute(
        "INSERT INTO cache_event_cursor_tombstones (
             cursor_token, account_user_id, reason, invalidated_at_unix_ms
         ) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(cursor_token) DO UPDATE SET
             account_user_id = excluded.account_user_id,
             reason = excluded.reason,
             invalidated_at_unix_ms = excluded.invalidated_at_unix_ms",
        params![cursor_token, account_user_id, reason, invalidated_at_unix_ms],
    )?;
    Ok(())
}

fn tombstone_event_cursors(
    connection: &Connection,
    account_user_id: i64,
    reason: &str,
    invalidated_at_unix_ms: i64,
) -> MessageCacheResult<()> {
    let invalidated_at_unix_ms = next_event_cursor_invalidation_timestamp(
        connection,
        account_user_id,
        invalidated_at_unix_ms,
    )?;
    connection.execute(
        "INSERT INTO cache_event_cursor_tombstones (
             cursor_token, account_user_id, reason, invalidated_at_unix_ms
         )
         SELECT cursor_token, account_user_id, ?2, ?3
         FROM cache_event_checkpoints
         WHERE account_user_id = ?1
         ON CONFLICT(cursor_token) DO UPDATE SET
             account_user_id = excluded.account_user_id,
             reason = excluded.reason,
             invalidated_at_unix_ms = excluded.invalidated_at_unix_ms",
        params![account_user_id, reason, invalidated_at_unix_ms],
    )?;
    Ok(())
}

fn next_event_cursor_invalidation_timestamp(
    connection: &Connection,
    account_user_id: i64,
    observed_at_unix_ms: i64,
) -> MessageCacheResult<i64> {
    if observed_at_unix_ms < 0 {
        return Err(MessageCacheError::CorruptData(
            "event cursor invalidation timestamp cannot be negative".to_string(),
        ));
    }
    let newest: Option<i64> = connection.query_row(
        "SELECT MAX(invalidated_at_unix_ms)
         FROM cache_event_cursor_tombstones
         WHERE account_user_id = ?1",
        [account_user_id],
        |row| row.get(0),
    )?;
    match newest {
        Some(newest) if newest >= observed_at_unix_ms => newest.checked_add(1).ok_or_else(|| {
            MessageCacheError::CorruptData(
                "event cursor invalidation timestamp overflow".to_string(),
            )
        }),
        _ => Ok(observed_at_unix_ms),
    }
}

fn prune_event_cursor_tombstones(
    connection: &Connection,
    account_user_id: i64,
) -> MessageCacheResult<()> {
    prune_event_cursor_tombstones_with_limit(
        connection,
        account_user_id,
        MAX_EVENT_CURSOR_TOMBSTONES_PER_ACCOUNT,
    )
}

fn prune_event_cursor_tombstones_with_limit(
    connection: &Connection,
    account_user_id: i64,
    maximum: usize,
) -> MessageCacheResult<()> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*)
         FROM cache_event_cursor_tombstones
         WHERE account_user_id = ?1",
        [account_user_id],
        |row| row.get(0),
    )?;
    let maximum = usize_to_i64(maximum)?;
    if count > maximum {
        connection.execute(
            "DELETE FROM cache_event_cursor_tombstones
             WHERE cursor_token IN (
                 SELECT cursor_token
                 FROM cache_event_cursor_tombstones
                 WHERE account_user_id = ?1
                 ORDER BY invalidated_at_unix_ms ASC, cursor_token ASC
                 LIMIT ?2
             )",
            params![account_user_id, count.saturating_sub(maximum)],
        )?;
    }
    Ok(())
}

fn invalidate_incoming_event_stream(
    connection: &Connection,
    account_user_id: i64,
    reason: &str,
    invalidated_at_unix_ms: i64,
) -> MessageCacheResult<()> {
    tombstone_event_cursors(connection, account_user_id, reason, invalidated_at_unix_ms)?;
    connection.execute(
        "DELETE FROM cache_event_checkpoints WHERE account_user_id = ?1",
        [account_user_id],
    )?;
    connection.execute(
        "DELETE FROM cache_incoming_events WHERE account_user_id = ?1",
        [account_user_id],
    )?;
    connection.execute(
        "DELETE FROM cache_event_stream_state WHERE account_user_id = ?1",
        [account_user_id],
    )?;
    prune_event_cursor_tombstones(connection, account_user_id)
}

fn upsert_global_update_cursor(
    connection: &Connection,
    account_user_id: i64,
    cursor: &GlobalUpdateCursor,
) -> MessageCacheResult<()> {
    connection.execute(
        "INSERT INTO cache_update_state (
             account_user_id, pts, qts, date, seq, unread_count, phase,
             observed_at_unix, last_error_code
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(account_user_id) DO UPDATE SET
             pts = excluded.pts,
             qts = excluded.qts,
             date = excluded.date,
             seq = excluded.seq,
             unread_count = excluded.unread_count,
             phase = excluded.phase,
             observed_at_unix = excluded.observed_at_unix,
             last_error_code = excluded.last_error_code",
        params![
            account_user_id,
            cursor.pts,
            cursor.qts,
            cursor.date,
            cursor.seq,
            cursor.unread_count,
            cursor.phase.as_str(),
            cursor.observed_at_unix,
            cursor.last_error_code,
        ],
    )?;
    Ok(())
}

fn load_global_update_cursor(
    connection: &Connection,
    account_user_id: i64,
) -> MessageCacheResult<Option<GlobalUpdateCursor>> {
    connection
        .query_row(
            "SELECT pts, qts, date, seq, unread_count, phase,
                    observed_at_unix, last_error_code
             FROM cache_update_state
             WHERE account_user_id = ?1",
            [account_user_id],
            |row| {
                Ok((
                    row.get::<_, i32>(0)?,
                    row.get::<_, i32>(1)?,
                    row.get::<_, i32>(2)?,
                    row.get::<_, i32>(3)?,
                    row.get::<_, Option<i32>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )
        .optional()?
        .map(|raw| {
            Ok(GlobalUpdateCursor {
                pts: raw.0,
                qts: raw.1,
                date: raw.2,
                seq: raw.3,
                unread_count: raw.4,
                phase: CacheSyncPhase::parse(raw.5)?,
                observed_at_unix: raw.6,
                last_error_code: raw.7,
            })
        })
        .transpose()
}

fn max_message_id_in_connection(
    connection: &Connection,
    account_user_id: i64,
    chat_id: &str,
) -> MessageCacheResult<Option<i32>> {
    connection
        .query_row(
            "SELECT MAX(message_id)
             FROM cache_messages
             WHERE account_user_id = ?1 AND chat_id = ?2",
            params![account_user_id, chat_id],
            |row| row.get::<_, Option<i32>>(0),
        )
        .map_err(Into::into)
}

fn chat_revision_in_connection(
    connection: &Connection,
    account_user_id: i64,
    chat_id: &str,
) -> MessageCacheResult<u64> {
    let revision = connection
        .query_row(
            "SELECT revision
             FROM cache_chat_revisions
             WHERE account_user_id = ?1 AND chat_id = ?2",
            params![account_user_id, chat_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    generation_to_u64(revision)
}

fn bump_chat_revisions(
    connection: &Connection,
    account_user_id: i64,
    chat_ids: &BTreeSet<String>,
) -> MessageCacheResult<()> {
    for chat_id in chat_ids {
        let current = chat_revision_in_connection(connection, account_user_id, chat_id)?;
        let successor = current.checked_add(1).ok_or_else(|| {
            MessageCacheError::CorruptData(format!(
                "chat revision overflow for account {account_user_id}, chat {chat_id:?}"
            ))
        })?;
        connection.execute(
            "INSERT INTO cache_chat_revisions (account_user_id, chat_id, revision)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(account_user_id, chat_id) DO UPDATE SET
                 revision = excluded.revision",
            params![account_user_id, chat_id, u64_to_i64(successor)?],
        )?;
    }
    Ok(())
}

fn dialog_checkpoint_in_connection(
    connection: &Connection,
    account_user_id: i64,
    chat_id: &str,
) -> MessageCacheResult<Option<DialogCheckpoint>> {
    connection
        .query_row(
            "SELECT chat_id, frontier_message_id, oldest_message_id,
                    history_complete, observed_at_unix
             FROM cache_sync_state
             WHERE account_user_id = ?1 AND chat_id = ?2",
            params![account_user_id, chat_id],
            |row| {
                Ok(DialogCheckpoint {
                    chat_id: row.get(0)?,
                    frontier_message_id: row.get(1)?,
                    oldest_message_id: row.get(2)?,
                    history_complete: row.get::<_, i64>(3)? != 0,
                    observed_at_unix: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn optional_generation(
    connection: &Connection,
    account_user_id: i64,
) -> MessageCacheResult<Option<u64>> {
    let generation = connection
        .query_row(
            "SELECT generation FROM cache_accounts WHERE account_user_id = ?1",
            [account_user_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    generation.map(generation_to_u64).transpose()
}

fn required_generation(connection: &Connection, account_user_id: i64) -> MessageCacheResult<u64> {
    optional_generation(connection, account_user_id)?
        .ok_or(MessageCacheError::AccountNotBound { account_user_id })
}

fn require_expected_generation(
    connection: &Connection,
    account_user_id: i64,
    expected: u64,
) -> MessageCacheResult<()> {
    let current = required_generation(connection, account_user_id)?;
    if current == expected {
        Ok(())
    } else {
        Err(MessageCacheError::StaleGeneration { expected, current })
    }
}

fn require_account(connection: &Connection, account_user_id: i64) -> MessageCacheResult<()> {
    required_generation(connection, account_user_id).map(|_| ())
}

fn validate_retention_days(max_age_days: u32) -> MessageCacheResult<()> {
    if max_age_days == 0 || max_age_days > MAX_MESSAGE_CACHE_AGE_DAYS {
        Err(MessageCacheError::InvalidRetentionDays {
            requested: max_age_days,
            maximum: MAX_MESSAGE_CACHE_AGE_DAYS,
        })
    } else {
        Ok(())
    }
}

fn prune_messages_before_in_transaction(
    connection: &Connection,
    account_user_id: i64,
    cutoff_unix: i64,
    observed_at_unix: i64,
) -> MessageCacheResult<MessageRetentionResult> {
    let affected_chats = {
        let mut statement = connection.prepare(
            "SELECT DISTINCT chat_id
             FROM cache_messages
             WHERE account_user_id = ?1 AND date_unix < ?2
             ORDER BY chat_id ASC",
        )?;
        statement
            .query_map(params![account_user_id, cutoff_unix], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let removed_messages = connection.execute(
        "DELETE FROM cache_messages
         WHERE account_user_id = ?1 AND date_unix < ?2",
        params![account_user_id, cutoff_unix],
    )?;
    for chat_id in &affected_chats {
        connection.execute(
            "UPDATE cache_sync_state
             SET oldest_message_id = (
                     SELECT MIN(message_id)
                     FROM cache_messages
                     WHERE account_user_id = ?1 AND chat_id = ?2
                 ),
                 history_complete = 0,
                 observed_at_unix = CASE
                     WHEN observed_at_unix < ?3 THEN ?3
                     ELSE observed_at_unix
                 END
             WHERE account_user_id = ?1 AND chat_id = ?2",
            params![account_user_id, chat_id, observed_at_unix],
        )?;
    }
    Ok(MessageRetentionResult {
        account_user_id,
        cutoff_unix,
        removed_messages: usize_count_to_u64(removed_messages)?,
        affected_chats,
    })
}

fn validate_window(window: &MessageWindow) -> MessageCacheResult<()> {
    if let (Some(after_unix), Some(before_unix)) = (window.after_unix, window.before_unix)
        && after_unix > before_unix
    {
        return Err(MessageCacheError::InvalidTimeRange { after_unix, before_unix });
    }
    Ok(())
}

fn validate_limit(requested: usize, maximum: usize) -> MessageCacheResult<()> {
    if requested == 0 || requested > maximum {
        Err(MessageCacheError::InvalidLimit { requested, maximum })
    } else {
        Ok(())
    }
}

fn generation_to_u64(value: i64) -> MessageCacheResult<u64> {
    u64::try_from(value)
        .map_err(|_| MessageCacheError::CorruptData(format!("negative cache generation {value}")))
}

fn count_to_u64(value: i64) -> MessageCacheResult<u64> {
    u64::try_from(value)
        .map_err(|_| MessageCacheError::CorruptData(format!("negative row count {value}")))
}

fn usize_count_to_u64(value: usize) -> MessageCacheResult<u64> {
    u64::try_from(value)
        .map_err(|_| MessageCacheError::CorruptData("row count does not fit u64".to_string()))
}

fn usize_to_i64(value: usize) -> MessageCacheResult<i64> {
    i64::try_from(value)
        .map_err(|_| MessageCacheError::CorruptData("row limit does not fit i64".to_string()))
}

fn u64_to_i64(value: u64) -> MessageCacheResult<i64> {
    i64::try_from(value)
        .map_err(|_| MessageCacheError::CorruptData("generation does not fit i64".to_string()))
}

fn push_csv_field(output: &mut String, value: &str) {
    let needs_quotes = value.bytes().any(|byte| matches!(byte, b',' | b'"' | b'\n' | b'\r'));
    if !needs_quotes {
        output.push_str(value);
        return;
    }
    output.push('"');
    for character in value.chars() {
        if character == '"' {
            output.push('"');
        }
        output.push(character);
    }
    output.push('"');
}

const SCHEMA_V1: &str = r#"
CREATE TABLE cache_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    database_kind TEXT NOT NULL,
    schema_version INTEGER NOT NULL CHECK (schema_version >= 1)
) WITHOUT ROWID;

CREATE TABLE cache_accounts (
    account_user_id INTEGER PRIMARY KEY,
    display_name TEXT,
    username TEXT,
    observed_at_unix INTEGER NOT NULL,
    generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0)
);

CREATE TABLE cache_dialogs (
    account_user_id INTEGER NOT NULL REFERENCES cache_accounts(account_user_id) ON DELETE CASCADE,
    chat_id TEXT NOT NULL,
    chat_ref TEXT,
    chat_kind TEXT,
    title TEXT,
    username TEXT,
    observed_at_unix INTEGER NOT NULL,
    PRIMARY KEY (account_user_id, chat_id)
) WITHOUT ROWID;

CREATE TABLE cache_messages (
    account_user_id INTEGER NOT NULL REFERENCES cache_accounts(account_user_id) ON DELETE CASCADE,
    chat_id TEXT NOT NULL,
    message_id INTEGER NOT NULL,
    message_json TEXT NOT NULL,
    sender_ref TEXT,
    text TEXT,
    date_unix INTEGER NOT NULL,
    edit_date INTEGER,
    observed_at_unix INTEGER NOT NULL,
    source TEXT NOT NULL CHECK (source IN (
        'dialog_history', 'search', 'live_update', 'exact_lookup', 'import', 'reconciliation'
    )),
    PRIMARY KEY (account_user_id, chat_id, message_id)
) WITHOUT ROWID;

CREATE INDEX cache_messages_account_date
ON cache_messages(account_user_id, date_unix DESC, message_id DESC, chat_id ASC);

CREATE INDEX cache_messages_chat_date
ON cache_messages(account_user_id, chat_id, date_unix DESC, message_id DESC);

CREATE INDEX cache_messages_sender
ON cache_messages(account_user_id, sender_ref, date_unix DESC);

CREATE TABLE cache_sync_state (
    account_user_id INTEGER NOT NULL REFERENCES cache_accounts(account_user_id) ON DELETE CASCADE,
    chat_id TEXT NOT NULL,
    frontier_message_id INTEGER,
    oldest_message_id INTEGER,
    history_complete INTEGER NOT NULL CHECK (history_complete IN (0, 1)),
    observed_at_unix INTEGER NOT NULL,
    PRIMARY KEY (account_user_id, chat_id)
) WITHOUT ROWID;

CREATE TABLE cache_tombstones (
    account_user_id INTEGER NOT NULL REFERENCES cache_accounts(account_user_id) ON DELETE CASCADE,
    chat_id TEXT NOT NULL,
    message_id INTEGER NOT NULL,
    deleted_at_unix INTEGER NOT NULL,
    source TEXT NOT NULL CHECK (source IN (
        'dialog_history', 'search', 'live_update', 'exact_lookup', 'import', 'reconciliation'
    )),
    PRIMARY KEY (account_user_id, chat_id, message_id)
) WITHOUT ROWID;
"#;

const SCHEMA_V2: &str = r#"
CREATE TABLE cache_update_state (
    account_user_id INTEGER PRIMARY KEY REFERENCES cache_accounts(account_user_id) ON DELETE CASCADE,
    pts INTEGER NOT NULL,
    qts INTEGER NOT NULL,
    date INTEGER NOT NULL,
    seq INTEGER NOT NULL,
    unread_count INTEGER,
    phase TEXT NOT NULL CHECK (phase IN (
        'uninitialized', 'catching_up', 'current', 'resync_required', 'degraded'
    )),
    observed_at_unix INTEGER NOT NULL,
    last_error_code TEXT
) WITHOUT ROWID;

CREATE TABLE cache_global_tombstones (
    account_user_id INTEGER NOT NULL REFERENCES cache_accounts(account_user_id) ON DELETE CASCADE,
    message_id INTEGER NOT NULL,
    deleted_at_unix INTEGER NOT NULL,
    PRIMARY KEY (account_user_id, message_id)
) WITHOUT ROWID;
"#;

const SCHEMA_V3: &str = r#"
CREATE TABLE cache_incoming_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_user_id INTEGER NOT NULL REFERENCES cache_accounts(account_user_id) ON DELETE CASCADE,
    chat_id TEXT NOT NULL,
    sender_user_id INTEGER NOT NULL,
    sender_name TEXT NOT NULL,
    sender_username TEXT,
    message_id INTEGER NOT NULL,
    message_date_unix INTEGER NOT NULL,
    observed_at_unix_ms INTEGER NOT NULL CHECK (observed_at_unix_ms >= 0),
    UNIQUE (account_user_id, chat_id, message_id)
);

CREATE INDEX cache_incoming_events_account_event
ON cache_incoming_events(account_user_id, event_id);

CREATE INDEX cache_incoming_events_account_chat_event
ON cache_incoming_events(account_user_id, chat_id, event_id);

CREATE TABLE cache_event_checkpoints (
    cursor_token TEXT PRIMARY KEY,
    account_user_id INTEGER NOT NULL REFERENCES cache_accounts(account_user_id) ON DELETE CASCADE,
    baseline_event_id INTEGER NOT NULL CHECK (baseline_event_id >= 0),
    consumed_through_json TEXT NOT NULL,
    created_at_unix_ms INTEGER NOT NULL CHECK (created_at_unix_ms >= 0),
    UNIQUE (account_user_id, baseline_event_id, consumed_through_json)
) WITHOUT ROWID;

CREATE INDEX cache_event_checkpoints_account_created
ON cache_event_checkpoints(account_user_id, created_at_unix_ms, cursor_token);

CREATE TABLE cache_event_stream_state (
    account_user_id INTEGER PRIMARY KEY REFERENCES cache_accounts(account_user_id) ON DELETE CASCADE,
    pruned_through_event_id INTEGER NOT NULL DEFAULT 0 CHECK (pruned_through_event_id >= 0)
) WITHOUT ROWID;
"#;

const SCHEMA_V4: &str = r#"
CREATE TABLE cache_event_cursor_tombstones (
    cursor_token TEXT PRIMARY KEY,
    account_user_id INTEGER NOT NULL REFERENCES cache_accounts(account_user_id) ON DELETE CASCADE,
    reason TEXT NOT NULL CHECK (reason IN (
        'telegram_resync', 'cache_clear', 'checkpoint_retention'
    )),
    invalidated_at_unix_ms INTEGER NOT NULL CHECK (invalidated_at_unix_ms >= 0)
) WITHOUT ROWID;

CREATE INDEX cache_event_cursor_tombstones_account_invalidated
ON cache_event_cursor_tombstones(account_user_id, invalidated_at_unix_ms, cursor_token);
"#;

const SCHEMA_V5: &str = r#"
CREATE TABLE cache_chat_revisions (
    account_user_id INTEGER NOT NULL REFERENCES cache_accounts(account_user_id) ON DELETE CASCADE,
    chat_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    PRIMARY KEY (account_user_id, chat_id)
) WITHOUT ROWID;
"#;

