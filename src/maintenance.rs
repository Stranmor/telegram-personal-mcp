//! One-shot maintenance for dormant approval-ledger state.
//!
//! This is intentionally narrower than the live approval tools. It can only
//! (a) bind an explicitly supplied, independently verified Telegram message
//! id to a dormant `outcome_unknown` mutation and (b) archive/delete
//! `prepared` rows that have no active approval. The state schema has no
//! terminal state for a prepared draft, so deletion is paired with a durable
//! JSON archive instead of changing the schema or inventing a state.

use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{MutationAction, MutationResult};
use crate::mutation_ledger::MutationLedger;
use crate::state_db::{SynchronousMode, open_private_wal};

#[derive(Debug, Clone, Default)]
pub struct ExpireStaleOptions {
    pub dry_run: bool,
    pub archive_path: Option<PathBuf>,
    /// Exact confirmation pairs. A pair is accepted only when the approval is currently a dormant `outcome_unknown` row.
    pub confirm_unknown: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnknownConfirmation {
    pub approval_id: String,
    pub mutation_id: String,
    pub message_id: i32,
    pub result_kind: &'static str,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StateCounts {
    pub unknown_approvals: usize,
    pub prepared_mutations: usize,
    pub prepared_sends: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExpireStaleReport {
    pub dry_run: bool,
    pub archive_path: Option<PathBuf>,
    pub unknowns_confirmed: Vec<UnknownConfirmation>,
    pub prepared_mutations_deleted: Vec<String>,
    pub mutation_approvals_deleted: Vec<String>,
    pub prepared_sends_deleted: Vec<String>,
    pub send_approvals_deleted: Vec<String>,
    pub counts_before: StateCounts,
    pub counts_after: StateCounts,
}

#[derive(Debug, Clone)]
struct UnknownSpec {
    approval_id: String,
    mutation_id: String,
    action_json: String,
}

type JsonRow = BTreeMap<String, serde_json::Value>;

/// Execute the explicitly scoped maintenance operation.
pub fn expire_stale(
    state_db_path: &Path,
    options: &ExpireStaleOptions,
) -> Result<ExpireStaleReport> {
    let confirmations = parse_confirmations(&options.confirm_unknown)?;
    let connection = open_private_wal(state_db_path, SynchronousMode::Full)
        .with_context(|| format!("failed to open state database {}", state_db_path.display()))?;

    let counts_before = state_counts(&connection)?;
    let unknown_specs = dormant_unknown_specs(&connection)?;
    let unknown_rows = dormant_unknown_rows(&connection)?;
    let stale_mutation_rows = stale_mutation_rows(&connection)?;
    let stale_send_rows = stale_send_rows(&connection)?;

    let stale_mutation_ids = ids_from_rows(&stale_mutation_rows, "mutation_id");
    let stale_send_ids = ids_from_rows(&stale_send_rows, "draft_id");
    assert_parent_approvals_are_safe(
        &connection,
        "mutation_approvals",
        "mutation_id",
        &stale_mutation_ids,
    )?;
    assert_parent_approvals_are_safe(&connection, "send_approvals", "draft_id", &stale_send_ids)?;
    let mutation_approval_rows = terminal_approval_rows(
        &connection,
        "mutation_approvals",
        "mutation_id",
        &stale_mutation_ids,
    )?;
    let send_approval_rows =
        terminal_approval_rows(&connection, "send_approvals", "draft_id", &stale_send_ids)?;
    let mutation_approval_ids = ids_from_rows(&mutation_approval_rows, "approval_id");
    let send_approval_ids = ids_from_rows(&send_approval_rows, "approval_id");
    let handoff_rows = handoff_rows(&connection, &mutation_approval_ids)?;
    let invite_target_rows = invite_target_rows(&connection, &stale_mutation_ids)?;

    let mut unknowns_confirmed = Vec::new();
    for spec in &unknown_specs {
        let Some(message_id) = confirmations.get(&spec.approval_id) else {
            continue;
        };
        unknowns_confirmed.push(UnknownConfirmation {
            approval_id: spec.approval_id.clone(),
            mutation_id: spec.mutation_id.clone(),
            message_id: *message_id,
            result_kind: "pending_confirmation",
        });
    }
    for approval_id in confirmations.keys() {
        if !unknowns_confirmed.iter().any(|item| &item.approval_id == approval_id) {
            bail!("confirmation {approval_id} does not match a dormant outcome_unknown approval");
        }
    }

    let mut report = ExpireStaleReport {
        dry_run: options.dry_run,
        archive_path: None,
        unknowns_confirmed,
        prepared_mutations_deleted: stale_mutation_ids,
        mutation_approvals_deleted: mutation_approval_ids,
        prepared_sends_deleted: stale_send_ids,
        send_approvals_deleted: send_approval_ids,
        counts_before: counts_before.clone(),
        counts_after: counts_before,
    };
    if options.dry_run {
        return Ok(report);
    }

    let archive_path = options.archive_path.clone().unwrap_or_else(|| {
        state_db_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("archive")
            .join(format!("stale-{}.json", now_unix()))
    });
    write_archive(
        &archive_path,
        &unknown_rows,
        &report.unknowns_confirmed,
        &stale_mutation_rows,
        &mutation_approval_rows,
        &handoff_rows,
        &invite_target_rows,
        &stale_send_rows,
        &send_approval_rows,
    )?;
    report.archive_path = Some(archive_path);

    confirm_unknowns(state_db_path, &unknown_specs, &mut report.unknowns_confirmed)?;
    delete_stale(state_db_path, &report)?;

    let after_connection = open_private_wal(state_db_path, SynchronousMode::Full)
        .with_context(|| format!("failed to reopen state database {}", state_db_path.display()))?;
    report.counts_after = state_counts(&after_connection)?;
    Ok(report)
}

fn parse_confirmations(entries: &[String]) -> Result<BTreeMap<String, i32>> {
    let mut confirmations = BTreeMap::new();
    for entry in entries {
        let (approval_id, message_id) = entry.split_once('=').with_context(|| {
            format!("confirmation {entry:?} must be a joined approval/message pair")
        })?;
        if approval_id.trim().is_empty() {
            bail!("confirmation {entry:?} has an empty approval id");
        }
        let message_id: i32 = message_id
            .parse()
            .with_context(|| format!("confirmation {entry:?} has an invalid message id"))?;
        if message_id <= 0 {
            bail!("confirmation {entry:?} must reference a positive message id");
        }
        if confirmations.insert(approval_id.to_string(), message_id).is_some() {
            bail!("confirmation contains duplicate approval id {approval_id}");
        }
    }
    Ok(confirmations)
}

fn state_counts(connection: &rusqlite::Connection) -> Result<StateCounts> {
    let unknown_approvals: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM mutation_approvals WHERE state = 'outcome_unknown'",
            [],
            |row| row.get(0),
        )
        .context("failed to count unknown approvals")?;
    let prepared_mutations: i64 = connection
        .query_row("SELECT COUNT(*) FROM prepared_mutations WHERE state = 'prepared'", [], |row| {
            row.get(0)
        })
        .context("failed to count prepared mutations")?;
    let prepared_sends: i64 = connection
        .query_row("SELECT COUNT(*) FROM prepared_sends WHERE state = 'prepared'", [], |row| {
            row.get(0)
        })
        .context("failed to count prepared sends")?;
    Ok(StateCounts {
        unknown_approvals: usize::try_from(unknown_approvals).unwrap_or(0),
        prepared_mutations: usize::try_from(prepared_mutations).unwrap_or(0),
        prepared_sends: usize::try_from(prepared_sends).unwrap_or(0),
    })
}

fn dormant_unknown_specs(connection: &rusqlite::Connection) -> Result<Vec<UnknownSpec>> {
    let mut statement = connection.prepare(
        "SELECT a.approval_id, a.mutation_id, p.action_json
         FROM mutation_approvals a
         JOIN prepared_mutations p ON p.mutation_id = a.mutation_id
         WHERE a.state = 'outcome_unknown' AND a.next_retry_at IS NULL
         ORDER BY a.created_at ASC",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok(UnknownSpec {
                approval_id: row.get(0)?,
                mutation_id: row.get(1)?,
                action_json: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn dormant_unknown_rows(connection: &rusqlite::Connection) -> Result<Vec<JsonRow>> {
    query_rows(
        connection,
        "SELECT a.approval_id, a.mutation_id, a.state, a.payload_sha256,
                a.created_at, a.expires_at, a.decided_at, a.automated_retry_count,
                a.next_retry_at, a.last_error_code, a.last_error_message,
                p.schema_version, p.account_user_id, p.action_json, p.result_json,
                p.created_at AS mutation_created_at
         FROM mutation_approvals a
         JOIN prepared_mutations p ON p.mutation_id = a.mutation_id
         WHERE a.state = 'outcome_unknown' AND a.next_retry_at IS NULL
         ORDER BY a.created_at ASC",
        [],
    )
}

fn stale_mutation_rows(connection: &rusqlite::Connection) -> Result<Vec<JsonRow>> {
    query_rows(
        connection,
        "SELECT mutation_id, schema_version, account_user_id, action_json,
                payload_sha256, state, result_json, created_at
         FROM prepared_mutations
         WHERE state = 'prepared'
           AND NOT EXISTS (
               SELECT 1 FROM mutation_approvals a
               WHERE a.mutation_id = prepared_mutations.mutation_id
                 AND a.state IN ('dispatching_card', 'pending', 'card_delivery_uncertain',
                                 'applying', 'outcome_unknown')
           )
         ORDER BY created_at ASC",
        [],
    )
}

fn stale_send_rows(connection: &rusqlite::Connection) -> Result<Vec<JsonRow>> {
    query_rows(
        connection,
        "SELECT draft_id, account_user_id, chat_id, target_display_name,
                target_username, target_kind, source_json, content_kind, text,
                reply_to_message_id, payload_sha256, random_id, state,
                message_id, created_at
         FROM prepared_sends
         WHERE state = 'prepared'
           AND NOT EXISTS (
               SELECT 1 FROM send_approvals a
               WHERE a.draft_id = prepared_sends.draft_id
                 AND a.state IN ('dispatching_card', 'pending', 'card_delivery_uncertain',
                                 'sending', 'delivery_uncertain')
           )
         ORDER BY created_at ASC",
        [],
    )
}

fn terminal_approval_rows(
    connection: &rusqlite::Connection,
    table: &str,
    parent_column: &str,
    parent_ids: &[String],
) -> Result<Vec<JsonRow>> {
    if parent_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = question_marks(parent_ids.len());
    let columns = if table == "mutation_approvals" {
        "approval_id, mutation_id, payload_sha256, state, approval_chat_id,
         approval_user_id, bot_message_id, created_at, expires_at, decided_at,
         automated_retry_count, next_retry_at, last_error_code, last_error_message"
    } else {
        "approval_id, draft_id, payload_sha256, state, approval_chat_id,
         approval_user_id, bot_message_id, created_at, expires_at, decided_at,
         target_message_id, automated_retry_count, next_retry_at, last_error_code"
    };
    let sql = format!(
        "SELECT {columns} FROM {table}
         WHERE {parent_column} IN ({placeholders})
           AND state IN ('cancelled', 'expired', 'failed')
         ORDER BY created_at ASC"
    );
    query_rows(connection, &sql, rusqlite::params_from_iter(parent_ids.iter().map(String::as_str)))
}

fn handoff_rows(
    connection: &rusqlite::Connection,
    approval_ids: &[String],
) -> Result<Vec<JsonRow>> {
    if approval_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = question_marks(approval_ids.len());
    let sql = format!(
        "SELECT approval_id, receipt, approved_at, consumed_at
         FROM mutation_external_handoffs WHERE approval_id IN ({placeholders})"
    );
    query_rows(
        connection,
        &sql,
        rusqlite::params_from_iter(approval_ids.iter().map(String::as_str)),
    )
}

fn invite_target_rows(
    connection: &rusqlite::Connection,
    mutation_ids: &[String],
) -> Result<Vec<JsonRow>> {
    if mutation_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = question_marks(mutation_ids.len());
    let sql = format!(
        "SELECT mutation_id, target_index, target_user_id, binding_sha256, state,
                attempt_count, observation_count, next_action_at, last_error_code,
                confirmation, member_json, updated_at
         FROM mutation_invite_targets WHERE mutation_id IN ({placeholders})"
    );
    query_rows(
        connection,
        &sql,
        rusqlite::params_from_iter(mutation_ids.iter().map(String::as_str)),
    )
}

fn assert_parent_approvals_are_safe(
    connection: &rusqlite::Connection,
    table: &str,
    parent_column: &str,
    parent_ids: &[String],
) -> Result<()> {
    if parent_ids.is_empty() {
        return Ok(());
    }
    let placeholders = question_marks(parent_ids.len());
    let sql = format!(
        "SELECT {parent_column}, approval_id, state FROM {table}
         WHERE {parent_column} IN ({placeholders})
           AND state NOT IN ('cancelled', 'expired', 'failed')
         ORDER BY {parent_column}, approval_id"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parent_ids.iter().map(String::as_str)), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if let Some((parent, approval, state)) = rows.first() {
        bail!("refusing to archive {parent}: approval {approval} is non-terminal ({state})");
    }
    Ok(())
}

fn ids_from_rows(rows: &[JsonRow], field: &str) -> Vec<String> {
    rows.iter()
        .filter_map(|row| row.get(field).and_then(serde_json::Value::as_str).map(str::to_string))
        .collect()
}

fn query_rows<P: rusqlite::Params>(
    connection: &rusqlite::Connection,
    sql: &str,
    params: P,
) -> Result<Vec<JsonRow>> {
    let mut statement = connection.prepare(sql).context("failed to prepare maintenance query")?;
    let columns =
        statement.column_names().iter().map(|name| (*name).to_string()).collect::<Vec<_>>();
    let rows = statement
        .query_map(params, |row| row_to_json(row, &columns))
        .context("failed to run maintenance query")?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn row_to_json(row: &rusqlite::Row<'_>, columns: &[String]) -> rusqlite::Result<JsonRow> {
    let mut result = BTreeMap::new();
    for (index, column) in columns.iter().enumerate() {
        let value: rusqlite::types::Value = row.get(index)?;
        result.insert(column.clone(), sql_value_to_json(value));
    }
    Ok(result)
}

fn sql_value_to_json(value: rusqlite::types::Value) -> serde_json::Value {
    match value {
        rusqlite::types::Value::Null => serde_json::Value::Null,
        rusqlite::types::Value::Integer(value) => serde_json::json!(value),
        rusqlite::types::Value::Real(value) => serde_json::json!(value),
        rusqlite::types::Value::Text(value) => serde_json::Value::String(value),
        rusqlite::types::Value::Blob(value) => {
            serde_json::Value::String(format!("hex:{}", hex_bytes(&value)))
        },
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    text
}

#[allow(clippy::too_many_arguments)]
fn write_archive(
    path: &Path,
    unknown_rows: &[JsonRow],
    confirmations: &[UnknownConfirmation],
    mutation_rows: &[JsonRow],
    mutation_approval_rows: &[JsonRow],
    handoff_rows: &[JsonRow],
    invite_rows: &[JsonRow],
    send_rows: &[JsonRow],
    send_approval_rows: &[JsonRow],
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create archive directory {}", parent.display()))?;
    }
    let archive = serde_json::json!({
        "generated_at_unix": now_unix(),
        "reason": "maintenance expire-stale: dormant prepared drafts and confirmed unknown outcomes",
        "dormant_unknown_approvals": unknown_rows,
        "offline_confirmations": confirmations,
        "stale_prepared_mutations": mutation_rows,
        "stale_mutation_approvals": mutation_approval_rows,
        "stale_mutation_handoffs": handoff_rows,
        "stale_mutation_invite_targets": invite_rows,
        "stale_prepared_sends": send_rows,
        "stale_send_approvals": send_approval_rows,
    });
    let payload = serde_json::to_vec_pretty(&archive).context("failed to serialize archive")?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("failed to create archive {}", path.display()))?;
    file.write_all(&payload)
        .with_context(|| format!("failed to write archive {}", path.display()))?;
    file.sync_all().with_context(|| format!("failed to sync archive {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn confirm_unknowns(
    state_db_path: &Path,
    specs: &[UnknownSpec],
    confirmations: &mut [UnknownConfirmation],
) -> Result<()> {
    let ledger = MutationLedger::new(state_db_path.to_path_buf());
    for confirmation in confirmations.iter_mut() {
        let spec =
            specs.iter().find(|spec| spec.approval_id == confirmation.approval_id).with_context(
                || format!("unknown approval {} disappeared", confirmation.approval_id),
            )?;
        let action: MutationAction = serde_json::from_str(&spec.action_json)
            .with_context(|| format!("failed to decode action {}", spec.mutation_id))?;
        let result = confirmation_result(&action, confirmation.message_id)?;
        confirmation.result_kind = result_kind_name(&result);
        ledger
            .mark_applied(&confirmation.approval_id, &result)
            .with_context(|| format!("failed to confirm {}", confirmation.approval_id))?;
    }
    Ok(())
}

fn confirmation_result(action: &MutationAction, message_id: i32) -> Result<MutationResult> {
    match action {
        MutationAction::SendFile { artifact, .. } => {
            Ok(MutationResult::FileSent { message_id, sha256: artifact.sha256.clone() })
        },
        MutationAction::ScheduleMessage { schedule_date_unix, .. } => {
            Ok(MutationResult::MessageScheduled {
                message_id,
                schedule_date_unix: *schedule_date_unix,
            })
        },
        MutationAction::ScheduleFile { schedule_date_unix, artifact, .. } => {
            Ok(MutationResult::FileScheduled {
                message_id,
                schedule_date_unix: *schedule_date_unix,
                sha256: artifact.sha256.clone(),
            })
        },
        _ => bail!(
            "offline confirmation supports only send_file, schedule_message, and schedule_file"
        ),
    }
}

fn result_kind_name(result: &MutationResult) -> &'static str {
    match result {
        MutationResult::FileSent { .. } => "file_sent",
        MutationResult::MessageScheduled { .. } => "message_scheduled",
        MutationResult::FileScheduled { .. } => "file_scheduled",
        _ => "other",
    }
}

fn delete_stale(state_db_path: &Path, report: &ExpireStaleReport) -> Result<()> {
    let mut connection = open_private_wal(state_db_path, SynchronousMode::Full)
        .with_context(|| format!("failed to reopen state database {}", state_db_path.display()))?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .context("failed to open maintenance write transaction")?;

    recheck_prepared_mutations(&transaction, &report.prepared_mutations_deleted)?;
    recheck_prepared_sends(&transaction, &report.prepared_sends_deleted)?;
    delete_ids(
        &transaction,
        "mutation_invite_targets",
        "mutation_id",
        &report.prepared_mutations_deleted,
    )?;
    delete_ids(
        &transaction,
        "mutation_external_handoffs",
        "approval_id",
        &report.mutation_approvals_deleted,
    )?;
    delete_terminal_approvals(
        &transaction,
        "mutation_approvals",
        &report.mutation_approvals_deleted,
    )?;
    delete_ids(
        &transaction,
        "prepared_mutations",
        "mutation_id",
        &report.prepared_mutations_deleted,
    )?;
    delete_terminal_approvals(&transaction, "send_approvals", &report.send_approvals_deleted)?;
    delete_ids(&transaction, "prepared_sends", "draft_id", &report.prepared_sends_deleted)?;
    transaction.commit().context("failed to commit maintenance deletes")?;
    Ok(())
}

fn recheck_prepared_mutations(
    transaction: &rusqlite::Transaction<'_>,
    ids: &[String],
) -> Result<()> {
    for id in ids {
        let state: Option<String> = transaction
            .query_row("SELECT state FROM prepared_mutations WHERE mutation_id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        if state.as_deref() != Some("prepared") {
            bail!("mutation {id} changed state; aborting maintenance");
        }
        let active: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM mutation_approvals
             WHERE mutation_id = ?1 AND state IN
             ('dispatching_card', 'pending', 'card_delivery_uncertain', 'applying', 'outcome_unknown')",
            [id],
            |row| row.get(0),
        )?;
        if active != 0 {
            bail!("mutation {id} gained an active approval; aborting maintenance");
        }
    }
    Ok(())
}

fn recheck_prepared_sends(transaction: &rusqlite::Transaction<'_>, ids: &[String]) -> Result<()> {
    for id in ids {
        let state: Option<String> = transaction
            .query_row("SELECT state FROM prepared_sends WHERE draft_id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        if state.as_deref() != Some("prepared") {
            bail!("draft {id} changed state; aborting maintenance");
        }
        let active: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM send_approvals
             WHERE draft_id = ?1 AND state IN
             ('dispatching_card', 'pending', 'card_delivery_uncertain', 'sending', 'delivery_uncertain')",
            [id],
            |row| row.get(0),
        )?;
        if active != 0 {
            bail!("draft {id} gained an active approval; aborting maintenance");
        }
    }
    Ok(())
}

fn delete_terminal_approvals(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    ids: &[String],
) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders = question_marks(ids.len());
    let sql = format!(
        "DELETE FROM {table}
         WHERE approval_id IN ({placeholders})
           AND state IN ('cancelled', 'expired', 'failed')"
    );
    let mut statement = transaction.prepare(&sql)?;
    let changed = statement.execute(rusqlite::params_from_iter(ids.iter().map(String::as_str)))?;
    if changed != ids.len() {
        bail!("{table}: expected {} terminal approvals, deleted {changed}", ids.len());
    }
    Ok(())
}

fn delete_ids(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
    ids: &[String],
) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders = question_marks(ids.len());
    let sql = format!("DELETE FROM {table} WHERE {column} IN ({placeholders})");
    transaction
        .execute(&sql, rusqlite::params_from_iter(ids.iter().map(String::as_str)))
        .with_context(|| format!("failed to delete scoped rows from {table}"))?;
    Ok(())
}

fn question_marks(count: usize) -> String {
    std::iter::repeat_n("?", count).collect::<Vec<_>>().join(",")
}

fn now_unix() -> i64 {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
        .unwrap_or(i64::MAX)
}
