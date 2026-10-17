mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    adapters::types::{Normalized, ResultItem},
    config::Config,
    files::images,
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{finalization, generation::Generation, jobs, request, requirements},
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn cases() -> Vec<Value> {
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/output-requirements.json");
    let values: Vec<Value> = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    let mut expected: HashSet<String> = [
        "original-aspect-tier",
        "opaque-aspect-tier",
        "known-missing-aspect",
        "known-unmatched-tier",
        "known-unmatched-aspect",
        "no-tier",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for model in ["gemini-3.1-flash-image", "gemini-3-pro-image"] {
        for ratio in [
            "1:1", "1:4", "1:8", "2:3", "3:2", "3:4", "4:1", "4:3", "4:5", "5:4", "8:1", "9:16",
            "16:9", "21:9",
        ] {
            if model == "gemini-3-pro-image" && ["1:4", "1:8", "4:1", "8:1"].contains(&ratio) {
                continue;
            }
            for tier in ["0.5K", "1K", "2K", "4K"] {
                if model == "gemini-3-pro-image" && tier == "0.5K" {
                    continue;
                }
                expected.insert(format!("{model}-{ratio}-{tier}"));
            }
        }
    }
    let names: HashSet<String> = values
        .iter()
        .map(|case| case["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(values.len(), 92);
    assert_eq!(
        names.len(),
        values.len(),
        "source scenario names are unique"
    );
    assert_eq!(
        names, expected,
        "six edges plus all 56 Flash and 30 Pro pairs"
    );
    values
}

#[test]
fn every_source_requirement_deviation_and_warning_matches_exact_metadata_and_json_round_trip() {
    let original = images::inspect(image_fixture::original(), &json!({}), None).unwrap();
    for case in cases() {
        let receipt = Receipt::new(case["receipt"].clone());
        let mut image = original.clone();
        image.width = case["image"]["width"].as_u64().unwrap() as u32;
        image.height = case["image"]["height"].as_u64().unwrap() as u32;
        image.mime = case["image"]["mime"].as_str().unwrap().into();
        image.alpha = case["image"]["alpha"].as_bool().unwrap();
        assert_eq!(
            json!(requirements::deviations(&receipt, &image, 0)),
            case["deviations"],
            "{}",
            case["name"]
        );
        assert_eq!(
            json!(requirements::warnings(&receipt)),
            case["warnings"],
            "{}",
            case["name"]
        );
        let round_trip = Receipt::new(Value::parse(&receipt.value.to_string()).unwrap());
        assert_eq!(
            requirements::deviations(&round_trip, &image, 0),
            requirements::deviations(&receipt, &image, 0)
        );
        assert_eq!(
            requirements::warnings(&round_trip),
            requirements::warnings(&receipt)
        );
    }
}

#[test]
fn exact_source_aspect_snapshot_round_trip_flash_four_thirds_and_all_documented_pairs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/requirements");
    let config = Config::parse(
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
    .unwrap();
    let (_, connection) = config.select(None).unwrap();
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/image-acceptance.json");
    let rows = Value::parse_with_limit(&std::fs::read_to_string(file).unwrap(), 1048576).unwrap();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "png")
        .unwrap();
    let bytes = images::decode_base64(row["base64"].as_str().unwrap(), 1048576).unwrap();
    let image = images::inspect(bytes.clone(), &config.value, None).unwrap();
    assert_eq!((image.width, image.height), (8, 6));
    assert!(image.alpha);
    let requested =
        request::parse(json!({"prompt":"draw","aspect_ratio":"16:9","image_size":"4K"})).unwrap();
    let receipt = |snapshot: Value| {
        let mut required = json!({"count":1});
        required
            .as_object_mut()
            .unwrap()
            .extend(snapshot.as_object().unwrap().clone());
        Receipt::new(json!({
            "request_id":"request-1","connection":"local","adapter":"openai-images",
            "operation":"generate","status":"prepared","requested_model":"image-model",
            "upstream_model":null,"outputs":[],"warnings":[],"deviations":[],"errors":[],
            "generation_outcome":"not_submitted","storage_outcome":"not_started",
            "another_submission_may_charge":false,"created_at":"2026-10-02T00:00:00.000Z",
            "updated_at":"2026-10-02T00:00:00.000Z","output_requirements":required
        }))
    };
    let opaque = receipt(requirements::snapshot(
        &connection,
        &"opaque-model".into(),
        &requested,
    ));
    let restored = Receipt::new(Value::parse(&opaque.value.to_string()).unwrap());
    let deviations = requirements::deviations(&opaque, &image, 0);
    assert_eq!(deviations, requirements::deviations(&restored, &image, 0));
    assert!(
        deviations
            .contains(&"Output 0 aspect ratio differs from requested aspect_ratio.".to_owned())
    );
    assert_eq!(requirements::warnings(&opaque).len(), 1);
    let native = receipt(requirements::snapshot(
        &connection,
        &"gemini-3.1-flash-image".into(),
        &request::parse(json!({"prompt":"draw","aspect_ratio":"4:3","image_size":"4K"})).unwrap(),
    ));
    assert!(requirements::deviations(&native, &image, 0).contains(
        &"Output 0 dimensions differ from the documented requested resolution tier.".to_owned(),
    ));
    for model in ["gemini-3.1-flash-image", "gemini-3-pro-image"] {
        let snapshot = requirements::snapshot(&connection, &model.into(), &requested);
        let evidence = &snapshot["dimension_evidence"];
        let sizes = evidence["pixel_sizes"].as_array().unwrap();
        assert_eq!(
            sizes.len(),
            if model == "gemini-3-pro-image" {
                30
            } else {
                56
            }
        );
        for size in sizes {
            let required = receipt(json!({
                "aspect_ratio":size["aspect_ratio"],"image_size":size["resolution"],
                "dimension_evidence":evidence
            }));
            let mut metadata = image.clone();
            metadata.width = size["width"].as_u64().unwrap() as u32;
            metadata.height = size["height"].as_u64().unwrap() as u32;
            assert!(requirements::deviations(&required, &metadata, 0).is_empty());
            assert!(requirements::warnings(&required).is_empty());
        }
    }
    assert_eq!(image.bytes, bytes);
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

async fn finalized(recover: bool, name: &str) {
    let action = async {
        let workspace = Workspace::new();
        let raw = workspace.config("https://invalid.example/v1", "openai-images");
        let g = generation(&workspace, &raw);
        let oracle = cases()
            .into_iter()
            .find(|case| case["name"] == name)
            .unwrap();
        let bytes = image_fixture::original();
        let mut request_raw = json!({"prompt": "draw", "count": 1});
        request_raw
            .as_object_mut()
            .unwrap()
            .extend(oracle["request"].as_object().unwrap().clone());
        let request = request::parse(request_raw).unwrap();
        let initial = Receipt::new(json!({
            "request_id": "requirements", "connection": "test", "adapter": "openai-images",
            "status": "prepared", "operation": "generate", "requested_model": oracle["model"],
            "outputs": [], "errors": [], "warnings": [], "deviations": [],
            "generation_outcome": "not_submitted", "storage_outcome": "not_started",
            "output_requirements": oracle["receipt"]["output_requirements"],
            "created_at": "2026-10-02T00:00:00.000Z"
        }));
        let receipt = {
            let mut store = g.store.lock().unwrap();
            let (_, receipt) = store.prepare(initial, "hash", "identity").unwrap();
            assert!(store.admit("requirements", "test", 2, 2).unwrap());
            assert!(store.claim("requirements", Some(&receipt), true).unwrap());
            store.get("requirements").unwrap()
        };
        let fault = rusqlite::Connection::open(workspace.0.join("state/receipts.sqlite")).unwrap();
        if recover {
            fault.execute_batch("CREATE TRIGGER reject_output BEFORE INSERT ON outputs BEGIN SELECT RAISE(FAIL,'Controlled commit failure'); END;").unwrap();
        }
        let signal = CancellationToken::new();
        let mut result = finalization::finalize(
            &g.config.value,
            &g.store,
            &g.logger,
            receipt,
            finalization::Finalization {
                result: Some(Normalized {
                    images: vec![ResultItem {
                        bytes: Some(bytes.clone()),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                request: &request,
                cancel: &signal,
                recovery: false,
            },
        )
        .await
        .unwrap();
        if recover {
            let pending = g.store.lock().unwrap().pending("requirements").unwrap();
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].bytes.as_ref().unwrap(), &bytes);
            assert_eq!(result.value["outputs"], json!([]));
            assert_eq!(result.value["errors"][0]["code"], "output_file_error");
            fault.execute_batch("DROP TRIGGER reject_output").unwrap();
            let mut removed = g.config.value.clone();
            removed["connections"] = json!({});
            removed.as_object_mut().unwrap().remove("defaultConnection");
            let recovered = Generation {
                config: Arc::new(
                    Config::parse(
                        &removed.to_string(),
                        &workspace.0.join("config.json"),
                        &[],
                        &HashMap::new(),
                    )
                    .unwrap(),
                ),
                store: g.store.clone(),
                logger: g.logger.clone(),
                runtime: g.runtime.clone(),
                env: HashMap::new().into(),
                active: g.active.clone(),
            };
            result = jobs::get(&recovered, "requirements", true, &signal, None)
                .await
                .unwrap();
            assert_eq!(
                result.value["outputs"][0]["output_id"],
                pending[0].output_id
            );
            assert_eq!(result.value["errors"], json!([]));
            assert_eq!(result.value["storage_outcome"], "completed");
        } else {
            assert_eq!(result.value["errors"], json!([]));
            assert_eq!(result.value["storage_outcome"], "completed");
        }
        assert_eq!(result.status(), "partial");
        assert_eq!(result.value["deviations"], oracle["deviations"]);
        assert_eq!(result.value["warnings"], oracle["warnings"]);
        assert_eq!(result.value["outputs"].as_array().unwrap().len(), 1);
        assert_eq!(
            std::fs::read(result.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            bytes
        );
        {
            let store = g.store.lock().unwrap();
            assert_eq!(store.list_outputs(0, 10).unwrap().len(), 1);
            assert!(!store.has_pending("requirements").unwrap());
            let stored = store.get("requirements").unwrap();
            assert_eq!(stored.value, result.value);
            assert_eq!(stored.value["output_requirements"]["aspect_ratio"], "16:9");
            assert_eq!(stored.value["output_requirements"]["image_size"], "4K");
        }
        assert_eq!(
            jobs::get(&g, "requirements", true, &signal, None)
                .await
                .unwrap()
                .value,
            result.value
        );
        drop(fault);
    };
    tokio::time::timeout(Duration::from_secs(8), action)
        .await
        .expect("output requirement recovery exceeded 8 seconds");
}

#[tokio::test]
async fn normal_finalization_preserves_exact_source_aspect_tier_deviations_and_original_bytes() {
    finalized(false, "original-aspect-tier").await;
}

#[tokio::test]
async fn real_sql_commit_failure_recovers_identical_source_requirements_without_network() {
    finalized(true, "original-aspect-tier").await;
}

#[tokio::test]
async fn opaque_tier_warning_is_persisted_exactly_once_through_sql_failure_and_local_recovery() {
    finalized(true, "opaque-aspect-tier").await;
}
