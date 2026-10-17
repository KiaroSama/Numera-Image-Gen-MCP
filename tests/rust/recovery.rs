mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::{
    config::Config,
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, jobs},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn completed_retained_bytes_recover_after_local_cancel_without_credentials_or_post() {
    let workspace = Workspace::new();
    let root = workspace.0.join("state");
    let raw = workspace.config("https://invalid.example/v1", "openai-images");
    let config = Config::parse(
        &raw.to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let mut store = Store::open(&root).unwrap();
    let initial = Receipt::new(
        json!({"request_id":"recover-original","status":"prepared","connection":"removed","adapter":"openai-images","operation":"generate","requested_model":"opaque/image","upstream_model":null,"outputs":[],"warnings":[],"errors":[],"deviations":[],"generation_outcome":"not_submitted","storage_outcome":"not_started","created_at":"2026-10-11T00:00:00.000Z","updated_at":"2026-10-11T00:00:00.000Z","output_requirements":{"count":1,"output_subdirectory":"recovered"}}),
    );
    let (_, mut receipt) = store.prepare(initial, "effective", "old-account").unwrap();
    assert!(store.admit("recover-original", "removed", 4, 2).unwrap());
    receipt.value["status"] = json!("submitting");
    store.update(&mut receipt).unwrap();
    assert!(
        store
            .claim("recover-original", Some(&receipt), false)
            .unwrap()
    );
    receipt = store.get("recover-original").unwrap();
    let bytes = image_fixture::original();
    store.reserve("recover-original", 1048576, 2097152).unwrap();
    store
        .retain("recover-original", 0, &bytes, Some(&receipt))
        .unwrap();
    store.cancel("recover-original").unwrap();
    store.release("recover-original", Some(&receipt)).unwrap();
    drop(store);
    let generation = Generation {
        store: Arc::new(Mutex::new(Store::open(&root).unwrap())),
        logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(&workspace.0).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
        config: Arc::new(config),
    };
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        jobs::get(
            &generation,
            "recover-original",
            true,
            &CancellationToken::new(),
            None,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.status(), "completed");
    assert_eq!(result.value["outputs"].as_array().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(result.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
        bytes
    );
    assert_eq!(
        generation
            .store
            .lock()
            .unwrap()
            .identity("recover-original")
            .unwrap(),
        "old-account"
    );
    assert!(generation.active.lock().unwrap().is_empty());
}
