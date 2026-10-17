mod common;
use common::Workspace;
use numera_image_gen_mcp::{
    adapters::types::{Normalized, ResultItem},
    compat::value::{Value, json},
    config::Config,
    files::images::{decode_base64, inspect},
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, jobs, request},
};
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
async fn exact_source_aborted_inline_finish_recovers_original_with_same_store_without_connection() {
    let workspace = Workspace::new();
    let config = config(&workspace.0);
    let store = Arc::new(Mutex::new(Store::open(&workspace.0.join("state")).unwrap()));
    let logger = Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR"));
    let initial = store
        .lock()
        .unwrap()
        .prepare(receipt("abort-inline"), "hash", "identity")
        .unwrap()
        .1;
    let g = Generation {
        config: Arc::new(config.clone()),
        store: store.clone(),
        logger: logger.clone(),
        runtime: RuntimePaths::new(&workspace.0).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    };
    let bytes = original();
    let action = async {
        let signal = CancellationToken::new();
        signal.cancel();
        let request = request::parse(json!({"prompt":"draw"})).unwrap();
        let result = Normalized {
            images: vec![ResultItem {
                bytes: Some(bytes.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let first = g
            .finish(initial, result, &request, &signal, false, false)
            .await
            .unwrap();
        assert_eq!(first.value["generation_outcome"], "completed");
        assert_eq!(first.value["outputs"], json!([]));
        assert_eq!(
            store.lock().unwrap().pending("abort-inline").unwrap().len(),
            1
        );
        let mut without_connection = config;
        without_connection.value["connections"] = json!({});
        without_connection
            .value
            .as_object_mut()
            .unwrap()
            .remove("defaultConnection");
        let recovery = Generation {
            config: Arc::new(without_connection),
            store: store.clone(),
            logger,
            runtime: g.runtime.clone(),
            env: g.env.clone(),
            active: Arc::new(Mutex::new(HashMap::new())),
        };
        let done = jobs::get(
            &recovery,
            "abort-inline",
            true,
            &CancellationToken::new(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(done.status(), "completed");
        assert_eq!(done.value["outputs"].as_array().unwrap().len(), 1);
        assert_eq!(
            std::fs::read(done.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            bytes
        );
        let store = store.lock().unwrap();
        assert_eq!(store.pending("abort-inline").unwrap().len(), 0);
        assert_eq!(store.list_outputs(0, 100).unwrap().len(), 1);
    };
    tokio::time::timeout(Duration::from_secs(3), action)
        .await
        .unwrap();
}

#[test]
fn exact_source_journal_reservation_refuses_second_request_until_terminal_update() {
    let workspace = Workspace::new();
    let mut store = Store::open(&workspace.0.join("state")).unwrap();
    store.prepare(receipt("a"), "a", "identity").unwrap();
    store.prepare(receipt("b"), "b", "identity").unwrap();
    store.reserve("a", 100, 100).unwrap();
    assert_eq!(
        store.reserve("b", 100, 100).unwrap_err().code,
        "rate_limited"
    );
    assert_eq!(
        store.get("b").unwrap().value["generation_outcome"],
        "not_submitted"
    );
    let mut terminal = store.get("a").unwrap();
    terminal.value["status"] = json!("failed");
    store.update(&mut terminal).unwrap();
    store.reserve("b", 100, 100).unwrap();
}
