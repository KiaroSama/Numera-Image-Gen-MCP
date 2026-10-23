mod common;
#[path = "common/wire.rs"]
mod wire;

use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::{Config, snapshot_credentials},
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
};
use tokio_util::sync::CancellationToken;

// Frozen runtime.ts defaults and generic 8x6 Sharp original, not the broad image oracle.
fn generation(root: &Path, origin: &str, terminal: bool) -> Generation {
    let workflow = if terminal {
        json!({
            "graph":{
                "text":{"class_type":"Text","inputs":{"text":""}},
                "save":{"class_type":"Save","inputs":{"text":"output"}}
            },
            "bindings":{"prompt":{"node":"text","input":"text"}},
            "outputNodes":["save"]
        })
    } else {
        json!({"graph":{"save":{"class_type":"Save","inputs":{}}},"bindings":{},"outputNodes":["save"]})
    };
    let mut connection = json!({
        "adapter":"comfyui","baseUrl":origin,"baseUrlMode":"origin",
        "auth":{"type":"none"},"workflow":workflow
    });
    if terminal {
        connection["defaultModel"] = json!("workflow");
    }
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
    let (_, selected) = config.select(None).unwrap();
    assert_eq!(selected["requestTimeoutMs"], 300000);
    assert_eq!(selected["discoveryTimeoutMs"], 15000);
    assert_eq!(selected["workflow"]["requireGpu"], true);
    Generation {
        config: Arc::new(config),
        store: Arc::new(Mutex::new(Store::open(&root.join("state")).unwrap())),
        logger: Arc::new(Logger::new(&root.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(root).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    }
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

fn view(request: &wire::Request, filename: &str) {
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.path,
        format!("/view?filename={filename}&subfolder=&type=output")
    );
    assert!(request.body.is_empty());
    assert!(
        !request
            .headers
            .to_ascii_lowercase()
            .contains("authorization:")
    );
}

#[tokio::test]
async fn exact_source_completed_comfy_first_download_failure_refreshes_same_live_generation() {
    let workspace = Workspace::new();
    let bytes = original();
    let served = bytes.clone();
    let root = &workspace.0;
    let mut downloads = 0;
    wire::serve(
        |request| match request.path.split('?').next().unwrap() {
            "/object_info" => wire::json_reply(json!({
                "Text":{"input":{"required":{"text":["STRING"]}}},
                "Save":{"output_node":true,"input":{"required":{"text":["STRING"]}}}
            })),
            // Descriptor only: no GPU or inference is executed by this fixture.
            "/system_stats" => wire::json_reply(json!({"devices":[{"type":"cuda"}]})),
            "/prompt" => {
                assert_eq!(request.method, "POST");
                let body = Value::parse_with_limit(
                    std::str::from_utf8(&request.body).unwrap(),
                    1048576,
                )
                .unwrap();
                assert_eq!(body["prompt"]["text"]["inputs"]["text"], "fixture");
                wire::json_reply(json!({"prompt_id":"job"}))
            }
            "/history/job" => wire::json_reply(json!({"job":{
                "status":{"completed":true,"status_str":"success"},
                "outputs":{"save":{"images":[{"filename":"final.png","subfolder":"","type":"output"}]}}
            }})),
            "/view" => {
                downloads += 1;
                if downloads == 1 {
                    wire::Reply {
                        status: 500,
                        ..wire::json_reply(json!({}))
                    }
                } else {
                    wire::Reply {
                        status: 200,
                        mime: "image/png",
                        body: served.clone(),
                    }
                }
            }
            _ => wire::Reply {
                status: 404,
                ..wire::json_reply(json!({}))
            },
        },
        |origin, requests| async move {
            let generation = generation(root, &origin, true);
            let store = generation.store.clone();
            let active = generation.active.clone();
            let owner = store.lock().unwrap().owner.clone();
            let signal = CancellationToken::new();
            let first = generation
                .run(
                    json!({"prompt":"fixture","request_id":"download-failure"}),
                    "generate",
                    &signal,
                )
                .await
                .unwrap();
            assert_eq!(first.status(), "running");
            assert_eq!(first.value["generation_outcome"], "completed");
            assert_eq!(first.value["outputs"], json!([]));
            let done = jobs::get(&generation, "download-failure", true, &signal, None)
                .await
                .unwrap();
            assert_eq!(done.value["outputs"].as_array().unwrap().len(), 1);
            assert_eq!(done.value["generation_outcome"], "completed");
            assert_eq!(
                std::fs::read(done.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
                bytes
            );
            assert!(Arc::ptr_eq(&store, &generation.store));
            assert!(Arc::ptr_eq(&active, &generation.active));
            assert_eq!(generation.store.lock().unwrap().owner, owner);
            let calls = requests.lock().unwrap();
            assert_eq!(calls.iter().filter(|call| call.method == "POST").count(), 1);
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| call.path.starts_with("/view?"))
                    .count(),
                2
            );
            assert_eq!(calls.len(), 7);
            assert_eq!(calls[0].path, "/object_info");
            assert_eq!(calls[1].path, "/system_stats");
            assert_eq!(calls[2].path, "/prompt");
            assert_eq!(calls[3].path, "/history/job");
            assert_eq!(calls[5].path, "/history/job");
            view(&calls[4], "final.png");
            view(&calls[6], "final.png");
        },
    )
    .await;
}

#[tokio::test]
async fn exact_source_comfy_retained_original_recovers_with_new_generation_and_same_store() {
    let workspace = Workspace::new();
    let bytes = original();
    let served = bytes.clone();
    let root = &workspace.0;
    wire::serve(
        |request| match request.path.split('?').next().unwrap() {
            "/history/owned" => wire::json_reply(json!({"owned":{
                "status":{"completed":true},"outputs":{"save":{"images":[
                    {"filename":"one.png","type":"output"},
                    {"filename":"two.png","type":"output"}
                ]}}
            }})),
            "/view" if request.path.starts_with("/view?filename=two.png&") => wire::Reply {
                status: 500,
                ..wire::json_reply(json!({}))
            },
            "/view" => wire::Reply {
                status: 200,
                mime: "image/png",
                body: served.clone(),
            },
            _ => wire::Reply {
                status: 404,
                ..wire::json_reply(json!({}))
            },
        },
        |origin, requests| async move {
            let first_generation = generation(root, &origin, false);
            let store = first_generation.store.clone();
            let owner = store.lock().unwrap().owner.clone();
            let (_, connection) = first_generation.config.select(Some("local")).unwrap();
            assert!(connection["defaultModel"].is_null());
            let pinned = snapshot_credentials(
                &connection,
                &first_generation.env,
                &first_generation.runtime,
            )
            .await
            .unwrap();
            let identity = connection_identity("local", &pinned).unwrap();
            let initial = Receipt::new(json!({
                "request_id":"comfy-partial","connection":"local","adapter":"openai-images",
                "operation":"generate","status":"running","requested_model":"image-model",
                "upstream_model":null,"outputs":[],"warnings":[],"deviations":[],"errors":[],
                "generation_outcome":"running","storage_outcome":"not_started",
                "another_submission_may_charge":false,
                "created_at":"2026-10-02T00:00:00.000Z","updated_at":"2026-10-02T00:00:00.000Z",
                "upstream_job":{"id":"owned","kind":"comfyui"},"output_requirements":{"count":2}
            }));
            {
                let mut store = store.lock().unwrap();
                let (fresh, receipt) = store.prepare(initial, "hash", &identity).unwrap();
                assert!(fresh);
                store.release("comfy-partial", Some(&receipt)).unwrap();
            }
            let signal = CancellationToken::new();
            let first = jobs::get(&first_generation, "comfy-partial", true, &signal, None)
                .await
                .unwrap();
            assert_eq!(first.value["outputs"], json!([]));
            let retained_id = {
                let pending = store.lock().unwrap().pending("comfy-partial").unwrap();
                assert_eq!(pending.len(), 1);
                assert_eq!(pending[0].bytes.as_ref().unwrap(), &bytes);
                pending[0].output_id.clone()
            };
            let mut config = first_generation.config.as_ref().clone();
            config.value["connections"] = json!({});
            let recovery = Generation {
                config: Arc::new(config),
                store: store.clone(),
                logger: first_generation.logger.clone(),
                runtime: first_generation.runtime.clone(),
                env: first_generation.env.clone(),
                active: Arc::new(Mutex::new(HashMap::new())),
            };
            assert!(Arc::ptr_eq(&first_generation.store, &recovery.store));
            assert!(!Arc::ptr_eq(&first_generation.active, &recovery.active));
            assert_eq!(recovery.store.lock().unwrap().owner, owner);
            assert_eq!(recovery.config.value["defaultConnection"], "local");
            assert_eq!(recovery.config.value["connections"], json!({}));
            let recovered = jobs::get(&recovery, "comfy-partial", true, &signal, None)
                .await
                .unwrap();
            assert_eq!(recovered.status(), "partial");
            assert_eq!(recovered.value["outputs"].as_array().unwrap().len(), 1);
            let output = &recovered.value["outputs"][0];
            assert_eq!(
                std::fs::read(output["path"].as_str().unwrap()).unwrap(),
                bytes
            );
            assert_eq!(output["output_id"], retained_id);
            assert_eq!(recovery.store.lock().unwrap().owner, owner);
            assert_eq!(
                recovery
                    .store
                    .lock()
                    .unwrap()
                    .identity("comfy-partial")
                    .unwrap(),
                identity
            );
            let calls = requests.lock().unwrap();
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| call.path.starts_with("/view?"))
                    .count(),
                2
            );
            assert_eq!(calls.iter().filter(|call| call.method == "POST").count(), 0);
            assert_eq!(calls.len(), 3);
            assert_eq!(calls[0].method, "GET");
            assert_eq!(calls[0].path, "/history/owned");
            view(&calls[1], "one.png");
            view(&calls[2], "two.png");
        },
    )
    .await;
}
