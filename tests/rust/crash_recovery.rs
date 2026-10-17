mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::json,
    config::Config,
    files::{images, output, paths},
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::generation::Generation,
};
use std::{
    collections::HashMap,
    io::{BufRead, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn checkpoint(phase: &str) {
    println!("checkpoint-ready:{phase}");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    panic!("Parent must terminate the child at its checkpoint");
}

#[test]
fn crash_checkpoint_child() {
    let Some(root) = std::env::var_os("NUMERA_CRASH_FIXTURE_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let phase = std::env::var("NUMERA_CRASH_FIXTURE_PHASE").unwrap();
    let config = Config::parse(
        &std::fs::read_to_string(root.join("config.json")).unwrap(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let mut store = Store::open(Path::new(config.value["stateDir"].as_str().unwrap())).unwrap();
    println!("checkpoint-start:{phase}");
    std::io::stdout().flush().unwrap();
    if phase == "preintent" {
        checkpoint(&phase);
    }
    let mut receipt = Receipt::new(json!({
        "request_id":"child-crash","connection":"test","adapter":"openai-images","operation":"generate",
        "status":if ["prepared","retained","published","committed"].contains(&phase.as_str()){"prepared"}else if phase=="finalizing"{"finalizing"}else if phase=="running"{"running"}else{"submitting"},
        "requested_model":"opaque/image","upstream_model":null,"outputs":[],"warnings":[],"deviations":[],"errors":[],
        "generation_outcome":if ["prepared","retained","published","committed"].contains(&phase.as_str()){"not_submitted"}else if phase=="running"{"running"}else{"unknown"},
        "storage_outcome":"writing","another_submission_may_charge":true,
        "output_requirements":{"count":1,"output_subdirectory":"crash"},
        "created_at":"2026-10-05T00:00:00.000Z","updated_at":"2026-10-05T00:00:00.000Z"
    }));
    if phase == "running" {
        receipt.value["upstream_job"] = json!({"id":"existing-owned-job","kind":"responses"});
    }
    let (_, owner) = store.prepare(receipt, "hash", "identity").unwrap();
    if ["prepared", "submitted", "finalizing", "running"].contains(&phase.as_str()) {
        checkpoint(&phase);
    }
    assert!(store.admit("child-crash", "test", 4, 4).unwrap());
    let mut completed = store.get("child-crash").unwrap();
    completed.value["status"] = json!("finalizing");
    completed.value["generation_outcome"] = json!("completed");
    store.update(&mut completed).unwrap();
    store.reserve("child-crash", 1048576, 2097152).unwrap();
    let bytes = std::fs::read(root.join("sample.png")).unwrap();
    store
        .retain("child-crash", 0, &bytes, Some(&owner))
        .unwrap();
    if phase == "retained" {
        checkpoint(&phase);
    }
    if phase == "published" {
        let directory = paths::output_directory(
            Path::new(config.value["outputDir"].as_str().unwrap()),
            Some("crash"),
        )
        .unwrap();
        let item = store
            .publication(
                "child-crash",
                0,
                |id| Ok(directory.join(format!("{id}.png")).to_str().unwrap().into()),
                Some(&owner),
            )
            .unwrap();
        let target = std::path::PathBuf::from(item.target.unwrap());
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, &bytes).unwrap();
        checkpoint(&phase);
    }
    if phase == "committed" {
        let image = images::inspect(bytes, &config.value, None).unwrap();
        output::save(
            &image,
            &config.value,
            &mut store,
            output::Publication {
                id: "child-crash",
                index: 0,
                subdir: Some("crash"),
                prefix: None,
                deviations: &[],
                expected: Some(&owner),
                stable: true,
            },
        )
        .unwrap();
        checkpoint(&phase);
    }
    panic!("Unknown fixture checkpoint");
}

struct OwnedChild {
    child: Child,
    reader: Option<std::thread::JoinHandle<()>>,
    errors: Option<std::thread::JoinHandle<()>>,
    diagnostics: Arc<Mutex<Vec<u8>>>,
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        self.child.stdin.take();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            reader.join().expect("checkpoint reader cleanup");
        }
        if let Some(errors) = self.errors.take() {
            errors.join().expect("checkpoint diagnostic cleanup");
        }
    }
}

#[tokio::test]
async fn actual_killed_children_preserve_all_eight_source_checkpoints_without_replay() {
    tokio::time::timeout(Duration::from_secs(50), async {
        for phase in [
            "preintent",
            "prepared",
            "submitted",
            "finalizing",
            "running",
            "retained",
            "published",
            "committed",
        ] {
            let workspace = Workspace::new();
            let config_path = workspace.0.join("config.json");
            let config_text = workspace
                .config("http://127.0.0.1:1/v1", "openai-images")
                .to_string();
            std::fs::write(&config_path, &config_text).unwrap();
            let bytes = image_fixture::original();
            std::fs::write(workspace.0.join("sample.png"), &bytes).unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "crash_checkpoint_child", "--nocapture"])
                .env("NUMERA_CRASH_FIXTURE_ROOT", &workspace.0)
                .env("NUMERA_CRASH_FIXTURE_PHASE", phase)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command.spawn().unwrap();
            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();
            let diagnostics = Arc::new(Mutex::new(Vec::new()));
            let captured = diagnostics.clone();
            let errors = std::thread::spawn(move || {
                use std::io::Read;
                let mut stderr = stderr;
                let mut buffer = [0u8; 1024];
                while let Ok(length) = stderr.read(&mut buffer) {
                    if length == 0 {
                        break;
                    }
                    let mut bytes = captured.lock().unwrap();
                    bytes.extend_from_slice(&buffer[..length]);
                    if bytes.len() > 4096 {
                        let excess = bytes.len() - 4096;
                        bytes.drain(..excess);
                    }
                }
            });
            let expected = format!("checkpoint-ready:{phase}");
            let (sender, receiver) = mpsc::sync_channel(1);
            let reader = std::thread::spawn(move || {
                for line in std::io::BufReader::new(stdout).lines() {
                    if line.unwrap() == expected {
                        let _ = sender.send(());
                        break;
                    }
                }
            });
            let mut child = OwnedChild {
                child,
                reader: Some(reader),
                errors: Some(errors),
                diagnostics,
            };
            if let Err(error) = receiver.recv_timeout(Duration::from_secs(5)) {
                let status = child.child.try_wait().unwrap();
                let bytes = child.diagnostics.lock().unwrap().clone();
                let diagnostics = String::from_utf8(bytes).unwrap_or_else(|_| {
                    "Child diagnostic tail contains incomplete or invalid UTF-8.".into()
                });
                panic!("Checkpoint {phase} was not reached: {error}; status={status:?}; child stderr={diagnostics}");
            }
            child.child.kill().unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            loop {
                if child.child.try_wait().unwrap().is_some() {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "child termination bound"
                );
                tokio::task::yield_now().await;
            }
            drop(child);
            let state = workspace.0.join("state");
            // Observe an expired writer immediately instead of sleeping for the 30-second lease.
            let db = rusqlite::Connection::open(state.join("receipts.sqlite")).unwrap();
            db.execute("UPDATE requests SET lease=0", []).unwrap();
            drop(db);
            let mut store = Store::open(&state).unwrap();
            if phase == "preintent" {
                assert!(store.get("child-crash").is_err());
                continue;
            }
            let old = store.get("child-crash").unwrap();
            assert!(!store.prepare(old, "hash", "identity").unwrap().0);
            let receipt = store.get("child-crash").unwrap();
            if phase == "prepared" {
                assert_eq!(receipt.value["generation_outcome"], "not_submitted");
            } else if phase == "running" {
                assert_eq!(receipt.value["upstream_job"]["id"], "existing-owned-job");
            } else if ["submitted", "finalizing"].contains(&phase) {
                assert_eq!(receipt.value["generation_outcome"], "unknown");
            } else {
                let pending = store.pending("child-crash").unwrap();
                if phase == "committed" {
                    assert!(pending.is_empty());
                    let outputs = store.list_outputs(0, 25).unwrap();
                    assert_eq!(outputs.len(), 1);
                    assert_eq!(
                        receipt.value["outputs"][0]["output_id"],
                        outputs[0]["output_id"]
                    );
                    assert_eq!(
                        std::fs::read(outputs[0]["path"].as_str().unwrap()).unwrap(),
                        bytes
                    );
                } else {
                    assert_eq!(pending[0].bytes.as_ref().unwrap(), &bytes);
                    let id = pending[0].output_id.clone();
                    let config =
                        Config::parse(&config_text, &config_path, &[], &HashMap::new()).unwrap();
                    let generation = Generation {
                        config: Arc::new(config),
                        store: Arc::new(Mutex::new(store)),
                        logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
                        runtime: RuntimePaths::new(&workspace.0).unwrap(),
                        env: HashMap::new().into(),
                        active: Arc::new(Mutex::new(HashMap::new())),
                    };
                    let receipt = numera_image_gen_mcp::services::jobs::recover_local(
                        &generation,
                        "child-crash",
                        &CancellationToken::new(),
                    )
                    .await
                    .unwrap();
                    assert_eq!(
                        receipt.status(),
                        "completed",
                        "phase {phase}: errors={}",
                        receipt.value["errors"]
                    );
                    assert_eq!(
                        receipt.value["outputs"][0]["output_id"],
                        id,
                        "phase {phase}: errors={}",
                        receipt.value["errors"]
                    );
                    assert_eq!(
                        std::fs::read(receipt.value["outputs"][0]["path"].as_str().unwrap())
                            .unwrap(),
                        bytes
                    );
                }
            }
        }
    })
    .await
    .expect("bounded eight-checkpoint crash matrix");
}
