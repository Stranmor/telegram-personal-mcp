use crate::fs_security::{restrict_private_directory, restrict_private_file};
use rusqlite::Connection;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

/// Durability mode for a private WAL-backed SQLite state database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynchronousMode {
    Full,
    Normal,
}

impl SynchronousMode {
    const fn sqlite_value(self) -> i32 {
        match self {
            Self::Full => 2,
            Self::Normal => 1,
        }
    }
}

/// Opens a private SQLite database with the state-store invariants used by this crate.
///
/// The caller selects only the durability trade-off. Journal mode, foreign-key
/// enforcement, timeout, and filesystem permissions are not caller-controlled.
pub fn open_private_wal(
    path: impl AsRef<Path>,
    synchronous: SynchronousMode,
) -> rusqlite::Result<Connection> {
    let path = path.as_ref();
    prepare_private_path(path).map_err(io_to_sql)?;

    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", synchronous.sqlite_value())?;

    restrict_private_sqlite_files(path)?;
    Ok(connection)
}

pub(crate) fn restrict_private_sqlite_files(path: &Path) -> rusqlite::Result<()> {
    restrict_private_file(path).map_err(io_to_sql)?;
    for sidecar in sqlite_sidecars(path) {
        if sidecar.exists() {
            restrict_private_file(&sidecar).map_err(io_to_sql)?;
        }
    }
    Ok(())
}

fn prepare_private_path(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
        restrict_private_directory(parent)?;
    }

    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    options.mode(0o600);
    drop(options.open(path)?);
    restrict_private_file(path)
}

fn sqlite_sidecars(path: &Path) -> [PathBuf; 2] {
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    let mut shared_memory = path.as_os_str().to_os_string();
    shared_memory.push("-shm");
    [PathBuf::from(wal), PathBuf::from(shared_memory)]
}

fn io_to_sql(error: std::io::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opener_applies_wal_foreign_keys_timeout_and_typed_synchronous_modes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("private").join("state.sqlite");

        let connection = open_private_wal(&path, SynchronousMode::Full).expect("open full");
        let journal: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .expect("journal mode");
        let foreign_keys: i64 = connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .expect("foreign keys");
        let synchronous: i64 = connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .expect("synchronous");
        let busy_timeout: i64 = connection
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .expect("busy timeout");
        assert_eq!(journal.to_ascii_lowercase(), "wal");
        assert_eq!(foreign_keys, 1);
        assert_eq!(synchronous, 2);
        assert_eq!(busy_timeout, 5_000);
        drop(connection);

        let connection = open_private_wal(&path, SynchronousMode::Normal).expect("open normal");
        let synchronous: i64 = connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .expect("synchronous");
        assert_eq!(synchronous, 1);
    }

    #[cfg(unix)]
    #[test]
    fn database_directory_and_existing_wal_sidecars_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let directory = temp.path().join("private");
        let path = directory.join("state.sqlite");
        let connection = open_private_wal(&path, SynchronousMode::Normal).expect("open");
        connection
            .execute_batch(
                "CREATE TABLE sample (id INTEGER PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO sample (value) VALUES ('one');",
            )
            .expect("write WAL state");
        restrict_private_sqlite_files(&path).expect("restrict sidecars");

        assert_eq!(
            std::fs::metadata(&directory).expect("directory metadata").permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).expect("database metadata").permissions().mode() & 0o777,
            0o600
        );
        for sidecar in sqlite_sidecars(&path) {
            if sidecar.exists() {
                assert_eq!(
                    std::fs::metadata(sidecar).expect("sidecar metadata").permissions().mode()
                        & 0o777,
                    0o600
                );
            }
        }
    }
}
