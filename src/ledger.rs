use crate::domain::{PreparedSend, PreparedSendState};
use crate::fs_security::{restrict_private_directory, restrict_private_file};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PreparedSendRecord {
    pub public: PreparedSend,
    pub random_id: i64,
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

    pub fn prepare(
        &self,
        account_user_id: i64,
        chat_id: String,
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
        let created_at = i64::try_from(
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
        )
        .unwrap_or(i64::MAX);
        connection.execute(
            "INSERT INTO prepared_sends (
                draft_id, account_user_id, chat_id, text,
                reply_to_message_id, payload_sha256, random_id, state,
                message_id, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'prepared', NULL, ?8)",
            params![
                draft_id,
                account_user_id,
                chat_id,
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
                "SELECT account_user_id, chat_id, text,
                        reply_to_message_id, payload_sha256, random_id, state, message_id
                 FROM prepared_sends WHERE draft_id = ?1",
                [draft_id],
                |row| {
                    let state: String = row.get(6)?;
                    Ok(PreparedSendRecord {
                        public: PreparedSend {
                            draft_id: draft_id.to_string(),
                            account_user_id: row.get(0)?,
                            chat_id: row.get(1)?,
                            text: row.get(2)?,
                            reply_to_message_id: row.get(3)?,
                            payload_sha256: row.get(4)?,
                            state: parse_state(&state)?,
                            message_id: row.get(7)?,
                        },
                        random_id: row.get(5)?,
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
        let existing_message_id = connection
            .query_row(
                "SELECT message_id FROM prepared_sends WHERE draft_id = ?1",
                [draft_id],
                |row| row.get::<_, Option<i32>>(0),
            )
            .optional()?;
        match existing_message_id {
            None => Err(rusqlite::Error::QueryReturnedNoRows),
            Some(Some(existing)) => Err(state_error(format!(
                "prepared send {draft_id} is already bound to Telegram message {existing}, not {message_id}"
            ))),
            Some(None) => {
                Err(state_error(format!("prepared send {draft_id} could not transition to sent")))
            },
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
        if let Some(parent) = self.path.parent() {
            let create_private_parent = !parent.exists();
            std::fs::create_dir_all(parent)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            if create_private_parent {
                restrict_private_directory(parent)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            }
        }
        let connection = Connection::open(&self.path)?;
        restrict_private_file(&self.path)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS prepared_sends (
                 draft_id TEXT PRIMARY KEY,
                 account_user_id INTEGER NOT NULL,
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
             );",
        )?;
        remove_legacy_session_path_column(&connection)?;
        for sidecar in sqlite_sidecars(&self.path) {
            if sidecar.exists() {
                restrict_private_file(&sidecar)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
            }
        }
        Ok(connection)
    }
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

fn sqlite_sidecars(path: &Path) -> [PathBuf; 2] {
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    let mut shared_memory = path.as_os_str().to_os_string();
    shared_memory.push("-shm");
    [PathBuf::from(wal), PathBuf::from(shared_memory)]
}

fn state_error(message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message).into(),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn prepared_send_survives_reopen_and_state_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("state.sqlite");
        let ledger = SendLedger::new(path.clone());
        let prepared = ledger
            .prepare(7, "@alex".to_string(), "Hello".to_string(), Some(11), "abc".to_string())
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
        let prepared =
            ledger.prepare(7, "@alex".into(), "Hello".into(), None, "abc".into()).expect("prepare");

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
        let prepared =
            ledger.prepare(7, "@alex".into(), "Hello".into(), None, "abc".into()).expect("prepare");

        ledger.mark_dispatching(&prepared.public.draft_id).expect("dispatching");
        ledger.mark_uncertain(&prepared.public.draft_id).expect("uncertain");
        ledger.mark_prepared(&prepared.public.draft_id).expect("late failure cannot downgrade");

        let record = ledger.get(&prepared.public.draft_id).expect("get").expect("record");
        assert_eq!(record.public.state, PreparedSendState::DeliveryUncertain);
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
    }

    #[cfg(unix)]
    #[test]
    fn newly_created_state_storage_is_owner_only() {
        let temp = tempfile::tempdir().expect("tempdir");
        let private_dir = temp.path().join("private-state");
        let path = private_dir.join("state.sqlite");
        let ledger = SendLedger::new(path.clone());
        ledger.prepare(7, "101".into(), "Hello".into(), None, "abc".into()).expect("prepare");

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
