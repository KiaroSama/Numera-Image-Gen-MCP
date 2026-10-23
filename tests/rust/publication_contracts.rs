mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    files::{
        images::inspect,
        output::{self, Publication},
    },
    jobs::{Receipt, Store},
};

fn exact_source_image(name: &str, config: &Value) -> numera_image_gen_mcp::files::images::Image {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".ci-work/parity/image-acceptance.json");
    let cases = Value::parse_with_limit(
        &std::fs::read_to_string(file).expect("exact source image fixtures"),
        1048576,
    )
    .unwrap();
    let case = cases
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap();
    inspect(
        numera_image_gen_mcp::files::images::decode_base64(
            case["base64"].as_str().unwrap(),
            1048576,
        )
        .unwrap(),
        config,
        None,
    )
    .unwrap()
}

#[test]
fn exact_source_owned_originals_reject_tampering_missing_files_prefixes_and_outside_records() {
    let workspace = Workspace::new();
    let raw = workspace.config("https://api.example/v1", "openai-images");
    let image = exact_source_image("png", &raw);
    let state = workspace.0.join("state");
    let mut store = Store::open(&state).unwrap();
    let mut saved = Vec::new();
    for _ in 0..2 {
        saved.push(
            output::save(
                &image,
                &raw,
                &mut store,
                Publication {
                    id: "receipt",
                    index: 0,
                    subdir: Some("batch"),
                    prefix: Some("café"),
                    deviations: &[],
                    expected: None,
                    stable: false,
                },
            )
            .unwrap(),
        );
    }
    assert_eq!(store.list_outputs(0, 25).unwrap().len(), 2);
    assert!(store.get("receipt").is_err());
    let first = &saved[0];
    let second = &saved[1];
    assert_ne!(first["path"], second["path"]);
    assert_eq!(first["request_id"], "receipt");
    assert_eq!(first["mime_type"], "image/png");
    assert_eq!(first["width"], 8);
    assert_eq!(first["height"], 6);
    assert_eq!(first["bytes"], image.bytes.len());
    assert_eq!(first["sha256"], image.sha256);
    assert_eq!(
        std::fs::read(first["path"].as_str().unwrap()).unwrap(),
        image.bytes
    );
    assert_eq!(
        output::owned(&raw, &store, first["output_id"].as_str().unwrap())
            .unwrap()
            .1
            .sha256,
        image.sha256
    );
    assert_eq!(
        std::fs::read_dir(workspace.0.join("outputs/batch"))
            .unwrap()
            .count(),
        2
    );
    std::fs::write(
        first["path"].as_str().unwrap(),
        exact_source_image("tamper-png", &raw).bytes,
    )
    .unwrap();
    assert_eq!(
        output::owned(&raw, &store, first["output_id"].as_str().unwrap())
            .err()
            .unwrap()
            .code,
        "output_file_error"
    );
    std::fs::remove_file(second["path"].as_str().unwrap()).unwrap();
    assert_eq!(
        output::owned(&raw, &store, second["output_id"].as_str().unwrap())
            .err()
            .unwrap()
            .code,
        "output_file_error"
    );
    for (subdir, prefix) in [
        (Some("batch"), Some("nested/prefix")),
        (Some("../escape"), None),
    ] {
        assert_eq!(
            output::save(
                &image,
                &raw,
                &mut store,
                Publication {
                    id: "receipt",
                    index: 1,
                    subdir,
                    prefix,
                    deviations: &[],
                    expected: None,
                    stable: false,
                }
            )
            .unwrap_err()
            .code,
            "invalid_input"
        );
    }
    let outside = workspace.0.join("outside.png");
    std::fs::write(&outside, &image.bytes).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let mut record = first.clone();
    record["output_id"] = json!(id);
    record["path"] = json!(outside.to_str().unwrap());
    store.add_output(&record).unwrap();
    assert_eq!(
        output::owned(&raw, &store, &id).err().unwrap().code,
        "permission_denied"
    );
    assert_eq!(store.list_outputs(0, 25).unwrap().len(), 3);
}

#[test]
fn exact_source_regular_file_output_root_fails_without_indexing_an_output() {
    let workspace = Workspace::new();
    let raw = workspace.config("https://api.example/v1", "openai-images");
    let mut store = Store::open(&workspace.0.join("state")).unwrap();
    std::fs::write(raw["outputDir"].as_str().unwrap(), b"blocked").unwrap();
    let image = exact_source_image("png", &raw);
    let (_, receipt) = store
        .prepare(
            Receipt::new(json!({
                "request_id":"receipt", "connection":"test", "status":"prepared",
                "outputs":[], "deviations":[], "generation_outcome":"not_submitted"
            })),
            "effective",
            "account",
        )
        .unwrap();
    store.reserve("receipt", 1048576, 2097152).unwrap();
    store
        .retain("receipt", 0, &image.bytes, Some(&receipt))
        .unwrap();
    assert_eq!(
        output::save(
            &image,
            &raw,
            &mut store,
            Publication {
                id: "receipt",
                index: 0,
                subdir: None,
                prefix: None,
                deviations: &[],
                expected: Some(&receipt),
                stable: true,
            }
        )
        .unwrap_err()
        .code,
        "storage_error"
    );
    assert!(store.list_outputs(0, 25).unwrap().is_empty());
    assert_eq!(
        store.pending("receipt").unwrap()[0].bytes.as_ref().unwrap(),
        &image.bytes
    );
    store.release("receipt", Some(&receipt)).unwrap();
}

#[test]
fn filesystem_sql_gap_reconciles_original_bytes_uuid_and_refuses_a_changed_publication_target() {
    let workspace = Workspace::new();
    let raw = workspace.config("https://invalid.example/v1", "openai-images");
    let image = inspect(image_fixture::original(), &raw, None).unwrap();
    let state = workspace.0.join("state");
    let mut store = Store::open(&state).unwrap();
    let (_,mut receipt)=store.prepare(Receipt::new(json!({"request_id":"gap","connection":"test","status":"prepared","outputs":[],"deviations":[],"generation_outcome":"not_submitted"})),"effective","account").unwrap();
    assert!(store.admit("gap", "test", 2, 2).unwrap());
    assert!(store.claim("gap", Some(&receipt), true).unwrap());
    receipt = store.get("gap").unwrap();
    store.reserve("gap", 1048576, 2097152).unwrap();
    store
        .retain("gap", 0, &image.bytes, Some(&receipt))
        .unwrap();
    let fault = rusqlite::Connection::open(state.join("receipts.sqlite")).unwrap();
    fault.execute_batch("CREATE TRIGGER reject_output BEFORE INSERT ON outputs BEGIN SELECT RAISE(FAIL,'Controlled output-index failure'); END;").unwrap();
    let publication = || Publication {
        id: "gap",
        index: 0,
        subdir: Some("originals"),
        prefix: Some("verified"),
        deviations: &[],
        expected: Some(&receipt),
        stable: true,
    };
    assert_eq!(
        output::save(&image, &raw, &mut store, publication())
            .unwrap_err()
            .code,
        "output_file_error"
    );
    let retained = store.pending("gap").unwrap().remove(0);
    let target = retained.target.clone().unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), image.bytes);
    assert_eq!(retained.bytes.as_ref().unwrap(), &image.bytes);
    fault.execute_batch("DROP TRIGGER reject_output").unwrap();
    std::fs::write(&target, b"changed fixture").unwrap();
    assert_eq!(
        output::save(&image, &raw, &mut store, publication())
            .unwrap_err()
            .code,
        "output_file_error"
    );
    assert!(store.list_outputs(0, 25).unwrap().is_empty());
    std::fs::write(&target, &image.bytes).unwrap();
    let saved = output::save(&image, &raw, &mut store, publication()).unwrap();
    assert_eq!(saved["output_id"], retained.output_id);
    assert_eq!(saved["path"], target);
    assert_eq!(store.list_outputs(0, 25).unwrap().len(), 1);
    assert!(store.pending("gap").unwrap().is_empty());
    assert_eq!(
        output::save(&image, &raw, &mut store, publication()).unwrap(),
        saved
    );
    assert_eq!(store.list_outputs(0, 25).unwrap().len(), 1);
    store.commit("gap", 0, &saved, &[], Some(&receipt)).unwrap();
    drop(fault);
    drop(store);
    let reopened = Store::open(&state).unwrap();
    assert_eq!(
        reopened
            .output(saved["output_id"].as_str().unwrap())
            .unwrap(),
        saved
    );
}
