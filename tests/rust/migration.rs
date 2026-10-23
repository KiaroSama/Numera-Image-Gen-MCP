mod common;
use common::Workspace;
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::{
    compat::hash::sha256,
    jobs::{Receipt, Store},
};
use rusqlite::Connection;

#[test]
fn legacy_wal_snapshot_preserves_receipts_and_refuses_future_or_incomplete_schemas() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("legacy");
    std::fs::create_dir(&directory).unwrap();
    let database = directory.join("receipts.sqlite");
    let legacy = Connection::open(&database).unwrap();
    legacy.execute_batch("PRAGMA journal_mode=WAL; PRAGMA user_version=1; CREATE TABLE requests(id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,identity TEXT NOT NULL,receipt TEXT NOT NULL,owner TEXT,lease INTEGER,cancel INTEGER NOT NULL DEFAULT 0);").unwrap();
    let value = json!({"request_id":"existing","connection":"test","status":"outcome_unknown","generation_outcome":"unknown","another_submission_may_charge":true,"outputs":[],"warnings":[],"errors":[],"deviations":[]});
    legacy
        .execute(
            "INSERT INTO requests VALUES(?1,?2,?3,?4,NULL,0,0)",
            rusqlite::params!["existing", "fingerprint", "account", value.to_string()],
        )
        .unwrap();
    let mut native = Store::open(&directory).unwrap();
    assert_eq!(native.get("existing").unwrap().value, value);
    assert_eq!(native.identity("existing").unwrap(), "account");
    assert!(
        !native
            .prepare(Receipt::new(value.clone()), "fingerprint", "account")
            .unwrap()
            .0
    );
    assert_eq!(
        native
            .prepare(Receipt::new(value.clone()), "changed", "account")
            .unwrap_err()
            .code,
        "request_id_conflict"
    );
    let snapshots = std::fs::read_dir(&directory)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("receipts-before-journal-")
                && p.extension().is_some_and(|x| x == "sqlite")
        })
        .collect::<Vec<_>>();
    assert_eq!(snapshots.len(), 1);
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(snapshots[0].with_extension("sqlite.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest["sha256"],
        sha256(&std::fs::read(&snapshots[0]).unwrap())
    );
    let backup = Connection::open(&snapshots[0]).unwrap();
    assert_eq!(
        backup
            .query_row(
                "SELECT receipt FROM requests WHERE id='existing'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        value.to_string()
    );
    drop(backup);
    drop(native);
    drop(legacy);
    for (name, ddl) in [
        ("future", "PRAGMA user_version=3;"),
        (
            "incomplete",
            "CREATE TABLE requests(id TEXT, generation INTEGER);",
        ),
    ] {
        let directory = workspace.0.join(name);
        std::fs::create_dir(&directory).unwrap();
        let db = Connection::open(directory.join("receipts.sqlite")).unwrap();
        db.execute_batch(ddl).unwrap();
        drop(db);
        assert_eq!(
            Store::open(&directory).err().unwrap().code,
            "invalid_configuration"
        );
    }
}
