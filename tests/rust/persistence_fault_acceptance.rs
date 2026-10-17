mod common;
use common::Workspace;
use numera_image_gen_mcp::{
    adapters::types::{Normalized, ResultItem},
    compat::value::{Value, json},
    config::Config,
    error::{NumeraError, Result},
    files::images::{decode_base64, inspect},
    jobs::{NativePersistence, Persistence, Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, jobs, request},
};
use std::{
    collections::HashMap,
    fs::File,
    io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Write,
    Sync,
    Link,
    Unlink,
    Receipt,
}

struct FaultState {
    phase: Phase,
    armed: AtomicBool,
    hits: AtomicUsize,
}

#[derive(Clone)]
struct Fault(Arc<FaultState>);

impl Fault {
    fn take(&self, phase: Phase) -> bool {
        if self.0.phase == phase && self.0.armed.swap(false, Ordering::SeqCst) {
            self.0.hits.fetch_add(1, Ordering::SeqCst);
            true
        } else {
            false
        }
    }
}

impl Persistence for Fault {
    fn before_receipt_update(&self) -> Result<()> {
        if self.take(Phase::Receipt) {
            Err(
                NumeraError::new("storage_error", "Controlled receipt SQL failure")
                    .stage("storage"),
            )
        } else {
            Ok(())
        }
    }

    fn write(&self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        if self.take(Phase::Write) {
            Err(io::Error::other("Controlled file handle failure"))
        } else {
            NativePersistence.write(file, bytes)
        }
    }

    fn sync(&self, file: &File) -> io::Result<()> {
        if self.take(Phase::Sync) {
            Err(io::Error::other("Controlled file handle failure"))
        } else {
            NativePersistence.sync(file)
        }
    }

    fn link(&self, original: &Path, target: &Path) -> io::Result<()> {
        if self.take(Phase::Link) {
            Err(io::Error::other("Controlled filesystem failure"))
        } else {
            NativePersistence.link(original, target)
        }
    }

    fn unlink(&self, path: &Path) -> io::Result<()> {
        if self.take(Phase::Unlink) {
            Err(io::Error::other("Controlled filesystem failure"))
        } else {
            NativePersistence.unlink(path)
        }
    }
}

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

fn original(config: &Value) -> Vec<u8> {
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
    bytes
}

fn receipt(id: &str) -> Receipt {
    Receipt::new(json!({
        "request_id":id,"connection":"local","adapter":"openai-images",
        "operation":"generate","status":"prepared","requested_model":"image-model",
        "upstream_model":null,"outputs":[],"warnings":[],"deviations":[],"errors":[],
        "generation_outcome":"not_submitted","storage_outcome":"not_started",
        "another_submission_may_charge":false,
        "created_at":"2026-10-02T00:00:00.000Z","updated_at":"2026-10-02T00:00:00.000Z",
        "output_requirements":{"count":1,"output_subdirectory":"faults"}
    }))
}

fn recover(phase: Phase, name: &str) {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let config = config(&workspace.0);
    let bytes = original(&config.value);
    let fault = Fault(Arc::new(FaultState {
        phase,
        armed: AtomicBool::new(false),
        hits: AtomicUsize::new(0),
    }));
    let mut store =
        Store::open_with_persistence(&workspace.0.join("state"), fault.clone()).unwrap();
    let id = format!("fault-{name}");
    let (_, initial) = store.prepare(receipt(&id), "hash", "identity").unwrap();
    let generation = Generation {
        config: Arc::new(config),
        store: Arc::new(Mutex::new(store)),
        logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(&workspace.0).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    };
    let request = request::parse(json!({"prompt":"draw"})).unwrap();
    let cancel = CancellationToken::new();
    fault.0.armed.store(true, Ordering::SeqCst);
    let first = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(3),
            generation.finish(
                initial,
                Normalized {
                    images: vec![ResultItem {
                        bytes: Some(bytes.clone()),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                &request,
                &cancel,
                false,
                false,
            ),
        )
        .await
        .unwrap()
    });
    fault.0.armed.store(false, Ordering::SeqCst);
    assert_eq!(fault.0.hits.load(Ordering::SeqCst), 1);
    if phase == Phase::Receipt {
        assert_eq!(
            first.err().unwrap().to_string(),
            "Controlled receipt SQL failure"
        );
    } else {
        assert_eq!(first.unwrap().value["outputs"].as_array().unwrap().len(), 0);
    }
    assert_eq!(
        generation.store.lock().unwrap().pending(&id).unwrap().len(),
        1
    );
    let done = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(3),
            jobs::get(&generation, &id, true, &cancel, None),
        )
        .await
        .unwrap()
        .unwrap()
    });
    assert_eq!(done.status(), "completed");
    assert_eq!(done.value["outputs"].as_array().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(done.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
        bytes
    );
    let store = generation.store.lock().unwrap();
    assert_eq!(store.list_outputs(0, 100).unwrap().len(), 1);
    assert!(store.pending(&id).unwrap().is_empty());
    assert_eq!(fault.0.hits.load(Ordering::SeqCst), 1);
}

#[test]
fn exact_source_write_failure_retains_original_and_recovers_one_stable_output() {
    recover(Phase::Write, "write");
}

#[test]
fn exact_source_sync_failure_retains_original_and_recovers_one_stable_output() {
    recover(Phase::Sync, "sync");
}

#[test]
fn exact_source_link_failure_retains_original_and_recovers_one_stable_output() {
    recover(Phase::Link, "link");
}

#[test]
fn exact_source_unlink_failure_retains_original_and_recovers_one_stable_output() {
    recover(Phase::Unlink, "unlink");
}

#[test]
fn exact_source_receipt_update_failure_retains_original_and_recovers_one_stable_output() {
    recover(Phase::Receipt, "receipt");
}
