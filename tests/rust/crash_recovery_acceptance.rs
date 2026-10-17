mod common;
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::Config,
    files::{images, output},
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, jobs},
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
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

fn receipt() -> Receipt {
    Receipt::new(json!({
        "request_id":"child-crash","connection":"local","adapter":"openai-images",
        "operation":"generate","status":"prepared","requested_model":"image-model",
        "upstream_model":null,"outputs":[],"warnings":[],"deviations":[],"errors":[],
        "generation_outcome":"not_submitted","storage_outcome":"not_started",
        "another_submission_may_charge":false,
        "created_at":"2026-10-02T00:00:00.000Z","updated_at":"2026-10-02T00:00:00.000Z"
    }))
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
    let bytes = images::decode_base64(row["base64"].as_str().unwrap(), 1048576).unwrap();
    let image = images::inspect(bytes.clone(), config, None).unwrap();
    assert_eq!((image.width, image.height), (8, 6));
    assert_eq!(image.mime, "image/png");
    assert!(image.alpha);
    assert_eq!(image.bytes, bytes);
    bytes
}

fn checkpoint(phase: &str) -> ! {
    println!("literal-checkpoint:{phase}");
    std::io::stdout().flush().unwrap();
    let mut byte = [0u8; 1];
    let _ = std::io::stdin().read(&mut byte);
    panic!("Parent must kill the actual child at its checkpoint");
}

#[test]
fn literal_crash_child() {
    let Some(root) = std::env::var_os("NUMERA_LITERAL_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let phase = std::env::var("NUMERA_LITERAL_CRASH_PHASE").unwrap();
    let config = Config::parse(
        &std::fs::read_to_string(root.join("config.json")).unwrap(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let state = Path::new(config.value["stateDir"].as_str().unwrap());
    let mut store = Store::open(state).unwrap();
    if phase == "preintent" {
        checkpoint(&phase);
    }
    let mut initial = receipt();
    initial.value["status"] = json!(match phase.as_str() {
        "prepared" => "prepared",
        "finalizing" => "finalizing",
        "running" => "running",
        _ => "submitting",
    });
    initial.value["generation_outcome"] = json!(match phase.as_str() {
        "prepared" => "not_submitted",
        "submitted" | "finalizing" => "unknown",
        "running" => "running",
        _ => "completed",
    });
    initial.value["storage_outcome"] = json!("writing");
    initial.value["another_submission_may_charge"] = json!(true);
    initial.value["output_requirements"] = json!({"count":1,"output_subdirectory":"crash"});
    initial.value["created_at"] = json!("2026-10-05T00:00:00.000Z");
    initial.value["updated_at"] = json!("2026-10-05T00:00:00.000Z");
    if phase == "running" {
        initial.value["upstream_job"] = json!({"id":"existing-owned-job","kind":"responses"});
    }
    store.prepare(initial, "hash", "identity").unwrap();
    if ["prepared", "submitted", "finalizing", "running"].contains(&phase.as_str()) {
        checkpoint(&phase);
    }
    store
        .reserve(
            "child-crash",
            config.value["files"]["maxAggregateBytes"].as_i64().unwrap(),
            config.value["files"]["maxJournalBytes"].as_i64().unwrap(),
        )
        .unwrap();
    let image = images::inspect(
        std::fs::read(root.join("sample.png")).unwrap(),
        &config.value,
        None,
    )
    .unwrap();
    store.retain("child-crash", 0, &image.bytes, None).unwrap();
    assert!(store.claim("child-crash", None, false).unwrap());
    if phase == "published" || phase == "committed" {
        let fault = if phase == "published" {
            let db = rusqlite::Connection::open(state.join("receipts.sqlite")).unwrap();
            db.execute_batch("CREATE TRIGGER reject_literal_crash_output BEFORE INSERT ON outputs BEGIN SELECT RAISE(FAIL,'Controlled crash before SQL receipt commit'); END;").unwrap();
            Some(db)
        } else {
            None
        };
        let result = output::save(
            &image,
            &config.value,
            &mut store,
            output::Publication {
                id: "child-crash",
                index: 0,
                subdir: Some("crash"),
                prefix: None,
                deviations: &[],
                expected: None,
                stable: true,
            },
        );
        if phase == "published" {
            assert_eq!(result.unwrap_err().code, "output_file_error");
            let item = store.pending("child-crash").unwrap().remove(0);
            assert_eq!(std::fs::read(item.target.unwrap()).unwrap(), image.bytes);
            assert!(store.list_outputs(0, 100).unwrap().is_empty());
        } else {
            result.unwrap();
        }
        // Keep the synthetic fault connection and SQLite writer alive until the real kill.
        let _fault = fault;
        checkpoint(&phase);
    }
    assert_eq!(phase, "retained");
    checkpoint(&phase);
}

struct OwnedChild {
    child: Child,
    readers: Vec<std::thread::JoinHandle<()>>,
    diagnostics: Arc<Mutex<Vec<u8>>>,
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        self.child.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(2);
        let reaped = loop {
            if self.child.try_wait().is_ok_and(|status| status.is_some()) {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::yield_now();
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.readers.iter().any(|reader| !reader.is_finished()) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        let mut readers_closed = true;
        for reader in self.readers.drain(..) {
            if reader.is_finished() {
                readers_closed &= reader.join().is_ok();
            } else {
                readers_closed = false;
            }
        }
        if !reaped || !readers_closed {
            let detail = format!(
                "Crash fixture cleanup incomplete: pid={}, reaped={reaped}, readers_closed={readers_closed}; outer CI owner must reap the command tree.",
                self.child.id()
            );
            if std::thread::panicking() {
                eprintln!("{detail}");
            } else {
                panic!("{detail}");
            }
        }
    }
}

fn killed(root: &Path, phase: &str) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "literal_crash_child", "--nocapture"])
        .env_clear()
        .env("NUMERA_LITERAL_CRASH_ROOT", root)
        .env("NUMERA_LITERAL_CRASH_PHASE", phase)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in ["PATH", "SystemRoot"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = OwnedChild {
        child: command.spawn().unwrap(),
        readers: Vec::new(),
        diagnostics: Arc::new(Mutex::new(Vec::new())),
    };
    let stdout = child.child.stdout.take().unwrap();
    let stderr = child.child.stderr.take().unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    let expected = format!("literal-checkpoint:{phase}");
    for (mut pipe, ready) in [
        (Box::new(stdout) as Box<dyn Read + Send>, Some(sender)),
        (Box::new(stderr) as Box<dyn Read + Send>, None),
    ] {
        let captured = child.diagnostics.clone();
        let expected = expected.clone();
        child.readers.push(
            std::thread::Builder::new()
                .spawn(move || {
                    let mut buffer = [0u8; 1024];
                    let mut tail = Vec::new();
                    let mut signalled = false;
                    loop {
                        let length = pipe.read(&mut buffer).unwrap();
                        if length == 0 {
                            break;
                        }
                        tail.extend_from_slice(&buffer[..length]);
                        if tail.len() > 8192 {
                            let excess = tail.len() - 8192;
                            tail.drain(..excess);
                        }
                        if !signalled
                            && tail
                                .windows(expected.len())
                                .any(|v| v == expected.as_bytes())
                        {
                            if let Some(sender) = &ready {
                                let _ = sender.send(());
                            }
                            signalled = true;
                        }
                        let mut diagnostics = captured.lock().unwrap();
                        diagnostics.extend_from_slice(&buffer[..length]);
                        if diagnostics.len() > 8192 {
                            let excess = diagnostics.len() - 8192;
                            diagnostics.drain(..excess);
                        }
                    }
                })
                .unwrap(),
        );
    }
    if let Err(error) = receiver.recv_timeout(Duration::from_secs(5)) {
        let tail = child.diagnostics.lock().unwrap().clone();
        panic!(
            "Actual child checkpoint {phase} not reached: {error}; {}",
            String::from_utf8_lossy(&tail)
        );
    }
    child.child.kill().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = child.child.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "Actual killed child failed to exit within two seconds"
        );
        std::thread::yield_now();
    };
    assert!(!status.success());
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(9));
    }
    drop(child);
}

async fn recover(root: &Path, phase: &str, mut store: Store, mut config: Config, bytes: &[u8]) {
    if phase == "preintent" {
        assert_eq!(store.get("child-crash").unwrap_err().code, "invalid_input");
        assert_eq!(store.queued().unwrap(), 0);
    } else if phase == "running" {
        store.queued().unwrap();
        let old = store.get("child-crash").unwrap();
        assert_eq!(old.status(), "running");
        assert_eq!(
            old.value["upstream_job"],
            json!({"id":"existing-owned-job","kind":"responses"})
        );
        let g = generation(root, config, store);
        let current = jobs::get(&g, "child-crash", false, &CancellationToken::new(), None)
            .await
            .unwrap();
        assert_eq!(current.status(), "running");
        assert!(
            !g.store
                .lock()
                .unwrap()
                .prepare(receipt(), "hash", "identity")
                .unwrap()
                .0
        );
    } else if ["prepared", "submitted", "finalizing"].contains(&phase) {
        store.queued().unwrap();
        let old = store.get("child-crash").unwrap();
        assert_eq!(
            old.status(),
            if phase == "prepared" {
                "failed"
            } else {
                "outcome_unknown"
            }
        );
        assert_eq!(
            old.value["generation_outcome"],
            if phase == "prepared" {
                "not_submitted"
            } else {
                "unknown"
            }
        );
        assert!(!store.prepare(receipt(), "hash", "identity").unwrap().0);
        assert!(store.list_outputs(0, 100).unwrap().is_empty());
    } else {
        config.value["connections"] = json!({});
        config
            .value
            .as_object_mut()
            .unwrap()
            .remove("defaultConnection");
        let g = generation(root, config, store);
        let done = jobs::get(&g, "child-crash", true, &CancellationToken::new(), None)
            .await
            .unwrap();
        assert_eq!(done.status(), "completed");
        assert_eq!(done.value["generation_outcome"], "completed");
        assert_eq!(done.value["errors"], json!([]));
        assert_eq!(done.value["outputs"].as_array().unwrap().len(), 1);
        assert_eq!(
            std::fs::read(done.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            bytes
        );
        let store = g.store.lock().unwrap();
        assert_eq!(store.list_outputs(0, 100).unwrap().len(), 1);
        assert!(store.pending("child-crash").unwrap().is_empty());
        let visible = std::fs::read_dir(root.join("outputs/crash"))
            .unwrap()
            .map(|v| v.unwrap())
            .filter(|v| !v.file_name().to_string_lossy().starts_with('.'))
            .count();
        assert_eq!(visible, 1);
    }
}

fn generation(root: &Path, config: Config, store: Store) -> Generation {
    Generation {
        config: Arc::new(config),
        store: Arc::new(Mutex::new(store)),
        logger: Arc::new(Logger::new(&root.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(root).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    }
}

fn scenario(phase: &str) {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let config = config(&workspace.0);
    let bytes = original(&config.value);
    std::fs::write(workspace.0.join("config.json"), config.value.to_string()).unwrap();
    std::fs::write(workspace.0.join("sample.png"), &bytes).unwrap();
    killed(&workspace.0, phase);
    let state = workspace.0.join("state");
    // The native clock is fixed; SQL expiry models the elapsed lease, not Date.now parity.
    let db = rusqlite::Connection::open(state.join("receipts.sqlite")).unwrap();
    db.execute("UPDATE requests SET lease=0 WHERE id='child-crash'", [])
        .unwrap();
    if phase == "published" {
        db.execute_batch("DROP TRIGGER reject_literal_crash_output")
            .unwrap();
    }
    drop(db);
    runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(3),
            recover(
                &workspace.0,
                phase,
                Store::open(&state).unwrap(),
                config,
                &bytes,
            ),
        )
        .await
        .unwrap();
    });
    let mut reopened = Store::open(&state).unwrap();
    if phase == "preintent" {
        assert!(reopened.get("child-crash").is_err());
    } else {
        assert!(!reopened.prepare(receipt(), "hash", "identity").unwrap().0);
    }
    if ["retained", "published", "committed"].contains(&phase) {
        assert_eq!(
            reopened.get("child-crash").unwrap().value["outputs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn exact_source_killed_preintent_has_no_receipt_or_queue_across_two_reopens() {
    scenario("preintent");
}

#[test]
fn exact_source_killed_prepared_is_failed_without_submission_across_two_reopens() {
    scenario("prepared");
}

#[test]
fn exact_source_killed_submitted_remains_unknown_across_two_reopens() {
    scenario("submitted");
}

#[test]
fn exact_source_killed_finalizing_remains_unknown_across_two_reopens() {
    scenario("finalizing");
}

#[test]
fn exact_source_killed_running_keeps_existing_handle_across_two_reopens() {
    scenario("running");
}

#[test]
fn exact_source_killed_retained_recovers_one_original_across_two_reopens() {
    scenario("retained");
}

#[test]
fn exact_source_killed_published_recovers_one_original_across_two_reopens() {
    scenario("published");
}

#[test]
fn exact_source_killed_committed_recovers_one_original_across_two_reopens() {
    scenario("committed");
}
