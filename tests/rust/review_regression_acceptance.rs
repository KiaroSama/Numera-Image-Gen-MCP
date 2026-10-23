mod common;
#[path = "common/wire.rs"]
mod wire;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::Workspace;
use numera_image_gen_mcp::{
    adapters::{
        build_request, normalize,
        types::{Input, Normalized, ResultItem},
    },
    capabilities,
    compat::value::{Value, json},
    config::{Config, snapshot_credentials},
    files::images::{decode_base64, inspect},
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, identity::connection_identity, jobs, request},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn config(root: &Path, adapter: &str, overrides: Value) -> Config {
    let mut c =
        json!({"adapter":adapter,"baseUrl":"https://api.example/v1","auth":{"type":"none"}});
    c.as_object_mut()
        .unwrap()
        .extend(overrides.as_object().unwrap().clone());
    Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":{"local":c}
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
        "request_id":"request-1","connection":"local","adapter":"openai-images",
        "operation":"generate","status":"prepared","requested_model":"image-model",
        "upstream_model":null,"outputs":[],"warnings":[],"deviations":[],"errors":[],
        "generation_outcome":"not_submitted","storage_outcome":"not_started",
        "another_submission_may_charge":false,
        "created_at":"2026-10-02T00:00:00.000Z","updated_at":"2026-10-02T00:00:00.000Z"
    }))
}

fn generation(config: Arc<Config>, store: Store, root: &Path, logger: Arc<Logger>) -> Generation {
    Generation {
        config,
        store: Arc::new(Mutex::new(store)),
        logger,
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
    bytes
}

fn ignored_batch(adapter: &str) {
    let workspace = Workspace::new();
    let config = config(
        &workspace.0,
        adapter,
        json!({"modelOverrides":{"model":{"maxCount":3}}}),
    );
    let (_, c) = config.select(None).unwrap();
    let r = request::parse(json!({"prompt":"draw","count":3})).unwrap();
    let error = capabilities::effective_request(&c, &r, &"model".into()).unwrap_err();
    assert!(error.message.contains("batch field"));
}

#[test]
fn exact_source_gemini_rejects_ignored_native_batch() {
    ignored_batch("gemini");
}

#[test]
fn exact_source_interactions_rejects_ignored_native_batch() {
    ignored_batch("gemini-interactions");
}

#[test]
fn exact_source_responses_rejects_ignored_native_batch() {
    ignored_batch("openai-responses");
}

#[test]
fn exact_source_chat_rejects_ignored_native_batch() {
    ignored_batch("chat-images");
}

fn nonforwarded(gateway: &str, model: &str, field: &str, value: &str) {
    let workspace = Workspace::new();
    let config = config(&workspace.0, "openai-images", json!({"gateway":gateway}));
    let (_, c) = config.select(None).unwrap();
    let mut raw = json!({"prompt":"draw"});
    raw[field] = json!(value);
    let r = request::parse(raw).unwrap();
    let error = build_request(&Input {
        connection: &c,
        request: &r,
        model: &model.into(),
        operation: "generate",
        references: &[],
        mask: None,
    })
    .err()
    .unwrap();
    assert!(error.message.contains("unsupported"));
}

#[test]
fn exact_source_omniroute_rejects_nonforwarded_format() {
    nonforwarded("omniroute", "codex/model", "output_format", "webp");
}

#[test]
fn exact_source_nine_router_rejects_nonforwarded_quality() {
    nonforwarded("9router", "antigravity/model", "quality", "high");
}

fn missing_async_id(adapter: &str) {
    let workspace = Workspace::new();
    let config = config(&workspace.0, adapter, json!({}));
    let (_, c) = config.select(None).unwrap();
    let error = normalize::normalize(&c, &json!({"status":"in_progress"}))
        .err()
        .unwrap();
    assert!(error.message.contains("recoverable job ID"));
}

#[test]
fn exact_source_interactions_requires_real_async_job_id() {
    missing_async_id("gemini-interactions");
}

#[test]
fn exact_source_responses_requires_real_async_job_id() {
    missing_async_id("openai-responses");
}

#[tokio::test]
async fn exact_source_two_stores_admit_one_finalizer_and_restore_original_output_requirements() {
    let workspace = Workspace::new();
    let bytes = original();
    let encoded = STANDARD.encode(&bytes);
    let (root, original) = (&workspace.0, &bytes);
    wire::serve(
        |request| {
            assert_eq!(request.method, "GET");
            assert_eq!(request.path, "/v1/responses/owned");
            assert!(
                !request
                    .headers
                    .to_ascii_lowercase()
                    .contains("authorization:")
            );
            assert!(request.body.is_empty());
            wire::json_reply(json!({
                "id":"owned","status":"completed","output":[
                    {"type":"image_generation_call","status":"completed","result":encoded},
                    {"type":"image_generation_call","status":"completed","result":encoded}
                ]
            }))
        },
        |origin, requests| async move {
            let config = Arc::new(config(
                root,
                "openai-responses",
                json!({"baseUrl":format!("{origin}/v1")}),
            ));
            let store1 = Store::open(&root.join("state")).unwrap();
            let store2 = Store::open(&root.join("state")).unwrap();
            let logger = Arc::new(Logger::new(&root.join("logs"), "ERROR"));
            let first = generation(config.clone(), store1, root, logger.clone());
            let second = generation(config, store2, root, logger);
            let (_, c) = first.config.select(None).unwrap();
            let pinned = snapshot_credentials(&c, &first.env, &first.runtime)
                .await
                .unwrap();
            let identity = connection_identity("local", &pinned).unwrap();
            let mut initial = receipt();
            initial.value["status"] = json!("running");
            initial.value["upstream_job"] = json!({"id":"owned","kind":"responses"});
            initial.value["output_requirements"] = json!({
                "count":2,"output_subdirectory":"requested","filename_prefix":"original",
                "output_format":"png","size":"8x6"
            });
            {
                let mut store = first.store.lock().unwrap();
                store.prepare(initial, "hash", &identity).unwrap();
                store.release("request-1", None).unwrap();
            }
            let signal = CancellationToken::new();
            let (one, two) = tokio::join!(
                jobs::get(&first, "request-1", true, &signal, None),
                jobs::get(&second, "request-1", true, &signal, None)
            );
            let results = [one.unwrap(), two.unwrap()];
            let store = first.store.lock().unwrap();
            let final_receipt = store.get("request-1").unwrap();
            assert_eq!(final_receipt.status(), "completed");
            assert_eq!(final_receipt.value["outputs"].as_array().unwrap().len(), 2);
            assert_eq!(final_receipt.value["deviations"], json!([]));
            assert_eq!(store.list_outputs(0, 100).unwrap().len(), 2);
            for output in final_receipt.value["outputs"].as_array().unwrap() {
                let path = output["path"].as_str().unwrap();
                assert!(path.contains("requested"));
                assert!(path.contains("original-"));
                assert_eq!(&std::fs::read(path).unwrap(), original);
            }
            assert!(
                results
                    .iter()
                    .any(|receipt| receipt.status() == "completed")
            );
            assert_eq!(requests.lock().unwrap().len(), 1);
        },
    )
    .await;
}

#[tokio::test]
async fn exact_source_excessive_final_items_refuse_before_url_processing() {
    let workspace = Workspace::new();
    let config = Arc::new(config(&workspace.0, "openai-images", json!({})));
    let g = generation(
        config,
        Store::open(&workspace.0.join("state")).unwrap(),
        &workspace.0,
        Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
    );
    let initial = g
        .store
        .lock()
        .unwrap()
        .prepare(receipt(), "hash", "identity")
        .unwrap()
        .1;
    let result = Normalized {
        images: (0..11)
            .map(|_| ResultItem {
                url: Some("http://127.0.0.1/never-fetch".into()),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let r = request::parse(json!({"prompt":"draw"})).unwrap();
    let signal = CancellationToken::new();
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        g.finish(initial, result, &r, &signal, false, false),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(error.code, "invalid_response");
    assert_eq!(
        error.message,
        "Provider returned too many final image items."
    );
    assert_eq!(
        g.store.lock().unwrap().list_outputs(0, 100).unwrap(),
        Vec::<Value>::new()
    );
}
