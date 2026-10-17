mod common;

use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::Config,
    files::{
        images::{Image, decode_base64, inspect},
        output::{self, Publication},
    },
    jobs::{Receipt, Store},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};

fn open_store(directory: &Path, clock: &Arc<AtomicI64>) -> Store {
    let clock = Arc::clone(clock);
    Store::open_with_lease_clock(directory, move || clock.load(Ordering::SeqCst)).unwrap()
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

fn original(config: &Value) -> Image {
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
    image
}

#[test]
fn exact_source_lease_renewal_preserves_live_owner_then_takeover_fences_final_catch() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    let clock = Arc::new(AtomicI64::new(1000));
    let mut first = open_store(&directory, &clock);
    let mut second = open_store(&directory, &clock);
    assert_eq!(Store::LEASE_MS, 30000);

    first.prepare(receipt("lease"), "hash", "identity").unwrap();
    assert!(first.admit("lease", "local", 1, 1).unwrap());
    clock.store(30999, Ordering::SeqCst);
    assert!(first.renew("lease", None).unwrap());
    clock.store(31001, Ordering::SeqCst);
    assert!(!second.claim("lease", None, false).unwrap());
    let stale = first.get("lease").unwrap();
    clock.store(61001, Ordering::SeqCst);
    assert!(second.claim("lease", None, false).unwrap());

    let mut final_catch = stale.clone();
    final_catch.value["status"] = json!("failed");
    assert_eq!(
        first.update(&mut final_catch).unwrap_err().code,
        "outcome_unknown"
    );
    assert!(!first.renew("lease", None).unwrap());
    assert_eq!(second.get("lease").unwrap().status(), "finalizing");
}

#[test]
fn exact_source_expired_prepared_queue_is_reclaimed_without_replaying_known_request() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    let clock = Arc::new(AtomicI64::new(1000));
    let mut store = open_store(&directory, &clock);
    assert_eq!(Store::LEASE_MS, 30000);

    for index in 0..20 {
        store
            .prepare(
                receipt(&format!("dead-{index}")),
                &format!("hash-{index}"),
                "identity",
            )
            .unwrap();
    }
    store
        .prepare(receipt("dead-prepared"), "hash", "identity")
        .unwrap();
    clock.store(31001, Ordering::SeqCst);
    assert_eq!(store.queued().unwrap(), 0);
    let expired = store.get("dead-prepared").unwrap();
    assert_eq!(expired.status(), "failed");
    assert_eq!(expired.value["generation_outcome"], "not_submitted");
    assert!(
        !store
            .prepare(receipt("dead-prepared"), "hash", "identity")
            .unwrap()
            .0
    );
    store
        .prepare(receipt("fresh"), "fresh-hash", "identity")
        .unwrap();
    assert_eq!(store.queued().unwrap(), 1);
    assert!(store.admit("fresh", "local", 1, 1).unwrap());
}

#[test]
fn exact_source_stale_same_instance_publication_is_fenced_after_recovery_generation() {
    let workspace = Workspace::new();
    let config = config(&workspace.0);
    let clock = Arc::new(AtomicI64::new(1000));
    let mut store = open_store(
        Path::new(config.value["stateDir"].as_str().unwrap()),
        &clock,
    );
    assert_eq!(Store::LEASE_MS, 30000);

    let initial = store
        .prepare(receipt("same-instance"), "hash", "identity")
        .unwrap()
        .1;
    let id = initial.id().unwrap();
    store
        .reserve(
            id,
            config.value["files"]["maxAggregateBytes"].as_i64().unwrap(),
            config.value["files"]["maxJournalBytes"].as_i64().unwrap(),
        )
        .unwrap();
    let image = original(&config.value);
    store.retain(id, 0, &image.bytes, None).unwrap();
    store.claim(id, None, false).unwrap();
    let stale = store.get(id).unwrap();
    clock.store(31001, Ordering::SeqCst);
    assert!(store.claim(id, None, false).unwrap());

    assert_eq!(
        output::save(
            &image,
            &config.value,
            &mut store,
            Publication {
                id,
                index: 0,
                subdir: Some("fenced"),
                prefix: None,
                deviations: &[],
                expected: Some(&stale),
                stable: true,
            },
        )
        .unwrap_err()
        .code,
        "outcome_unknown"
    );
    assert_eq!(
        store
            .retain(id, 0, &image.bytes, Some(&stale))
            .unwrap_err()
            .code,
        "outcome_unknown"
    );
    let mut final_catch = stale.clone();
    final_catch.value["status"] = json!("failed");
    assert_eq!(
        store.update(&mut final_catch).unwrap_err().code,
        "outcome_unknown"
    );
    store.release(id, Some(&stale)).unwrap();
    assert!(store.owns(id, None).unwrap());
    assert!(store.list_outputs(0, 100).unwrap().is_empty());
}
