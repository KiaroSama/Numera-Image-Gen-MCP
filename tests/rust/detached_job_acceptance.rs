mod common;
#[path = "common/wire.rs"]
mod wire;
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::Config,
    files::images::{decode_base64, inspect},
    jobs::Store,
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, jobs},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

fn generation(root: &Path, origin: &str) -> Generation {
    let config = Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":{"local":{
                "adapter":"comfyui","baseUrl":origin,"baseUrlMode":"origin",
                "auth":{"type":"none"},"defaultModel":"workflow",
                "workflow":{
                    "graph":{
                        "text":{"class_type":"Text","inputs":{"text":""}},
                        "save":{"class_type":"Save","inputs":{"text":"output"}}
                    },
                    "bindings":{"prompt":{"node":"text","input":"text"}},
                    "outputNodes":["save"]
                }
            }}
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let (_, connection) = config.select(None).unwrap();
    assert_eq!(connection["requestTimeoutMs"], 300000);
    assert_eq!(connection["discoveryTimeoutMs"], 15000);
    assert_eq!(connection["workflow"]["requireGpu"], true);
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

#[tokio::test]
async fn exact_source_detached_job_refresh_saves_original_once_on_same_generation_and_store() {
    let workspace = Workspace::new();
    let root = &workspace.0;
    let bytes = original();
    let served = bytes.clone();
    wire::serve(
        |request| {
            let path = request.path.split('?').next().unwrap();
            match path {
                "/object_info" => wire::json_reply(json!({
                    "Text":{"input":{"required":{"text":["STRING"]}}},
                    "Save":{"output_node":true,"input":{"required":{"text":["STRING"]}}}
                })),
                "/system_stats" => wire::json_reply(json!({"devices":[{"type":"cuda"}]})),
                "/prompt" => {
                    assert_eq!(request.method, "POST");
                    assert!(
                        request
                            .headers
                            .to_ascii_lowercase()
                            .contains("content-type: application/json")
                    );
                    let body = Value::parse_with_limit(
                        std::str::from_utf8(&request.body).unwrap(),
                        1048576,
                    )
                    .unwrap();
                    assert_eq!(body["prompt"]["text"]["inputs"]["text"], "exact prompt");
                    wire::json_reply(json!({"prompt_id":"job"}))
                }
                "/history/job" => wire::json_reply(json!({
                    "job":{
                        "status":{"completed":true,"status_str":"success"},
                        "outputs":{"save":{"images":[{
                            "filename":"final.png","subfolder":"","type":"output"
                        }]}}
                    }
                })),
                "/view" => wire::Reply {
                    status: 200,
                    mime: "image/png",
                    body: served.clone(),
                },
                _ => wire::Reply {
                    status: 404,
                    ..wire::json_reply(json!({}))
                },
            }
        },
        |origin, requests| async move {
            let generation = generation(root, &origin);
            let signal = CancellationToken::new();
            let first = generation
                .run(
                    json!({"prompt":"exact prompt","request_id":"detached","wait":false}),
                    "generate",
                    &signal,
                )
                .await
                .unwrap();
            assert_eq!(first.status(), "running");
            assert_eq!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| request.path == "/prompt")
                    .count(),
                1,
            );
            let done = jobs::get(&generation, "detached", true, &signal, None)
                .await
                .unwrap();
            assert_eq!(done.status(), "completed");
            assert_eq!(done.value["outputs"].as_array().unwrap().len(), 1);
            let again = jobs::get(&generation, "detached", true, &signal, None)
                .await
                .unwrap();
            assert_eq!(again.value["outputs"].as_array().unwrap().len(), 1);
            assert_eq!(
                std::fs::read(done.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
                bytes,
            );
            assert_eq!(again.value["outputs"], done.value["outputs"]);
            assert_eq!(
                generation
                    .store
                    .lock()
                    .unwrap()
                    .list_outputs(0, 25)
                    .unwrap()
                    .len(),
                1
            );
            let captured = requests.lock().unwrap();
            assert_eq!(
                captured
                    .iter()
                    .filter(|request| request.path == "/prompt")
                    .count(),
                1
            );
            assert_eq!(
                captured
                    .iter()
                    .map(|request| (request.method.as_str(), request.path.as_str()))
                    .collect::<Vec<_>>(),
                [
                    ("GET", "/object_info"),
                    ("GET", "/system_stats"),
                    ("POST", "/prompt"),
                    ("GET", "/history/job"),
                    ("GET", "/view?filename=final.png&subfolder=&type=output"),
                ],
            );
        },
    )
    .await;
}
