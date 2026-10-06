//! One-shot maintenance for dormant approval-ledger state.
//!
//! This is intentionally narrower than the live approval tools. It can
//! (a) bind an explicitly supplied, independently verified Telegram message
//! id to a dormant `outcome_unknown` mutation, (b) resolve a dormant
//! `outcome_unknown` mutation as a known failure when offline verification
//! shows the approved effect did not materialize, and (c) archive/delete
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

use crate::domain::{MutationAction, MutationResult, SentDocumentMediaSummary};
use crate::mutation_ledger::MutationLedger;
use crate::state_db::{SynchronousMode, open_private_wal};

/// Approval error code recorded when offline maintenance verifies that the
/// approved effect never materialized.
const VERIFIED_NO_EFFECT_CODE: &str = "MUTATION_VERIFIED_NO_EFFECT";
const VERIFIED_NO_EFFECT_MESSAGE: &str =
    "offline maintenance: independent verification found no effect";

#[derive(Debug, Clone, Default)]
pub struct ExpireStaleOptions {
    pub dry_run: bool,
    pub archive_path: Option<PathBuf>,
    /// Exact confirmation pairs. A pair is accepted only when the approval is currently a dormant `outcome_unknown` row.
    pub confirm_unknown: Vec<String>,
    /// Approval ids independently verified to have had no effect. Only
    /// dormant `outcome_unknown` rows are accepted.
    pub confirm_no_effect: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnknownConfirmation {
    pub approval_id: String,
    pub mutation_id: String,
    pub message_id: i32,
    pub result_kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnknownNoEffect {
    pub approval_id: String,
    pub mutation_id: String,
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
    pub unknowns_no_effect: Vec<UnknownNoEffect>,
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
    let no_effect_ids = parse_no_effect(&options.confirm_no_effect)?;
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
    let mut unknowns_no_effect = Vec::new();
    for approval_id in &no_effect_ids {
        if confirmations.contains_key(approval_id.as_str()) {
            bail!(
                "approval {approval_id} is supplied both as an applied confirmation and as a no-effect confirmation"
            );
        }
        let Some(spec) = unknown_specs.iter().find(|spec| &spec.approval_id == approval_id) else {
            bail!(
                "no-effect confirmation {approval_id} does not match a dormant outcome_unknown approval"
            );
        };
        unknowns_no_effect.push(UnknownNoEffect {
            approval_id: spec.approval_id.clone(),
            mutation_id: spec.mutation_id.clone(),
        });
    }

    let mut report = ExpireStaleReport {
        dry_run: options.dry_run,
        archive_path: None,
        unknowns_confirmed,
        unknowns_no_effect,
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
        &report.unknowns_no_effect,
        &stale_mutation_rows,
        &mutation_approval_rows,
        &handoff_rows,
        &invite_target_rows,
        &stale_send_rows,
        &send_approval_rows,
    )?;
    report.archive_path = Some(archive_path);

    confirm_unknowns(state_db_path, &unknown_specs, &mut report.unknowns_confirmed)?;
    mark_no_effect(state_db_path, &report.unknowns_no_effect)?;
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

fn parse_no_effect(entries: &[String]) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    for entry in entries {
        let id = entry.trim();
        if id.is_empty() {
            bail!("no-effect confirmation has an empty approval id");
        }
        if !ids.insert(id.to_string()) {
            bail!("no-effect confirmations contain duplicate approval id {id}");
        }
    }
    Ok(ids)
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
    no_effect: &[UnknownNoEffect],
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
        "verified_no_effect": no_effect,
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

fn mark_no_effect(state_db_path: &Path, entries: &[UnknownNoEffect]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let ledger = MutationLedger::new(state_db_path.to_path_buf());
    let now = now_unix();
    for entry in entries {
        ledger
            .mark_known_failure(&entry.approval_id, VERIFIED_NO_EFFECT_CODE, now)
            .with_context(|| {
                format!("failed to mark {} as verified no-effect", entry.approval_id)
            })?;
        ledger
            .set_last_error_message(&entry.approval_id, Some(VERIFIED_NO_EFFECT_MESSAGE))
            .with_context(|| {
                format!("failed to record the no-effect note for {}", entry.approval_id)
            })?;
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
        MutationAction::SendVoice { artifact, .. } => Ok(MutationResult::VoiceSent {
            message_id,
            sha256: artifact.sha256.clone(),
            media: SentDocumentMediaSummary {
                media_type: "voice".to_string(),
                mime_type: "audio/ogg".to_string(),
                duration_seconds: None,
                width_px: None,
                height_px: None,
            },
        }),
        MutationAction::SendVideoNote { artifact, duration_seconds, length_pixels, .. } => {
            Ok(MutationResult::VideoNoteSent {
                message_id,
                sha256: artifact.sha256.clone(),
                media: SentDocumentMediaSummary {
                    media_type: "video_note".to_string(),
                    mime_type: "video/mp4".to_string(),
                    duration_seconds: Some(*duration_seconds),
                    width_px: Some(*length_pixels),
                    height_px: Some(*length_pixels),
                },
            })
        },
        _ => bail!(
            "offline confirmation supports only send_file, send_voice, send_video_note, schedule_message, and schedule_file"
        ),
    }
}

fn result_kind_name(result: &MutationResult) -> &'static str {
    match result {
        MutationResult::FileSent { .. } => "file_sent",
        MutationResult::MessageScheduled { .. } => "message_scheduled",
        MutationResult::FileScheduled { .. } => "file_scheduled",
        MutationResult::VoiceSent { .. } => "voice_sent",
        MutationResult::VideoNoteSent { .. } => "video_note_sent",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CaptionParseMode, MutationFileArtifact};

    fn artifact() -> MutationFileArtifact {
        MutationFileArtifact {
            artifact_id: "art-1".to_string(),
            file_name: "probe.ogg".to_string(),
            size_bytes: 1234,
            sha256: "aa".repeat(32),
        }
    }

    #[test]
    fn offline_confirmation_builds_voice_result() {
        let action = voice_action();
        let result = confirmation_result(&action, 395649).expect("voice confirmation");
        match result {
            MutationResult::VoiceSent { message_id, sha256, media } => {
                assert_eq!(message_id, 395649);
                assert_eq!(sha256, "aa".repeat(32));
                assert_eq!(media.media_type, "voice");
                assert!(media.mime_type.starts_with("audio/"));
                assert_eq!(media.width_px, None);
            },
            other => panic!("unexpected result {other:?}"),
        }
    }

    #[test]
    fn offline_confirmation_builds_video_note_result() {
        let action = MutationAction::SendVideoNote {
            chat_id: "8816022027".to_string(),
            artifact: artifact(),
            duration_seconds: 2,
            length_pixels: 240,
            reply_to_message_id: None,
            topic_id: None,
            silent: true,
            noforwards: false,
            random_id: 2,
        };
        let result = confirmation_result(&action, 395651).expect("video-note confirmation");
        match result {
            MutationResult::VideoNoteSent { message_id, media, .. } => {
                assert_eq!(message_id, 395651);
                assert_eq!(media.media_type, "video_note");
                assert_eq!(media.mime_type, "video/mp4");
                assert_eq!(media.duration_seconds, Some(2));
                assert_eq!(media.width_px, Some(240));
                assert_eq!(media.height_px, Some(240));
            },
            other => panic!("unexpected result {other:?}"),
        }
    }

    #[test]
    fn no_effect_ids_must_be_non_empty_and_unique() {
        assert!(parse_no_effect(&[]).unwrap().is_empty());
        assert!(parse_no_effect(&[" ".to_string()]).is_err());
        assert!(parse_no_effect(&["a".to_string(), "a".to_string()]).is_err());
        assert_eq!(parse_no_effect(&[" a ".to_string()]).unwrap().len(), 1);
    }

    fn voice_action() -> MutationAction {
        MutationAction::SendVoice {
            chat_id: "8816022027".to_string(),
            artifact: artifact(),
            caption: String::new(),
            caption_parse_mode: CaptionParseMode::Plain,
            reply_to_message_id: None,
            topic_id: None,
            silent: false,
            noforwards: false,
            random_id: 1,
        }
    }

    /// The ledger's own schema (created on first use) plus one production-shaped
    /// dormant `outcome_unknown` pair.
    fn seed_dormant_unknown(path: &Path, approval_id: &str, mutation_id: &str) {
        let _ = MutationLedger::new(path.to_path_buf()).get("schema-seed");
        let connection = open_private_wal(path, SynchronousMode::Full).expect("open fixture");
        let action_json = serde_json::to_string(&voice_action()).expect("action json");
        connection
            .execute(
                "INSERT INTO prepared_mutations (mutation_id, schema_version, account_user_id,
                     action_json, payload_sha256, state, result_json, created_at)
                 VALUES (?1, 1, 700, ?2, 'payload-sha', 'outcome_unknown', NULL, 1000)",
                params![mutation_id, action_json],
            )
            .expect("insert mutation");
        connection
            .execute(
                "INSERT INTO mutation_approvals (approval_id, mutation_id, payload_sha256, state,
                     approval_chat_id, approval_user_id, bot_message_id, created_at, expires_at,
                     decided_at, automated_retry_count, next_retry_at, last_error_code,
                     last_error_message)
                 VALUES (?1, ?2, 'payload-sha', 'outcome_unknown', 700, 700, NULL, 1000, 2000,
                     1500, 2, NULL, 'MUTATION_OUTCOME_UNKNOWN', 'fixture')",
                params![approval_id, mutation_id],
            )
            .expect("insert approval");
    }

    /// A dormant `prepared` draft with no approval, as left by an abandoned
    /// prepare; a stale sweep may delete it.
    fn seed_stale_draft(path: &Path, mutation_id: &str) {
        let _ = MutationLedger::new(path.to_path_buf()).get("schema-seed");
        let connection = open_private_wal(path, SynchronousMode::Full).expect("open fixture");
        let action_json = serde_json::to_string(&voice_action()).expect("action json");
        connection
            .execute(
                "INSERT INTO prepared_mutations (mutation_id, schema_version, account_user_id,
                     action_json, payload_sha256, state, result_json, created_at)
                 VALUES (?1, 1, 700, ?2, 'payload-sha', 'prepared', NULL, 1000)",
                params![mutation_id, action_json],
            )
            .expect("insert stale draft");
    }

    fn approval_row(path: &Path, approval_id: &str) -> (String, Option<String>, Option<String>) {
        let connection = open_private_wal(path, SynchronousMode::Full).expect("open");
        connection
            .query_row(
                "SELECT state, last_error_code, last_error_message
                 FROM mutation_approvals WHERE approval_id = ?1",
                [approval_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("approval row")
    }

    fn mutation_state(path: &Path, mutation_id: &str) -> String {
        let connection = open_private_wal(path, SynchronousMode::Full).expect("open");
        connection
            .query_row(
                "SELECT state FROM prepared_mutations WHERE mutation_id = ?1",
                [mutation_id],
                |row| row.get(0),
            )
            .expect("mutation row")
    }

    fn mutation_result(path: &Path, mutation_id: &str) -> serde_json::Value {
        let connection = open_private_wal(path, SynchronousMode::Full).expect("open");
        let raw: String = connection
            .query_row(
                "SELECT result_json FROM prepared_mutations WHERE mutation_id = ?1",
                [mutation_id],
                |row| row.get(0),
            )
            .expect("result json");
        serde_json::from_str(&raw).expect("result parses")
    }

    #[test]
    fn dry_run_reports_no_effect_without_writing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        seed_dormant_unknown(&path, "appr-a", "mut-a");
        seed_stale_draft(&path, "draft-stale");
        let report = expire_stale(
            &path,
            &ExpireStaleOptions {
                dry_run: true,
                confirm_no_effect: vec!["appr-a".to_string()],
                ..ExpireStaleOptions::default()
            },
        )
        .expect("dry run");
        assert_eq!(report.unknowns_no_effect.len(), 1);
        assert_eq!(report.unknowns_no_effect[0].approval_id, "appr-a");
        assert_eq!(report.unknowns_no_effect[0].mutation_id, "mut-a");
        assert!(report.prepared_mutations_deleted.contains(&"draft-stale".to_string()));
        assert_eq!(report.counts_after.unknown_approvals, 1);
        assert_eq!(approval_row(&path, "appr-a").0, "outcome_unknown");
        assert_eq!(mutation_state(&path, "mut-a"), "outcome_unknown");
        assert_eq!(mutation_state(&path, "draft-stale"), "prepared");
        assert!(!temp.path().join("archive").exists());
    }

    #[test]
    fn apply_marks_only_the_confirmed_no_effect_rows() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        seed_dormant_unknown(&path, "appr-a", "mut-a");
        seed_dormant_unknown(&path, "appr-b", "mut-b");
        let archive = temp.path().join("archive.json");
        let report = expire_stale(
            &path,
            &ExpireStaleOptions {
                dry_run: false,
                archive_path: Some(archive.clone()),
                confirm_no_effect: vec!["appr-a".to_string()],
                ..ExpireStaleOptions::default()
            },
        )
        .expect("apply");
        assert_eq!(report.counts_after.unknown_approvals, 1);
        assert_eq!(report.archive_path, Some(archive.clone()));
        let (state, code, message) = approval_row(&path, "appr-a");
        assert_eq!(state, "failed");
        assert_eq!(code.as_deref(), Some(VERIFIED_NO_EFFECT_CODE));
        assert_eq!(message.as_deref(), Some(VERIFIED_NO_EFFECT_MESSAGE));
        assert_eq!(mutation_state(&path, "mut-a"), "prepared");
        assert_eq!(approval_row(&path, "appr-b").0, "outcome_unknown");
        assert_eq!(mutation_state(&path, "mut-b"), "outcome_unknown");
        let archived: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&archive).expect("archive readable"),
        )
        .expect("archive json");
        let verified = archived
            .pointer("/verified_no_effect")
            .and_then(|value| value.as_array())
            .expect("verified_no_effect section");
        assert_eq!(verified.len(), 1);
        assert_eq!(
            verified[0].get("approval_id").and_then(|value| value.as_str()),
            Some("appr-a")
        );
        let connection = open_private_wal(&path, SynchronousMode::Full).expect("open");
        let decided: Option<i64> = connection
            .query_row(
                "SELECT decided_at FROM mutation_approvals WHERE approval_id = 'appr-a'",
                [],
                |row| row.get(0),
            )
            .expect("decided_at");
        assert_eq!(decided, Some(1500));
    }

    #[test]
    fn apply_confirms_a_verified_applied_outcome() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        seed_dormant_unknown(&path, "appr-v", "mut-v");
        let report = expire_stale(
            &path,
            &ExpireStaleOptions {
                dry_run: false,
                archive_path: Some(temp.path().join("archive.json")),
                confirm_unknown: vec!["appr-v=395649".to_string()],
                ..ExpireStaleOptions::default()
            },
        )
        .expect("apply");
        assert_eq!(report.counts_after.unknown_approvals, 0);
        assert_eq!(report.unknowns_confirmed.len(), 1);
        assert_eq!(report.unknowns_confirmed[0].result_kind, "voice_sent");
        let (state, code, _) = approval_row(&path, "appr-v");
        assert_eq!(state, "applied");
        assert!(code.is_none());
        assert_eq!(mutation_state(&path, "mut-v"), "applied");
        let result = mutation_result(&path, "mut-v");
        assert_eq!(result.pointer("/kind").and_then(|value| value.as_str()), Some("voice_sent"));
        assert_eq!(result.pointer("/message_id").and_then(|value| value.as_i64()), Some(395649));
    }

    #[test]
    fn rejects_no_effect_intersection_and_unknown_ids() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        seed_dormant_unknown(&path, "appr-x", "mut-x");
        let err = expire_stale(
            &path,
            &ExpireStaleOptions {
                dry_run: true,
                confirm_unknown: vec!["appr-x=5".to_string()],
                confirm_no_effect: vec!["appr-x".to_string()],
                ..ExpireStaleOptions::default()
            },
        )
        .expect_err("intersection must be rejected");
        assert!(
            err.to_string().contains("both as an applied confirmation"),
            "unexpected error: {err}"
        );
        let err = expire_stale(
            &path,
            &ExpireStaleOptions {
                dry_run: true,
                confirm_no_effect: vec!["missing-approval".to_string()],
                ..ExpireStaleOptions::default()
            },
        )
        .expect_err("unknown id must be rejected");
        assert!(
            err.to_string().contains("does not match a dormant outcome_unknown approval"),
            "unexpected error: {err}"
        );
        assert_eq!(approval_row(&path, "appr-x").0, "outcome_unknown");
    }

    #[test]
    fn rejects_non_dormant_rows_without_touching_them() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        seed_dormant_unknown(&path, "appr-d", "mut-d");
        let connection = open_private_wal(&path, SynchronousMode::Full).expect("open");
        connection
            .execute(
                "UPDATE mutation_approvals SET next_retry_at = 3000 WHERE approval_id = 'appr-d'",
                [],
            )
            .expect("arm auto-retry");
        drop(connection);
        let archive = temp.path().join("archive.json");
        let err = expire_stale(
            &path,
            &ExpireStaleOptions {
                dry_run: false,
                archive_path: Some(archive.clone()),
                confirm_no_effect: vec!["appr-d".to_string()],
                ..ExpireStaleOptions::default()
            },
        )
        .expect_err("a row with an armed auto-retry must not be resolved");
        assert!(
            err.to_string().contains("does not match a dormant outcome_unknown approval"),
            "unexpected error: {err}"
        );
        assert_eq!(approval_row(&path, "appr-d").0, "outcome_unknown");
        assert_eq!(mutation_state(&path, "mut-d"), "outcome_unknown");
        assert!(!archive.exists());
    }

    #[test]
    fn archive_failure_leaves_the_ledger_untouched() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        seed_dormant_unknown(&path, "appr-z", "mut-z");
        let archive = temp.path().join("archive.json");
        std::fs::write(&archive, "existing archive").expect("create existing archive");
        expire_stale(
            &path,
            &ExpireStaleOptions {
                dry_run: false,
                archive_path: Some(archive),
                confirm_no_effect: vec!["appr-z".to_string()],
                ..ExpireStaleOptions::default()
            },
        )
        .expect_err("the archive must be written before any ledger mutation");
        assert_eq!(approval_row(&path, "appr-z").0, "outcome_unknown");
        assert_eq!(mutation_state(&path, "mut-z"), "outcome_unknown");
    }
}
