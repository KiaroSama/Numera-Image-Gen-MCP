mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/http.rs"]
mod http_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::{
    hash::sha256,
    value::{Value, json},
};
use numera_image_gen_mcp::{
    config::{Config, snapshot_credentials},
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, identity::connection_identity, jobs},
};
use std::{
    collections::HashMap,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

// Frozen source: integration/{comfy-terminal-recovery,comfy-result-retention}.test.ts.
// Real loopback/SQLite/Sharp originals; requireGpu=false makes no inference/GPU claim.
fn config(workspace: &Workspace, origin: &str) -> Value {
    let mut raw = workspace.config(origin, "comfyui");
    raw["connections"]["test"]["baseUrlMode"] = json!("origin");
    raw["connections"]["test"]["defaultModel"] = json!("workflow");
    raw["connections"]["test"]["discoveryTimeoutMs"] = json!(3000);
    raw["connections"]["test"]["workflow"] = json!({
        "graph": {
            "text": {"class_type": "Text", "inputs": {"text": ""}},
            "save": {"class_type": "Save", "inputs": {"text": "output"}}
        },
        "bindings": {"prompt": {"node": "text", "input": "text"}},
        "outputNodes": ["save"],
        "requireGpu": false
    });
    raw["files"]["maxOutputBytes"] = json!(1048576);
    raw["files"]["maxAggregateBytes"] = json!(2097152);
    raw["files"]["maxJournalBytes"] = json!(4194304);
    raw
}

fn generation(workspace: &Workspace, raw: &Value) -> Generation {
    Generation {
        config: Arc::new(
            Config::parse(
                &raw.to_string(),
                &workspace.0.join("config.json"),
                &[],
                &HashMap::new(),
            )
            .unwrap(),
        ),
        store: Arc::new(Mutex::new(Store::open(&workspace.0.join("state")).unwrap())),
        logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(&workspace.0).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    }
}

async fn checked(server: Server, action: impl Future<Output = ()> + Send + 'static) {
    // Capture assertion panics as well as elapsed deadlines before joining the owned server.
    let mut task = tokio::spawn(action);
    let result = tokio::time::timeout(Duration::from_secs(8), &mut task).await;
    if result.is_err() {
        task.abort();
        let _ = task.await;
    }
    server.close().await;
    result
        .expect("Comfy recovery scenario exceeded 8 seconds")
        .unwrap();
}

fn download(path: &str, filename: &str) {
    let url = url::Url::parse(&format!("http://fixture{path}")).unwrap();
    assert_eq!(url.path(), "/view");
    let pairs: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(pairs.len(), 3);
    assert_eq!(pairs.get("filename").map(String::as_str), Some(filename));
    assert_eq!(pairs.get("subfolder").map(String::as_str), Some(""));
    assert_eq!(pairs.get("type").map(String::as_str), Some("output"));
}

#[tokio::test]
async fn synchronous_comfy_completed_history_survives_first_download_failure_and_reopens_without_replay()
 {
    let bytes = image_fixture::original();
    let served = bytes.clone();
    let downloads = Arc::new(AtomicUsize::new(0));
    let observed = downloads.clone();
    let server = Server::new(move |path| match path {
        "/object_info" => Reply {
            status: 200,
            bytes: None,
            value: json!({
                "Text": {"input": {"required": {"text": ["STRING"]}}},
                "Save": {"output_node": true, "input": {"required": {"text": ["STRING"]}}}
            }),
        },
        "/prompt" => Reply {
            status: 200,
            bytes: None,
            value: json!({"prompt_id": "job"}),
        },
        "/history/job" => Reply {
            status: 200,
            bytes: None,
            value: json!({"job": {
                "status": {"completed": true, "status_str": "success"},
                "outputs": {"save": {"images": [
                    {"filename": "final.png", "subfolder": "", "type": "output"}
                ]}}
            }}),
        },
        path if path.starts_with("/view?") => {
            let first = observed.fetch_add(1, Ordering::SeqCst) == 0;
            Reply {
                status: if first { 500 } else { 200 },
                bytes: (!first).then(|| served.clone()),
                value: json!({}),
            }
        }
        _ => Reply {
            status: 404,
            bytes: None,
            value: json!({}),
        },
    })
    .await;
    let origin = server.origin.clone();
    let requests = server.requests.clone();
    checked(server, async move {
        let workspace = Workspace::new();
        let raw = config(&workspace, &origin);
        let g = generation(&workspace, &raw);
        let signal = CancellationToken::new();
        let first = g
            .run(
                json!({"prompt": "fixture", "request_id": "download-failure"}),
                "generate",
                &signal,
            )
            .await
            .unwrap();
        assert_eq!(first.status(), "running");
        assert_eq!(first.value["generation_outcome"], "completed");
        assert_eq!(first.value["storage_outcome"], "not_started");
        assert_eq!(
            first.value["upstream_job"],
            json!({"id": "job", "kind": "comfyui"})
        );
        assert!(first.value["upstream_terminal"].is_null());
        assert!(first.value["outputs"].as_array().unwrap().is_empty());
        assert_eq!(first.value["errors"].as_array().unwrap().len(), 1);
        assert_eq!(first.value["errors"][0]["code"], "upstream_error");
        assert_eq!(first.value["errors"][0]["http_status"], 500);
        assert_eq!(
            g.store
                .lock()
                .unwrap()
                .get("download-failure")
                .unwrap()
                .value,
            first.value
        );
        assert!(
            !g.store
                .lock()
                .unwrap()
                .has_pending("download-failure")
                .unwrap()
        );
        assert!(g.active.lock().unwrap().is_empty());
        assert_eq!(downloads.load(Ordering::SeqCst), 1);
        {
            let captured = requests.lock().unwrap();
            assert_eq!(captured.len(), 4);
            assert_eq!(&captured[..3], ["/object_info", "/prompt", "/history/job"]);
            download(&captured[3], "final.png");
        }
        drop(g);
        let g = generation(&workspace, &raw);
        assert_eq!(
            g.store
                .lock()
                .unwrap()
                .get("download-failure")
                .unwrap()
                .value,
            first.value
        );
        let recovered = jobs::get(&g, "download-failure", true, &signal, None)
            .await
            .unwrap();
        assert_eq!(recovered.status(), "completed");
        assert_eq!(recovered.value["generation_outcome"], "completed");
        assert_eq!(recovered.value["storage_outcome"], "completed");
        assert_eq!(recovered.value["upstream_job"], first.value["upstream_job"]);
        assert_eq!(recovered.value["outputs"].as_array().unwrap().len(), 1);
        assert!(recovered.value["errors"].as_array().unwrap().is_empty());
        let output = &recovered.value["outputs"][0];
        assert_eq!(
            std::fs::read(output["path"].as_str().unwrap()).unwrap(),
            bytes
        );
        assert_eq!(output["request_id"], "download-failure");
        assert_eq!(output["sha256"], sha256(&bytes));
        assert_eq!(output["mime_type"], "image/png");
        assert!(uuid::Uuid::parse_str(output["output_id"].as_str().unwrap()).is_ok());
        assert!(
            g.store
                .lock()
                .unwrap()
                .pending("download-failure")
                .unwrap()
                .is_empty()
        );
        let repeated = jobs::get(&g, "download-failure", true, &signal, None)
            .await
            .unwrap();
        assert_eq!(repeated.value, recovered.value);
        assert_eq!(
            g.store.lock().unwrap().list_outputs(0, 10).unwrap(),
            vec![output.clone()]
        );
        assert_eq!(downloads.load(Ordering::SeqCst), 2);
        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 6);
        assert_eq!(
            captured
                .iter()
                .filter(|path| path.as_str() == "/prompt")
                .count(),
            1
        );
        assert_eq!(captured[4], "/history/job");
        download(&captured[5], "final.png");
        assert_eq!(captured[3], captured[5]);
    })
    .await;
}

#[tokio::test]
async fn seeded_comfy_second_download_failure_retains_first_original_and_recovers_locally_after_removal()
 {
    let bytes = image_fixture::original();
    let served = bytes.clone();
    let server = Server::new(move |path| match path {
        "/history/owned" => Reply {
            status: 200,
            bytes: None,
            value: json!({"owned": {
                "status": {"completed": true},
                "outputs": {"save": {"images": [
                    {"filename": "one.png", "type": "output"},
                    {"filename": "two.png", "type": "output"}
                ]}}
            }}),
        },
        path if path.starts_with("/view?") => {
            let url = url::Url::parse(&format!("http://fixture{path}")).unwrap();
            let second = url
                .query_pairs()
                .any(|(key, value)| key == "filename" && value == "two.png");
            Reply {
                status: if second { 500 } else { 200 },
                bytes: (!second).then(|| served.clone()),
                value: json!({}),
            }
        }
        _ => Reply {
            status: 404,
            bytes: None,
            value: json!({}),
        },
    })
    .await;
    let origin = server.origin.clone();
    let requests = server.requests.clone();
    checked(server, async move {
        let workspace = Workspace::new();
        let mut raw = config(&workspace, &origin);
        raw["connections"]["test"]["workflow"] = json!({
            "graph": {"save": {"class_type": "Save", "inputs": {}}},
            "bindings": {}, "outputNodes": ["save"], "requireGpu": false
        });
        let g = generation(&workspace, &raw);
        let (_, connection) = g.config.select(Some("test")).unwrap();
        let pinned = snapshot_credentials(&connection, &g.env, &g.runtime)
            .await
            .unwrap();
        let identity = connection_identity("test", &pinned).unwrap();
        {
            let mut store = g.store.lock().unwrap();
            let initial = Receipt::new(json!({
                "request_id": "comfy-partial", "connection": "test", "adapter": "comfyui",
                "operation": "generate", "status": "running", "requested_model": "workflow",
                "outputs": [], "warnings": [], "errors": [], "deviations": [],
                "generation_outcome": "running", "storage_outcome": "not_started",
                "upstream_job": {"id": "owned", "kind": "comfyui"},
                "output_requirements": {"count": 2}, "another_submission_may_charge": false,
                "created_at": "2026-10-02T00:00:00.000Z"
            }));
            let (fresh, receipt) = store.prepare(initial, "hash", &identity).unwrap();
            assert!(fresh);
            store.release("comfy-partial", Some(&receipt)).unwrap();
        }
        assert!(requests.lock().unwrap().is_empty());
        let signal = CancellationToken::new();
        let first = jobs::get(&g, "comfy-partial", true, &signal, None)
            .await
            .unwrap();
        assert_eq!(first.status(), "failed");
        assert_eq!(first.value["generation_outcome"], "completed");
        assert_eq!(
            first.value["upstream_job"],
            json!({"id": "owned", "kind": "comfyui"})
        );
        assert!(first.value["outputs"].as_array().unwrap().is_empty());
        assert_eq!(first.value["errors"].as_array().unwrap().len(), 1);
        assert_eq!(first.value["errors"][0]["code"], "upstream_error");
        assert_eq!(first.value["errors"][0]["http_status"], 500);
        let retained = {
            let store = g.store.lock().unwrap();
            let mut pending = store.pending("comfy-partial").unwrap();
            assert_eq!(pending.len(), 1);
            assert!(store.needs_local_recovery("comfy-partial").unwrap());
            pending.remove(0)
        };
        assert_eq!(retained.index, 0);
        assert_eq!(retained.phase, "retained");
        assert_eq!(retained.bytes.as_ref().unwrap(), &bytes);
        assert_eq!(retained.sha256.as_deref(), Some(sha256(&bytes).as_str()));
        assert!(retained.url.is_none());
        assert!(retained.target.is_none());
        assert!(uuid::Uuid::parse_str(&retained.output_id).is_ok());
        let before = requests.lock().unwrap().clone();
        assert_eq!(before.len(), 3);
        assert_eq!(before[0], "/history/owned");
        download(&before[1], "one.png");
        download(&before[2], "two.png");
        drop(g);
        raw["connections"] = json!({});
        raw.as_object_mut().unwrap().remove("defaultConnection");
        let g = generation(&workspace, &raw);
        assert!(g.config.select(Some("test")).is_err());
        {
            let store = g.store.lock().unwrap();
            assert_eq!(store.get("comfy-partial").unwrap().value, first.value);
            assert_eq!(store.identity("comfy-partial").unwrap(), identity);
            let pending = store.pending("comfy-partial").unwrap();
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].output_id, retained.output_id);
            assert_eq!(pending[0].bytes.as_ref().unwrap(), &bytes);
        }
        let recovered = jobs::get(&g, "comfy-partial", true, &signal, None)
            .await
            .unwrap();
        assert_eq!(recovered.status(), "partial");
        assert_eq!(recovered.value["generation_outcome"], "completed");
        assert_eq!(recovered.value["storage_outcome"], "completed");
        assert_eq!(recovered.value["output_requirements"]["count"], 2);
        assert_eq!(recovered.value["upstream_job"], first.value["upstream_job"]);
        assert_eq!(recovered.value["outputs"].as_array().unwrap().len(), 1);
        assert_eq!(
            recovered.value["deviations"],
            json!(["Requested 2 image(s), saved 1."])
        );
        let output = &recovered.value["outputs"][0];
        assert_eq!(output["output_id"], retained.output_id);
        assert_eq!(output["request_id"], "comfy-partial");
        assert_eq!(output["sha256"], sha256(&bytes));
        assert_eq!(
            std::fs::read(output["path"].as_str().unwrap()).unwrap(),
            bytes
        );
        assert!(
            g.store
                .lock()
                .unwrap()
                .pending("comfy-partial")
                .unwrap()
                .is_empty()
        );
        assert_eq!(*requests.lock().unwrap(), before);
        drop(g);
        let g = generation(&workspace, &raw);
        let repeated = jobs::get(&g, "comfy-partial", true, &signal, None)
            .await
            .unwrap();
        assert_eq!(repeated.value, recovered.value);
        assert_eq!(
            g.store.lock().unwrap().list_outputs(0, 10).unwrap(),
            vec![output.clone()]
        );
        assert_eq!(
            std::fs::read(repeated.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            bytes
        );
        assert_eq!(*requests.lock().unwrap(), before);
        assert!(g.active.lock().unwrap().is_empty());
    })
    .await;
}
