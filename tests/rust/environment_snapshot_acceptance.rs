mod common;
#[path = "common/wire.rs"]
mod wire;

use base64::{Engine, engine::general_purpose::STANDARD};
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::{Config, credentials, snapshot_credentials},
    files::images::{decode_base64, inspect},
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, identity::connection_identity, jobs},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const QUEUE_KEY: &str = "NUMERA_TEST_SNAPSHOT_KEY";
const CANCEL_KEY: &str = "NUMERA_TEST_CANCEL_SNAPSHOT_KEY";

fn generation(root: &Path, connection: Value, key: &str) -> Generation {
    let config = Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":{"local":connection}
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    Generation {
        config: Arc::new(config),
        store: Arc::new(Mutex::new(Store::open(&root.join("state")).unwrap())),
        logger: Arc::new(Logger::new(&root.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(root).unwrap(),
        env: HashMap::from([(key.to_owned(), "synthetic-a".to_owned())]).into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    }
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

fn original() -> Vec<u8> {
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
    let image = inspect(bytes.clone(), &json!({}), None).unwrap();
    assert_eq!((image.width, image.height), (8, 6));
    assert_eq!(image.mime, "image/png");
    assert!(image.alpha);
    assert_eq!(image.bytes, bytes);
    bytes
}

fn assert_original(receipt: &Receipt, bytes: &[u8]) {
    let outputs = receipt.value["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 1);
    assert_eq!(
        std::fs::read(outputs[0]["path"].as_str().unwrap()).unwrap(),
        bytes
    );
}

#[test]
fn exact_source_env_credentials_pin_across_queue_but_refresh_on_same_generation() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(10), queue_case(&workspace))
            .await
            .expect("Source credential case exceeded ten seconds");
    });
}

async fn queue_case(workspace: &Workspace) {
    let bytes = original();
    let encoded = STANDARD.encode(&bytes);
    let mut accounts = Vec::new();
    let root = &workspace.0;
    let expected = bytes.as_slice();
    wire::serve(
        |request| {
            assert_eq!(request.method, "POST");
            assert_eq!(request.path, "/v1/images/generations");
            assert_eq!(
                Value::parse_with_limit(std::str::from_utf8(&request.body).unwrap(), 1048576)
                    .unwrap(),
                json!({"model":"image","prompt":"fixture","n":1})
            );
            accounts.push(
                wire::header(&request.headers, "authorization") == Some("Bearer synthetic-a"),
            );
            wire::json_reply(json!({"data":[{"b64_json":encoded}]}))
        },
        |origin, requests| async move {
            let mut g = generation(
                root,
                json!({
                    "adapter":"openai-images","baseUrl":format!("{origin}/v1"),
                    "defaultModel":"image","requestTimeoutMs":5000,
                    "auth":{"type":"bearer","secretEnv":QUEUE_KEY}
                }),
                QUEUE_KEY,
            );
            Arc::get_mut(&mut g.config).unwrap().value["maxConcurrentRequests"] = json!(1);
            {
                let mut store = g.store.lock().unwrap();
                let mut blocker = store
                    .prepare(receipt("blocker"), "fixture", "fixture")
                    .unwrap()
                    .1;
                assert!(store.admit("blocker", "local", 1, 1).unwrap());
                blocker.value["status"] = json!("running");
                blocker.value["upstream_job"] = json!({"id":"blocker","kind":"responses"});
                store.update(&mut blocker).unwrap();
                store.release("blocker", Some(&blocker)).unwrap();
            }
            let signal = CancellationToken::new();
            let pending = g.run(
                json!({"prompt":"fixture","request_id":"queued"}),
                "generate",
                &signal,
            );
            let rotate = async {
                let prepared = tokio::time::timeout(Duration::from_secs(2), async {
                    let mut poll = tokio::time::interval(Duration::from_millis(10));
                    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    loop {
                        poll.tick().await;
                        if g.store
                            .lock()
                            .unwrap()
                            .get("queued")
                            .is_ok_and(|receipt| receipt.status() == "prepared")
                        {
                            break;
                        }
                    }
                })
                .await;
                if prepared.is_err() {
                    signal.cancel();
                    return None;
                }
                let identity = g.store.lock().unwrap().identity("queued").unwrap();
                g.env.set(QUEUE_KEY, "synthetic-b").unwrap();
                g.store
                    .lock()
                    .unwrap()
                    .terminate(
                        "blocker",
                        "cancelled",
                        &json!({"code":"fixture"}),
                        None,
                        None,
                    )
                    .unwrap();
                Some(identity)
            };
            let (first, identity) = tokio::join!(pending, rotate);
            signal.cancel();
            let identity = identity.expect("Source prepared queue checkpoint exceeded two seconds");
            let first = first.unwrap();
            let next_signal = CancellationToken::new();
            let next = g
                .run(
                    json!({"prompt":"fixture","request_id":"next"}),
                    "generate",
                    &next_signal,
                )
                .await
                .unwrap();
            assert_eq!(first.status(), "completed");
            assert_eq!(next.status(), "completed");
            assert_original(&first, expected);
            assert_original(&next, expected);
            {
                let store = g.store.lock().unwrap();
                assert_eq!(store.identity("queued").unwrap(), identity);
                assert_ne!(store.identity("next").unwrap(), identity);
            }
            let persisted = json!([first.value, next.value, g.config.value]).to_string();
            assert!(!persisted.contains("synthetic-a"));
            assert!(!persisted.contains("synthetic-b"));
            assert_eq!(requests.lock().unwrap().len(), 2);
        },
    )
    .await;
    assert_eq!(accounts, [true, false]);
}

#[test]
fn exact_source_cancellation_account_survives_nested_completed_job_recovery_rotation() {
    let workspace = Workspace::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(10), cancellation_case(&workspace))
            .await
            .expect("Source credential case exceeded ten seconds");
    });
}

async fn cancellation_case(workspace: &Workspace) {
    let bytes = original();
    let mut accounts = Vec::new();
    let mut submissions = 0;
    // Share only the test's Generation handle so the status responder can rotate its env.
    let generation_slot = Mutex::new(None::<Arc<Generation>>);
    let slot = &generation_slot;
    let root = &workspace.0;
    let expected = bytes.as_slice();
    wire::serve(
        |request| {
            accounts.push(
                wire::header(&request.headers, "authorization") == Some("Bearer synthetic-a"),
            );
            match request.path.as_str() {
                "/api/jobs/owned/cancel" => {
                    assert_eq!(request.method, "POST");
                    wire::json_reply(json!({"cancelled":true}))
                }
                "/api/jobs/owned" => {
                    assert_eq!(request.method, "GET");
                    let g = slot.lock().unwrap().as_ref().unwrap().clone();
                    g.env.set(CANCEL_KEY, "synthetic-b").unwrap();
                    wire::json_reply(json!({"id":"owned","status":"completed"}))
                }
                "/history/owned" => {
                    assert_eq!(request.method, "GET");
                    wire::json_reply(json!({"owned":{
                        "status":{"completed":true,"status_str":"success"},
                        "outputs":{"save":{"images":[{
                            "filename":"original.png","type":"output","subfolder":""
                        }]}}
                    }}))
                }
                path if path.starts_with("/view") => {
                    assert_eq!(request.method, "GET");
                    wire::Reply {
                        status: 200,
                        mime: "image/png",
                        body: bytes.clone(),
                    }
                }
                _ => {
                    submissions += 1;
                    wire::Reply {
                        status: 404,
                        mime: "application/json",
                        body: b"{}".to_vec(),
                    }
                }
            }
        },
        |origin, requests| async move {
            let g = Arc::new(generation(
                root,
                json!({
                    "adapter":"comfyui","baseUrl":origin,"baseUrlMode":"origin",
                    "auth":{"type":"bearer","secretEnv":CANCEL_KEY},
                    "workflow":{
                        "graph":{"save":{"class_type":"Save","inputs":{}}},
                        "bindings":{},"outputNodes":["save"]
                    }
                }),
                CANCEL_KEY,
            ));
            *slot.lock().unwrap() = Some(g.clone());
            let (_, connection) = g.config.select(Some("local")).unwrap();
            assert_eq!(connection["requestTimeoutMs"], 300000);
            assert_eq!(connection["discoveryTimeoutMs"], 15000);
            assert_eq!(connection["workflow"]["requireGpu"], true);
            let identity = {
                let pinned = snapshot_credentials(&connection, &g.env, &g.runtime)
                    .await
                    .unwrap();
                connection_identity("local", &pinned).unwrap()
            };
            {
                let mut store = g.store.lock().unwrap();
                let mut old = store
                    .prepare(receipt("cancel-recover"), "hash", &identity)
                    .unwrap()
                    .1;
                assert!(store.admit("cancel-recover", "local", 1, 1).unwrap());
                old.value["status"] = json!("running");
                old.value["upstream_job"] = json!({"id":"owned","kind":"comfyui"});
                store.update(&mut old).unwrap();
                store.release("cancel-recover", Some(&old)).unwrap();
            }
            let signal = CancellationToken::new();
            let cancelled = jobs::cancel(&g, "cancel-recover", &signal).await.unwrap();
            assert_eq!(cancelled["refund_verified"], false);
            let result = jobs::get(&g, "cancel-recover", false, &signal, None)
                .await
                .unwrap();
            assert_eq!(result.status(), "completed");
            assert_original(&result, expected);
            assert!(!result.value.to_string().contains("synthetic-a"));
            let headers = credentials(&connection, &g.env, &g.runtime).await.unwrap();
            assert_eq!(
                headers,
                HashMap::from([("Authorization".to_owned(), "Bearer synthetic-b".to_owned())])
            );
            assert_eq!(requests.lock().unwrap().len(), 4);
        },
    )
    .await;
    assert_eq!(accounts, [true, true, true, true]);
    assert_eq!(submissions, 0);
}
