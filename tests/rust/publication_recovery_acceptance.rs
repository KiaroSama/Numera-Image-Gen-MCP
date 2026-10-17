mod common;
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::Config,
    files::{
        images::{Image, decode_base64, inspect},
        output::{self, Publication},
    },
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, jobs},
};
use rusqlite::{Connection, OpenFlags, params};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn config(root: &Path) -> Config {
    Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":{"local":{
                "adapter":"openai-images","baseUrl":"https://api.example/v1",
                "auth":{"type":"none"}
            }}
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap()
}

fn receipt(id: &str) -> Receipt {
    Receipt::new(json!({
        "request_id":id,"connection":"local","adapter":"openai-images",
        "operation":"generate","status":"prepared","requested_model":"image-model",
        "upstream_model":null,"outputs":[],"warnings":[],"deviations":[],"errors":[],
        "generation_outcome":"not_submitted","storage_outcome":"not_started",
        "another_submission_may_charge":false,
        "created_at":"2026-10-02T00:00:00.000Z","updated_at":"2026-10-02T00:00:00.000Z"
    }))
}

fn original(config: &Value) -> Image {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/image-acceptance.json");
    assert!(std::fs::metadata(&path).unwrap().len() <= 1048576);
    let rows = Value::parse_with_limit(&std::fs::read_to_string(path).unwrap(), 1048576).unwrap();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "png")
        .unwrap();
    let bytes = decode_base64(row["base64"].as_str().unwrap(), 1048576).unwrap();
    let image = inspect(bytes.clone(), config, None).unwrap();
    assert_eq!((image.width, image.height), (8, 6));
    assert_eq!(image.mime, "image/png");
    assert!(image.alpha);
    assert_eq!(image.bytes, bytes);
    image
}

fn visible_files(directory: &Path) -> usize {
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .count()
}

fn reserve(store: &mut Store, config: &Config, id: &str) {
    store
        .reserve(
            id,
            config.value["files"]["maxAggregateBytes"].as_i64().unwrap(),
            config.value["files"]["maxJournalBytes"].as_i64().unwrap(),
        )
        .unwrap();
}

fn fail_commit(directory: &Path) -> Connection {
    let fault = Connection::open(directory.join("receipts.sqlite")).unwrap();
    fault
        .execute_batch("CREATE TRIGGER reject_output BEFORE INSERT ON outputs BEGIN SELECT RAISE(FAIL,'fixture SQL failure'); END;")
        .unwrap();
    fault
}

#[test]
fn exact_source_published_file_recovers_by_journal_hash_without_duplicate_output() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let config = config(&workspace.0);
    let state = workspace.0.join("state");
    let mut store = Store::open(&state).unwrap();
    let image = original(&config.value);
    let mut initial = receipt("published");
    initial.value["output_requirements"] = json!({"count":1,"output_subdirectory":"published"});
    store.prepare(initial, "hash", "identity").unwrap();
    reserve(&mut store, &config, "published");
    store.retain("published", 0, &image.bytes, None).unwrap();
    assert!(store.claim("published", None, false).unwrap());
    let fault = fail_commit(&state);
    let error = output::save(
        &image,
        &config.value,
        &mut store,
        Publication {
            id: "published",
            index: 0,
            subdir: Some("published"),
            prefix: None,
            deviations: &[],
            expected: None,
            stable: true,
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "output_file_error");
    fault.execute_batch("DROP TRIGGER reject_output").unwrap();
    drop(fault);
    let published = workspace.0.join("outputs/published");
    assert_eq!(visible_files(&published), 1);
    assert_eq!(store.get("published").unwrap().value["outputs"], json!([]));
    store.release("published", None).unwrap();
    let g = Generation {
        config: Arc::new(config),
        store: Arc::new(Mutex::new(store)),
        logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(&workspace.0).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    };
    let done = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(3),
            jobs::get(&g, "published", true, &CancellationToken::new(), None),
        )
        .await
        .unwrap()
        .unwrap()
    });
    assert_eq!(done.value["outputs"].as_array().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(done.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
        image.bytes
    );
    let store = g.store.lock().unwrap();
    assert_eq!(store.list_outputs(0, 100).unwrap().len(), 1);
    assert!(store.pending("published").unwrap().is_empty());
    assert_eq!(visible_files(&published), 1);
}

#[test]
fn exact_source_changed_stable_filename_refuses_without_index_or_retained_byte_loss() {
    let workspace = Workspace::new();
    let config = config(&workspace.0);
    let state = workspace.0.join("state");
    let mut store = Store::open(&state).unwrap();
    store
        .prepare(receipt("mismatch"), "hash", "identity")
        .unwrap();
    let image = original(&config.value);
    reserve(&mut store, &config, "mismatch");
    store.retain("mismatch", 0, &image.bytes, None).unwrap();
    assert!(store.claim("mismatch", None, false).unwrap());
    let fault = fail_commit(&state);
    let publication = || Publication {
        id: "mismatch",
        index: 0,
        subdir: None,
        prefix: None,
        deviations: &[],
        expected: None,
        stable: true,
    };
    assert!(output::save(&image, &config.value, &mut store, publication()).is_err());
    fault.execute_batch("DROP TRIGGER reject_output").unwrap();
    drop(fault);
    let item = store.pending("mismatch").unwrap().remove(0);
    std::fs::write(item.target.unwrap(), b"not the image").unwrap();
    assert_eq!(
        output::save(&image, &config.value, &mut store, publication())
            .unwrap_err()
            .code,
        "output_file_error"
    );
    assert_eq!(store.get("mismatch").unwrap().value["outputs"], json!([]));
    assert!(store.list_outputs(0, 100).unwrap().is_empty());
    assert_eq!(store.pending("mismatch").unwrap().len(), 1);
}

#[test]
fn exact_source_legacy_backup_preserves_completed_receipt_from_live_wal() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    std::fs::create_dir(&directory).unwrap();
    let legacy = Connection::open(directory.join("receipts.sqlite")).unwrap();
    legacy.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE requests(id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,identity TEXT NOT NULL,receipt TEXT NOT NULL,owner TEXT,lease INTEGER,cancel INTEGER NOT NULL DEFAULT 0);").unwrap();
    let mut initial = receipt("legacy");
    initial.value["status"] = json!("completed");
    initial.value["generation_outcome"] = json!("completed");
    legacy
        .execute(
            "INSERT INTO requests(id,fingerprint,identity,receipt) VALUES(?,?,?,?)",
            params!["legacy", "hash", "identity", initial.value.to_string()],
        )
        .unwrap();
    assert!(
        std::fs::metadata(directory.join("receipts.sqlite-wal"))
            .unwrap()
            .len()
            > 0
    );
    let mut store = Store::open(&directory).unwrap();
    assert_eq!(store.get("legacy").unwrap().value, initial.value);
    let backups = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.starts_with("receipts-before-journal-") && name.ends_with(".sqlite")
        })
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    let backup =
        Connection::open_with_flags(&backups[0], OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let saved: String = backup
        .query_row(
            "SELECT receipt FROM requests WHERE id='legacy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        Value::parse_with_limit(&saved, 1048576).unwrap(),
        initial.value
    );
    assert!(
        !store
            .prepare(receipt("legacy"), "hash", "identity")
            .unwrap()
            .0
    );
    drop(backup);
    drop(store);
    drop(legacy);
}
