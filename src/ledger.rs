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

use crate::domain::{
    PreparedOutboundSource, PreparedSend, PreparedSendKind, PreparedSendState, PreparedSendTarget,
    SendApproval, SendApprovalState,
};
use crate::fs_security::{restrict_private_directory, restrict_private_file};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PreparedSendRecord {
    pub public: PreparedSend,
    pub random_id: i64,
}

#[derive(Debug, Clone)]
pub struct ApprovalRequestRecord {
    pub public: SendApproval,
    pub should_dispatch_card: bool,
}

#[derive(Debug, Clone)]
pub struct SendLedger {
    path: PathBuf,
}

impl SendLedger {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &self,
        account_user_id: i64,
        chat_id: String,
        target: PreparedSendTarget,
        source: PreparedOutboundSource,
        text: String,
        reply_to_message_id: Option<i32>,
        payload_sha256: String,
    ) -> rusqlite::Result<PreparedSendRecord> {
        self.prepare_with_kind(
            account_user_id,
            chat_id,
            target,
            source,
            PreparedSendKind::PlainText,
            text,
            reply_to_message_id,
            payload_sha256,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_rich_markdown(
        &self,
        account_user_id: i64,
        chat_id: String,
        target: PreparedSendTarget,
        source: PreparedOutboundSource,
        markdown: String,
        reply_to_message_id: Option<i32>,
        payload_sha256: String,
    ) -> rusqlite::Result<PreparedSendRecord> {
        self.prepare_with_kind(
            account_user_id,
            chat_id,
            target,
            source,
            PreparedSendKind::RichMarkdown,
            markdown,
            reply_to_message_id,
            payload_sha256,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_with_kind(
        &self,
        account_user_id: i64,
        chat_id: String,
        target: PreparedSendTarget,
        source: PreparedOutboundSource,
        kind: PreparedSendKind,
        text: String,
        reply_to_message_id: Option<i32>,
        payload_sha256: String,
    ) -> rusqlite::Result<PreparedSendRecord> {
        let connection = self.connection()?;
        let draft_id = Uuid::new_v4().to_string();
        let uuid = Uuid::new_v4();
        let mut random_bytes = [0_u8; 8];
        random_bytes.copy_from_slice(&uuid.as_bytes()[..8]);
        let random_id = (i64::from_le_bytes(random_bytes) & i64::MAX).max(1);
        let created_at = now_unix();
        let source_json = serde_json::to_string(&source)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        connection.execute(
            "INSERT INTO prepared_sends (
                draft_id, account_user_id, chat_id, target_display_name, target_username,
                target_kind, source_json, content_kind, text,
                reply_to_message_id, payload_sha256, random_id, state,
                message_id, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'prepared', NULL, ?13)",
            params![
                draft_id,
                account_user_id,
                chat_id,
                target.display_name,
                target.username,
                target.kind,
                source_json,
                prepared_send_kind_name(kind),
                text,
                reply_to_message_id,
                payload_sha256,
                random_id,
                created_at,
            ],
        )?;
        self.get(&draft_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn get(&self, draft_id: &str) -> rusqlite::Result<Option<PreparedSendRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT account_user_id, chat_id, target_display_name, target_username,
                        target_kind, source_json, content_kind, text, reply_to_message_id,
                        payload_sha256, random_id, state, message_id
                 FROM prepared_sends WHERE draft_id = ?1",
                [draft_id],
                |row| {
                    let chat_id: String = row.get(1)?;
                    let source_json: Option<String> = row.get(5)?;
                    let kind: String = row.get(6)?;
                    let state: String = row.get(11)?;
                    Ok(PreparedSendRecord {
                        public: PreparedSend {
                            draft_id: draft_id.to_string(),
                            account_user_id: row.get(0)?,
                            chat_id: chat_id.clone(),
                            target: PreparedSendTarget {
                                display_name: row
                                    .get::<_, Option<String>>(2)?
                                    .filter(|value| !value.trim().is_empty())
                                    .unwrap_or_else(|| chat_id.clone()),
                                username: row.get(3)?,
                                kind: row
                                    .get::<_, Option<String>>(4)?
                                    .filter(|value| !value.trim().is_empty())
                                    .unwrap_or_else(|| "unknown".to_string()),
                            },
                            source: parse_prepared_source(source_json.as_deref())?,
                            kind: parse_prepared_send_kind(&kind)?,
                            text: row.get(7)?,
                            reply_to_message_id: row.get(8)?,
                            payload_sha256: row.get(9)?,
                            state: parse_state(&state)?,
                            message_id: row.get(12)?,
                        },
                        random_id: row.get(10)?,
                    })
                },
            )
            .optional()
    }

    pub fn mark_dispatching(&self, draft_id: &str) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE prepared_sends
             SET state = 'dispatching'
             WHERE draft_id = ?1
               AND state IN ('prepared', 'dispatching', 'delivery_uncertain')",
            [draft_id],
        )?;
        self.require_existing_or_changed(&connection, draft_id, changed)
    }

    pub fn mark_sent(&self, draft_id: &str, message_id: i32) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE prepared_sends
             SET state = 'sent', message_id = ?2
             WHERE draft_id = ?1
               AND (state != 'sent' OR message_id = ?2)",
            params![draft_id, message_id],
        )?;
        if changed > 0 {
            return Ok(());
        }
        let existing = connection
            .query_row(
                "SELECT state, message_id FROM prepared_sends WHERE draft_id = ?1",
                [draft_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i32>>(1)?)),
            )
            .optional()?;
        match existing {
            None => Err(rusqlite::Error::QueryReturnedNoRows),
            Some((_, Some(existing))) if existing == message_id => Ok(()),
            Some((state, Some(existing))) => Err(state_error(format!(
                "prepared send {draft_id} cannot transition {state} → sent with Telegram message {message_id}: already bound to message {existing}"
            ))),
            Some((state, None)) => Err(state_error(format!(
                "prepared send {draft_id} cannot transition {state} → sent with Telegram message {message_id}: no receipt recorded"
            ))),
        }
    }

    pub fn mark_uncertain(&self, draft_id: &str) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE prepared_sends
             SET state = 'delivery_uncertain'
             WHERE draft_id = ?1 AND state != 'sent'",
            [draft_id],
        )?;
        self.require_existing_or_changed(&connection, draft_id, changed)
    }

    pub fn mark_prepared(&self, draft_id: &str) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE prepared_sends
             SET state = 'prepared', message_id = NULL
             WHERE draft_id = ?1 AND state = 'dispatching'",
            [draft_id],
        )?;
        self.require_existing_or_changed(&connection, draft_id, changed)
    }

    pub fn create_or_get_approval(
        &self,
        draft_id: &str,
        payload_sha256: &str,
        approval_chat_id: i64,
        approval_user_id: i64,
        ttl_seconds: u64,
    ) -> rusqlite::Result<ApprovalRequestRecord> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = now_unix();
        expire_due_approvals(&transaction, now)?;
        if let Some(existing) = get_latest_reusable_approval(&transaction, draft_id)? {
            transaction.commit()?;
            return Ok(ApprovalRequestRecord { public: existing, should_dispatch_card: false });
        }

        let approval_id = Uuid::new_v4().to_string();
        let ttl = i64::try_from(ttl_seconds).unwrap_or(i64::MAX);
        let expires_at = now.saturating_add(ttl);
        let insert = transaction.execute(
            "INSERT INTO send_approvals (
                approval_id, draft_id, payload_sha256, state,
                approval_chat_id, approval_user_id, bot_message_id,
                created_at, expires_at, decided_at, target_message_id,
                automated_retry_count, next_retry_at, last_error_code
             ) VALUES (
                ?1, ?2, ?3, 'dispatching_card', ?4, ?5, NULL,
                ?6, ?7, NULL, NULL, 0, NULL, NULL
             )",
            params![
                approval_id,
                draft_id,
                payload_sha256,
                approval_chat_id,
                approval_user_id,
                now,
                expires_at,
            ],
        );
        if let Err(error) = insert {
            if let Some(existing) = get_latest_reusable_approval(&transaction, draft_id)? {
                transaction.commit()?;
                return Ok(ApprovalRequestRecord { public: existing, should_dispatch_card: false });
            }
            return Err(error);
        }
        let approval = get_approval_from_connection(&transaction, &approval_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        transaction.commit()?;
        Ok(ApprovalRequestRecord { public: approval, should_dispatch_card: true })
    }

    pub fn get_approval(&self, approval_id: &str) -> rusqlite::Result<Option<SendApproval>> {
        let connection = self.connection()?;
        get_approval_from_connection(&connection, approval_id)
    }

    /// A granted send needs no card at all: move it straight from
    /// `dispatching_card` to `pending`, the state the send path accepts, and
    /// leave no bot message behind. Mirrors the mutation ledger so the
    /// owner-grant path reaches `sent` through the honest
    /// `dispatching_card → pending → sending → sent` chain.
    pub fn mark_approval_card_skipped(&self, approval_id: &str) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'pending', last_error_code = NULL
             WHERE approval_id = ?1 AND state = 'dispatching_card'",
            params![approval_id],
        )?;
        if changed > 0 {
            return Ok(());
        }
        let existing =
            self.get_approval(approval_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        match existing.state {
            SendApprovalState::Pending
            | SendApprovalState::Sending
            | SendApprovalState::DeliveryUncertain
            | SendApprovalState::Sent => Ok(()),
            other => Err(state_error(format!(
                "approval {approval_id} cannot skip its card from state {}: only dispatching_card → pending is allowed",
                send_approval_state_name(other)
            ))),
        }
    }

    pub fn mark_approval_card_delivered(
        &self,
        approval_id: &str,
        message_id: i64,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'pending', bot_message_id = ?2, last_error_code = NULL
             WHERE approval_id = ?1 AND state = 'dispatching_card'",
            params![approval_id, message_id],
        )?;
        if changed > 0 {
            return Ok(());
        }
        let existing =
            self.get_approval(approval_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        if existing.state == SendApprovalState::Pending
            && existing.approval_message_id == Some(message_id)
        {
            Ok(())
        } else {
            Err(state_error(format!(
                "approval {approval_id} cannot transition {} → pending with bot message {message_id}: card already bound differently",
                send_approval_state_name(existing.state)
            )))
        }
    }

    pub fn mark_approval_card_uncertain(
        &self,
        approval_id: &str,
        error_code: &str,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'card_delivery_uncertain', last_error_code = ?2
             WHERE approval_id = ?1 AND state = 'dispatching_card'",
            params![approval_id, error_code],
        )?;
        require_approval_existing_or_changed(&connection, approval_id, changed)
    }

    pub fn bind_uncertain_approval_card(
        &self,
        approval_id: &str,
        message_id: i64,
    ) -> rusqlite::Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'pending', bot_message_id = ?2, last_error_code = NULL
             WHERE approval_id = ?1
               AND state = 'card_delivery_uncertain'
               AND bot_message_id IS NULL",
            params![approval_id, message_id],
        )?;
        Ok(changed > 0)
    }

    pub fn mark_approval_failed(
        &self,
        approval_id: &str,
        error_code: &str,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'failed', last_error_code = ?2, next_retry_at = NULL
             WHERE approval_id = ?1
               AND state IN ('dispatching_card', 'card_delivery_uncertain')",
            params![approval_id, error_code],
        )?;
        require_approval_existing_or_changed(&connection, approval_id, changed)
    }

    pub fn mark_approval_sending(&self, approval_id: &str, now: i64) -> rusqlite::Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'sending', decided_at = COALESCE(decided_at, ?2),
                 next_retry_at = ?2, last_error_code = NULL
             WHERE approval_id = ?1
               AND state = 'pending'
               AND expires_at > ?2",
            params![approval_id, now],
        )?;
        Ok(changed > 0)
    }

    pub fn cancel_approval(&self, approval_id: &str, now: i64) -> rusqlite::Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'cancelled', decided_at = COALESCE(decided_at, ?2),
                 next_retry_at = NULL, last_error_code = NULL
             WHERE approval_id = ?1
               AND state IN ('dispatching_card', 'pending', 'card_delivery_uncertain')
               AND expires_at > ?2",
            params![approval_id, now],
        )?;
        Ok(changed > 0)
    }

    pub fn expire_approval(&self, approval_id: &str, now: i64) -> rusqlite::Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'expired', decided_at = COALESCE(decided_at, ?2),
                 next_retry_at = NULL
             WHERE approval_id = ?1
               AND state IN ('dispatching_card', 'pending', 'card_delivery_uncertain')
               AND expires_at <= ?2",
            params![approval_id, now],
        )?;
        Ok(changed > 0)
    }

    pub fn mark_approval_sent(
        &self,
        approval_id: &str,
        target_message_id: i32,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'sent', target_message_id = ?2,
                 next_retry_at = NULL, last_error_code = NULL
             WHERE approval_id = ?1
               AND (
                   state IN ('sending', 'delivery_uncertain')
                   OR (state = 'sent' AND target_message_id = ?2)
               )",
            params![approval_id, target_message_id],
        )?;
        if changed > 0 {
            return Ok(());
        }
        let existing =
            self.get_approval(approval_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        match existing.target_message_id {
            Some(existing_message_id) if existing_message_id == target_message_id => Ok(()),
            Some(existing_message_id) => Err(state_error(format!(
                "approval {approval_id} cannot transition {} → sent with Telegram message {target_message_id}: already bound to message {existing_message_id}",
                send_approval_state_name(existing.state)
            ))),
            None => Err(state_error(format!(
                "approval {approval_id} cannot transition {} → sent with Telegram message {target_message_id}: only sending → sent and delivery_uncertain → sent are allowed",
                send_approval_state_name(existing.state)
            ))),
        }
    }

    pub fn mark_approval_delivery_uncertain(
        &self,
        approval_id: &str,
        error_code: &str,
        next_retry_at: Option<i64>,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'delivery_uncertain',
                 automated_retry_count = automated_retry_count + 1,
                 next_retry_at = ?3, last_error_code = ?2
             WHERE approval_id = ?1 AND state IN ('sending', 'delivery_uncertain')",
            params![approval_id, error_code, next_retry_at],
        )?;
        require_approval_existing_or_changed(&connection, approval_id, changed)
    }

    pub fn mark_approval_retryable_failure(
        &self,
        approval_id: &str,
        error_code: &str,
        next_retry_at: i64,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'sending',
                 automated_retry_count = automated_retry_count + 1,
                 next_retry_at = ?3, last_error_code = ?2
             WHERE approval_id = ?1 AND state IN ('sending', 'delivery_uncertain')",
            params![approval_id, error_code, next_retry_at],
        )?;
        if changed > 0 {
            Ok(())
        } else {
            let current = self
                .get_approval(approval_id)?
                .map(|approval| send_approval_state_name(approval.state).to_string())
                .unwrap_or_else(|| "missing".to_string());
            Err(state_error(format!(
                "approval {approval_id} cannot schedule a send retry from state {current}: only sending and delivery_uncertain can retry"
            )))
        }
    }

    pub fn mark_approval_known_send_failure(
        &self,
        approval_id: &str,
        error_code: &str,
        now: i64,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'failed', decided_at = COALESCE(decided_at, ?3),
                 next_retry_at = NULL, last_error_code = ?2
             WHERE approval_id = ?1 AND state IN ('sending', 'delivery_uncertain')",
            params![approval_id, error_code, now],
        )?;
        require_approval_existing_or_changed(&connection, approval_id, changed)
    }

    pub fn claim_due_recovery(&self, approval_id: &str, now: i64) -> rusqlite::Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE send_approvals
             SET state = 'sending', next_retry_at = ?2
             WHERE approval_id = ?1
               AND state = 'delivery_uncertain'
               AND next_retry_at IS NOT NULL
               AND next_retry_at <= ?2",
            params![approval_id, now],
        )?;
        if changed > 0 {
            return Ok(true);
        }
        let state = connection
            .query_row(
                "SELECT state, next_retry_at FROM send_approvals WHERE approval_id = ?1",
                [approval_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .optional()?;
        Ok(matches!(state, Some((ref state, Some(next))) if state == "sending" && next <= now))
    }

    pub fn due_approval_recoveries(
        &self,
        now: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<SendApproval>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT a.approval_id, a.draft_id, p.account_user_id, p.chat_id,
                    p.target_display_name, p.target_username, p.target_kind,
                    p.source_json, p.content_kind, p.text, p.reply_to_message_id,
                    a.payload_sha256, a.state,
                    a.approval_chat_id, a.approval_user_id, a.bot_message_id,
                    a.created_at, a.expires_at, a.target_message_id,
                    a.automated_retry_count, a.next_retry_at, a.last_error_code
             FROM send_approvals a
             JOIN prepared_sends p ON p.draft_id = a.draft_id
             WHERE a.state IN ('sending', 'delivery_uncertain')
               AND a.next_retry_at IS NOT NULL
               AND a.next_retry_at <= ?1
             ORDER BY a.next_retry_at ASC, a.created_at ASC
             LIMIT ?2",
        )?;
        statement
            .query_map(params![now, i64::try_from(limit).unwrap_or(i64::MAX)], map_approval_row)?
            .collect()
    }

    pub fn expire_due_approval_records(
        &self,
        now: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<SendApproval>> {
        let connection = self.connection()?;
        let approval_ids = {
            let mut statement = connection.prepare(
                "SELECT approval_id
                 FROM send_approvals
                 WHERE state IN (
                     'dispatching_card', 'pending', 'card_delivery_uncertain'
                 )
                   AND expires_at <= ?1
                 ORDER BY expires_at ASC
                 LIMIT ?2",
            )?;
            statement
                .query_map(params![now, i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut expired = Vec::with_capacity(approval_ids.len());
        for approval_id in approval_ids {
            connection.execute(
                "UPDATE send_approvals
                 SET state = 'expired', decided_at = COALESCE(decided_at, ?2),
                     next_retry_at = NULL
                 WHERE approval_id = ?1
                   AND state IN ('dispatching_card', 'pending', 'card_delivery_uncertain')",
                params![approval_id, now],
            )?;
            if let Some(approval) = get_approval_from_connection(&connection, &approval_id)? {
                expired.push(approval);
            }
        }
        Ok(expired)
    }

    pub fn approval_update_offset(&self) -> rusqlite::Result<i64> {
        let connection = self.connection()?;
        connection.query_row(
            "SELECT next_update_id FROM approval_runtime WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
    }

    pub fn advance_approval_update_offset(&self, next_update_id: i64) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE approval_runtime
             SET next_update_id = MAX(next_update_id, ?1)
             WHERE singleton = 1",
            [next_update_id],
        )?;
        Ok(())
    }

    fn require_existing_or_changed(
        &self,
        connection: &Connection,
        draft_id: &str,
        changed: usize,
    ) -> rusqlite::Result<()> {
        if changed > 0 {
            return Ok(());
        }
        let exists = connection
            .query_row("SELECT 1 FROM prepared_sends WHERE draft_id = ?1", [draft_id], |_| Ok(()))
            .optional()?
            .is_some();
        exists.then_some(()).ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    fn connection(&self) -> rusqlite::Result<Connection> {
        open_state_connection(&self.path)
    }
}

/// One reusable state connection for a polling loop.
///
/// [`SendLedger`] is only a path handle: every method opens a fresh connection
/// and runs the schema bootstrap, so a poll loop that re-reads through a new
/// `SendLedger` pays for a connection and a bootstrap on every tick. Open a
/// reader once and reuse it for the whole cycle.
pub struct SendLedgerReader {
    connection: Connection,
}

impl SendLedgerReader {
    pub fn get_approval(&self, approval_id: &str) -> rusqlite::Result<Option<SendApproval>> {
        get_approval_from_connection(&self.connection, approval_id)
    }
}

impl SendLedger {
    /// Open one reusable read connection for the rest of a polling cycle.
    pub fn reader(&self) -> rusqlite::Result<SendLedgerReader> {
        Ok(SendLedgerReader { connection: self.connection()? })
    }
}

pub(crate) fn open_state_connection(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        let create_private_parent = !parent.exists();
        std::fs::create_dir_all(parent)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        if create_private_parent {
            restrict_private_directory(parent)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        }
    }
    let connection = Connection::open(path)?;
    restrict_private_file(path)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS prepared_sends (
                 draft_id TEXT PRIMARY KEY,
                 account_user_id INTEGER NOT NULL,
                 chat_id TEXT NOT NULL,
                 target_display_name TEXT,
                 target_username TEXT,
                 target_kind TEXT,
                 source_json TEXT,
                 content_kind TEXT NOT NULL DEFAULT 'plain_text' CHECK (
                     content_kind IN ('plain_text', 'rich_markdown')
                 ),
                 text TEXT NOT NULL,
                 reply_to_message_id INTEGER,
                 payload_sha256 TEXT NOT NULL,
                 random_id INTEGER NOT NULL,
                 state TEXT NOT NULL CHECK (
                     state IN ('prepared', 'dispatching', 'sent', 'delivery_uncertain')
                 ),
                 message_id INTEGER,
                 created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS send_approvals (
                 approval_id TEXT PRIMARY KEY,
                 draft_id TEXT NOT NULL REFERENCES prepared_sends(draft_id),
                 payload_sha256 TEXT NOT NULL,
                 state TEXT NOT NULL CHECK (
                     state IN (
                         'dispatching_card', 'pending', 'card_delivery_uncertain',
                         'sending', 'delivery_uncertain', 'sent', 'cancelled',
                         'expired', 'failed'
                     )
                 ),
                 approval_chat_id INTEGER NOT NULL,
                 approval_user_id INTEGER NOT NULL,
                 bot_message_id INTEGER,
                 created_at INTEGER NOT NULL,
                 expires_at INTEGER NOT NULL,
                 decided_at INTEGER,
                 target_message_id INTEGER,
                 automated_retry_count INTEGER NOT NULL DEFAULT 0,
                 next_retry_at INTEGER,
                 last_error_code TEXT
             );
             CREATE UNIQUE INDEX IF NOT EXISTS send_approvals_one_active_per_draft
             ON send_approvals(draft_id)
             WHERE state IN (
                 'dispatching_card', 'pending', 'card_delivery_uncertain',
                 'sending', 'delivery_uncertain'
             );
             CREATE TABLE IF NOT EXISTS approval_runtime (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                 next_update_id INTEGER NOT NULL
             );",
    )?;
    // Seed the runtime row only when it is missing. An unconditional
    // `INSERT OR IGNORE` starts a write transaction on *every* open, so every
    // connection (worker tick, tool call, poll loop) contended for SQLite's
    // write lock and a stalled host surfaced it as "database is locked".
    let runtime_row_present = connection
        .query_row("SELECT 1 FROM approval_runtime WHERE singleton = 1", [], |_| Ok(()))
        .optional()?
        .is_some();
    if !runtime_row_present {
        connection.execute(
            "INSERT INTO approval_runtime (singleton, next_update_id) VALUES (1, 0)",
            [],
        )?;
    }
    remove_legacy_session_path_column(&connection)?;
    ensure_prepared_send_kind_column(&connection)?;
    ensure_prepared_send_target_columns(&connection)?;
    ensure_prepared_send_source_column(&connection)?;
    for sidecar in sqlite_sidecars(path) {
        if sidecar.exists() {
            restrict_private_file(&sidecar)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        }
    }
    Ok(connection)
}

fn get_approval_from_connection(
    connection: &Connection,
    approval_id: &str,
) -> rusqlite::Result<Option<SendApproval>> {
    connection
        .query_row(
            "SELECT a.approval_id, a.draft_id, p.account_user_id, p.chat_id,
                    p.target_display_name, p.target_username, p.target_kind,
                    p.source_json, p.content_kind, p.text, p.reply_to_message_id,
                    a.payload_sha256, a.state,
                    a.approval_chat_id, a.approval_user_id, a.bot_message_id,
                    a.created_at, a.expires_at, a.target_message_id,
                    a.automated_retry_count, a.next_retry_at, a.last_error_code
             FROM send_approvals a
             JOIN prepared_sends p ON p.draft_id = a.draft_id
             WHERE a.approval_id = ?1",
            [approval_id],
            map_approval_row,
        )
        .optional()
}

fn get_latest_reusable_approval(
    connection: &Connection,
    draft_id: &str,
) -> rusqlite::Result<Option<SendApproval>> {
    connection
        .query_row(
            "SELECT a.approval_id, a.draft_id, p.account_user_id, p.chat_id,
                    p.target_display_name, p.target_username, p.target_kind,
                    p.source_json, p.content_kind, p.text, p.reply_to_message_id,
                    a.payload_sha256, a.state,
                    a.approval_chat_id, a.approval_user_id, a.bot_message_id,
                    a.created_at, a.expires_at, a.target_message_id,
                    a.automated_retry_count, a.next_retry_at, a.last_error_code
             FROM send_approvals a
             JOIN prepared_sends p ON p.draft_id = a.draft_id
             WHERE a.draft_id = ?1
               AND a.state IN (
                   'dispatching_card', 'pending', 'card_delivery_uncertain',
                   'sending', 'delivery_uncertain', 'sent'
               )
             ORDER BY a.created_at DESC
             LIMIT 1",
            [draft_id],
            map_approval_row,
        )
        .optional()
}

fn map_approval_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SendApproval> {
    let chat_id: String = row.get(3)?;
    let source_json: Option<String> = row.get(7)?;
    let kind: String = row.get(8)?;
    let state: String = row.get(12)?;
    let retry_count: i64 = row.get(19)?;
    Ok(SendApproval {
        approval_id: row.get(0)?,
        draft_id: row.get(1)?,
        account_user_id: row.get(2)?,
        chat_id: chat_id.clone(),
        target: PreparedSendTarget {
            display_name: row
                .get::<_, Option<String>>(4)?
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| chat_id.clone()),
            username: row.get(5)?,
            kind: row
                .get::<_, Option<String>>(6)?
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "unknown".to_string()),
        },
        source: parse_prepared_source(source_json.as_deref())?,
        kind: parse_prepared_send_kind(&kind)?,
        text: row.get(9)?,
        reply_to_message_id: row.get(10)?,
        payload_sha256: row.get(11)?,
        state: parse_approval_state(&state)?,
        approval_chat_id: row.get(13)?,
        approval_user_id: row.get(14)?,
        approval_message_id: row.get(15)?,
        created_at_unix: row.get(16)?,
        expires_at_unix: row.get(17)?,
        target_message_id: row.get(18)?,
        automated_retry_count: u32::try_from(retry_count)
            .map_err(|_| state_error(format!("invalid approval retry count: {retry_count}")))?,
        next_retry_at_unix: row.get(20)?,
        last_error_code: row.get(21)?,
    })
}

fn expire_due_approvals(connection: &Connection, now: i64) -> rusqlite::Result<()> {
    connection.execute(
        "UPDATE send_approvals
         SET state = 'expired', decided_at = COALESCE(decided_at, ?1),
             next_retry_at = NULL
         WHERE state IN (
             'dispatching_card', 'pending', 'card_delivery_uncertain'
         )
           AND expires_at <= ?1",
        [now],
    )?;
    Ok(())
}

fn require_approval_existing_or_changed(
    connection: &Connection,
    approval_id: &str,
    changed: usize,
) -> rusqlite::Result<()> {
    if changed > 0 {
        return Ok(());
    }
    let exists = connection
        .query_row("SELECT 1 FROM send_approvals WHERE approval_id = ?1", [approval_id], |_| Ok(()))
        .optional()?
        .is_some();
    exists.then_some(()).ok_or(rusqlite::Error::QueryReturnedNoRows)
}

fn now_unix() -> i64 {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
        .unwrap_or(i64::MAX)
}

fn remove_legacy_session_path_column(connection: &Connection) -> rusqlite::Result<()> {
    let has_legacy_column = {
        let mut statement = connection.prepare("PRAGMA table_info(prepared_sends)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        let mut found = false;
        for column in columns {
            if column? == "session_path" {
                found = true;
                break;
            }
        }
        found
    };
    if has_legacy_column {
        connection.execute("ALTER TABLE prepared_sends DROP COLUMN session_path", [])?;
    }
    Ok(())
}

fn ensure_prepared_send_kind_column(connection: &Connection) -> rusqlite::Result<()> {
    let has_kind_column = {
        let mut statement = connection.prepare("PRAGMA table_info(prepared_sends)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        let mut found = false;
        for column in columns {
            if column? == "content_kind" {
                found = true;
                break;
            }
        }
        found
    };
    if !has_kind_column {
        connection.execute(
            "ALTER TABLE prepared_sends
             ADD COLUMN content_kind TEXT NOT NULL DEFAULT 'plain_text'
             CHECK (content_kind IN ('plain_text', 'rich_markdown'))",
            [],
        )?;
    }
    Ok(())
}

fn ensure_prepared_send_target_columns(connection: &Connection) -> rusqlite::Result<()> {
    let columns = {
        let mut statement = connection.prepare("PRAGMA table_info(prepared_sends)")?;
        statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (column, sql) in [
        ("target_display_name", "ALTER TABLE prepared_sends ADD COLUMN target_display_name TEXT"),
        ("target_username", "ALTER TABLE prepared_sends ADD COLUMN target_username TEXT"),
        ("target_kind", "ALTER TABLE prepared_sends ADD COLUMN target_kind TEXT"),
    ] {
        if !columns.iter().any(|existing| existing == column) {
            connection.execute(sql, [])?;
        }
    }
    Ok(())
}

fn ensure_prepared_send_source_column(connection: &Connection) -> rusqlite::Result<()> {
    let has_source_column = {
        let mut statement = connection.prepare("PRAGMA table_info(prepared_sends)")?;
        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
        let mut found = false;
        for column in columns {
            if column? == "source_json" {
                found = true;
                break;
            }
        }
        found
    };
    if !has_source_column {
        connection.execute("ALTER TABLE prepared_sends ADD COLUMN source_json TEXT", [])?;
    }
    Ok(())
}

fn sqlite_sidecars(path: &Path) -> [PathBuf; 2] {
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    let mut shared_memory = path.as_os_str().to_os_string();
    shared_memory.push("-shm");
    [PathBuf::from(wal), PathBuf::from(shared_memory)]
}

fn state_error(message: String) -> rusqlite::Error {
    // Business-logic transition refusals must never look like type
    // conversions: `SqliteFailure(_, Some(_))` renders the message verbatim,
    // while `FromSqlConversionFailure` would prefix it with
    // "Conversion error from type Text at index: 0,".
    rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(1), Some(message))
}

fn send_approval_state_name(state: SendApprovalState) -> &'static str {
    match state {
        SendApprovalState::DispatchingCard => "dispatching_card",
        SendApprovalState::Pending => "pending",
        SendApprovalState::CardDeliveryUncertain => "card_delivery_uncertain",
        SendApprovalState::Sending => "sending",
        SendApprovalState::DeliveryUncertain => "delivery_uncertain",
        SendApprovalState::Sent => "sent",
        SendApprovalState::Cancelled => "cancelled",
        SendApprovalState::Expired => "expired",
        SendApprovalState::Failed => "failed",
    }
}

fn parse_prepared_source(value: Option<&str>) -> rusqlite::Result<Option<PreparedOutboundSource>> {
    value
        .map(|value| {
            serde_json::from_str(value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn parse_state(value: &str) -> rusqlite::Result<PreparedSendState> {
    match value {
        "prepared" => Ok(PreparedSendState::Prepared),
        "dispatching" => Ok(PreparedSendState::Dispatching),
        "sent" => Ok(PreparedSendState::Sent),
        "delivery_uncertain" => Ok(PreparedSendState::DeliveryUncertain),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            7,
            rusqlite::types::Type::Text,
            format!("unknown prepared send state: {other}").into(),
        )),
    }
}

fn prepared_send_kind_name(kind: PreparedSendKind) -> &'static str {
    match kind {
        PreparedSendKind::PlainText => "plain_text",
        PreparedSendKind::RichMarkdown => "rich_markdown",
    }
}

fn parse_prepared_send_kind(value: &str) -> rusqlite::Result<PreparedSendKind> {
    match value {
        "plain_text" => Ok(PreparedSendKind::PlainText),
        "rich_markdown" => Ok(PreparedSendKind::RichMarkdown),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            format!("unknown prepared send kind: {other}").into(),
        )),
    }
}

fn parse_approval_state(value: &str) -> rusqlite::Result<SendApprovalState> {
    match value {
        "dispatching_card" => Ok(SendApprovalState::DispatchingCard),
        "pending" => Ok(SendApprovalState::Pending),
        "card_delivery_uncertain" => Ok(SendApprovalState::CardDeliveryUncertain),
        "sending" => Ok(SendApprovalState::Sending),
        "delivery_uncertain" => Ok(SendApprovalState::DeliveryUncertain),
        "sent" => Ok(SendApprovalState::Sent),
        "cancelled" => Ok(SendApprovalState::Cancelled),
        "expired" => Ok(SendApprovalState::Expired),
        "failed" => Ok(SendApprovalState::Failed),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            7,
            rusqlite::types::Type::Text,
            format!("unknown send approval state: {other}").into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn target(display_name: &str) -> PreparedSendTarget {
        PreparedSendTarget {
            display_name: display_name.to_string(),
            username: Some("alex".to_string()),
            kind: "user".to_string(),
        }
    }

    fn self_originated_source() -> PreparedOutboundSource {
        PreparedOutboundSource::NonTelegram(
            crate::domain::PreparedNonTelegramSource::SelfOriginated,
        )
    }

    #[test]
    fn prepared_send_survives_reopen_and_state_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        let ledger = SendLedger::new(path.clone());
        let prepared = ledger
            .prepare(
                7,
                "@alex".to_string(),
                target("Alex"),
                self_originated_source(),
                "Hello".to_string(),
                Some(11),
                "abc".to_string(),
            )
            .expect("prepare");
        assert_eq!(prepared.public.state, PreparedSendState::Prepared);

        ledger.mark_dispatching(&prepared.public.draft_id).expect("dispatching");
        let reopened = SendLedger::new(path);
        let record = reopened.get(&prepared.public.draft_id).expect("get").expect("record");
        assert_eq!(record.public.state, PreparedSendState::Dispatching);
        assert_eq!(record.random_id, prepared.random_id);

        reopened.mark_sent(&prepared.public.draft_id, 42).expect("sent");
        let sent = reopened.get(&prepared.public.draft_id).expect("get").expect("record");
        assert_eq!(sent.public.state, PreparedSendState::Sent);
        assert_eq!(sent.public.message_id, Some(42));
    }

    #[test]
    fn sent_state_is_terminal_and_receipt_cannot_change() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = SendLedger::new(temp.path().join("state.sqlite"));
        let prepared = ledger
            .prepare(
                7,
                "@alex".into(),
                target("Alex"),
                self_originated_source(),
                "Hello".into(),
                None,
                "abc".into(),
            )
            .expect("prepare");

        ledger.mark_dispatching(&prepared.public.draft_id).expect("dispatching");
        ledger.mark_sent(&prepared.public.draft_id, 42).expect("sent");
        ledger.mark_prepared(&prepared.public.draft_id).expect("no regression");
        ledger.mark_uncertain(&prepared.public.draft_id).expect("no regression");
        ledger.mark_dispatching(&prepared.public.draft_id).expect("no regression");
        ledger.mark_sent(&prepared.public.draft_id, 42).expect("same receipt is idempotent");
        assert!(ledger.mark_sent(&prepared.public.draft_id, 43).is_err());

        let record = ledger.get(&prepared.public.draft_id).expect("get").expect("record");
        assert_eq!(record.public.state, PreparedSendState::Sent);
        assert_eq!(record.public.message_id, Some(42));
    }

    #[test]
    fn uncertain_state_is_not_downgraded_by_a_late_known_failure() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = SendLedger::new(temp.path().join("state.sqlite"));
        let prepared = ledger
            .prepare(
                7,
                "@alex".into(),
                target("Alex"),
                self_originated_source(),
                "Hello".into(),
                None,
                "abc".into(),
            )
            .expect("prepare");

        ledger.mark_dispatching(&prepared.public.draft_id).expect("dispatching");
        ledger.mark_uncertain(&prepared.public.draft_id).expect("uncertain");
        ledger.mark_prepared(&prepared.public.draft_id).expect("late failure cannot downgrade");

        let record = ledger.get(&prepared.public.draft_id).expect("get").expect("record");
        assert_eq!(record.public.state, PreparedSendState::DeliveryUncertain);
    }

    #[test]
    fn approval_is_idempotent_persistent_and_terminal_after_send() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        let ledger = SendLedger::new(path.clone());
        let prepared = ledger
            .prepare(
                7,
                "101".into(),
                target("Alex"),
                self_originated_source(),
                "Hello".into(),
                Some(11),
                "abc".into(),
            )
            .expect("prepare draft");

        let first = ledger
            .create_or_get_approval(&prepared.public.draft_id, "abc", 700, 700, 3_600)
            .expect("create approval");
        assert!(first.should_dispatch_card);
        let second = ledger
            .create_or_get_approval(&prepared.public.draft_id, "abc", 700, 700, 3_600)
            .expect("reuse approval");
        assert!(!second.should_dispatch_card);
        assert_eq!(second.public.approval_id, first.public.approval_id);

        ledger
            .mark_approval_card_delivered(&first.public.approval_id, 55)
            .expect("bind approval card");
        assert!(
            ledger
                .mark_approval_sending(&first.public.approval_id, now_unix())
                .expect("claim approval")
        );
        ledger.mark_sent(&prepared.public.draft_id, 42).expect("prepared sent");
        ledger.mark_approval_sent(&first.public.approval_id, 42).expect("approval sent");

        let reopened = SendLedger::new(path);
        let approval = reopened
            .get_approval(&first.public.approval_id)
            .expect("read approval")
            .expect("approval record");
        assert_eq!(approval.state, SendApprovalState::Sent);
        assert_eq!(approval.approval_message_id, Some(55));
        assert_eq!(approval.target_message_id, Some(42));
        assert!(
            !reopened
                .cancel_approval(&approval.approval_id, now_unix())
                .expect("sent approval cannot be cancelled")
        );
        reopened.mark_approval_sent(&approval.approval_id, 42).expect("same receipt is idempotent");
        assert!(reopened.mark_approval_sent(&approval.approval_id, 43).is_err());
    }

    #[test]
    fn accepted_send_cannot_expire_or_be_cancelled_during_recovery() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = SendLedger::new(temp.path().join("state.sqlite"));
        let prepared = ledger
            .prepare(
                7,
                "101".into(),
                target("Alex"),
                self_originated_source(),
                "Hello".into(),
                None,
                "abc".into(),
            )
            .expect("prepare draft");
        let requested = ledger
            .create_or_get_approval(&prepared.public.draft_id, "abc", 700, 700, 1)
            .expect("approval");
        ledger.mark_approval_card_delivered(&requested.public.approval_id, 55).expect("card");
        let accepted_at = requested.public.created_at_unix;
        assert!(
            ledger
                .mark_approval_sending(&requested.public.approval_id, accepted_at)
                .expect("accepted")
        );
        let after_expiry = requested.public.expires_at_unix.saturating_add(10);
        assert!(
            !ledger
                .expire_approval(&requested.public.approval_id, after_expiry)
                .expect("accepted send cannot expire")
        );
        assert!(
            !ledger
                .cancel_approval(&requested.public.approval_id, after_expiry)
                .expect("accepted send cannot be cancelled")
        );
        assert!(
            ledger.expire_due_approval_records(after_expiry, 10).expect("expiry pass").is_empty()
        );
        ledger
            .mark_approval_delivery_uncertain(
                &requested.public.approval_id,
                "DELIVERY_UNCERTAIN",
                None,
            )
            .expect("uncertain");
        assert_eq!(
            ledger
                .get_approval(&requested.public.approval_id)
                .expect("read")
                .expect("approval")
                .state,
            SendApprovalState::DeliveryUncertain
        );
        ledger
            .mark_approval_sent(&requested.public.approval_id, 42)
            .expect("late terminal receipt remains legal");
    }

    #[test]
    fn cancelled_send_cannot_be_reclassified_as_sent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = SendLedger::new(temp.path().join("state.sqlite"));
        let prepared = ledger
            .prepare(
                7,
                "101".into(),
                target("Alex"),
                self_originated_source(),
                "Hello".into(),
                None,
                "abc".into(),
            )
            .expect("prepare draft");
        let requested = ledger
            .create_or_get_approval(&prepared.public.draft_id, "abc", 700, 700, 3_600)
            .expect("approval");
        ledger.mark_approval_card_delivered(&requested.public.approval_id, 55).expect("card");
        assert!(ledger.cancel_approval(&requested.public.approval_id, now_unix()).expect("cancel"));
        assert!(ledger.mark_approval_sent(&requested.public.approval_id, 42).is_err());
        assert_eq!(
            ledger
                .get_approval(&requested.public.approval_id)
                .expect("read")
                .expect("approval")
                .state,
            SendApprovalState::Cancelled
        );
    }

    #[test]
    fn approval_update_offset_only_moves_forward() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = SendLedger::new(temp.path().join("state.sqlite"));
        assert_eq!(ledger.approval_update_offset().expect("initial offset"), 0);
        ledger.advance_approval_update_offset(9).expect("advance");
        ledger.advance_approval_update_offset(4).expect("cannot regress");
        assert_eq!(ledger.approval_update_offset().expect("final offset"), 9);
    }

    #[test]
    fn legacy_session_path_column_is_removed_without_losing_drafts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        let connection = Connection::open(&path).expect("open legacy state");
        connection
            .execute_batch(
                "CREATE TABLE prepared_sends (
                    draft_id TEXT PRIMARY KEY,
                    account_user_id INTEGER NOT NULL,
                    session_path TEXT,
                    chat_id TEXT NOT NULL,
                    text TEXT NOT NULL,
                    reply_to_message_id INTEGER,
                    payload_sha256 TEXT NOT NULL,
                    random_id INTEGER NOT NULL,
                    state TEXT NOT NULL CHECK (
                        state IN ('prepared', 'dispatching', 'sent', 'delivery_uncertain')
                    ),
                    message_id INTEGER,
                    created_at INTEGER NOT NULL
                );
                INSERT INTO prepared_sends (
                    draft_id, account_user_id, session_path, chat_id, text,
                    reply_to_message_id, payload_sha256, random_id, state,
                    message_id, created_at
                ) VALUES (
                    'legacy-draft', 7, '/private/account.session', '101', 'Hello',
                    11, 'abc', 42, 'prepared', NULL, 1
                );",
            )
            .expect("seed legacy state");
        drop(connection);

        let ledger = SendLedger::new(path.clone());
        let record = ledger.get("legacy-draft").expect("migrate and read").expect("draft");
        assert_eq!(record.public.account_user_id, 7);
        assert_eq!(record.public.chat_id, "101");
        assert_eq!(record.public.kind, PreparedSendKind::PlainText);
        assert_eq!(record.public.reply_to_message_id, Some(11));
        assert_eq!(record.random_id, 42);

        let connection = Connection::open(path).expect("reopen migrated state");
        let mut statement =
            connection.prepare("PRAGMA table_info(prepared_sends)").expect("table info");
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("columns")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("column names");
        assert!(!columns.iter().any(|column| column == "session_path"));
        assert!(columns.iter().any(|column| column == "content_kind"));
    }

    #[test]
    fn rich_markdown_kind_survives_ledger_and_approval_round_trip() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = SendLedger::new(temp.path().join("state.sqlite"));
        let prepared = ledger
            .prepare_rich_markdown(
                7,
                "101".into(),
                target("Alex"),
                self_originated_source(),
                "# Exact Rich Text".into(),
                Some(11),
                "abc".into(),
            )
            .expect("prepare rich draft");
        assert_eq!(prepared.public.kind, PreparedSendKind::RichMarkdown);
        let approval = ledger
            .create_or_get_approval(&prepared.public.draft_id, "abc", 700, 700, 3_600)
            .expect("rich approval");
        assert_eq!(approval.public.kind, PreparedSendKind::RichMarkdown);
        assert_eq!(approval.public.text, "# Exact Rich Text");
        ledger
            .mark_approval_card_delivered(&approval.public.approval_id, 55)
            .expect("deliver rich approval card");
        let now = now_unix();
        assert!(
            ledger
                .mark_approval_sending(&approval.public.approval_id, now)
                .expect("mark rich approval sending")
        );
        let due = ledger.due_approval_recoveries(now, 10).expect("read rich recovery row");
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].kind, PreparedSendKind::RichMarkdown);
    }

    #[test]
    fn opening_the_state_store_does_not_wait_for_the_write_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        let ledger = SendLedger::new(path.clone());
        let prepared = ledger
            .prepare(
                7,
                "101".into(),
                target("Alex"),
                self_originated_source(),
                "Hello".into(),
                None,
                "abc".into(),
            )
            .expect("prepare draft");
        let approval = ledger
            .create_or_get_approval(&prepared.public.draft_id, "abc", 700, 700, 3_600)
            .expect("create approval");

        // A fresh open must not write: the bootstrap seeds the runtime row only
        // when it is missing, so an open taken while another connection owns the
        // write lock still reads instead of failing with "database is locked"
        // once the busy timeout runs out.
        let writer = Connection::open(&path).expect("writer connection");
        writer.execute_batch("BEGIN IMMEDIATE").expect("writer holds the write lock");
        let reopened = SendLedger::new(path.clone()).get_approval(&approval.public.approval_id);
        writer.execute_batch("COMMIT").expect("release the write lock");
        assert_eq!(
            reopened.expect("open under a held write lock").expect("approval record").state,
            SendApprovalState::DispatchingCard
        );
        assert_eq!(SendLedger::new(path).approval_update_offset().expect("offset"), 0);
    }

    #[test]
    fn reader_keeps_reading_while_a_writer_holds_the_write_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        let ledger = SendLedger::new(path.clone());
        let prepared = ledger
            .prepare(
                7,
                "101".into(),
                target("Alex"),
                self_originated_source(),
                "Hello".into(),
                None,
                "abc".into(),
            )
            .expect("prepare draft");
        let approval = ledger
            .create_or_get_approval(&prepared.public.draft_id, "abc", 700, 700, 3_600)
            .expect("create approval");

        // A poll loop opens its connection once and then keeps reading while the
        // worker owns the write lock: a reused WAL connection keeps polling
        // instead of paying for a connection and a bootstrap on every tick.
        let reader = ledger.reader().expect("reader");
        let writer = Connection::open(&path).expect("writer connection");
        writer.execute_batch("BEGIN IMMEDIATE").expect("writer holds the write lock");
        let observed = reader
            .get_approval(&approval.public.approval_id)
            .expect("read under a held write lock")
            .expect("approval record");
        assert_eq!(observed.state, SendApprovalState::DispatchingCard);

        writer.execute_batch("COMMIT").expect("release the write lock");
        ledger
            .mark_approval_card_delivered(&approval.public.approval_id, 55)
            .expect("bind approval card");
        let delivered = reader
            .get_approval(&approval.public.approval_id)
            .expect("read after the write")
            .expect("approval record");
        assert_eq!(delivered.approval_message_id, Some(55));
    }

    #[cfg(unix)]
    #[test]
    fn newly_created_state_storage_is_owner_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        let private_dir = temp.path().join("private-state");
        let path = private_dir.join("state.sqlite");
        let ledger = SendLedger::new(path.clone());
        ledger
            .prepare(
                7,
                "101".into(),
                target("Alex"),
                self_originated_source(),
                "Hello".into(),
                None,
                "abc".into(),
            )
            .expect("prepare");

        assert_eq!(
            std::fs::metadata(private_dir).expect("directory metadata").permissions().mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(path).expect("database metadata").permissions().mode() & 0o777,
            0o600
        );
    }
}
