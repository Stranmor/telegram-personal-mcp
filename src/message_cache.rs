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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn cache() -> (tempfile::TempDir, MessageCache) {
        let temp = tempfile::tempdir().expect("tempdir");
        let cache = MessageCache::open(temp.path().join("state").join("messages.sqlite"))
            .expect("open cache");
        (temp, cache)
    }

    fn bind(cache: &MessageCache, account_user_id: i64) {
        cache
            .bind_account(&AccountMetadata {
                account_user_id,
                display_name: Some(format!("Account {account_user_id}")),
                username: Some(format!("user{account_user_id}")),
                observed_at_unix: 10,
            })
            .expect("bind account");
    }

    fn summary(message_id: i32, text: &str, date: i32, sender: Option<&str>) -> MessageSummary {
        MessageSummary {
            message_id,
            chat_ref: None,
            sender_ref: sender.map(str::to_owned),
            outgoing: false,
            pinned: false,
            silent: false,
            noforwards: false,
            text: Some(text.to_string()),
            date,
            edit_date: None,
            reply_to_message_id: None,
            views: None,
            forwards: None,
            reply_count: None,
            media_type: None,
            forward_origin: None,
        }
    }

    fn cached(chat_id: &str, message: MessageSummary) -> CachedMessageInput {
        CachedMessageInput {
            chat_id: chat_id.to_string(),
            observed_at_unix: i64::from(message.date) + 100,
            source: MessageSource::DialogHistory,
            message,
        }
    }

    #[test]
    fn creates_current_schema_and_persists_across_restart() {
        let (temp, cache) = cache();
        bind(&cache, 7);
        cache
            .upsert_message(7, &cached("chat-a", summary(1, "persisted", 100, Some("alice"))))
            .expect("upsert");
        let path = cache.path().to_path_buf();
        drop(cache);

        let reopened = MessageCache::open(path).expect("reopen");
        let message = reopened.exact_get(7, "chat-a", 1).expect("get").expect("persisted message");
        assert_eq!(message.message.text.as_deref(), Some("persisted"));
        assert_eq!(reopened.account_generation(7).expect("generation"), Some(0));
        drop(temp);
    }

    #[test]
    fn future_schema_fails_closed() {
        let (_temp, cache) = cache();
        let path = cache.path().to_path_buf();
        drop(cache);
        let connection = Connection::open(&path).expect("raw open");
        connection
            .pragma_update(None, "user_version", MESSAGE_CACHE_SCHEMA_VERSION + 1)
            .expect("set future version");
        drop(connection);

        assert!(matches!(
            MessageCache::open(path),
            Err(MessageCacheError::UnsupportedSchema { found: 6, supported: 5 })
        ));
    }

    #[test]
    fn migrates_v1_forward_without_losing_messages() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("messages.sqlite");
        let message = summary(9, "kept through migration", 123, Some("alice"));
        let connection = Connection::open(&path).expect("raw open");
        connection.execute_batch(SCHEMA_V1).expect("create v1 schema");
        connection
            .execute(
                "INSERT INTO cache_meta (singleton, database_kind, schema_version)
                 VALUES (1, ?1, 1)",
                [MESSAGE_CACHE_KIND],
            )
            .expect("v1 metadata");
        connection
            .execute(
                "INSERT INTO cache_accounts (
                     account_user_id, display_name, username, observed_at_unix, generation
                 ) VALUES (7, 'Account 7', 'user7', 10, 0)",
                [],
            )
            .expect("v1 account");
        connection
            .execute(
                "INSERT INTO cache_messages (
                     account_user_id, chat_id, message_id, message_json, sender_ref,
                     text, date_unix, edit_date, observed_at_unix, source
                 ) VALUES (7, 'chat-a', 9, ?1, 'alice', 'kept through migration',
                           123, NULL, 223, 'dialog_history')",
                [serde_json::to_string(&message).expect("serialize message")],
            )
            .expect("v1 message");
        connection
            .pragma_update(None, "application_id", MESSAGE_CACHE_APPLICATION_ID)
            .expect("application id");
        connection.pragma_update(None, "user_version", 1_i64).expect("v1 version");
        drop(connection);

        let migrated = MessageCache::open(&path).expect("migrate");
        assert_eq!(
            migrated
                .exact_get(7, "chat-a", 9)
                .expect("get")
                .expect("preserved")
                .message
                .text
                .as_deref(),
            Some("kept through migration")
        );
        assert!(migrated.global_update_cursor(7).expect("cursor").is_none());
        let connection = Connection::open(&path).expect("inspect migrated db");
        let version: i64 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0)).expect("version");
        assert_eq!(version, MESSAGE_CACHE_SCHEMA_VERSION);
        assert_eq!(user_tables(&connection).expect("tables"), expected_tables(5));
    }

    #[test]
    fn migrates_v2_forward_and_accepts_durable_event_cursors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("messages.sqlite");
        let connection = Connection::open(&path).expect("raw open");
        connection.execute_batch(SCHEMA_V1).expect("create v1 schema");
        connection.execute_batch(SCHEMA_V2).expect("create v2 schema");
        connection
            .execute(
                "INSERT INTO cache_meta (singleton, database_kind, schema_version)
                 VALUES (1, ?1, 2)",
                [MESSAGE_CACHE_KIND],
            )
            .expect("v2 metadata");
        connection
            .execute(
                "INSERT INTO cache_accounts (
                     account_user_id, display_name, username, observed_at_unix, generation
                 ) VALUES (7, 'Account 7', 'user7', 10, 0)",
                [],
            )
            .expect("v2 account");
        connection
            .pragma_update(None, "application_id", MESSAGE_CACHE_APPLICATION_ID)
            .expect("application id");
        connection.pragma_update(None, "user_version", 2_i64).expect("v2 version");
        drop(connection);

        let migrated = MessageCache::open(&path).expect("migrate");
        let cursor = migrated.incoming_event_window(7, None, 100).expect("new cursor");
        assert!(cursor.events.is_empty());
        assert!(!cursor.cursor.token.is_empty());
        let connection = Connection::open(&path).expect("inspect migrated db");
        let version: i64 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0)).expect("version");
        assert_eq!(version, MESSAGE_CACHE_SCHEMA_VERSION);
        assert_eq!(user_tables(&connection).expect("tables"), expected_tables(5));
    }

    #[test]
    fn migrates_v3_forward_and_preserves_existing_event_checkpoints() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("messages.sqlite");
        let connection = Connection::open(&path).expect("raw open");
        connection.execute_batch(SCHEMA_V1).expect("create v1 schema");
        connection.execute_batch(SCHEMA_V2).expect("create v2 schema");
        connection.execute_batch(SCHEMA_V3).expect("create v3 schema");
        connection
            .execute(
                "INSERT INTO cache_meta (singleton, database_kind, schema_version)
                 VALUES (1, ?1, 3)",
                [MESSAGE_CACHE_KIND],
            )
            .expect("v3 metadata");
        connection
            .execute(
                "INSERT INTO cache_accounts (
                     account_user_id, display_name, username, observed_at_unix, generation
                 ) VALUES (7, 'Account 7', 'user7', 10, 0)",
                [],
            )
            .expect("v3 account");
        connection
            .execute(
                "INSERT INTO cache_event_checkpoints (
                     cursor_token, account_user_id, baseline_event_id,
                     consumed_through_json, created_at_unix_ms
                 ) VALUES ('legacy-token', 7, 0, '{}', 100)",
                [],
            )
            .expect("v3 checkpoint");
        connection
            .pragma_update(None, "application_id", MESSAGE_CACHE_APPLICATION_ID)
            .expect("application id");
        connection.pragma_update(None, "user_version", 3_i64).expect("v3 version");
        drop(connection);

        let migrated = MessageCache::open(&path).expect("migrate");
        assert!(
            migrated
                .incoming_event_window(7, Some("legacy-token"), 200)
                .expect("legacy checkpoint remains usable")
                .events
                .is_empty()
        );
        let connection = Connection::open(&path).expect("inspect migrated db");
        let version: i64 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0)).expect("version");
        assert_eq!(version, MESSAGE_CACHE_SCHEMA_VERSION);
        assert_eq!(user_tables(&connection).expect("tables"), expected_tables(5));
    }

    #[test]
    fn migrates_v4_forward_with_zero_initial_chat_revisions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("messages.sqlite");
        let connection = Connection::open(&path).expect("raw open");
        connection.execute_batch(SCHEMA_V1).expect("create v1 schema");
        connection.execute_batch(SCHEMA_V2).expect("create v2 schema");
        connection.execute_batch(SCHEMA_V3).expect("create v3 schema");
        connection.execute_batch(SCHEMA_V4).expect("create v4 schema");
        connection
            .execute(
                "INSERT INTO cache_meta (singleton, database_kind, schema_version)
                 VALUES (1, ?1, 4)",
                [MESSAGE_CACHE_KIND],
            )
            .expect("v4 metadata");
        connection
            .execute(
                "INSERT INTO cache_accounts (
                     account_user_id, display_name, username, observed_at_unix, generation
                 ) VALUES (7, 'Account 7', 'user7', 10, 0)",
                [],
            )
            .expect("v4 account");
        connection
            .pragma_update(None, "application_id", MESSAGE_CACHE_APPLICATION_ID)
            .expect("application id");
        connection.pragma_update(None, "user_version", 4_i64).expect("v4 version");
        drop(connection);

        let migrated = MessageCache::open(&path).expect("migrate");
        assert_eq!(migrated.chat_revision(7, "chat").expect("initial revision"), 0);
        let connection = Connection::open(&path).expect("inspect migrated db");
        let version: i64 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0)).expect("version");
        assert_eq!(version, MESSAGE_CACHE_SCHEMA_VERSION);
        assert_eq!(user_tables(&connection).expect("tables"), expected_tables(5));
    }

    #[test]
    fn configured_retention_prunes_strictly_before_cutoff_and_repairs_checkpoints() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        bind(&cache, 2);
        cache.clear_account(1, 99).expect("advance account one generation");
        assert_eq!(
            cache.bound_account_generations().expect("bound accounts"),
            vec![(1, 1), (2, 0)]
        );
        cache
            .upsert_dialog_checkpoint(
                1,
                &DialogCheckpoint {
                    chat_id: "chat".to_string(),
                    frontier_message_id: Some(3),
                    oldest_message_id: Some(1),
                    history_complete: true,
                    observed_at_unix: 100,
                },
            )
            .expect("seed complete checkpoint");
        let now_unix = 200_000_i64;
        let cutoff_unix = now_unix - SECONDS_PER_DAY;
        cache
            .upsert_messages(
                1,
                &[
                    cached(
                        "chat",
                        summary(1, "expired", i32::try_from(cutoff_unix - 1).unwrap(), None),
                    ),
                    cached(
                        "chat",
                        summary(2, "boundary", i32::try_from(cutoff_unix).unwrap(), None),
                    ),
                    cached(
                        "chat",
                        summary(3, "recent", i32::try_from(cutoff_unix + 1).unwrap(), None),
                    ),
                ],
            )
            .expect("seed account one");
        cache
            .upsert_message(
                2,
                &cached(
                    "chat",
                    summary(1, "other account", i32::try_from(cutoff_unix - 1).unwrap(), None),
                ),
            )
            .expect("seed account two");
        let generation = cache.account_generation(1).expect("generation").expect("bound");
        let retained = cache.clone().with_max_age_days(1).expect("retention policy");
        let result = retained
            .prune_configured_messages_if_generation(1, generation, now_unix)
            .expect("prune")
            .expect("configured result");
        assert_eq!(result.cutoff_unix, cutoff_unix);
        assert_eq!(result.removed_messages, 1);
        assert_eq!(result.affected_chats, vec!["chat"]);
        assert!(retained.exact_get(1, "chat", 1).expect("expired get").is_none());
        assert!(retained.exact_get(1, "chat", 2).expect("boundary get").is_some());
        assert!(retained.exact_get(1, "chat", 3).expect("recent get").is_some());
        assert!(retained.exact_get(2, "chat", 1).expect("other account get").is_some());
        let checkpoint =
            retained.dialog_checkpoint(1, "chat").expect("checkpoint").expect("stored checkpoint");
        assert_eq!(checkpoint.frontier_message_id, Some(3));
        assert_eq!(checkpoint.oldest_message_id, Some(2));
        assert!(!checkpoint.history_complete);
        assert_eq!(checkpoint.observed_at_unix, now_unix);

        retained.clear_account(1, now_unix + 1).expect("advance generation");
        assert!(matches!(
            retained.prune_configured_messages_if_generation(1, generation, now_unix + 2),
            Err(MessageCacheError::StaleGeneration { .. })
        ));
    }

    #[test]
    fn configured_retention_is_enforced_inside_every_message_write() {
        let (_temp, cache) = cache();
        let cache = cache.with_max_age_days(1).expect("retention policy");
        bind(&cache, 1);
        let now_unix = Utc::now().timestamp();
        let expired_date = i32::try_from(now_unix - SECONDS_PER_DAY - 1).expect("expired date");
        let recent_date = i32::try_from(now_unix).expect("recent date");
        cache
            .upsert_messages(
                1,
                &[
                    cached("chat", summary(1, "expired", expired_date, None)),
                    cached("chat", summary(2, "recent", recent_date, None)),
                ],
            )
            .expect("write with retention");
        assert!(cache.exact_get(1, "chat", 1).expect("expired get").is_none());
        assert!(cache.exact_get(1, "chat", 2).expect("recent get").is_some());

        cache
            .apply_dialog_batch(
                1,
                None,
                &[cached("chat", summary(3, "expired again", expired_date, None))],
                &DialogCheckpoint {
                    chat_id: "chat".to_string(),
                    frontier_message_id: Some(3),
                    oldest_message_id: Some(2),
                    history_complete: true,
                    observed_at_unix: now_unix,
                },
            )
            .expect("dialog write with retention");
        assert!(cache.exact_get(1, "chat", 3).expect("expired dialog get").is_none());
        assert!(
            !cache
                .dialog_checkpoint(1, "chat")
                .expect("checkpoint")
                .expect("stored checkpoint")
                .history_complete
        );
    }

    #[test]
    fn global_update_batch_is_atomic_idempotent_and_preserves_ambiguous_deletes() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        cache
            .upsert_dialog(
                1,
                &DialogMetadata {
                    chat_id: "101".to_string(),
                    chat_ref: Some("101".to_string()),
                    chat_kind: Some("user".to_string()),
                    title: Some("Alex".to_string()),
                    username: Some("alex".to_string()),
                    observed_at_unix: 9,
                },
            )
            .expect("seed private dialog identity");
        cache
            .upsert_dialog(
                1,
                &DialogMetadata {
                    chat_id: "-100505".to_string(),
                    chat_ref: Some("@fixture_channel".to_string()),
                    chat_kind: Some("channel".to_string()),
                    title: Some("Fixture Channel".to_string()),
                    username: Some("fixture_channel".to_string()),
                    observed_at_unix: 9,
                },
            )
            .expect("seed channel dialog identity");
        cache
            .upsert_message(1, &cached("-100505", summary(49, "old", 10, Some("alice"))))
            .expect("seed channel message");
        cache
            .upsert_message(
                1,
                &cached("-100505", summary(41, "independent channel id", 10, Some("alice"))),
            )
            .expect("seed same-id channel message");
        cache
            .upsert_message(1, &cached("101", summary(41, "globally deleted", 11, Some("alex"))))
            .expect("seed global-delete candidate");
        cache
            .upsert_message(1, &cached("unclassified", summary(41, "unknown namespace", 11, None)))
            .expect("seed unclassified collision");
        cache
            .upsert_dialog(
                1,
                &DialogMetadata {
                    chat_id: "-100505".to_string(),
                    chat_ref: Some("-100505".to_string()),
                    chat_kind: None,
                    title: None,
                    username: None,
                    observed_at_unix: 12,
                },
            )
            .expect("fallback metadata must not erase the known channel namespace");

        let mut edited = summary(50, "edited payload", 20, Some("bob"));
        edited.edit_date = Some(25);
        let batch = CacheUpdateBatch {
            dialogs: Vec::new(),
            messages: vec![CachedMessageInput {
                chat_id: "-100505".to_string(),
                message: edited.clone(),
                observed_at_unix: 30,
                source: MessageSource::LiveUpdate,
            }],
            incoming_events: Vec::new(),
            channel_tombstones: vec![MessageTombstone {
                chat_id: "-100505".to_string(),
                message_id: 49,
                deleted_at_unix: 30,
                source: MessageSource::LiveUpdate,
            }],
            global_deleted_message_ids: vec![41, 42],
            cursor: GlobalUpdateCursor {
                pts: 120,
                qts: 7,
                date: 1_700_004_000,
                seq: 10,
                unread_count: Some(2),
                phase: CacheSyncPhase::Current,
                observed_at_unix: 30,
                last_error_code: None,
            },
        };

        cache.apply_update_batch(1, &batch).expect("apply update");
        cache.apply_update_batch(1, &batch).expect("idempotent replay");
        assert!(cache.exact_get(1, "-100505", 49).expect("deleted get").is_none());
        assert!(cache.exact_get(1, "101", 41).expect("global deleted get").is_none());
        assert!(
            cache.exact_get(1, "unclassified", 41).expect("unknown namespace get").is_none(),
            "an unclassified collision must fail closed instead of being exposed as live"
        );
        assert_eq!(
            cache
                .exact_get(1, "-100505", 41)
                .expect("same-id channel get")
                .expect("same-id channel row")
                .message
                .text
                .as_deref(),
            Some("independent channel id")
        );
        let cached_edit =
            cache.exact_get(1, "-100505", 50).expect("edited get").expect("edited row").message;
        assert_eq!(cached_edit.message_id, edited.message_id);
        assert_eq!(cached_edit.text, edited.text);
        assert_eq!(cached_edit.edit_date, edited.edit_date);
        assert_eq!(cache.global_update_cursor(1).expect("cursor"), Some(batch.cursor.clone()));
        let stats = cache.account_stats(1).expect("stats").expect("account");
        assert_eq!(stats.message_count, 2);
        assert_eq!(stats.tombstone_count, 1);
        assert_eq!(stats.global_tombstone_count, 2);
        assert_eq!(stats.last_checkpoint_unix, Some(30));

        let mut later_channel_edit = summary(41, "later channel edit", 10, Some("alice"));
        later_channel_edit.edit_date = Some(31);
        cache
            .apply_update_batch(
                1,
                &CacheUpdateBatch {
                    dialogs: batch.dialogs.clone(),
                    messages: vec![CachedMessageInput {
                        chat_id: "-100505".to_string(),
                        message: later_channel_edit,
                        observed_at_unix: 31,
                        source: MessageSource::LiveUpdate,
                    }],
                    incoming_events: Vec::new(),
                    channel_tombstones: Vec::new(),
                    global_deleted_message_ids: Vec::new(),
                    cursor: GlobalUpdateCursor {
                        pts: 121,
                        qts: 7,
                        date: 1_700_004_001,
                        seq: 11,
                        unread_count: Some(2),
                        phase: CacheSyncPhase::Current,
                        observed_at_unix: 31,
                        last_error_code: None,
                    },
                },
            )
            .expect("apply later channel edit");
        cache
            .upsert_message(1, &cached("101", summary(41, "stale private row", 11, Some("alex"))))
            .expect("stale private write is ignored by common tombstone");
        assert!(cache.exact_get(1, "101", 41).expect("private row remains deleted").is_none());
        assert_eq!(
            cache
                .exact_get(1, "-100505", 41)
                .expect("later channel edit get")
                .expect("later channel edit row")
                .message
                .text
                .as_deref(),
            Some("later channel edit")
        );

        let cleared = cache.clear_account(1, 40).expect("clear");
        assert_eq!(cleared.deleted_messages, 2);
        assert_eq!(cleared.deleted_tombstones, 1);
        assert_eq!(cleared.deleted_global_tombstones, 2);
        assert_eq!(cleared.deleted_update_states, 1);
        assert!(cache.global_update_cursor(1).expect("cleared cursor").is_none());
    }

    #[test]
    fn incoming_event_journal_is_idempotent_restart_safe_and_non_consuming_across_cursors() {
        let (temp, cache) = cache();
        bind(&cache, 1);
        let initial = cache.incoming_event_window(1, None, 100).expect("initial cursor");
        assert!(initial.events.is_empty());

        let batch = CacheUpdateBatch {
            dialogs: Vec::new(),
            messages: Vec::new(),
            incoming_events: vec![
                IncomingMessageEventInput {
                    chat_id: "101".to_string(),
                    sender_user_id: 101,
                    sender_name: "Alex".to_string(),
                    sender_username: Some("alex".to_string()),
                    message_id: 10,
                    message_date_unix: 1,
                    observed_at_unix_ms: 1_000,
                },
                IncomingMessageEventInput {
                    chat_id: "202".to_string(),
                    sender_user_id: 202,
                    sender_name: "Blair".to_string(),
                    sender_username: None,
                    message_id: 20,
                    message_date_unix: 1,
                    observed_at_unix_ms: 1_500,
                },
                IncomingMessageEventInput {
                    chat_id: "101".to_string(),
                    sender_user_id: 101,
                    sender_name: "Alex".to_string(),
                    sender_username: Some("alex".to_string()),
                    message_id: 11,
                    message_date_unix: 2,
                    observed_at_unix_ms: 2_000,
                },
            ],
            channel_tombstones: Vec::new(),
            global_deleted_message_ids: Vec::new(),
            cursor: GlobalUpdateCursor {
                pts: 1,
                qts: 0,
                date: 2,
                seq: 1,
                unread_count: Some(3),
                phase: CacheSyncPhase::Current,
                observed_at_unix: 2,
                last_error_code: None,
            },
        };
        assert_eq!(
            cache.apply_update_batch(1, &batch).expect("first apply").inserted_incoming_events,
            3
        );
        assert_eq!(
            cache.apply_update_batch(1, &batch).expect("replay").inserted_incoming_events,
            0
        );

        let first = cache
            .incoming_event_window(1, Some(&initial.cursor.token), 2_100)
            .expect("first client");
        let second = cache
            .incoming_event_window(1, Some(&initial.cursor.token), 2_100)
            .expect("second client");
        assert_eq!(first.events, second.events);
        assert_eq!(first.events.len(), 3);

        let alex_last =
            first.events.iter().rev().find(|event| event.chat_id == "101").expect("Alex event");
        let after_alex = cache
            .advance_incoming_event_cursor_chat(1, &first.cursor, "101", alex_last.event_id, 2_200)
            .expect("advance one chat");
        let remaining =
            cache.incoming_event_window(1, Some(&after_alex), 2_300).expect("remaining events");
        assert_eq!(
            remaining
                .events
                .iter()
                .map(|event| (event.chat_id.as_str(), event.message_id))
                .collect::<Vec<_>>(),
            vec![("202", 20)]
        );
        assert_eq!(
            cache
                .incoming_event_window(1, Some(&initial.cursor.token), 2_300)
                .expect("original cursor remains replayable")
                .events
                .len(),
            3
        );

        let path = cache.path().to_path_buf();
        drop(cache);
        let reopened = MessageCache::open(path).expect("reopen");
        assert_eq!(
            reopened
                .incoming_event_window(1, Some(&after_alex), 2_400)
                .expect("restart-safe cursor")
                .events
                .len(),
            1
        );
        let stats = reopened.account_stats(1).expect("stats").expect("account");
        assert_eq!(stats.incoming_event_count, 3);
        assert!(stats.event_checkpoint_count >= 2);

        let cleared = reopened.clear_account(1, 3).expect("clear");
        assert_eq!(cleared.deleted_incoming_events, 3);
        assert!(cleared.deleted_event_checkpoints >= 2);
        assert!(matches!(
            reopened.incoming_event_window(1, Some(&after_alex), 3_000),
            Err(MessageCacheError::StaleEventCursor { .. })
        ));
        drop(temp);
    }

    #[test]
    fn mixed_chat_then_global_advancement_does_not_resurrect_consumed_events() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        let initial = cache.incoming_event_window(1, None, 100).expect("initial cursor");
        let batch = CacheUpdateBatch {
            dialogs: Vec::new(),
            messages: Vec::new(),
            incoming_events: vec![
                IncomingMessageEventInput {
                    chat_id: "101".to_string(),
                    sender_user_id: 101,
                    sender_name: "Alex".to_string(),
                    sender_username: None,
                    message_id: 1,
                    message_date_unix: 1,
                    observed_at_unix_ms: 1_000,
                },
                IncomingMessageEventInput {
                    chat_id: "202".to_string(),
                    sender_user_id: 202,
                    sender_name: "Blair".to_string(),
                    sender_username: None,
                    message_id: 2,
                    message_date_unix: 2,
                    observed_at_unix_ms: 2_000,
                },
                IncomingMessageEventInput {
                    chat_id: "101".to_string(),
                    sender_user_id: 101,
                    sender_name: "Alex".to_string(),
                    sender_username: None,
                    message_id: 3,
                    message_date_unix: 3,
                    observed_at_unix_ms: 3_000,
                },
            ],
            channel_tombstones: Vec::new(),
            global_deleted_message_ids: Vec::new(),
            cursor: GlobalUpdateCursor {
                pts: 3,
                qts: 0,
                date: 3,
                seq: 3,
                unread_count: Some(3),
                phase: CacheSyncPhase::Current,
                observed_at_unix: 3,
                last_error_code: None,
            },
        };
        cache.apply_update_batch(1, &batch).expect("seed events");
        let all =
            cache.incoming_event_window(1, Some(&initial.cursor.token), 3_100).expect("all events");
        let alex_last = all.events.iter().find(|event| event.message_id == 3).expect("A3");
        let after_alex = cache
            .advance_incoming_event_cursor_chat(1, &all.cursor, "101", alex_last.event_id, 3_200)
            .expect("consume A1 and A3");
        let blair =
            cache.incoming_event_window(1, Some(&after_alex), 3_300).expect("only B2 remains");
        assert_eq!(blair.events.iter().map(|event| event.message_id).collect::<Vec<_>>(), vec![2]);
        let final_cursor = cache
            .advance_incoming_event_cursor_all(1, &blair.cursor, blair.events[0].event_id, 3_400)
            .expect("consume B2 globally");
        assert!(
            cache
                .incoming_event_window(1, Some(&final_cursor), 3_500)
                .expect("nothing can resurrect")
                .events
                .is_empty()
        );
    }

    #[test]
    fn cursor_advancement_revalidates_account_and_checkpoint_identity() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        bind(&cache, 2);
        let initial = cache.incoming_event_window(1, None, 100).expect("account one cursor");
        let batch = CacheUpdateBatch {
            dialogs: Vec::new(),
            messages: Vec::new(),
            incoming_events: vec![IncomingMessageEventInput {
                chat_id: "101".to_string(),
                sender_user_id: 101,
                sender_name: "Alex".to_string(),
                sender_username: None,
                message_id: 1,
                message_date_unix: 1,
                observed_at_unix_ms: 1_000,
            }],
            channel_tombstones: Vec::new(),
            global_deleted_message_ids: Vec::new(),
            cursor: GlobalUpdateCursor {
                pts: 1,
                qts: 0,
                date: 1,
                seq: 1,
                unread_count: Some(1),
                phase: CacheSyncPhase::Current,
                observed_at_unix: 1,
                last_error_code: None,
            },
        };
        cache.apply_update_batch(1, &batch).expect("seed event");
        let window = cache
            .incoming_event_window(1, Some(&initial.cursor.token), 1_100)
            .expect("event window");
        let event_id = window.events[0].event_id;
        assert!(matches!(
            cache.advance_incoming_event_cursor_all(2, &window.cursor, event_id, 1_200),
            Err(MessageCacheError::InvalidEventCursor(_))
        ));
        cache.clear_account(1, 2).expect("invalidate account one cursor");
        assert!(matches!(
            cache.advance_incoming_event_cursor_all(1, &window.cursor, event_id, 1_300),
            Err(MessageCacheError::StaleEventCursor { .. })
        ));
    }

    #[test]
    fn telegram_resync_invalidates_event_continuity_and_rejects_new_waits_until_rebased() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        let initial = cache.incoming_event_window(1, None, 100).expect("initial cursor");
        cache
            .apply_update_batch(
                1,
                &CacheUpdateBatch {
                    dialogs: Vec::new(),
                    messages: Vec::new(),
                    incoming_events: vec![IncomingMessageEventInput {
                        chat_id: "101".to_string(),
                        sender_user_id: 101,
                        sender_name: "Alex".to_string(),
                        sender_username: None,
                        message_id: 1,
                        message_date_unix: 1,
                        observed_at_unix_ms: 1_000,
                    }],
                    channel_tombstones: Vec::new(),
                    global_deleted_message_ids: Vec::new(),
                    cursor: GlobalUpdateCursor {
                        pts: 9,
                        qts: 0,
                        date: 9,
                        seq: 9,
                        unread_count: Some(1),
                        phase: CacheSyncPhase::Current,
                        observed_at_unix: 9,
                        last_error_code: None,
                    },
                },
            )
            .expect("seed event before gap");
        cache
            .set_global_update_cursor(
                1,
                &GlobalUpdateCursor {
                    pts: 10,
                    qts: 0,
                    date: 10,
                    seq: 10,
                    unread_count: Some(0),
                    phase: CacheSyncPhase::ResyncRequired,
                    observed_at_unix: 10,
                    last_error_code: Some("CACHE_RESYNC_REQUIRED".to_string()),
                },
            )
            .expect("mark resync");
        assert!(matches!(
            cache.incoming_event_window(1, Some(&initial.cursor.token), 10_100),
            Err(MessageCacheError::StaleEventCursor { .. })
        ));
        assert!(matches!(
            cache.incoming_event_window(1, None, 10_100),
            Err(MessageCacheError::EventStreamResyncRequired)
        ));
        let stats = cache.account_stats(1).expect("stats").expect("account");
        assert_eq!(stats.incoming_event_count, 0);
        assert_eq!(stats.event_checkpoint_count, 0);

        cache
            .set_global_update_cursor(
                1,
                &GlobalUpdateCursor {
                    pts: 20,
                    qts: 0,
                    date: 20,
                    seq: 20,
                    unread_count: Some(0),
                    phase: CacheSyncPhase::CatchingUp,
                    observed_at_unix: 20,
                    last_error_code: None,
                },
            )
            .expect("rebase cursor");
        assert!(cache.incoming_event_window(1, None, 20_100).expect("new epoch").events.is_empty());
    }

    #[test]
    fn checkpoint_retention_eviction_is_typed_stale_not_invalid() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        let first = cache.incoming_event_window(1, None, 100).expect("first cursor");
        let connection = cache.connection().expect("connection");
        connection
            .execute(
                "INSERT INTO cache_incoming_events (
                     account_user_id, chat_id, sender_user_id, sender_name,
                     sender_username, message_id, message_date_unix, observed_at_unix_ms
                 ) VALUES (1, '101', 101, 'Alex', NULL, 1, 1, 1000)",
                [],
            )
            .expect("seed event");
        drop(connection);
        let protected = cache.incoming_event_window(1, None, 200).expect("second cursor");
        let connection = cache.connection().expect("connection");
        for (token, invalidated_at_unix_ms) in [("future-a", 10_000), ("future-b", 10_001)] {
            connection
                .execute(
                    "INSERT INTO cache_event_cursor_tombstones (
                         cursor_token, account_user_id, reason, invalidated_at_unix_ms
                     ) VALUES (?1, 1, 'checkpoint_retention', ?2)",
                    params![token, invalidated_at_unix_ms],
                )
                .expect("seed future tombstone");
        }
        prune_event_checkpoints_with_limit(&connection, 1, &protected.cursor.token, 300, 1)
            .expect("prune oldest checkpoint");
        prune_event_cursor_tombstones_with_limit(&connection, 1, 2)
            .expect("apply bounded tombstone cap");
        drop(connection);
        assert!(matches!(
            cache.incoming_event_window(1, Some(&first.cursor.token), 400),
            Err(MessageCacheError::StaleEventCursor { .. })
        ));
        assert!(matches!(
            cache.incoming_event_window(1, Some("fabricated-token"), 400),
            Err(MessageCacheError::InvalidEventCursor(_))
        ));
    }

    #[test]
    fn newly_invalidated_resync_cursor_survives_old_tombstones_under_cap_pressure() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        let cursor = cache.incoming_event_window(1, None, 100).expect("cursor");
        let now_unix_ms = Utc::now().timestamp_millis();
        let connection = cache.connection().expect("connection");
        for (token, invalidated_at_unix_ms) in
            [("older-a", now_unix_ms - 2_000), ("older-b", now_unix_ms - 1_000)]
        {
            connection
                .execute(
                    "INSERT INTO cache_event_cursor_tombstones (
                         cursor_token, account_user_id, reason, invalidated_at_unix_ms
                     ) VALUES (?1, 1, 'checkpoint_retention', ?2)",
                    params![token, invalidated_at_unix_ms],
                )
                .expect("seed older tombstone");
        }
        drop(connection);

        cache
            .set_global_update_cursor(
                1,
                &GlobalUpdateCursor {
                    pts: 10,
                    qts: 0,
                    date: 10,
                    seq: 10,
                    unread_count: Some(0),
                    phase: CacheSyncPhase::ResyncRequired,
                    observed_at_unix: 1,
                    last_error_code: Some("CACHE_RESYNC_REQUIRED".to_string()),
                },
            )
            .expect("invalidate with current wall-clock ordering");
        let connection = cache.connection().expect("connection");
        prune_event_cursor_tombstones_with_limit(&connection, 1, 2).expect("apply bounded cap");
        let retained: bool = connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM cache_event_cursor_tombstones
                     WHERE account_user_id = 1 AND cursor_token = ?1
                 )",
                [&cursor.cursor.token],
                |row| row.get(0),
            )
            .expect("inspect resync tombstone");
        assert!(retained, "the newly invalidated cursor must not be the oldest tombstone");
        drop(connection);
        assert!(matches!(
            cache.incoming_event_window(1, Some(&cursor.cursor.token), now_unix_ms + 1),
            Err(MessageCacheError::StaleEventCursor { .. })
        ));
    }

    #[test]
    fn newly_invalidated_clear_cursor_survives_same_second_tombstones_under_cap_pressure() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        let cursor = cache.incoming_event_window(1, None, 100).expect("cursor");
        let connection = cache.connection().expect("connection");
        for (token, invalidated_at_unix_ms) in [("same-second-a", 1_001), ("same-second-b", 1_002)]
        {
            connection
                .execute(
                    "INSERT INTO cache_event_cursor_tombstones (
                         cursor_token, account_user_id, reason, invalidated_at_unix_ms
                     ) VALUES (?1, 1, 'checkpoint_retention', ?2)",
                    params![token, invalidated_at_unix_ms],
                )
                .expect("seed same-second tombstone");
        }
        drop(connection);

        cache.clear_account(1, 1).expect("clear account");
        let connection = cache.connection().expect("connection");
        prune_event_cursor_tombstones_with_limit(&connection, 1, 2).expect("apply bounded cap");
        let retained: bool = connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM cache_event_cursor_tombstones
                     WHERE account_user_id = 1 AND cursor_token = ?1
                 )",
                [&cursor.cursor.token],
                |row| row.get(0),
            )
            .expect("inspect clear tombstone");
        assert!(retained, "the newly invalidated clear cursor must be ordered after prior rows");
        drop(connection);
        assert!(matches!(
            cache.incoming_event_window(1, Some(&cursor.cursor.token), 200),
            Err(MessageCacheError::StaleEventCursor { .. })
        ));
    }

    #[test]
    fn pruned_event_frontier_fails_old_cursors_closed() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        let cursor = cache.incoming_event_window(1, None, 100).expect("cursor");
        let connection = cache.connection().expect("connection");
        connection
            .execute(
                "INSERT INTO cache_event_stream_state (
                     account_user_id, pruned_through_event_id
                 ) VALUES (1, 5)",
                [],
            )
            .expect("simulate retained frontier");
        drop(connection);
        assert!(matches!(
            cache.incoming_event_window(1, Some(&cursor.cursor.token), 200),
            Err(MessageCacheError::StaleEventCursor { .. })
        ));
    }

    #[test]
    fn refuses_to_mix_with_an_unrelated_sqlite_schema() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("mixed.sqlite");
        Connection::open(&path)
            .expect("open")
            .execute("CREATE TABLE approvals (id TEXT PRIMARY KEY)", [])
            .expect("create unrelated table");
        assert!(matches!(
            MessageCache::open(path),
            Err(MessageCacheError::IncompatibleDatabase(_))
        ));
    }

    #[test]
    fn dialog_batch_commits_rows_metadata_and_checkpoint_atomically() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        let dialog = DialogMetadata {
            chat_id: "chat".to_string(),
            chat_ref: Some("@chat".to_string()),
            chat_kind: Some("group".to_string()),
            title: Some("A chat".to_string()),
            username: Some("chat".to_string()),
            observed_at_unix: 100,
        };
        let checkpoint = DialogCheckpoint {
            chat_id: "chat".to_string(),
            frontier_message_id: Some(2),
            oldest_message_id: Some(1),
            history_complete: true,
            observed_at_unix: 101,
        };
        let mismatched = [
            cached("chat", summary(1, "one", 10, Some("alice"))),
            cached("other-chat", summary(2, "wrong", 20, Some("alice"))),
        ];
        assert!(matches!(
            cache.apply_dialog_batch(1, Some(&dialog), &mismatched, &checkpoint),
            Err(MessageCacheError::DialogBatchChatMismatch { .. })
        ));
        assert!(cache.exact_get(1, "chat", 1).expect("get after rejected batch").is_none());
        assert!(cache.dialog_checkpoint(1, "chat").expect("checkpoint").is_none());

        let messages = [
            cached("chat", summary(1, "one", 10, Some("alice"))),
            cached("chat", summary(2, "two", 20, Some("bob"))),
        ];
        cache.apply_dialog_batch(1, Some(&dialog), &messages, &checkpoint).expect("atomic batch");
        assert_eq!(cache.account_stats(1).expect("stats").expect("account").message_count, 2);
        assert_eq!(
            cache.dialog_checkpoint(1, "chat").expect("checkpoint").expect("stored checkpoint"),
            checkpoint
        );
        let connection = Connection::open(cache.path()).expect("raw open");
        let title: String = connection
            .query_row(
                "SELECT title FROM cache_dialogs
                 WHERE account_user_id = 1 AND chat_id = 'chat'",
                [],
                |row| row.get(0),
            )
            .expect("dialog metadata");
        assert_eq!(title, "A chat");
    }

    #[test]
    fn account_scope_isolation_and_duplicate_ids_across_chats_are_preserved() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        bind(&cache, 2);
        cache
            .upsert_messages(
                1,
                &[
                    cached("chat-a", summary(5, "account-one-a", 100, Some("alice"))),
                    cached("chat-b", summary(5, "account-one-b", 100, Some("alice"))),
                ],
            )
            .expect("account one messages");
        cache
            .upsert_message(2, &cached("chat-a", summary(5, "account-two", 100, Some("bob"))))
            .expect("account two message");

        assert_eq!(
            cache.exact_get(1, "chat-a", 5).expect("get").expect("message").message.text.as_deref(),
            Some("account-one-a")
        );
        assert_eq!(
            cache.exact_get(2, "chat-a", 5).expect("get").expect("message").message.text.as_deref(),
            Some("account-two")
        );
        assert_eq!(cache.account_stats(1).expect("stats").expect("bound").message_count, 2);
    }

    #[test]
    fn full_upsert_reflects_edits() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        cache
            .upsert_message(1, &cached("chat", summary(8, "before", 100, Some("alice"))))
            .expect("initial upsert");
        let mut edited = summary(8, "after", 100, Some("alice"));
        edited.edit_date = Some(150);
        edited.pinned = true;
        cache
            .upsert_message(
                1,
                &CachedMessageInput {
                    chat_id: "chat".to_string(),
                    message: edited,
                    observed_at_unix: 151,
                    source: MessageSource::LiveUpdate,
                },
            )
            .expect("edit upsert");

        let cached = cache.exact_get(1, "chat", 8).expect("get").expect("message");
        assert_eq!(cached.message.text.as_deref(), Some("after"));
        assert_eq!(cached.message.edit_date, Some(150));
        assert!(cached.message.pinned);
        assert_eq!(cached.source, MessageSource::LiveUpdate);
    }

    #[test]
    fn inclusive_time_filters_and_tie_breakers_are_stable() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        cache
            .upsert_messages(
                1,
                &[
                    cached("chat-b", summary(10, "b10", 100, Some("s"))),
                    cached("chat-a", summary(10, "a10", 100, Some("s"))),
                    cached("chat-z", summary(9, "z9", 100, Some("s"))),
                    cached("chat-a", summary(11, "older", 99, Some("s"))),
                    cached("chat-a", summary(12, "newer", 101, Some("s"))),
                ],
            )
            .expect("upsert");

        let page = cache
            .query_newest(
                1,
                &MessageWindow { chat_id: None, after_unix: Some(100), before_unix: Some(100) },
                10,
            )
            .expect("query");
        let identities = page
            .messages
            .iter()
            .map(|message| (message.chat_id.as_str(), message.message.message_id))
            .collect::<Vec<_>>();
        assert_eq!(identities, vec![("chat-a", 10), ("chat-b", 10), ("chat-z", 9)]);
    }

    #[test]
    fn regex_is_case_insensitive_bounded_and_reports_invalid_patterns() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        cache
            .upsert_messages(
                1,
                &[
                    cached("chat", summary(1, "HeLLo there", 10, Some("alice"))),
                    cached("chat", summary(2, "newest non-match", 20, Some("alice"))),
                ],
            )
            .expect("upsert");

        let page = cache
            .regex_search(
                1,
                &RegexSearchRequest {
                    pattern: "hello".to_string(),
                    window: MessageWindow::default(),
                    max_results: 10,
                    max_scan_rows: 10,
                },
            )
            .expect("search");
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.messages[0].message.message_id, 1);

        let bounded = cache
            .regex_search(
                1,
                &RegexSearchRequest {
                    pattern: "hello".to_string(),
                    window: MessageWindow::default(),
                    max_results: 10,
                    max_scan_rows: 1,
                },
            )
            .expect("bounded search");
        assert_eq!(bounded.scanned_rows, 1);
        assert!(bounded.messages.is_empty());
        assert!(bounded.truncated);

        assert!(matches!(
            cache.regex_search(
                1,
                &RegexSearchRequest {
                    pattern: "[".to_string(),
                    window: MessageWindow::default(),
                    max_results: 10,
                    max_scan_rows: 10,
                }
            ),
            Err(MessageCacheError::InvalidRegex(_))
        ));
        assert!(matches!(
            cache.regex_search(
                1,
                &RegexSearchRequest {
                    pattern: "a".repeat(MAX_REGEX_PATTERN_BYTES + 1),
                    window: MessageWindow::default(),
                    max_results: 10,
                    max_scan_rows: 10,
                }
            ),
            Err(MessageCacheError::RegexPatternTooLong { .. })
        ));
    }

    #[test]
    fn analytics_timeline_and_utc_today_use_exact_utc_bounds() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        let now = Utc.with_ymd_and_hms(2026, 7, 23, 12, 0, 0).single().expect("fixture time");
        let start = now.date_naive().and_hms_opt(0, 0, 0).expect("midnight").and_utc().timestamp();
        let fixtures = [
            (1, "previous", start - 1, "alice"),
            (2, "midnight", start, "alice"),
            (3, "first-hour", start + 3_599, "alice"),
            (4, "second-hour", start + 3_600, "bob"),
            (5, "tomorrow", start + 86_400, "bob"),
        ]
        .into_iter()
        .map(|(id, text, date, sender)| {
            cached("chat", summary(id, text, i32::try_from(date).unwrap(), Some(sender)))
        })
        .collect::<Vec<_>>();
        cache.upsert_messages(1, &fixtures).expect("upsert");
        let today_window = MessageWindow {
            chat_id: None,
            after_unix: Some(start),
            before_unix: Some(start + 86_399),
        };

        assert_eq!(
            cache.top_senders(1, &today_window, 10).expect("top senders"),
            vec![
                SenderCount { sender_ref: "alice".to_string(), message_count: 2 },
                SenderCount { sender_ref: "bob".to_string(), message_count: 1 },
            ]
        );
        assert_eq!(
            cache.timeline(1, &today_window, TimelineGranularity::Hour).expect("hour timeline"),
            vec![
                TimelinePoint { start_unix: start, message_count: 2 },
                TimelinePoint { start_unix: start + 3_600, message_count: 1 },
            ]
        );
        assert_eq!(
            cache.timeline(1, &today_window, TimelineGranularity::Day).expect("day timeline"),
            vec![TimelinePoint { start_unix: start, message_count: 3 }]
        );
        let today = cache.utc_today(1, None, now, 10).expect("today");
        assert_eq!(
            today.messages.iter().map(|message| message.message.message_id).collect::<Vec<_>>(),
            vec![2, 3, 4]
        );
    }

    #[test]
    fn source_rows_are_chronological_and_csv_is_escaped() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        cache
            .upsert_messages(
                1,
                &[
                    cached("chat", summary(2, "later", 20, Some("alice"))),
                    cached("chat", summary(1, "hello, \"world\"\nnext", 10, Some("alice"))),
                ],
            )
            .expect("upsert");

        let json = cache.source_rows_json(1, &MessageWindow::default(), 10).expect("json");
        let value: serde_json::Value = serde_json::from_str(&json.body).expect("parse json");
        assert_eq!(value[0]["message_id"], 1);
        assert_eq!(value[1]["message_id"], 2);

        let csv = cache.source_rows_csv(1, &MessageWindow::default(), 10).expect("csv");
        assert!(csv.body.starts_with(CSV_HEADER));
        assert!(csv.body.contains("\"hello, \"\"world\"\"\nnext\""));

        bind(&cache, 2);
        let empty_json =
            cache.source_rows_json(2, &MessageWindow::default(), 10).expect("empty json");
        let empty_csv = cache.source_rows_csv(2, &MessageWindow::default(), 10).expect("empty csv");
        assert_eq!(empty_json.body, "[]");
        assert_eq!(empty_json.row_count, 0);
        assert_eq!(empty_csv.body, "");
        assert_eq!(empty_csv.row_count, 0);
    }

    #[test]
    fn chat_frontier_counts_and_account_stats_are_account_scoped() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        cache
            .upsert_dialog_checkpoint(
                1,
                &DialogCheckpoint {
                    chat_id: "chat".to_string(),
                    frontier_message_id: Some(50),
                    oldest_message_id: Some(2),
                    history_complete: true,
                    observed_at_unix: 100,
                },
            )
            .expect("checkpoint");
        cache
            .upsert_messages(
                1,
                &[
                    cached("chat", summary(2, "old", 10, Some("alice"))),
                    cached("chat", summary(49, "new", 20, Some("bob"))),
                ],
            )
            .expect("messages");
        cache
            .tombstone_message(
                1,
                &MessageTombstone {
                    chat_id: "chat".to_string(),
                    message_id: 3,
                    deleted_at_unix: 30,
                    source: MessageSource::LiveUpdate,
                },
            )
            .expect("tombstone");

        assert_eq!(cache.max_message_id(1, "chat").expect("max"), Some(49));
        let stats = cache.chat_stats(1, "chat").expect("stats").expect("chat");
        assert_eq!(stats.message_count, 2);
        assert_eq!(stats.tombstone_count, 1);
        assert_eq!(stats.min_message_id, Some(2));
        assert_eq!(stats.max_message_id, Some(49));
        assert_eq!(stats.frontier_message_id, Some(50));
        assert!(stats.history_complete);
        assert_eq!(stats.checkpoint_observed_at_unix, Some(100));
        let account = cache.account_stats(1).expect("account stats").expect("account");
        assert_eq!(account.message_count, 2);
        assert_eq!(account.sync_state_count, 1);
        assert_eq!(account.complete_dialog_count, 1);
        assert_eq!(account.tombstone_count, 1);
        assert_eq!(account.last_checkpoint_unix, Some(100));
    }

    #[test]
    fn clear_increments_generation_and_preserves_unrelated_accounts() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        bind(&cache, 2);
        cache
            .upsert_message(1, &cached("chat", summary(1, "one", 10, Some("alice"))))
            .expect("account one");
        cache
            .upsert_message(2, &cached("chat", summary(1, "two", 10, Some("bob"))))
            .expect("account two");

        let cleared = cache.clear_account(1, 100).expect("clear");
        assert_eq!(cleared.previous_generation, 0);
        assert_eq!(cleared.generation, 1);
        assert_eq!(cleared.deleted_messages, 1);
        assert_eq!(cache.account_generation(1).expect("generation"), Some(1));
        assert!(cache.exact_get(1, "chat", 1).expect("account one get").is_none());
        assert_eq!(
            cache
                .exact_get(2, "chat", 1)
                .expect("account two get")
                .expect("account two message")
                .message
                .text
                .as_deref(),
            Some("two")
        );
        assert!(matches!(
            cache.require_generation(1, 0),
            Err(MessageCacheError::StaleGeneration { expected: 0, current: 1 })
        ));
    }

    #[test]
    fn stale_async_writes_cannot_repopulate_or_regress_a_cleared_account() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        let pre_clear_cursor = GlobalUpdateCursor {
            pts: 10,
            qts: 0,
            date: 10,
            seq: 10,
            unread_count: Some(1),
            phase: CacheSyncPhase::Current,
            observed_at_unix: 10,
            last_error_code: None,
        };
        cache.set_global_update_cursor(1, &pre_clear_cursor).expect("seed cursor");
        let (generation, revision, frontier, checkpoint) =
            cache.dialog_sync_state(1, "chat").expect("capture pre-fetch dialog state");
        assert_eq!(generation, 0);
        let successor_cursor = GlobalUpdateCursor {
            pts: 20,
            qts: 0,
            date: 20,
            seq: 20,
            unread_count: Some(0),
            phase: CacheSyncPhase::Current,
            observed_at_unix: 20,
            last_error_code: None,
        };
        cache
            .clear_account_with_cursor_if_generation(1, generation, 20, Some(&successor_cursor))
            .expect("clear wins");

        let late_message = cached("chat", summary(1, "late", 10, Some("alice")));
        assert!(matches!(
            cache.upsert_messages_if_generation(1, generation, std::slice::from_ref(&late_message)),
            Err(MessageCacheError::StaleGeneration { expected: 0, current: 1 })
        ));
        let late_checkpoint = DialogCheckpoint {
            chat_id: "chat".to_string(),
            frontier_message_id: Some(1),
            oldest_message_id: Some(1),
            history_complete: false,
            observed_at_unix: 21,
        };
        assert!(matches!(
            cache.apply_dialog_batch_if_state(
                1,
                generation,
                revision,
                frontier,
                checkpoint.as_ref(),
                None,
                std::slice::from_ref(&late_message),
                &late_checkpoint,
            ),
            Err(MessageCacheError::StaleGeneration { expected: 0, current: 1 })
        ));
        let late_batch = CacheUpdateBatch {
            dialogs: Vec::new(),
            messages: vec![late_message],
            incoming_events: vec![IncomingMessageEventInput {
                chat_id: "101".to_string(),
                sender_user_id: 101,
                sender_name: "Alex".to_string(),
                sender_username: None,
                message_id: 1,
                message_date_unix: 10,
                observed_at_unix_ms: 10_000,
            }],
            channel_tombstones: Vec::new(),
            global_deleted_message_ids: Vec::new(),
            cursor: GlobalUpdateCursor {
                pts: 11,
                qts: 0,
                date: 11,
                seq: 11,
                unread_count: Some(1),
                phase: CacheSyncPhase::Current,
                observed_at_unix: 21,
                last_error_code: None,
            },
        };
        assert!(matches!(
            cache.apply_update_batch_if_state(1, generation, &pre_clear_cursor, &late_batch),
            Err(MessageCacheError::StaleGeneration { expected: 0, current: 1 })
        ));
        assert!(matches!(
            cache.set_global_update_cursor_if_state(
                1,
                generation,
                Some(&pre_clear_cursor),
                &late_batch.cursor,
            ),
            Err(MessageCacheError::StaleGeneration { expected: 0, current: 1 })
        ));

        let stats = cache.account_stats(1).expect("stats").expect("account");
        assert_eq!(stats.generation, 1);
        assert_eq!(stats.message_count, 0);
        assert_eq!(stats.dialog_count, 0);
        assert_eq!(stats.incoming_event_count, 0);
        assert_eq!(cache.global_update_cursor(1).expect("cursor"), Some(successor_cursor));
    }

    #[test]
    fn resync_reset_rejects_a_cursor_changed_after_its_snapshot_without_clearing() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        cache
            .upsert_message(1, &cached("chat", summary(1, "preserve", 10, Some("alice"))))
            .expect("seed message");
        let expected_cursor = GlobalUpdateCursor {
            pts: 10,
            qts: 0,
            date: 10,
            seq: 10,
            unread_count: Some(1),
            phase: CacheSyncPhase::ResyncRequired,
            observed_at_unix: 10,
            last_error_code: Some("CACHE_RESYNC_REQUIRED".to_string()),
        };
        cache.set_global_update_cursor(1, &expected_cursor).expect("seed resync cursor");
        let mut concurrent_cursor = expected_cursor.clone();
        concurrent_cursor.pts = 11;
        concurrent_cursor.phase = CacheSyncPhase::CatchingUp;
        concurrent_cursor.last_error_code = None;
        cache
            .set_global_update_cursor_if_state(1, 0, Some(&expected_cursor), &concurrent_cursor)
            .expect("advance cursor concurrently");
        let mut successor = concurrent_cursor.clone();
        successor.pts = 20;

        assert!(matches!(
            cache.clear_account_with_cursor_if_state(
                1,
                0,
                Some(&expected_cursor),
                20,
                Some(&successor),
            ),
            Err(MessageCacheError::StaleUpdateCursor)
        ));
        assert_eq!(cache.account_generation(1).expect("generation"), Some(0));
        assert!(cache.exact_get(1, "chat", 1).expect("preserved row").is_some());
        assert_eq!(cache.global_update_cursor(1).expect("cursor"), Some(concurrent_cursor));
    }

    #[test]
    fn async_batches_compare_the_exact_input_cursor_even_within_one_generation() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        let cursor = GlobalUpdateCursor {
            pts: 10,
            qts: 0,
            date: 10,
            seq: 10,
            unread_count: Some(0),
            phase: CacheSyncPhase::Current,
            observed_at_unix: 10,
            last_error_code: None,
        };
        cache.set_global_update_cursor(1, &cursor).expect("seed cursor");
        let mut wrong_cursor = cursor.clone();
        wrong_cursor.pts = 9;
        let batch = CacheUpdateBatch {
            dialogs: Vec::new(),
            messages: Vec::new(),
            incoming_events: Vec::new(),
            channel_tombstones: Vec::new(),
            global_deleted_message_ids: Vec::new(),
            cursor: cursor.clone(),
        };
        assert!(matches!(
            cache.apply_update_batch_if_state(1, 0, &wrong_cursor, &batch),
            Err(MessageCacheError::StaleUpdateCursor)
        ));

        let (_generation, revision, frontier, checkpoint) =
            cache.dialog_sync_state(1, "chat").expect("dialog state");
        cache
            .upsert_message(1, &cached("chat", summary(1, "concurrent", 1, Some("alice"))))
            .expect("concurrent cache write");
        let replacement_checkpoint = DialogCheckpoint {
            chat_id: "chat".to_string(),
            frontier_message_id: Some(2),
            oldest_message_id: Some(2),
            history_complete: false,
            observed_at_unix: 2,
        };
        assert!(matches!(
            cache.apply_dialog_batch_if_state(
                1,
                0,
                revision,
                frontier,
                checkpoint.as_ref(),
                None,
                &[cached("chat", summary(2, "late history", 2, Some("bob")))],
                &replacement_checkpoint,
            ),
            Err(MessageCacheError::StaleDialogCursor { .. })
        ));
        assert!(cache.exact_get(1, "chat", 2).expect("message 2").is_none());
    }

    #[test]
    fn same_generation_read_snapshot_cannot_overwrite_live_edit_or_deletion() {
        let (_temp, cache) = cache();
        bind(&cache, 1);
        cache
            .upsert_message(1, &cached("chat", summary(8, "before", 100, Some("alice"))))
            .expect("seed message");
        let (generation, stale_edit_revision, _, _) =
            cache.dialog_sync_state(1, "chat").expect("capture read snapshot");

        let mut edited = summary(8, "after", 100, Some("alice"));
        edited.edit_date = Some(150);
        cache
            .apply_update_batch(
                1,
                &CacheUpdateBatch {
                    dialogs: Vec::new(),
                    messages: vec![CachedMessageInput {
                        chat_id: "chat".to_string(),
                        message: edited,
                        observed_at_unix: 151,
                        source: MessageSource::LiveUpdate,
                    }],
                    incoming_events: Vec::new(),
                    channel_tombstones: Vec::new(),
                    global_deleted_message_ids: Vec::new(),
                    cursor: GlobalUpdateCursor {
                        pts: 2,
                        qts: 0,
                        date: 151,
                        seq: 2,
                        unread_count: Some(0),
                        phase: CacheSyncPhase::Current,
                        observed_at_unix: 151,
                        last_error_code: None,
                    },
                },
            )
            .expect("apply live edit");
        let stale_before = cached("chat", summary(8, "before", 100, Some("alice")));
        assert!(matches!(
            cache.upsert_messages_if_state(
                1,
                generation,
                "chat",
                stale_edit_revision,
                std::slice::from_ref(&stale_before),
            ),
            Err(MessageCacheError::StaleDialogCursor { .. })
        ));
        assert_eq!(
            cache
                .exact_get(1, "chat", 8)
                .expect("get edited")
                .expect("edited message")
                .message
                .text
                .as_deref(),
            Some("after")
        );

        let (_, stale_delete_revision, _, _) =
            cache.dialog_sync_state(1, "chat").expect("capture delete race snapshot");
        cache
            .apply_update_batch(
                1,
                &CacheUpdateBatch {
                    dialogs: Vec::new(),
                    messages: Vec::new(),
                    incoming_events: Vec::new(),
                    channel_tombstones: vec![MessageTombstone {
                        chat_id: "chat".to_string(),
                        message_id: 8,
                        deleted_at_unix: 160,
                        source: MessageSource::LiveUpdate,
                    }],
                    global_deleted_message_ids: Vec::new(),
                    cursor: GlobalUpdateCursor {
                        pts: 3,
                        qts: 0,
                        date: 160,
                        seq: 3,
                        unread_count: Some(0),
                        phase: CacheSyncPhase::Current,
                        observed_at_unix: 160,
                        last_error_code: None,
                    },
                },
            )
            .expect("apply live deletion");
        assert!(matches!(
            cache.upsert_messages_if_state(
                1,
                generation,
                "chat",
                stale_delete_revision,
                std::slice::from_ref(&stale_before),
            ),
            Err(MessageCacheError::StaleDialogCursor { .. })
        ));
        let current_revision = cache.chat_revision(1, "chat").expect("current revision");
        cache
            .upsert_messages_if_state(
                1,
                generation,
                "chat",
                current_revision,
                std::slice::from_ref(&stale_before),
            )
            .expect("post-delete stale provider row is ignored by tombstone");
        assert!(cache.exact_get(1, "chat", 8).expect("deleted get").is_none());
        assert_eq!(cache.chat_revision(1, "chat").expect("unchanged revision"), current_revision);
    }

    #[test]
    fn schema_has_only_typed_message_json_and_no_raw_provider_or_secret_columns() {
        let (_temp, cache) = cache();
        let connection = Connection::open(cache.path()).expect("raw open");
        let mut statement =
            connection.prepare("PRAGMA table_info(cache_messages)").expect("table info");
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("columns")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("column names");
        assert_eq!(
            columns
                .iter()
                .filter(|column| column.ends_with("_json"))
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["message_json"]
        );
        for forbidden in
            ["raw_json", "session_path", "access_hash", "api_hash", "proxy", "bot_token", "invite"]
        {
            assert!(!columns.iter().any(|column| column == forbidden));
        }
    }
}
