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
    AccountEmojiStatusSpec, AudioTranscriptionSummary, ChatMemberSummary, InviteBatchProgress,
    InviteConfirmation, InviteTargetProgress, InviteTargetState, MutationAction, MutationApproval,
    MutationApprovalState, MutationExecutionOwner, MutationResult, PreparedMutation,
    PreparedMutationState,
};
use crate::ledger::open_state_connection;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const MUTATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct PreparedMutationRecord {
    pub public: PreparedMutation,
    /// Exact immutable bytes that were hashed when this mutation was prepared.
    ///
    /// Keep this separately from the typed action: deserializing an older row
    /// and serializing it with a newer schema may add defaulted fields and must
    /// not invalidate the original v1 payload binding.
    pub action_json: String,
}

#[derive(Debug, Clone)]
pub struct MutationApprovalRequestRecord {
    pub public: MutationApproval,
    pub should_dispatch_card: bool,
}

#[derive(Debug, Clone)]
pub struct MutationLedger {
    path: PathBuf,
}

impl MutationLedger {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn prepare(
        &self,
        account_user_id: i64,
        action: MutationAction,
        payload_sha256: String,
    ) -> rusqlite::Result<PreparedMutationRecord> {
        self.prepare_with_id(Uuid::new_v4().to_string(), account_user_id, action, payload_sha256)
    }

    pub fn prepare_with_id(
        &self,
        mutation_id: String,
        account_user_id: i64,
        action: MutationAction,
        payload_sha256: String,
    ) -> rusqlite::Result<PreparedMutationRecord> {
        let parsed_id = Uuid::parse_str(&mutation_id).map_err(|_| {
            state_error("prepared mutation id must be a canonical UUID".to_string())
        })?;
        if parsed_id.to_string() != mutation_id {
            return Err(state_error("prepared mutation id must be a canonical UUID".to_string()));
        }
        let connection = self.connection()?;
        let action_json = serde_json::to_string(&action).map_err(json_to_sql)?;
        let created_at = now_unix();
        connection.execute(
            "INSERT OR IGNORE INTO prepared_mutations (
                mutation_id, schema_version, account_user_id, action_json,
                payload_sha256, state, result_json, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'prepared', NULL, ?6)",
            params![
                mutation_id,
                i64::from(MUTATION_SCHEMA_VERSION),
                account_user_id,
                action_json,
                payload_sha256,
                created_at,
            ],
        )?;
        let prepared = self.get(&mutation_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        if prepared.public.account_user_id != account_user_id
            || prepared.public.payload_sha256 != payload_sha256
            || prepared.action_json != action_json
        {
            return Err(state_error(
                "prepared mutation id is already bound to a different immutable payload"
                    .to_string(),
            ));
        }
        Ok(prepared)
    }

    pub fn prepare_invite_batch(
        &self,
        account_user_id: i64,
        action: MutationAction,
        payload_sha256: String,
        initial_targets: &[InviteTargetProgress],
    ) -> rusqlite::Result<PreparedMutationRecord> {
        let MutationAction::InviteChatMembers { targets, .. } = &action else {
            return Err(state_error(
                "prepare_invite_batch requires an invite_chat_members action".to_string(),
            ));
        };
        if targets.is_empty() || targets.len() > 50 || targets.len() != initial_targets.len() {
            return Err(state_error(
                "invite batch must contain between 1 and 50 exactly bound targets".to_string(),
            ));
        }
        for (index, (target, progress)) in targets.iter().zip(initial_targets).enumerate() {
            if target.index != index || progress.target != *target {
                return Err(state_error(format!(
                    "invite target {index} does not match the immutable action binding"
                )));
            }
            match progress.state {
                InviteTargetState::Pending
                    if progress.attempt_count == 0
                        && progress.observation_count == 0
                        && progress.confirmation.is_none()
                        && progress.member.is_none() => {},
                InviteTargetState::AlreadyPresent
                    if progress.attempt_count == 0
                        && progress.observation_count == 0
                        && progress.confirmation == Some(InviteConfirmation::ActiveBeforeCall)
                        && progress.member.is_some() => {},
                InviteTargetState::Rejected
                    if progress.attempt_count == 0
                        && progress.observation_count == 0
                        && progress.confirmation.is_none()
                        && progress.last_error_code.is_some() => {},
                _ => {
                    return Err(state_error(format!(
                        "invite target {index} has an invalid initial state"
                    )));
                },
            }
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mutation_id = Uuid::new_v4().to_string();
        let action_json = serde_json::to_string(&action).map_err(json_to_sql)?;
        let created_at = now_unix();
        transaction.execute(
            "INSERT INTO prepared_mutations (
                mutation_id, schema_version, account_user_id, action_json,
                payload_sha256, state, result_json, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'prepared', NULL, ?6)",
            params![
                mutation_id,
                i64::from(MUTATION_SCHEMA_VERSION),
                account_user_id,
                action_json,
                payload_sha256,
                created_at,
            ],
        )?;
        for progress in initial_targets {
            let member_json = progress
                .member
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(json_to_sql)?;
            transaction.execute(
                "INSERT INTO mutation_invite_targets (
                    mutation_id, target_index, target_user_id, binding_sha256,
                    state, attempt_count, observation_count, next_action_at,
                    last_error_code, confirmation, member_json, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6, ?7, ?8, ?9, ?10)",
                params![
                    mutation_id,
                    i64::try_from(progress.target.index).map_err(|_| {
                        state_error("invite target index does not fit SQLite integer".to_string())
                    })?,
                    progress.target.user_id,
                    invite_target_binding_hash(&progress.target)?,
                    invite_target_state_name(progress.state),
                    progress.next_action_at_unix,
                    progress.last_error_code,
                    progress.confirmation.map(invite_confirmation_name),
                    member_json,
                    created_at,
                ],
            )?;
        }
        let prepared = get_mutation_from_connection(&transaction, &mutation_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        transaction.commit()?;
        Ok(prepared)
    }

    pub fn get(&self, mutation_id: &str) -> rusqlite::Result<Option<PreparedMutationRecord>> {
        let connection = self.connection()?;
        get_mutation_from_connection(&connection, mutation_id)
    }

    pub fn get_invite_progress(
        &self,
        mutation_id: &str,
    ) -> rusqlite::Result<Option<InviteBatchProgress>> {
        let connection = self.connection()?;
        get_invite_progress_from_connection(&connection, mutation_id)
    }

    pub fn mark_stale_invite_issuing_unknown(
        &self,
        mutation_id: &str,
        error_code: &str,
        now: i64,
    ) -> rusqlite::Result<usize> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE mutation_invite_targets
             SET state = 'outcome_unknown', next_action_at = ?3,
                 last_error_code = ?2, updated_at = ?3
             WHERE mutation_id = ?1 AND state = 'issuing'",
            params![mutation_id, error_code, now],
        )
    }

    pub fn claim_invite_target(
        &self,
        mutation_id: &str,
        target_index: usize,
        now: i64,
    ) -> rusqlite::Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_invite_targets
             SET state = 'issuing', attempt_count = attempt_count + 1,
                 next_action_at = NULL, last_error_code = NULL, updated_at = ?3
             WHERE mutation_id = ?1 AND target_index = ?2
               AND state IN ('pending', 'retry_wait')
               AND (next_action_at IS NULL OR next_action_at <= ?3)",
            params![
                mutation_id,
                i64::try_from(target_index).map_err(|_| {
                    state_error("invite target index does not fit SQLite integer".to_string())
                })?,
                now,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn mark_invite_target_confirmed(
        &self,
        mutation_id: &str,
        target_index: usize,
        member: &ChatMemberSummary,
        confirmation: InviteConfirmation,
        now: i64,
    ) -> rusqlite::Result<()> {
        let member_json = serde_json::to_string(member).map_err(json_to_sql)?;
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_invite_targets
             SET state = 'confirmed', next_action_at = NULL,
                 last_error_code = NULL, confirmation = ?4, member_json = ?5,
                 updated_at = ?6
             WHERE mutation_id = ?1 AND target_index = ?2
               AND target_user_id = ?3
               AND state IN ('issuing', 'outcome_unknown')",
            params![
                mutation_id,
                i64::try_from(target_index).map_err(|_| {
                    state_error("invite target index does not fit SQLite integer".to_string())
                })?,
                member.user.user_id,
                invite_confirmation_name(confirmation),
                member_json,
                now,
            ],
        )?;
        require_invite_target_changed(&connection, mutation_id, target_index, changed)
    }

    pub fn mark_invite_target_already_present(
        &self,
        mutation_id: &str,
        target_index: usize,
        member: &ChatMemberSummary,
        now: i64,
    ) -> rusqlite::Result<()> {
        let member_json = serde_json::to_string(member).map_err(json_to_sql)?;
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_invite_targets
             SET state = 'already_present', next_action_at = NULL,
                 last_error_code = NULL, confirmation = 'active_before_call',
                 member_json = ?4, updated_at = ?5
             WHERE mutation_id = ?1 AND target_index = ?2
               AND target_user_id = ?3
               AND state IN ('pending', 'retry_wait')",
            params![
                mutation_id,
                i64::try_from(target_index).map_err(|_| {
                    state_error("invite target index does not fit SQLite integer".to_string())
                })?,
                member.user.user_id,
                member_json,
                now,
            ],
        )?;
        require_invite_target_changed(&connection, mutation_id, target_index, changed)
    }

    pub fn mark_invite_target_preflight_rejected(
        &self,
        mutation_id: &str,
        target_index: usize,
        error_code: &str,
        member: Option<&ChatMemberSummary>,
        now: i64,
    ) -> rusqlite::Result<()> {
        let member_json = member.map(serde_json::to_string).transpose().map_err(json_to_sql)?;
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_invite_targets
             SET state = 'rejected', next_action_at = NULL,
                 last_error_code = ?3, confirmation = NULL,
                 member_json = ?4, updated_at = ?5
             WHERE mutation_id = ?1 AND target_index = ?2
               AND state IN ('pending', 'retry_wait')",
            params![
                mutation_id,
                i64::try_from(target_index).map_err(|_| {
                    state_error("invite target index does not fit SQLite integer".to_string())
                })?,
                error_code,
                member_json,
                now,
            ],
        )?;
        require_invite_target_changed(&connection, mutation_id, target_index, changed)
    }

    pub fn mark_invite_target_rejected(
        &self,
        mutation_id: &str,
        target_index: usize,
        error_code: &str,
        now: i64,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_invite_targets
             SET state = 'rejected', next_action_at = NULL,
                 last_error_code = ?3, confirmation = NULL, updated_at = ?4
             WHERE mutation_id = ?1 AND target_index = ?2 AND state = 'issuing'",
            params![
                mutation_id,
                i64::try_from(target_index).map_err(|_| {
                    state_error("invite target index does not fit SQLite integer".to_string())
                })?,
                error_code,
                now,
            ],
        )?;
        require_invite_target_changed(&connection, mutation_id, target_index, changed)
    }

    pub fn mark_invite_target_retry_wait(
        &self,
        mutation_id: &str,
        target_index: usize,
        error_code: &str,
        next_action_at: i64,
        now: i64,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_invite_targets
             SET state = 'retry_wait', next_action_at = ?4,
                 last_error_code = ?3, confirmation = NULL, updated_at = ?5
             WHERE mutation_id = ?1 AND target_index = ?2 AND state = 'issuing'",
            params![
                mutation_id,
                i64::try_from(target_index).map_err(|_| {
                    state_error("invite target index does not fit SQLite integer".to_string())
                })?,
                error_code,
                next_action_at,
                now,
            ],
        )?;
        require_invite_target_changed(&connection, mutation_id, target_index, changed)
    }

    pub fn mark_invite_target_unknown(
        &self,
        mutation_id: &str,
        target_index: usize,
        error_code: &str,
        next_action_at: Option<i64>,
        now: i64,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_invite_targets
             SET state = 'outcome_unknown', next_action_at = ?4,
                 last_error_code = ?3, confirmation = NULL, updated_at = ?5
             WHERE mutation_id = ?1 AND target_index = ?2 AND state = 'issuing'",
            params![
                mutation_id,
                i64::try_from(target_index).map_err(|_| {
                    state_error("invite target index does not fit SQLite integer".to_string())
                })?,
                error_code,
                next_action_at,
                now,
            ],
        )?;
        require_invite_target_changed(&connection, mutation_id, target_index, changed)
    }

    pub fn mark_invite_target_observed_unknown(
        &self,
        mutation_id: &str,
        target_index: usize,
        error_code: &str,
        next_action_at: Option<i64>,
        now: i64,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_invite_targets
             SET observation_count = observation_count + 1,
                 next_action_at = ?4, last_error_code = ?3, updated_at = ?5
             WHERE mutation_id = ?1 AND target_index = ?2
               AND state = 'outcome_unknown'",
            params![
                mutation_id,
                i64::try_from(target_index).map_err(|_| {
                    state_error("invite target index does not fit SQLite integer".to_string())
                })?,
                error_code,
                next_action_at,
                now,
            ],
        )?;
        require_invite_target_changed(&connection, mutation_id, target_index, changed)
    }

    pub fn reconcile_invite_parent(
        &self,
        approval_id: &str,
        now: i64,
    ) -> rusqlite::Result<MutationApproval> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mutation_id = transaction
            .query_row(
                "SELECT mutation_id FROM mutation_approvals WHERE approval_id = ?1",
                [approval_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let progress = get_invite_progress_from_connection(&transaction, &mutation_id)?
            .ok_or_else(|| {
                state_error(format!("mutation {mutation_id} is not a durable invite batch"))
            })?;
        let all_terminal = progress.targets.iter().all(|target| {
            matches!(
                target.state,
                InviteTargetState::Confirmed
                    | InviteTargetState::AlreadyPresent
                    | InviteTargetState::Rejected
            )
        });
        if all_terminal {
            let result = MutationResult::ChatMembersInvited {
                chat_id: progress.chat.chat_id.clone(),
                outcomes: progress.targets.clone(),
            };
            let result_json = serde_json::to_string(&result).map_err(json_to_sql)?;
            transaction.execute(
                "UPDATE prepared_mutations
                 SET state = 'applied', result_json = ?2
                 WHERE mutation_id = ?1",
                params![mutation_id, result_json],
            )?;
            transaction.execute(
                "UPDATE mutation_approvals
                 SET state = 'applied', next_retry_at = NULL,
                     last_error_code = NULL
                 WHERE approval_id = ?1
                   AND state IN ('applying', 'outcome_unknown', 'applied')",
                [approval_id],
            )?;
        } else {
            let has_unknown = progress
                .targets
                .iter()
                .any(|target| target.state == InviteTargetState::OutcomeUnknown);
            let next_action_at = if has_unknown {
                progress
                    .targets
                    .iter()
                    .find(|target| {
                        target.state == InviteTargetState::OutcomeUnknown
                            && target.next_action_at_unix.is_some()
                    })
                    .and_then(|target| target.next_action_at_unix)
                    .or_else(|| {
                        progress.targets.iter().find_map(|target| match target.state {
                            InviteTargetState::Pending | InviteTargetState::Issuing => {
                                Some(target.next_action_at_unix.unwrap_or(now))
                            },
                            InviteTargetState::RetryWait => target.next_action_at_unix,
                            _ => None,
                        })
                    })
            } else {
                progress.targets.iter().find_map(|target| match target.state {
                    InviteTargetState::Pending | InviteTargetState::Issuing => {
                        Some(target.next_action_at_unix.unwrap_or(now))
                    },
                    InviteTargetState::RetryWait => target.next_action_at_unix,
                    _ => None,
                })
            };
            let (approval_state, mutation_state, error_code) = if has_unknown {
                ("outcome_unknown", "outcome_unknown", "INVITE_BATCH_OUTCOME_UNKNOWN")
            } else {
                ("applying", "applying", "INVITE_BATCH_IN_PROGRESS")
            };
            transaction.execute(
                "UPDATE prepared_mutations
                 SET state = ?2, result_json = NULL
                 WHERE mutation_id = ?1 AND state != 'applied'",
                params![mutation_id, mutation_state],
            )?;
            transaction.execute(
                "UPDATE mutation_approvals
                 SET state = ?2, next_retry_at = ?3, last_error_code = ?4
                 WHERE approval_id = ?1
                   AND state IN ('applying', 'outcome_unknown')",
                params![approval_id, approval_state, next_action_at, error_code],
            )?;
        }
        let approval = get_approval_from_connection(&transaction, approval_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        transaction.commit()?;
        Ok(approval)
    }

    pub fn find_applied_transcription(
        &self,
        account_user_id: i64,
        chat_id: &str,
        message_id: i32,
    ) -> rusqlite::Result<Option<AudioTranscriptionSummary>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT action_json, result_json
             FROM prepared_mutations
             WHERE account_user_id = ?1 AND state = 'applied'
             ORDER BY created_at DESC, rowid DESC",
        )?;
        let mut rows = statement.query([account_user_id])?;
        while let Some(row) = rows.next()? {
            let action_json: String = row.get(0)?;
            let action = serde_json::from_str::<MutationAction>(&action_json)
                .map_err(|error| json_from_sql(0, error))?;
            let MutationAction::TranscribeAudio { chat_id: action_chat_id, before } = action else {
                continue;
            };
            if action_chat_id != chat_id || before.message_id != message_id {
                continue;
            }
            let result_json: Option<String> = row.get(1)?;
            let Some(result_json) = result_json else {
                return Err(state_error(
                    "an applied transcription mutation has no result receipt".to_string(),
                ));
            };
            let result = serde_json::from_str::<MutationResult>(&result_json)
                .map_err(|error| json_from_sql(1, error))?;
            return match result {
                MutationResult::AudioTranscribed { transcription }
                    if transcription.chat_id == chat_id
                        && transcription.message_id == message_id =>
                {
                    Ok(Some(transcription))
                },
                _ => Err(state_error(
                    "an applied transcription mutation is bound to an incompatible result"
                        .to_string(),
                )),
            };
        }
        Ok(None)
    }

    pub fn create_or_get_approval(
        &self,
        mutation_id: &str,
        payload_sha256: &str,
        approval_chat_id: i64,
        approval_user_id: i64,
        ttl_seconds: u64,
        purpose: Option<&str>,
    ) -> rusqlite::Result<MutationApprovalRequestRecord> {
        self.create_or_get_approval_with_owner(
            mutation_id,
            payload_sha256,
            approval_chat_id,
            approval_user_id,
            ttl_seconds,
            purpose,
            MutationExecutionOwner::ManagedMcp,
        )
    }

    pub fn create_or_get_external_owner_approval(
        &self,
        mutation_id: &str,
        payload_sha256: &str,
        approval_chat_id: i64,
        approval_user_id: i64,
        ttl_seconds: u64,
        purpose: Option<&str>,
    ) -> rusqlite::Result<MutationApprovalRequestRecord> {
        self.create_or_get_approval_with_owner(
            mutation_id,
            payload_sha256,
            approval_chat_id,
            approval_user_id,
            ttl_seconds,
            purpose,
            MutationExecutionOwner::ExternalOwner,
        )
    }

    fn create_or_get_approval_with_owner(
        &self,
        mutation_id: &str,
        payload_sha256: &str,
        approval_chat_id: i64,
        approval_user_id: i64,
        ttl_seconds: u64,
        purpose: Option<&str>,
        execution_owner: MutationExecutionOwner,
    ) -> rusqlite::Result<MutationApprovalRequestRecord> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = now_unix();
        expire_due_approvals(&transaction, now)?;
        let prepared = get_mutation_from_connection(&transaction, mutation_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        if prepared.public.payload_sha256 != payload_sha256 {
            return Err(state_error(
                "approved mutation payload hash does not match the prepared mutation".to_string(),
            ));
        }
        if let Some(existing) = get_latest_reusable_approval(&transaction, mutation_id)? {
            if existing.execution_owner != execution_owner {
                return Err(state_error(format!(
                    "mutation {mutation_id} already has an approval owned by {:?}",
                    existing.execution_owner
                )));
            }
            transaction.commit()?;
            return Ok(MutationApprovalRequestRecord {
                public: existing,
                should_dispatch_card: false,
            });
        }

        let approval_id = Uuid::new_v4().to_string();
        let ttl = i64::try_from(ttl_seconds).unwrap_or(i64::MAX);
        let expires_at = mutation_action_approval_deadline_unix(&prepared.public.action)
            .map_or_else(
                || now.saturating_add(ttl),
                |deadline| now.saturating_add(ttl).min(deadline),
            );
        let insert = transaction.execute(
            "INSERT INTO mutation_approvals (
                approval_id, mutation_id, payload_sha256, state,
                approval_chat_id, approval_user_id, bot_message_id,
                created_at, expires_at, decided_at,
                automated_retry_count, next_retry_at, last_error_code, last_error_message,
                purpose
             ) VALUES (
                ?1, ?2, ?3, 'dispatching_card', ?4, ?5, NULL,
                ?6, ?7, NULL, 0, NULL, NULL, NULL,
                ?8
             )",
            params![
                approval_id,
                mutation_id,
                payload_sha256,
                approval_chat_id,
                approval_user_id,
                now,
                expires_at,
                purpose,
            ],
        );
        if let Err(error) = insert {
            if let Some(existing) = get_latest_reusable_approval(&transaction, mutation_id)? {
                if existing.execution_owner != execution_owner {
                    return Err(state_error(format!(
                        "mutation {mutation_id} already has an approval owned by {:?}",
                        existing.execution_owner
                    )));
                }
                transaction.commit()?;
                return Ok(MutationApprovalRequestRecord {
                    public: existing,
                    should_dispatch_card: false,
                });
            }
            return Err(error);
        }
        if execution_owner == MutationExecutionOwner::ExternalOwner {
            transaction.execute(
                "INSERT INTO mutation_external_handoffs (
                    approval_id, receipt, approved_at, consumed_at
                 ) VALUES (?1, ?2, NULL, NULL)",
                params![approval_id, Uuid::new_v4().to_string()],
            )?;
        }
        let approval = get_approval_from_connection(&transaction, &approval_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        transaction.commit()?;
        Ok(MutationApprovalRequestRecord { public: approval, should_dispatch_card: true })
    }

    pub fn mark_external_owner_approved(
        &self,
        approval_id: &str,
        now: i64,
    ) -> rusqlite::Result<bool> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "UPDATE mutation_external_handoffs
             SET approved_at = COALESCE(approved_at, ?2)
             WHERE approval_id = ?1
               AND approved_at IS NULL
               AND consumed_at IS NULL
               AND EXISTS (
                   SELECT 1
                   FROM mutation_approvals
                   WHERE approval_id = ?1
                     AND state = 'pending'
                     AND expires_at > ?2
               )",
            params![approval_id, now],
        )?;
        if changed > 0 {
            transaction.execute(
                "UPDATE mutation_approvals
                 SET decided_at = COALESCE(decided_at, ?2),
                     next_retry_at = NULL,
                     last_error_code = NULL
                 WHERE approval_id = ?1 AND state = 'pending'",
                params![approval_id, now],
            )?;
        }
        let ready = transaction.query_row(
            "SELECT EXISTS (
                 SELECT 1
                 FROM mutation_external_handoffs h
                 JOIN mutation_approvals a ON a.approval_id = h.approval_id
                 WHERE h.approval_id = ?1
                   AND h.approved_at IS NOT NULL
                   AND h.consumed_at IS NULL
                   AND a.state = 'pending'
             )",
            [approval_id],
            |row| row.get::<_, bool>(0),
        )?;
        transaction.commit()?;
        Ok(changed > 0 || ready)
    }

    pub fn start_external_owner_scheduled_approval(
        &self,
        mutation_id: &str,
        payload_sha256: &str,
        approval_id: &str,
        external_owner_receipt: &str,
    ) -> rusqlite::Result<MutationApproval> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = now_unix();
        expire_due_approvals(&transaction, now)?;
        let prepared = get_mutation_from_connection(&transaction, mutation_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        if prepared.public.payload_sha256 != payload_sha256 {
            return Err(state_error(
                "external-owner payload hash does not match the prepared mutation".to_string(),
            ));
        }
        let schedule_deadline = mutation_action_approval_deadline_unix(&prepared.public.action)
            .ok_or_else(|| {
                state_error(
                    "external-owner application is only legal for scheduled mutations".to_string(),
                )
            })?;
        if schedule_deadline <= now {
            return Err(state_error(
                "scheduled delivery time elapsed before external-owner application".to_string(),
            ));
        }
        let approval = get_approval_from_connection(&transaction, approval_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        if approval.mutation_id != mutation_id
            || approval.payload_sha256 != payload_sha256
            || approval.execution_owner != MutationExecutionOwner::ExternalOwner
            || !approval.external_owner_ready
            || approval.external_owner_receipt.as_deref() != Some(external_owner_receipt)
        {
            return Err(state_error(
                "external-owner approval receipt is not bound to the exact ready mutation"
                    .to_string(),
            ));
        }
        let changed = transaction.execute(
            "UPDATE mutation_approvals
             SET state = 'applying', next_retry_at = ?2,
                 last_error_code = NULL, last_error_message = NULL
             WHERE approval_id = ?1
               AND state = 'pending'
               AND expires_at > ?2",
            params![approval_id, now],
        )?;
        if changed != 1 {
            return Err(state_error("external-owner approval is no longer claimable".to_string()));
        }
        let consumed = transaction.execute(
            "UPDATE mutation_external_handoffs
             SET consumed_at = ?2
             WHERE approval_id = ?1
               AND approved_at IS NOT NULL
               AND consumed_at IS NULL
               AND receipt = ?3",
            params![approval_id, now, external_owner_receipt],
        )?;
        if consumed != 1 {
            return Err(state_error(
                "external-owner approval receipt was already consumed".to_string(),
            ));
        }
        transaction.execute(
            "UPDATE prepared_mutations
             SET state = 'applying'
             WHERE mutation_id = ?1 AND state != 'applied'",
            [mutation_id],
        )?;
        let approval = get_approval_from_connection(&transaction, approval_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        transaction.commit()?;
        Ok(approval)
    }

    /// Starts one exact already-prepared scheduled mutation under explicit
    /// current-session authority without creating an operator-bot card.
    ///
    /// This transition is deliberately narrow: it does not reuse a card-bound
    /// approval, and an interrupted/unknown direct application stays unresolved
    /// instead of being replayed automatically.
    pub fn start_current_session_scheduled_approval(
        &self,
        mutation_id: &str,
        payload_sha256: &str,
        approval_chat_id: i64,
        approval_user_id: i64,
    ) -> rusqlite::Result<MutationApproval> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = now_unix();
        expire_due_approvals(&transaction, now)?;
        let prepared = get_mutation_from_connection(&transaction, mutation_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        if prepared.public.payload_sha256 != payload_sha256 {
            return Err(state_error(
                "current-session payload hash does not match the prepared mutation".to_string(),
            ));
        }
        let schedule_deadline = mutation_action_approval_deadline_unix(&prepared.public.action)
            .ok_or_else(|| {
                state_error(
                    "current-session direct application is only legal for scheduled mutations"
                        .to_string(),
                )
            })?;
        if schedule_deadline <= now {
            return Err(state_error(
                "scheduled delivery time elapsed before current-session application".to_string(),
            ));
        }
        if let Some(existing) = get_latest_reusable_approval(&transaction, mutation_id)? {
            transaction.commit()?;
            if existing.approval_message_id.is_some() {
                return Err(state_error(
                    "prepared mutation already has a card-bound approval and cannot bypass it"
                        .to_string(),
                ));
            }
            return match existing.state {
                MutationApprovalState::Applied => Ok(existing),
                MutationApprovalState::Applying | MutationApprovalState::OutcomeUnknown => {
                    Err(state_error(
                        "current-session scheduled mutation has an unresolved provider outcome and will not be replayed"
                            .to_string(),
                    ))
                },
                state => Err(state_error(format!(
                    "current-session scheduled mutation has incompatible existing approval state {state:?}"
                ))),
            };
        }

        let approval_id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO mutation_approvals (
                approval_id, mutation_id, payload_sha256, state,
                approval_chat_id, approval_user_id, bot_message_id,
                created_at, expires_at, decided_at,
                automated_retry_count, next_retry_at, last_error_code, last_error_message
             ) VALUES (
                ?1, ?2, ?3, 'applying', ?4, ?5, NULL,
                ?6, ?7, ?6, 0, NULL, NULL, NULL
             )",
            params![
                approval_id,
                mutation_id,
                payload_sha256,
                approval_chat_id,
                approval_user_id,
                now,
                schedule_deadline,
            ],
        )?;
        transaction.execute(
            "UPDATE prepared_mutations
             SET state = 'applying'
             WHERE mutation_id = ?1 AND state != 'applied'",
            [mutation_id],
        )?;
        let approval = get_approval_from_connection(&transaction, &approval_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        transaction.commit()?;
        Ok(approval)
    }

    pub fn get_approval(&self, approval_id: &str) -> rusqlite::Result<Option<MutationApproval>> {
        let connection = self.connection()?;
        get_approval_from_connection(&connection, approval_id)
    }

    /// A granted mutation needs no card at all: move it straight from
    /// `dispatching_card` to `pending`, the state the apply path accepts, and
    /// leave no bot message behind.
    pub fn mark_approval_card_skipped(&self, approval_id: &str) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_approvals
             SET state = 'pending', last_error_code = NULL
             WHERE approval_id = ?1 AND state = 'dispatching_card'",
            params![approval_id],
        )?;
        if changed > 0 {
            return Ok(());
        }
        let existing =
            self.get_approval(approval_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        if existing.state == MutationApprovalState::Pending {
            Ok(())
        } else {
            Err(state_error(format!(
                "mutation approval {approval_id} could not skip its card from state {:?}",
                existing.state
            )))
        }
    }

    pub fn mark_approval_card_delivered(
        &self,
        approval_id: &str,
        message_id: i64,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_approvals
             SET state = 'pending', bot_message_id = ?2, last_error_code = NULL
             WHERE approval_id = ?1 AND state = 'dispatching_card'",
            params![approval_id, message_id],
        )?;
        if changed > 0 {
            return Ok(());
        }
        let existing =
            self.get_approval(approval_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        if existing.state == MutationApprovalState::Pending
            && existing.approval_message_id == Some(message_id)
        {
            Ok(())
        } else {
            Err(state_error(format!(
                "mutation approval {approval_id} could not bind bot message {message_id}"
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
            "UPDATE mutation_approvals
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
            "UPDATE mutation_approvals
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
            "UPDATE mutation_approvals
             SET state = 'failed', last_error_code = ?2, next_retry_at = NULL
             WHERE approval_id = ?1
               AND state IN ('dispatching_card', 'card_delivery_uncertain')",
            params![approval_id, error_code],
        )?;
        require_approval_existing_or_changed(&connection, approval_id, changed)
    }

    pub fn mark_approval_applying(&self, approval_id: &str, now: i64) -> rusqlite::Result<bool> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mutation_id = transaction
            .query_row(
                "SELECT mutation_id FROM mutation_approvals WHERE approval_id = ?1",
                [approval_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(mutation_id) = mutation_id else {
            transaction.commit()?;
            return Ok(false);
        };
        let changed = transaction.execute(
            "UPDATE mutation_approvals
             SET state = 'applying', decided_at = COALESCE(decided_at, ?2),
                 next_retry_at = ?2, last_error_code = NULL,
                 last_error_message = NULL
            WHERE approval_id = ?1
              AND state IN ('pending', 'dispatching_card')
              AND NOT EXISTS (
                  SELECT 1 FROM mutation_external_handoffs
                  WHERE approval_id = ?1
              )
              AND expires_at > ?2",
            params![approval_id, now],
        )?;
        if changed > 0 {
            transaction.execute(
                "UPDATE prepared_mutations
                 SET state = 'applying'
                 WHERE mutation_id = ?1 AND state != 'applied'",
                [mutation_id],
            )?;
        }
        let state = transaction
            .query_row(
                "SELECT state, next_retry_at
                 FROM mutation_approvals WHERE approval_id = ?1",
                [approval_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .optional()?;
        transaction.commit()?;
        Ok(changed > 0
            || matches!(state, Some((ref state, Some(next))) if state == "applying" && next <= now))
    }

    pub fn cancel_approval(&self, approval_id: &str, now: i64) -> rusqlite::Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_approvals
             SET state = 'cancelled', decided_at = COALESCE(decided_at, ?2),
                 next_retry_at = NULL, last_error_code = NULL
            WHERE approval_id = ?1
               AND state = 'pending'
               AND NOT EXISTS (
                   SELECT 1 FROM mutation_external_handoffs
                   WHERE approval_id = ?1
                     AND approved_at IS NOT NULL
                     AND consumed_at IS NULL
               )
               AND expires_at > ?2",
            params![approval_id, now],
        )?;
        Ok(changed > 0)
    }

    pub fn expire_approval(&self, approval_id: &str, now: i64) -> rusqlite::Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_approvals
             SET state = 'expired', decided_at = COALESCE(decided_at, ?2),
                 next_retry_at = NULL
             WHERE approval_id = ?1
               AND state IN ('dispatching_card', 'pending', 'card_delivery_uncertain')
               AND expires_at <= ?2",
            params![approval_id, now],
        )?;
        Ok(changed > 0)
    }

    pub fn expire_approval_due_action(
        &self,
        approval_id: &str,
        now: i64,
        error_code: &str,
    ) -> rusqlite::Result<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_approvals
             SET state = 'expired', decided_at = COALESCE(decided_at, ?2),
                 next_retry_at = NULL, last_error_code = ?3
             WHERE approval_id = ?1
               AND state IN ('dispatching_card', 'pending', 'card_delivery_uncertain')",
            params![approval_id, now, error_code],
        )?;
        Ok(changed > 0)
    }

    pub fn mark_applied(&self, approval_id: &str, result: &MutationResult) -> rusqlite::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (mutation_id, approval_state) = transaction
            .query_row(
                "SELECT mutation_id, state FROM mutation_approvals WHERE approval_id = ?1",
                [approval_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let result_json = serde_json::to_string(result).map_err(json_to_sql)?;
        let existing_result = transaction
            .query_row(
                "SELECT result_json FROM prepared_mutations WHERE mutation_id = ?1",
                [&mutation_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        if approval_state == "applied" {
            return if existing_result.as_deref() == Some(result_json.as_str()) {
                transaction.commit()
            } else {
                Err(state_error(format!(
                    "mutation {mutation_id} is already bound to a different result"
                )))
            };
        }
        if !matches!(approval_state.as_str(), "applying" | "outcome_unknown") {
            return Err(state_error(format!(
                "mutation approval {approval_id} cannot become applied from {approval_state}"
            )));
        }
        transaction.execute(
            "UPDATE prepared_mutations
             SET state = 'applied', result_json = ?2
             WHERE mutation_id = ?1",
            params![mutation_id, result_json],
        )?;
        transaction.execute(
            "UPDATE mutation_approvals
             SET state = 'applied', next_retry_at = NULL,
                 last_error_code = NULL, last_error_message = NULL
             WHERE approval_id = ?1",
            [approval_id],
        )?;
        transaction.commit()
    }

    pub fn mark_outcome_unknown(
        &self,
        approval_id: &str,
        error_code: &str,
        next_retry_at: Option<i64>,
    ) -> rusqlite::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mutation_id = transaction
            .query_row(
                "SELECT mutation_id FROM mutation_approvals WHERE approval_id = ?1",
                [approval_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let changed = transaction.execute(
            "UPDATE mutation_approvals
             SET state = 'outcome_unknown',
                 automated_retry_count = automated_retry_count + 1,
                 next_retry_at = ?3, last_error_code = ?2,
                 last_error_message = NULL
             WHERE approval_id = ?1 AND state IN ('applying', 'outcome_unknown')",
            params![approval_id, error_code, next_retry_at],
        )?;
        if changed == 0 {
            return Err(state_error(format!(
                "mutation approval {approval_id} cannot become outcome_unknown"
            )));
        }
        transaction.execute(
            "UPDATE prepared_mutations
             SET state = 'outcome_unknown'
             WHERE mutation_id = ?1 AND state != 'applied'",
            [mutation_id],
        )?;
        transaction.commit()
    }

    pub fn mark_known_failure(
        &self,
        approval_id: &str,
        error_code: &str,
        now: i64,
    ) -> rusqlite::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mutation_id = transaction
            .query_row(
                "SELECT mutation_id FROM mutation_approvals WHERE approval_id = ?1",
                [approval_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let changed = transaction.execute(
            "UPDATE mutation_approvals
             SET state = 'failed', decided_at = COALESCE(decided_at, ?3),
                 next_retry_at = NULL, last_error_code = ?2
             WHERE approval_id = ?1 AND state IN ('applying', 'outcome_unknown')",
            params![approval_id, error_code, now],
        )?;
        if changed == 0 {
            return Err(state_error(format!(
                "mutation approval {approval_id} cannot become failed"
            )));
        }
        transaction.execute(
            "UPDATE prepared_mutations
             SET state = 'prepared'
             WHERE mutation_id = ?1 AND state != 'applied'",
            [mutation_id],
        )?;
        transaction.commit()
    }

    pub fn set_last_error_message(
        &self,
        approval_id: &str,
        message: Option<&str>,
    ) -> rusqlite::Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mutation_approvals
             SET last_error_message = ?2
             WHERE approval_id = ?1",
            params![approval_id, message],
        )?;
        require_approval_existing_or_changed(&connection, approval_id, changed)
    }

    pub fn mark_retryable_failure(
        &self,
        approval_id: &str,
        error_code: &str,
        next_retry_at: i64,
    ) -> rusqlite::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mutation_id = transaction
            .query_row(
                "SELECT mutation_id FROM mutation_approvals WHERE approval_id = ?1",
                [approval_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let changed = transaction.execute(
            "UPDATE mutation_approvals
             SET state = 'applying',
                 automated_retry_count = automated_retry_count + 1,
                 next_retry_at = ?3, last_error_code = ?2,
                 last_error_message = NULL
             WHERE approval_id = ?1 AND state IN ('applying', 'outcome_unknown')",
            params![approval_id, error_code, next_retry_at],
        )?;
        if changed == 0 {
            return Err(state_error(format!(
                "mutation approval {approval_id} cannot schedule a retry"
            )));
        }
        transaction.execute(
            "UPDATE prepared_mutations
             SET state = 'applying'
             WHERE mutation_id = ?1 AND state != 'applied'",
            [mutation_id],
        )?;
        transaction.commit()
    }

    pub fn claim_due_recovery(&self, approval_id: &str, now: i64) -> rusqlite::Result<bool> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mutation_id = transaction
            .query_row(
                "SELECT mutation_id FROM mutation_approvals WHERE approval_id = ?1",
                [approval_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(mutation_id) = mutation_id else {
            transaction.commit()?;
            return Ok(false);
        };
        let changed = transaction.execute(
            "UPDATE mutation_approvals
             SET state = 'applying', next_retry_at = ?2,
                 last_error_message = NULL
             WHERE approval_id = ?1
               AND state = 'outcome_unknown'
               AND next_retry_at IS NOT NULL
               AND next_retry_at <= ?2",
            params![approval_id, now],
        )?;
        if changed > 0 {
            transaction.execute(
                "UPDATE prepared_mutations
                 SET state = 'applying'
                 WHERE mutation_id = ?1 AND state != 'applied'",
                [mutation_id],
            )?;
        }
        let state = transaction
            .query_row(
                "SELECT state, next_retry_at
                 FROM mutation_approvals WHERE approval_id = ?1",
                [approval_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .optional()?;
        transaction.commit()?;
        Ok(changed > 0
            || matches!(state, Some((ref state, Some(next))) if state == "applying" && next <= now))
    }

    pub fn due_recoveries(
        &self,
        now: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<MutationApproval>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT a.approval_id, a.mutation_id, p.schema_version,
                    p.account_user_id, p.action_json, a.payload_sha256, a.state,
                    a.approval_chat_id, a.approval_user_id, a.bot_message_id,
                    a.created_at, a.expires_at, p.result_json,
                    a.automated_retry_count, a.next_retry_at, a.last_error_code,
                    a.last_error_message, a.purpose,
                    h.receipt, h.approved_at, h.consumed_at
             FROM mutation_approvals a
             JOIN prepared_mutations p ON p.mutation_id = a.mutation_id
             LEFT JOIN mutation_external_handoffs h ON h.approval_id = a.approval_id
             WHERE a.state IN ('applying', 'outcome_unknown')
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
    ) -> rusqlite::Result<Vec<MutationApproval>> {
        let connection = self.connection()?;
        let approval_ids = {
            let mut statement = connection.prepare(
                "SELECT a.approval_id
                 FROM mutation_approvals a
                 JOIN prepared_mutations p ON p.mutation_id = a.mutation_id
                 WHERE a.state IN (
                     'dispatching_card', 'pending', 'card_delivery_uncertain'
                 )
                   AND (
                       a.expires_at <= ?1
                       OR (
                           json_extract(p.action_json, '$.kind') IN ('schedule_message', 'schedule_file', 'edit_scheduled_message')
                           AND CAST(json_extract(p.action_json, '$.schedule_date_unix') AS INTEGER)
                               != 2147483646
                           AND CAST(json_extract(p.action_json, '$.schedule_date_unix') AS INTEGER)
                               <= ?1
                       )
                   )
                 ORDER BY MIN(
                     a.expires_at,
                     CASE
                         WHEN json_extract(p.action_json, '$.kind') IN ('schedule_message', 'schedule_file', 'edit_scheduled_message')
                         THEN CAST(json_extract(p.action_json, '$.schedule_date_unix') AS INTEGER)
                         ELSE a.expires_at
                     END
                 ) ASC
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
            let current = get_approval_from_connection(&connection, &approval_id)?
                .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            let action_time_elapsed = matches!(
                current.action,
                MutationAction::ScheduleMessage { schedule_date_unix, .. }
                | MutationAction::ScheduleFile { schedule_date_unix, .. }
                | MutationAction::EditScheduledMessage { schedule_date_unix, .. }
                    if schedule_date_unix != 0x7fff_fffe
                        && i64::from(schedule_date_unix) <= now
            );
            let error_code = action_time_elapsed.then_some("SCHEDULE_TIME_ELAPSED");
            connection.execute(
                "UPDATE mutation_approvals
                 SET state = 'expired', decided_at = COALESCE(decided_at, ?2),
                     next_retry_at = NULL,
                     last_error_code = COALESCE(?3, last_error_code)
                 WHERE approval_id = ?1
                   AND state IN ('dispatching_card', 'pending', 'card_delivery_uncertain')",
                params![approval_id, now, error_code],
            )?;
            if let Some(approval) = get_approval_from_connection(&connection, &approval_id)? {
                expired.push(approval);
            }
        }
        Ok(expired)
    }

    fn connection(&self) -> rusqlite::Result<Connection> {
        let connection = open_state_connection(&self.path)?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS prepared_mutations (
                 mutation_id TEXT PRIMARY KEY,
                 schema_version INTEGER NOT NULL CHECK (schema_version = 1),
                 account_user_id INTEGER NOT NULL,
                 action_json TEXT NOT NULL,
                 payload_sha256 TEXT NOT NULL,
                 state TEXT NOT NULL CHECK (
                     state IN ('prepared', 'applying', 'applied', 'outcome_unknown')
                 ),
                 result_json TEXT,
                 created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS mutation_approvals (
                 approval_id TEXT PRIMARY KEY,
                 mutation_id TEXT NOT NULL REFERENCES prepared_mutations(mutation_id),
                 payload_sha256 TEXT NOT NULL,
                 state TEXT NOT NULL CHECK (
                     state IN (
                         'dispatching_card', 'pending', 'card_delivery_uncertain',
                         'applying', 'outcome_unknown', 'applied', 'cancelled',
                         'expired', 'failed'
                     )
                 ),
                 approval_chat_id INTEGER NOT NULL,
                 approval_user_id INTEGER NOT NULL,
                 bot_message_id INTEGER,
                 created_at INTEGER NOT NULL,
                 expires_at INTEGER NOT NULL,
                 decided_at INTEGER,
                 automated_retry_count INTEGER NOT NULL DEFAULT 0,
                 next_retry_at INTEGER,
                 last_error_code TEXT,
                 last_error_message TEXT,
                 purpose TEXT
             );
             CREATE TABLE IF NOT EXISTS mutation_external_handoffs (
                 approval_id TEXT PRIMARY KEY REFERENCES mutation_approvals(approval_id),
                 receipt TEXT NOT NULL UNIQUE,
                 approved_at INTEGER,
                 consumed_at INTEGER,
                 CHECK (approved_at IS NULL OR approved_at > 0),
                 CHECK (consumed_at IS NULL OR consumed_at > 0)
             );
             CREATE TABLE IF NOT EXISTS mutation_invite_targets (
                 mutation_id TEXT NOT NULL REFERENCES prepared_mutations(mutation_id),
                 target_index INTEGER NOT NULL CHECK (target_index BETWEEN 0 AND 49),
                 target_user_id INTEGER NOT NULL,
                 binding_sha256 TEXT NOT NULL,
                 state TEXT NOT NULL CHECK (
                     state IN (
                         'pending', 'issuing', 'retry_wait', 'confirmed',
                         'already_present', 'rejected', 'outcome_unknown'
                     )
                 ),
                 attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
                 observation_count INTEGER NOT NULL DEFAULT 0 CHECK (observation_count >= 0),
                 next_action_at INTEGER,
                 last_error_code TEXT,
                 confirmation TEXT CHECK (
                     confirmation IS NULL OR confirmation IN (
                         'active_before_call', 'provider_receipt_and_readback',
                         'active_observed_after_uncertain'
                     )
                 ),
                 member_json TEXT,
                 updated_at INTEGER NOT NULL,
                 PRIMARY KEY (mutation_id, target_index),
                 UNIQUE (mutation_id, target_user_id)
             );
             CREATE UNIQUE INDEX IF NOT EXISTS mutation_approvals_one_active_per_mutation
             ON mutation_approvals(mutation_id)
             WHERE state IN (
                 'dispatching_card', 'pending', 'card_delivery_uncertain',
                 'applying', 'outcome_unknown'
             );",
        )?;
        let has_last_error_message = {
            let mut statement = connection.prepare("PRAGMA table_info(mutation_approvals)")?;
            let mut rows = statement.query([])?;
            let mut found = false;
            while let Some(row) = rows.next()? {
                let column_name: String = row.get(1)?;
                if column_name == "last_error_message" {
                    found = true;
                    break;
                }
            }
            found
        };
        if !has_last_error_message {
            connection
                .execute("ALTER TABLE mutation_approvals ADD COLUMN last_error_message TEXT", [])?;
        }
        let has_purpose = {
            let mut statement = connection.prepare("PRAGMA table_info(mutation_approvals)")?;
            let mut rows = statement.query([])?;
            let mut found = false;
            while let Some(row) = rows.next()? {
                let column_name: String = row.get(1)?;
                if column_name == "purpose" {
                    found = true;
                    break;
                }
            }
            found
        };
        if !has_purpose {
            connection
                .execute("ALTER TABLE mutation_approvals ADD COLUMN purpose TEXT", [])?;
        }
        Ok(connection)
    }
}

fn get_mutation_from_connection(
    connection: &Connection,
    mutation_id: &str,
) -> rusqlite::Result<Option<PreparedMutationRecord>> {
    connection
        .query_row(
            "SELECT schema_version, account_user_id, action_json,
                    payload_sha256, state, result_json, created_at
             FROM prepared_mutations WHERE mutation_id = ?1",
            [mutation_id],
            |row| {
                let schema_version_raw: i64 = row.get(0)?;
                let action_json: String = row.get(2)?;
                let state: String = row.get(4)?;
                let result_json: Option<String> = row.get(5)?;
                Ok(PreparedMutationRecord {
                    public: PreparedMutation {
                        mutation_id: mutation_id.to_string(),
                        schema_version: u32::try_from(schema_version_raw).map_err(|_| {
                            state_error(format!(
                                "invalid mutation schema version: {schema_version_raw}"
                            ))
                        })?,
                        account_user_id: row.get(1)?,
                        action: serde_json::from_str(&action_json)
                            .map_err(|error| json_from_sql(2, error))?,
                        payload_sha256: row.get(3)?,
                        state: parse_mutation_state(&state)?,
                        result: result_json
                            .map(|json| {
                                serde_json::from_str(&json).map_err(|error| json_from_sql(5, error))
                            })
                            .transpose()?,
                        created_at_unix: row.get(6)?,
                    },
                    action_json,
                })
            },
        )
        .optional()
}

fn get_invite_progress_from_connection(
    connection: &Connection,
    mutation_id: &str,
) -> rusqlite::Result<Option<InviteBatchProgress>> {
    let Some(prepared) = get_mutation_from_connection(connection, mutation_id)? else {
        return Ok(None);
    };
    let MutationAction::InviteChatMembers { chat, targets } = prepared.public.action else {
        return Ok(None);
    };
    let rows = {
        let mut statement = connection.prepare(
            "SELECT target_index, target_user_id, binding_sha256, state,
                    attempt_count, observation_count, next_action_at,
                    last_error_code, confirmation, member_json
             FROM mutation_invite_targets
             WHERE mutation_id = ?1
             ORDER BY target_index ASC",
        )?;
        statement
            .query_map([mutation_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    if rows.len() != targets.len() {
        return Err(state_error(format!(
            "invite mutation {mutation_id} has {} immutable targets but {} progress rows",
            targets.len(),
            rows.len()
        )));
    }
    let mut progress_targets = Vec::with_capacity(targets.len());
    for (expected_index, (target, row)) in targets.into_iter().zip(rows).enumerate() {
        let (
            target_index,
            target_user_id,
            binding_sha256,
            state,
            attempt_count,
            observation_count,
            next_action_at_unix,
            last_error_code,
            confirmation,
            member_json,
        ) = row;
        let stored_index = usize::try_from(target_index).map_err(|_| {
            state_error(format!(
                "invite mutation {mutation_id} has invalid target index {target_index}"
            ))
        })?;
        if stored_index != expected_index
            || target.index != expected_index
            || target_user_id != target.user_id
            || binding_sha256 != invite_target_binding_hash(&target)?
        {
            return Err(state_error(format!(
                "invite mutation {mutation_id} target {expected_index} does not match its immutable binding"
            )));
        }
        let member = member_json
            .map(|json| {
                serde_json::from_str::<ChatMemberSummary>(&json)
                    .map_err(|error| json_from_sql(9, error))
            })
            .transpose()?;
        if member.as_ref().is_some_and(|member| {
            member.user.user_id != target.user_id || member.chat_id != chat.chat_id
        }) {
            return Err(state_error(format!(
                "invite mutation {mutation_id} target {expected_index} has a mismatched member receipt"
            )));
        }
        progress_targets.push(InviteTargetProgress {
            target,
            state: parse_invite_target_state(&state)?,
            attempt_count: u32::try_from(attempt_count).map_err(|_| {
                state_error(format!(
                    "invite mutation {mutation_id} target {expected_index} has invalid attempt count {attempt_count}"
                ))
            })?,
            observation_count: u32::try_from(observation_count).map_err(|_| {
                state_error(format!(
                    "invite mutation {mutation_id} target {expected_index} has invalid observation count {observation_count}"
                ))
            })?,
            next_action_at_unix,
            last_error_code,
            confirmation: confirmation
                .as_deref()
                .map(parse_invite_confirmation)
                .transpose()?,
            member,
        });
    }
    let mut progress = InviteBatchProgress {
        chat,
        total_count: progress_targets.len(),
        pending_count: 0,
        issuing_count: 0,
        retry_wait_count: 0,
        confirmed_count: 0,
        already_present_count: 0,
        rejected_count: 0,
        outcome_unknown_count: 0,
        next_action_at_unix: None,
        targets: progress_targets,
    };
    for target in &progress.targets {
        match target.state {
            InviteTargetState::Pending => {
                progress.pending_count = progress.pending_count.saturating_add(1);
            },
            InviteTargetState::Issuing => {
                progress.issuing_count = progress.issuing_count.saturating_add(1);
            },
            InviteTargetState::RetryWait => {
                progress.retry_wait_count = progress.retry_wait_count.saturating_add(1);
            },
            InviteTargetState::Confirmed => {
                progress.confirmed_count = progress.confirmed_count.saturating_add(1);
            },
            InviteTargetState::AlreadyPresent => {
                progress.already_present_count = progress.already_present_count.saturating_add(1);
            },
            InviteTargetState::Rejected => {
                progress.rejected_count = progress.rejected_count.saturating_add(1);
            },
            InviteTargetState::OutcomeUnknown => {
                progress.outcome_unknown_count = progress.outcome_unknown_count.saturating_add(1);
            },
        }
        if let Some(next) = target.next_action_at_unix {
            progress.next_action_at_unix =
                Some(progress.next_action_at_unix.map_or(next, |current| current.min(next)));
        }
    }
    Ok(Some(progress))
}

fn get_approval_from_connection(
    connection: &Connection,
    approval_id: &str,
) -> rusqlite::Result<Option<MutationApproval>> {
    connection
        .query_row(
            "SELECT a.approval_id, a.mutation_id, p.schema_version,
                    p.account_user_id, p.action_json, a.payload_sha256, a.state,
                    a.approval_chat_id, a.approval_user_id, a.bot_message_id,
                    a.created_at, a.expires_at, p.result_json,
                    a.automated_retry_count, a.next_retry_at, a.last_error_code,
                    a.last_error_message, a.purpose,
                    h.receipt, h.approved_at, h.consumed_at
             FROM mutation_approvals a
             JOIN prepared_mutations p ON p.mutation_id = a.mutation_id
             LEFT JOIN mutation_external_handoffs h ON h.approval_id = a.approval_id
             WHERE a.approval_id = ?1",
            [approval_id],
            map_approval_row,
        )
        .optional()
}

fn get_latest_reusable_approval(
    connection: &Connection,
    mutation_id: &str,
) -> rusqlite::Result<Option<MutationApproval>> {
    connection
        .query_row(
            "SELECT a.approval_id, a.mutation_id, p.schema_version,
                    p.account_user_id, p.action_json, a.payload_sha256, a.state,
                    a.approval_chat_id, a.approval_user_id, a.bot_message_id,
                    a.created_at, a.expires_at, p.result_json,
                    a.automated_retry_count, a.next_retry_at, a.last_error_code,
                    a.last_error_message, a.purpose,
                    h.receipt, h.approved_at, h.consumed_at
             FROM mutation_approvals a
             JOIN prepared_mutations p ON p.mutation_id = a.mutation_id
             LEFT JOIN mutation_external_handoffs h ON h.approval_id = a.approval_id
             WHERE a.mutation_id = ?1
               AND a.state IN (
                   'dispatching_card', 'pending', 'card_delivery_uncertain',
                   'applying', 'outcome_unknown', 'applied'
               )
             ORDER BY a.created_at DESC
             LIMIT 1",
            [mutation_id],
            map_approval_row,
        )
        .optional()
}

fn map_approval_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MutationApproval> {
    let schema_version_raw: i64 = row.get(2)?;
    let action_json: String = row.get(4)?;
    let state: String = row.get(6)?;
    let result_json: Option<String> = row.get(12)?;
    let retry_count: i64 = row.get(13)?;
    let purpose: Option<String> = row.get(17)?;
    let external_owner_receipt: Option<String> = row.get(18)?;
    let external_owner_approved_at: Option<i64> = row.get(19)?;
    let external_owner_consumed_at: Option<i64> = row.get(20)?;
    let execution_owner = if external_owner_receipt.is_some() {
        MutationExecutionOwner::ExternalOwner
    } else {
        MutationExecutionOwner::ManagedMcp
    };
    let external_owner_ready = execution_owner == MutationExecutionOwner::ExternalOwner
        && state == "pending"
        && external_owner_approved_at.is_some()
        && external_owner_consumed_at.is_none();
    Ok(MutationApproval {
        approval_id: row.get(0)?,
        mutation_id: row.get(1)?,
        schema_version: u32::try_from(schema_version_raw).map_err(|_| {
            state_error(format!("invalid mutation schema version: {schema_version_raw}"))
        })?,
        account_user_id: row.get(3)?,
        action: serde_json::from_str(&action_json).map_err(|error| json_from_sql(4, error))?,
        payload_sha256: row.get(5)?,
        state: parse_approval_state(&state)?,
        execution_owner,
        external_owner_ready,
        // The one-shot handoff receipt is a capability, not ordinary status.
        // Keep it hidden until the operator callback has made the exact
        // external-owner handoff ready, and hide it again once consumed.
        external_owner_receipt: external_owner_ready.then_some(external_owner_receipt).flatten(),
        approval_chat_id: row.get(7)?,
        approval_user_id: row.get(8)?,
        approval_message_id: row.get(9)?,
        created_at_unix: row.get(10)?,
        expires_at_unix: row.get(11)?,
        result: result_json
            .map(|json| serde_json::from_str(&json).map_err(|error| json_from_sql(12, error)))
            .transpose()?,
        automated_retry_count: u32::try_from(retry_count).map_err(|_| {
            state_error(format!("invalid mutation approval retry count: {retry_count}"))
        })?,
        next_retry_at_unix: row.get(14)?,
        last_error_code: row.get(15)?,
        last_error_message: row.get(16)?,
        purpose,
    })
}

fn expire_due_approvals(connection: &Connection, now: i64) -> rusqlite::Result<()> {
    connection.execute(
        "UPDATE mutation_approvals
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
        .query_row("SELECT 1 FROM mutation_approvals WHERE approval_id = ?1", [approval_id], |_| {
            Ok(())
        })
        .optional()?
        .is_some();
    exists.then_some(()).ok_or(rusqlite::Error::QueryReturnedNoRows)
}

fn require_invite_target_changed(
    connection: &Connection,
    mutation_id: &str,
    target_index: usize,
    changed: usize,
) -> rusqlite::Result<()> {
    if changed > 0 {
        return Ok(());
    }
    let exists = connection
        .query_row(
            "SELECT 1 FROM mutation_invite_targets
             WHERE mutation_id = ?1 AND target_index = ?2",
            params![
                mutation_id,
                i64::try_from(target_index).map_err(|_| {
                    state_error("invite target index does not fit SQLite integer".to_string())
                })?,
            ],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if exists {
        Err(state_error(format!(
            "invite target {mutation_id}:{target_index} cannot make the requested state transition"
        )))
    } else {
        Err(rusqlite::Error::QueryReturnedNoRows)
    }
}

fn invite_target_binding_hash(
    target: &crate::domain::InviteUserBinding,
) -> rusqlite::Result<String> {
    let bytes = serde_json::to_vec(target).map_err(json_to_sql)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn invite_target_state_name(state: InviteTargetState) -> &'static str {
    match state {
        InviteTargetState::Pending => "pending",
        InviteTargetState::Issuing => "issuing",
        InviteTargetState::RetryWait => "retry_wait",
        InviteTargetState::Confirmed => "confirmed",
        InviteTargetState::AlreadyPresent => "already_present",
        InviteTargetState::Rejected => "rejected",
        InviteTargetState::OutcomeUnknown => "outcome_unknown",
    }
}

fn parse_invite_target_state(value: &str) -> rusqlite::Result<InviteTargetState> {
    match value {
        "pending" => Ok(InviteTargetState::Pending),
        "issuing" => Ok(InviteTargetState::Issuing),
        "retry_wait" => Ok(InviteTargetState::RetryWait),
        "confirmed" => Ok(InviteTargetState::Confirmed),
        "already_present" => Ok(InviteTargetState::AlreadyPresent),
        "rejected" => Ok(InviteTargetState::Rejected),
        "outcome_unknown" => Ok(InviteTargetState::OutcomeUnknown),
        other => Err(state_error(format!("unknown invite target state: {other}"))),
    }
}

fn invite_confirmation_name(confirmation: InviteConfirmation) -> &'static str {
    match confirmation {
        InviteConfirmation::ActiveBeforeCall => "active_before_call",
        InviteConfirmation::ProviderReceiptAndReadback => "provider_receipt_and_readback",
        InviteConfirmation::ActiveObservedAfterUncertain => "active_observed_after_uncertain",
    }
}

fn parse_invite_confirmation(value: &str) -> rusqlite::Result<InviteConfirmation> {
    match value {
        "active_before_call" => Ok(InviteConfirmation::ActiveBeforeCall),
        "provider_receipt_and_readback" => Ok(InviteConfirmation::ProviderReceiptAndReadback),
        "active_observed_after_uncertain" => Ok(InviteConfirmation::ActiveObservedAfterUncertain),
        other => Err(state_error(format!("unknown invite confirmation: {other}"))),
    }
}

fn parse_mutation_state(value: &str) -> rusqlite::Result<PreparedMutationState> {
    match value {
        "prepared" => Ok(PreparedMutationState::Prepared),
        "applying" => Ok(PreparedMutationState::Applying),
        "applied" => Ok(PreparedMutationState::Applied),
        "outcome_unknown" => Ok(PreparedMutationState::OutcomeUnknown),
        other => Err(state_error(format!("unknown prepared mutation state: {other}"))),
    }
}

fn parse_approval_state(value: &str) -> rusqlite::Result<MutationApprovalState> {
    match value {
        "dispatching_card" => Ok(MutationApprovalState::DispatchingCard),
        "pending" => Ok(MutationApprovalState::Pending),
        "card_delivery_uncertain" => Ok(MutationApprovalState::CardDeliveryUncertain),
        "applying" => Ok(MutationApprovalState::Applying),
        "outcome_unknown" => Ok(MutationApprovalState::OutcomeUnknown),
        "applied" => Ok(MutationApprovalState::Applied),
        "cancelled" => Ok(MutationApprovalState::Cancelled),
        "expired" => Ok(MutationApprovalState::Expired),
        "failed" => Ok(MutationApprovalState::Failed),
        other => Err(state_error(format!("unknown mutation approval state: {other}"))),
    }
}

fn json_to_sql(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

fn json_from_sql(index: usize, error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(error))
}

fn state_error(message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message).into(),
    )
}

fn now_unix() -> i64 {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
        .unwrap_or(i64::MAX)
}

fn mutation_action_approval_deadline_unix(action: &MutationAction) -> Option<i64> {
    match action {
        MutationAction::ScheduleMessage { schedule_date_unix, .. }
        | MutationAction::ScheduleFile { schedule_date_unix, .. }
        | MutationAction::EditScheduledMessage { schedule_date_unix, .. }
            if *schedule_date_unix != 0x7fff_fffe =>
        {
            Some(i64::from(*schedule_date_unix))
        },
        MutationAction::SendInlineBotResult { query, .. } => Some(query.expires_at_unix),
        MutationAction::SetAccountEmojiStatus {
            status:
                AccountEmojiStatusSpec::Default { until_unix: Some(until), .. }
                | AccountEmojiStatusSpec::Collectible { until_unix: Some(until), .. },
            ..
        } => Some(i64::from(*until)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        CaptionParseMode, ChatMemberRole, ContactSummary, InviteChatBinding, InviteChatKind,
        InviteUserBinding, MutationMessageSnapshot,
    };

    #[test]
    fn mutation_and_approval_survive_reopen_and_bind_one_result() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        let ledger = MutationLedger::new(path.clone());
        let action = MutationAction::EditMessage {
            chat_id: "101".to_string(),
            before: MutationMessageSnapshot {
                message_id: 10,
                sender_ref: Some("user:7".to_string()),
                outgoing: true,
                pinned: false,
                silent: false,
                noforwards: false,
                text: Some("Original".to_string()),
                date_unix: 1_700_000_000,
                edit_date_unix: None,
                reply_to_message_id: None,
                media_type: None,
                forward_origin: None,
                restore: None,
            },
            text: "Updated".to_string(),
            parse_mode: CaptionParseMode::Plain,
        };
        let prepared = ledger.prepare(7, action.clone(), "abc".to_string()).expect("prepare");
        let requested = ledger
            .create_or_get_approval(
                &prepared.public.mutation_id,
                "abc",
                700,
                700,
                3600,
                Some("тест: цель запроса"),
            )
            .expect("approval");
        assert_eq!(requested.public.purpose.as_deref(), Some("тест: цель запроса"));
        ledger.mark_approval_card_delivered(&requested.public.approval_id, 55).expect("card");
        assert!(
            ledger
                .mark_approval_applying(&requested.public.approval_id, now_unix())
                .expect("applying")
        );
        let result = MutationResult::MessageEdited { message_id: 10 };
        ledger.mark_applied(&requested.public.approval_id, &result).expect("applied");

        let reopened = MutationLedger::new(path);
        let mutation = reopened.get(&prepared.public.mutation_id).expect("get").expect("mutation");
        assert_eq!(mutation.public.action, action);
        assert_eq!(mutation.public.state, PreparedMutationState::Applied);
        assert_eq!(mutation.public.result, Some(result.clone()));
        assert_eq!(
            reopened
                .get_approval(&requested.public.approval_id)
                .expect("get approval")
                .expect("approval")
                .state,
            MutationApprovalState::Applied
        );
        reopened
            .mark_applied(&requested.public.approval_id, &result)
            .expect("same result is idempotent");
        assert!(
            reopened
                .mark_applied(
                    &requested.public.approval_id,
                    &MutationResult::MessageEdited { message_id: 11 },
                )
                .is_err()
        );
    }

    #[test]
    fn elapsed_schedule_expires_even_when_the_generic_approval_ttl_is_still_live() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = MutationLedger::new(temp.path().join("state.sqlite"));
        let now = now_unix();
        let action = MutationAction::ScheduleMessage {
            chat_id: "101".to_string(),
            target: None,
            source: None,
            text: "Scheduled text".to_string(),
            parse_mode: crate::domain::CaptionParseMode::Plain,
            reply_to: None,
            schedule_date_unix: i32::try_from(now.saturating_sub(1)).expect("test timestamp"),
            silent: false,
            noforwards: false,
            random_id: 42,
        };
        let prepared = ledger.prepare(7, action, "schedule-hash".to_string()).expect("prepare");
        let requested = ledger
            .create_or_get_approval(&prepared.public.mutation_id, "schedule-hash", 700, 700, 3_600, None)
            .expect("approval");
        ledger.mark_approval_card_delivered(&requested.public.approval_id, 55).expect("card");

        let expired = ledger.expire_due_approval_records(now, 10).expect("expire due action");
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].state, MutationApprovalState::Expired);
        assert_eq!(expired[0].last_error_code.as_deref(), Some("SCHEDULE_TIME_ELAPSED"));
        assert!(expired[0].expires_at_unix <= now);
    }

    #[test]
    fn current_session_schedule_starts_once_without_card_and_never_reopens_unknown_outcome() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = MutationLedger::new(temp.path().join("state.sqlite"));
        let schedule_date_unix =
            i32::try_from(now_unix().saturating_add(3_600)).expect("test timestamp");
        let action = MutationAction::ScheduleMessage {
            chat_id: "101".to_string(),
            target: None,
            source: None,
            text: "Scheduled text".to_string(),
            parse_mode: crate::domain::CaptionParseMode::Html,
            reply_to: None,
            schedule_date_unix,
            silent: false,
            noforwards: false,
            random_id: 42,
        };
        let prepared = ledger.prepare(7, action, "schedule-hash".to_string()).expect("prepare");
        let applying = ledger
            .start_current_session_scheduled_approval(
                &prepared.public.mutation_id,
                "schedule-hash",
                700,
                700,
            )
            .expect("start current-session approval");
        assert_eq!(applying.state, MutationApprovalState::Applying);
        assert_eq!(applying.approval_message_id, None);

        ledger
            .mark_outcome_unknown(&applying.approval_id, "DELIVERY_UNCERTAIN", None)
            .expect("record unknown outcome");
        assert!(
            ledger
                .start_current_session_scheduled_approval(
                    &prepared.public.mutation_id,
                    "schedule-hash",
                    700,
                    700,
                )
                .is_err()
        );
    }

    #[test]
    fn scheduled_file_failure_preserves_exact_provider_message() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        let ledger = MutationLedger::new(path.clone());
        let schedule_date_unix =
            i32::try_from(now_unix().saturating_add(3_600)).expect("test timestamp");
        let action = MutationAction::ScheduleFile {
            chat_id: "-100101".to_string(),
            target: None,
            source: None,
            artifact: crate::domain::MutationFileArtifact {
                artifact_id: "0aeb30a5-4c58-4bb4-8ab4-e357873251e7".to_string(),
                file_name: "visual.jpg".to_string(),
                size_bytes: 42,
                sha256: "media-sha256".to_string(),
            },
            caption: "Exact caption".to_string(),
            caption_parse_mode: crate::domain::CaptionParseMode::Plain,
            reply_to: None,
            schedule_date_unix,
            silent: false,
            noforwards: false,
            mode: crate::domain::FileSendMode::Photo,
            random_id: 43,
        };
        let prepared =
            ledger.prepare(7, action, "schedule-file-hash".to_string()).expect("prepare");
        let applying = ledger
            .start_current_session_scheduled_approval(
                &prepared.public.mutation_id,
                "schedule-file-hash",
                700,
                700,
            )
            .expect("start current-session approval");
        ledger
            .mark_known_failure(&applying.approval_id, "TELEGRAM_API_ERROR", now_unix())
            .expect("record failure");
        ledger
            .set_last_error_message(
                &applying.approval_id,
                Some("Telegram API error: PHOTO_INVALID_DIMENSIONS"),
            )
            .expect("record provider detail");

        let failed = MutationLedger::new(path)
            .get_approval(&applying.approval_id)
            .expect("read approval")
            .expect("approval");
        assert_eq!(failed.state, MutationApprovalState::Failed);
        assert_eq!(failed.last_error_code.as_deref(), Some("TELEGRAM_API_ERROR"));
        assert_eq!(
            failed.last_error_message.as_deref(),
            Some("Telegram API error: PHOTO_INVALID_DIMENSIONS")
        );
    }

    #[test]
    fn external_owner_schedule_handoff_is_one_shot_and_not_managed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = MutationLedger::new(temp.path().join("state.sqlite"));
        let schedule_date_unix =
            i32::try_from(now_unix().saturating_add(3_600)).expect("test timestamp");
        let action = MutationAction::ScheduleMessage {
            chat_id: "101".to_string(),
            target: None,
            source: None,
            text: "Scheduled text".to_string(),
            parse_mode: crate::domain::CaptionParseMode::Plain,
            reply_to: None,
            schedule_date_unix,
            silent: false,
            noforwards: false,
            random_id: 42,
        };
        let prepared = ledger.prepare(7, action, "schedule-hash".to_string()).expect("prepare");
        let requested = ledger
            .create_or_get_external_owner_approval(
                &prepared.public.mutation_id,
                "schedule-hash",
                700,
                700,
                3_600,
                None,
            )
            .expect("external-owner approval");
        assert_eq!(requested.public.execution_owner, MutationExecutionOwner::ExternalOwner);
        assert!(!requested.public.external_owner_ready);
        assert!(requested.public.external_owner_receipt.is_none());
        ledger.mark_approval_card_delivered(&requested.public.approval_id, 55).expect("card");

        assert!(
            !ledger
                .mark_approval_applying(&requested.public.approval_id, now_unix())
                .expect("managed path remains closed")
        );
        assert!(
            ledger
                .mark_external_owner_approved(&requested.public.approval_id, now_unix())
                .expect("operator approval")
        );
        let ready = ledger
            .get_approval(&requested.public.approval_id)
            .expect("get ready approval")
            .expect("ready approval");
        assert!(ready.external_owner_ready);
        let receipt = ready.external_owner_receipt.clone().expect("ready receipt");
        assert!(
            ledger
                .mark_external_owner_approved(&requested.public.approval_id, now_unix())
                .expect("duplicate operator callback is idempotent")
        );
        assert!(
            !ledger
                .mark_approval_applying(&requested.public.approval_id, now_unix())
                .expect("managed path remains closed after approval")
        );

        let applying = ledger
            .start_external_owner_scheduled_approval(
                &prepared.public.mutation_id,
                "schedule-hash",
                &requested.public.approval_id,
                &receipt,
            )
            .expect("external owner starts exact scheduled mutation");
        assert_eq!(applying.state, MutationApprovalState::Applying);
        assert!(!applying.external_owner_ready);
        assert!(
            ledger
                .start_external_owner_scheduled_approval(
                    &prepared.public.mutation_id,
                    "schedule-hash",
                    &requested.public.approval_id,
                    &receipt,
                )
                .is_err()
        );
    }

    #[test]
    fn caller_owned_mutation_id_replays_only_the_exact_immutable_record() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ledger = MutationLedger::new(temp.path().join("state.sqlite"));
        let action = MutationAction::ScheduleMessage {
            chat_id: "101".to_string(),
            target: None,
            source: None,
            text: "Scheduled text".to_string(),
            parse_mode: crate::domain::CaptionParseMode::Plain,
            reply_to: None,
            schedule_date_unix: now_unix().saturating_add(3_600).try_into().expect("timestamp"),
            silent: false,
            noforwards: false,
            random_id: 42,
        };
        let mutation_id = "018f6f7c-7c20-7e9f-9b35-1c7d6db3d2c1".to_string();
        let first = ledger
            .prepare_with_id(mutation_id.clone(), 7, action.clone(), "schedule-hash".to_string())
            .expect("first prepare");
        let replay = ledger
            .prepare_with_id(mutation_id, 7, action, "schedule-hash".to_string())
            .expect("exact replay");
        assert_eq!(first.public.mutation_id, replay.public.mutation_id);
        assert!(
            ledger
                .prepare_with_id(
                    first.public.mutation_id,
                    7,
                    replay.public.action,
                    "different-hash".to_string(),
                )
                .is_err()
        );
    }

    #[test]
    fn stale_invite_issuing_becomes_readback_only_and_never_reclaims() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        let ledger = MutationLedger::new(path.clone());
        let user = ContactSummary {
            user_id: 404,
            first_name: "Dana".to_string(),
            last_name: None,
            display_name: "Dana".to_string(),
            username: Some("dana".to_string()),
            phone: None,
            is_contact: false,
            mutual_contact: false,
            bot: false,
            verified: false,
            premium: false,
            scam: false,
            restricted: false,
        };
        let chat = InviteChatBinding {
            chat_id: "-100505".to_string(),
            kind: InviteChatKind::Supergroup,
            title_at_prepare: "Ops".to_string(),
        };
        let target = InviteUserBinding {
            index: 0,
            source_ref: "@dana".to_string(),
            user_id: user.user_id,
            display_name_at_prepare: user.display_name.clone(),
            username_at_prepare: user.username.clone(),
            before: None,
        };
        let action =
            MutationAction::InviteChatMembers { chat: chat.clone(), targets: vec![target.clone()] };
        let initial = InviteTargetProgress {
            target,
            state: InviteTargetState::Pending,
            attempt_count: 0,
            observation_count: 0,
            next_action_at_unix: Some(now_unix()),
            last_error_code: None,
            confirmation: None,
            member: None,
        };
        let prepared = ledger
            .prepare_invite_batch(7, action, "invite-hash".to_string(), &[initial])
            .expect("prepare batch");
        let requested = ledger
            .create_or_get_approval(&prepared.public.mutation_id, "invite-hash", 700, 700, 3_600, None)
            .expect("approval");
        ledger.mark_approval_card_delivered(&requested.public.approval_id, 55).expect("card");
        let now = now_unix();
        assert!(
            ledger.mark_approval_applying(&requested.public.approval_id, now).expect("applying")
        );
        assert!(ledger.claim_invite_target(&prepared.public.mutation_id, 0, now).expect("claim"));
        assert_eq!(
            ledger
                .get_invite_progress(&prepared.public.mutation_id)
                .expect("progress")
                .expect("batch")
                .targets[0]
                .state,
            InviteTargetState::Issuing
        );

        let reopened = MutationLedger::new(path);
        assert_eq!(
            reopened
                .mark_stale_invite_issuing_unknown(
                    &prepared.public.mutation_id,
                    "INVITE_PROVIDER_OUTCOME_UNKNOWN",
                    now,
                )
                .expect("stale issuing"),
            1
        );
        assert!(
            !reopened
                .claim_invite_target(&prepared.public.mutation_id, 0, now)
                .expect("unknown is not claimable")
        );
        let unknown = reopened
            .get_invite_progress(&prepared.public.mutation_id)
            .expect("progress")
            .expect("batch")
            .targets
            .remove(0);
        assert_eq!(unknown.state, InviteTargetState::OutcomeUnknown);
        assert_eq!(unknown.attempt_count, 1);

        let member = ChatMemberSummary {
            chat_id: chat.chat_id.clone(),
            user,
            role: ChatMemberRole::Member,
            can_edit: false,
            is_self: false,
            left: false,
            joined_date_unix: Some(1_700_000_000),
            inviter_user_id: Some(7),
            promoted_by_user_id: None,
            kicked_by_user_id: None,
            rank: None,
            subscription_until_date_unix: None,
            via_request: false,
            admin_rights: None,
            banned_rights: None,
        };
        reopened
            .mark_invite_target_confirmed(
                &prepared.public.mutation_id,
                0,
                &member,
                InviteConfirmation::ActiveObservedAfterUncertain,
                now,
            )
            .expect("readback confirmation");
        let applied = reopened
            .reconcile_invite_parent(&requested.public.approval_id, now)
            .expect("reconcile");
        assert_eq!(applied.state, MutationApprovalState::Applied);
        assert!(matches!(
            applied.result,
            Some(MutationResult::ChatMembersInvited { outcomes, .. })
                if outcomes.len() == 1
                    && outcomes[0].confirmation
                        == Some(InviteConfirmation::ActiveObservedAfterUncertain)
        ));
    }
}
