use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error)]
pub enum SqliteReadError {
    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("blocking SQLite task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("I/O operation failed: {0}")]
    Io(#[from] std::io::Error),
}

pub fn open_read_only(path: &Path) -> Result<Connection, SqliteReadError> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags)?;
    connection.busy_timeout(SQLITE_BUSY_TIMEOUT)?;
    Ok(connection)
}

pub async fn private_snapshot(source: PathBuf) -> Result<PrivateSqliteSnapshot, SqliteReadError> {
    tokio::task::spawn_blocking(move || {
        let temporary = tempfile::Builder::new()
            .prefix("openusage-sqlite-")
            .suffix(".db")
            .tempfile()?;
        let source_connection = open_read_only(&source)?;
        let mut destination = Connection::open(temporary.path())?;
        let backup = rusqlite::backup::Backup::new(&source_connection, &mut destination)?;
        backup.run_to_completion(64, Duration::from_millis(10), None)?;
        drop(backup);
        drop(destination);
        drop(source_connection);
        Ok(PrivateSqliteSnapshot { temporary })
    })
    .await?
}

#[derive(Debug)]
pub struct PrivateSqliteSnapshot {
    temporary: tempfile::NamedTempFile,
}

impl PrivateSqliteSnapshot {
    pub fn path(&self) -> &Path {
        self.temporary.path()
    }

    pub fn open(&self) -> Result<Connection, SqliteReadError> {
        open_read_only(self.path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn private_snapshot_includes_committed_wal_data_and_cleans_up() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.db");
        let connection = Connection::open(&source).unwrap();
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .unwrap();
        connection
            .execute_batch("CREATE TABLE usage(value INTEGER); INSERT INTO usage VALUES (42);")
            .unwrap();

        let snapshot = private_snapshot(source).await.unwrap();
        let snapshot_path = snapshot.path().to_owned();
        let value: i64 = snapshot
            .open()
            .unwrap()
            .query_row("SELECT value FROM usage", [], |row| row.get(0))
            .unwrap();

        assert_eq!(value, 42);
        drop(snapshot);
        assert!(!snapshot_path.exists());
    }
}
