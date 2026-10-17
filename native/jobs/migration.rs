use crate::compat::hash::sha256;
use crate::{
    error::{NumeraError, Result},
    runtime_paths::private_directory,
};
use chrono::Utc;
use rusqlite::Connection;
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub fn open(directory: &Path) -> Result<Connection> {
    private_directory(directory)?;
    let path = directory.join("receipts.sqlite");
    let existed = path.exists();
    let connection = Connection::open(&path).map_err(|_| NumeraError::storage())?;
    connection
        .busy_timeout(Duration::from_secs(3))
        .map_err(|_| NumeraError::storage())?;
    let version: u32 = connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|_| NumeraError::storage())?;
    if version > 2 {
        return Err(NumeraError::new("invalid_configuration", "State schema is newer than this build; use the matching Numera version without downgrading state.").stage("storage"));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let mode: rusqlite::Result<String> =
            connection.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0));
        match mode {
            Ok(mode) if mode == "wal" => break,
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::DatabaseBusy && Instant::now() < deadline =>
            {
                // Short bounded local SQLite contention, never a provider submission retry.
                std::thread::sleep(
                    Duration::from_millis(20)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            _ => {
                return Err(NumeraError::new(
                    "invalid_configuration",
                    "State database cannot enable durable WAL journaling.",
                )
                .stage("storage"));
            }
        }
    }
    connection
        .execute_batch("PRAGMA synchronous=FULL")
        .map_err(|_| NumeraError::storage())?;
    private_file(&path)?;
    for suffix in ["-wal", "-shm"] {
        let file = directory.join(format!("receipts.sqlite{suffix}"));
        if file.exists() {
            private_file(&file)?;
        }
    }
    let columns: Vec<String> = connection
        .prepare("PRAGMA table_info(requests)")
        .and_then(|mut s| s.query_map([], |r| r.get(1))?.collect())
        .map_err(|_| NumeraError::storage())?;
    let generation = columns.iter().any(|v| v == "generation");
    let reservation = columns.iter().any(|v| v == "reservation");
    if generation != reservation {
        return Err(NumeraError::new("invalid_configuration", "State schema migration is incomplete; preserve the database and restore a verified backup.").stage("storage"));
    }
    if existed && !columns.is_empty() && !generation {
        let backup = directory.join(format!("receipts-before-journal-{}.sqlite", Uuid::new_v4()));
        connection
            .execute("VACUUM INTO ?", [backup.to_string_lossy().as_ref()])
            .map_err(|_| NumeraError::storage())?;
        private_file(&backup)?;
        let bytes = fs::read(&backup).map_err(|_| NumeraError::storage())?;
        let metadata = serde_json::json!({"schema_version":version,"target_schema_version":2,
            "created_at":Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis,true),
            "sha256":sha256(&bytes)});
        let manifest = backup.with_extension("sqlite.json");
        use std::io::Write;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(manifest).map_err(|_| NumeraError::storage())?;
        writeln!(file, "{metadata}")
            .and_then(|_| file.sync_all())
            .map_err(|_| NumeraError::storage())?;
    }
    connection
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(|_| NumeraError::storage())?;
    let result = (|| -> rusqlite::Result<()> {
        connection.execute_batch("CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,identity TEXT NOT NULL,receipt TEXT NOT NULL,owner TEXT,lease INTEGER,cancel INTEGER NOT NULL DEFAULT 0,generation INTEGER NOT NULL DEFAULT 1,reservation INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS outputs(id TEXT PRIMARY KEY,request_id TEXT NOT NULL,metadata TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS output_request ON outputs(request_id);
            CREATE TABLE IF NOT EXISTS continuation(id TEXT PRIMARY KEY,scope TEXT NOT NULL,value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS result_journal(request_id TEXT NOT NULL,item INTEGER NOT NULL CHECK(item>=0 AND item<10),output_id TEXT NOT NULL UNIQUE,target TEXT,bytes BLOB,url TEXT,sha256 TEXT,phase TEXT NOT NULL,PRIMARY KEY(request_id,item));")?;
        if !columns.is_empty() && !generation {
            connection.execute_batch("ALTER TABLE requests ADD COLUMN generation INTEGER NOT NULL DEFAULT 1;ALTER TABLE requests ADD COLUMN reservation INTEGER NOT NULL DEFAULT 0;")?;
        }
        connection.execute_batch("PRAGMA user_version=2;COMMIT")
    })();
    if result.is_err() {
        let _ = connection.execute_batch("ROLLBACK");
        return Err(NumeraError::storage());
    }
    Ok(connection)
}

fn private_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|_| NumeraError::storage())?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
