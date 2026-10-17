mod common;
#[path = "common/http.rs"]
mod http_fixture;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::Workspace;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    adapters::types::{Normalized, ResultItem},
    config::Config,
    files::images::{decode_base64, inspect},
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, jobs, request},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn generation(workspace: &Workspace, aggregate: Option<usize>) -> Generation {
    let connection = json!({
        "adapter":"openai-images","baseUrl":"https://api.example/v1","auth":{"type":"none"}
    });
    generation_with_connection(workspace, connection, aggregate)
}

fn generation_with_connection(
    workspace: &Workspace,
    connection: Value,
    aggregate: Option<usize>,
) -> Generation {
    let mut raw = json!({
        "schemaVersion":1,"defaultConnection":"local",
        "outputDir":workspace.0.join("outputs").to_str().unwrap(),
        "stateDir":workspace.0.join("state").to_str().unwrap(),
        "logging":{"directory":workspace.0.join("logs").to_str().unwrap()},
        "files":{"allowedInputRoots":[workspace.0.join("inputs").to_str().unwrap()]},
        "connections":{"local":connection}
    });
    if let Some(limit) = aggregate {
        raw["files"]["maxAggregateBytes"] = json!(limit);
    }
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
        runtime: RuntimePaths::new(&workspace.0).unwrap(),
        store: Arc::new(Mutex::new(Store::open(&workspace.0.join("state")).unwrap())),
        logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    }
}

fn original(format: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".ci-work/parity/image-acceptance.json");
    let text = std::fs::read_to_string(path).unwrap();
    let cases = Value::parse_with_limit(&text, 1048576).unwrap();
    let case = cases
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == format)
        .unwrap();
    let bytes = decode_base64(case["base64"].as_str().unwrap(), 1048576).unwrap();
    let image = inspect(bytes.clone(), &json!({}), None).unwrap();
    assert_eq!((image.width, image.height), (8, 6));
    assert_eq!(image.mime, format!("image/{format}"));
    bytes
}

fn prepare(generation: &Generation, id: &str, hash: &str) -> Receipt {
    let initial = Receipt::new(json!({
        "request_id":id,"connection":"local","adapter":"openai-images","operation":"generate",
        "status":"prepared","requested_model":"image-model","upstream_model":null,
        "outputs":[],"warnings":[],"deviations":[],"errors":[],
        "generation_outcome":"not_submitted","storage_outcome":"not_started",
        "another_submission_may_charge":false,
        "created_at":"2026-10-02T00:00:00.000Z","updated_at":"2026-10-02T00:00:00.000Z"
    }));
    generation
        .store
        .lock()
        .unwrap()
        .prepare(initial, hash, "identity")
        .unwrap()
        .1
}

fn bytes_result(bytes: Vec<u8>) -> Normalized {
    Normalized {
        images: vec![ResultItem {
            bytes: Some(bytes),
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[tokio::test]
async fn exact_source_preintent_refusals_leave_no_queued_or_active_work() {
    let workspace = Workspace::new();
    let g = generation(&workspace, None);
    let signal = CancellationToken::new();
    for (raw, operation, code) in [
        (json!({"prompt":"draw"}), "generate", "invalid_input"),
        (
            json!({"prompt":"draw","model":"image"}),
            "edit",
            "invalid_input",
        ),
        (
            json!({"prompt":"draw","model":"image","output_subdirectory":"../escape"}),
            "generate",
            "invalid_input",
        ),
        (
            json!({"prompt":"draw","model":"image","filename_prefix":"nested/prefix"}),
            "generate",
            "invalid_input",
        ),
        (
            json!({"prompt":"draw","model":"image","wait":false}),
            "generate",
            "unsupported_operation",
        ),
    ] {
        let error = tokio::time::timeout(Duration::from_secs(3), g.run(raw, operation, &signal))
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, code);
    }
    assert_eq!(g.store.lock().unwrap().queued().unwrap(), 0);
    assert!(g.active.lock().unwrap().is_empty());
}

#[tokio::test]
async fn exact_source_data_url_edit_mask_and_aggregate_refusals_precede_intent() {
    let workspace = Workspace::new();
    let connection = json!({
        "adapter":"openai-images","baseUrl":"https://api.example/v1","auth":{"type":"none"},
        "defaultModel":"image","edit":{
            "mode":"multipart","encoding":"image","masks":true,
            "maskPolarity":"transparent-edit","maxReferences":2
        }
    });
    let mut g = generation_with_connection(&workspace, connection, None);
    let png = original("png");
    let jpeg = original("jpeg");
    let reference = json!({
        "type":"data_url","data_url":format!("data:image/png;base64,{}", STANDARD.encode(&png))
    });
    let signal = CancellationToken::new();
    for (raw, operation) in [
        (
            json!({
                "prompt":"edit","reference_images":[reference],
                "mask":{"type":"data_url","data_url":format!("data:image/jpeg;base64,{}",STANDARD.encode(&jpeg))}
            }),
            "edit",
        ),
        (json!({"prompt":"edit","mask":reference}), "generate"),
    ] {
        let error = tokio::time::timeout(Duration::from_secs(3), g.run(raw, operation, &signal))
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, "invalid_input");
    }
    Arc::get_mut(&mut g.config).unwrap().value["files"]["maxAggregateBytes"] = json!(png.len());
    for raw in [
        json!({"prompt":"edit","reference_images":[reference,reference]}),
        json!({
            "prompt":"edit","reference_images":[{"type":"data_url","data_url":"data:image/png;base64,YQ=="}]
        }),
    ] {
        let error = tokio::time::timeout(Duration::from_secs(3), g.run(raw, "edit", &signal))
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, "invalid_input");
    }
    assert_eq!(g.store.lock().unwrap().queued().unwrap(), 0);
    assert!(g.active.lock().unwrap().is_empty());
}

#[tokio::test]
async fn exact_source_owned_responses_job_refresh_and_active_cancel_are_separate_from_refunds() {
    let workspace = Workspace::new();
    let bytes = original("png");
    let encoded = STANDARD.encode(&bytes);
    let server = Server::new(move |_| Reply {
        status: 200,
        bytes: None,
        value: json!({
            "id":"owned job","status":"completed","output":[{
                "type":"image_generation_call","status":"completed","result":encoded
            }]
        }),
    })
    .await;
    let connection = json!({
        "adapter":"openai-responses","baseUrl":format!("{}/v1",server.origin),
        "auth":{"type":"none"},"orchestrationModel":"text","discoveryTimeoutMs":2000
    });
    let g = generation_with_connection(&workspace, connection, None);
    let result = tokio::time::timeout(Duration::from_secs(6), async {
        let (_, connection) = g.config.select(None).unwrap();
        let pinned = numera_image_gen_mcp::config::snapshot_credentials(
            &connection,
            &HashMap::new(),
            &g.runtime,
        )
        .await
        .unwrap();
        let identity =
            numera_image_gen_mcp::services::identity::connection_identity("local", &pinned)
                .unwrap();
        let mut initial = Receipt::new(json!({
            "request_id":"request-1","connection":"local","adapter":"openai-images",
            "operation":"generate","status":"running","requested_model":"image-model",
            "upstream_model":null,"outputs":[],"warnings":[],"deviations":[],"errors":[],
            "generation_outcome":"not_submitted","storage_outcome":"not_started",
            "another_submission_may_charge":false,
            "created_at":"2026-10-02T00:00:00.000Z","updated_at":"2026-10-02T00:00:00.000Z"
        }));
        initial.value["upstream_job"] = json!({"id":"owned job","kind":"responses"});
        g.store
            .lock()
            .unwrap()
            .prepare(initial, "hash", &identity)
            .unwrap();
        let signal = CancellationToken::new();
        assert_eq!(
            jobs::get(&g, "request-1", false, &signal, None)
                .await
                .unwrap()
                .status(),
            "running"
        );
        assert!(server.requests.lock().unwrap().is_empty());
        let completed = jobs::get(&g, "request-1", true, &signal, None)
            .await
            .unwrap();
        assert_eq!(completed.status(), "completed");
        assert_eq!(
            server.requests.lock().unwrap().as_slice(),
            ["/v1/responses/owned%20job"]
        );
        let controller = CancellationToken::new();
        g.active
            .lock()
            .unwrap()
            .insert("request-1".into(), controller.clone());
        let result = jobs::cancel(&g, "request-1", &signal).await.unwrap();
        assert_eq!(result["refund_verified"], false);
        assert!(controller.is_cancelled());
        g.active.lock().unwrap().remove("request-1");
        assert_eq!(
            std::fs::read(completed.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            bytes
        );
    })
    .await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn exact_source_direct_finish_keeps_jpeg_sibling_metadata_and_ordered_deviations() {
    let workspace = Workspace::new();
    let g = generation(&workspace, None);
    let initial = prepare(&g, "request-1", "hash");
    let jpeg = original("jpeg");
    let mut result = bytes_result(jpeg.clone());
    result.images.push(ResultItem {
        error: Some("fixture output failure".into()),
        ..Default::default()
    });
    result.upstream_model = Some("actual-model".into());
    result.upstream_id = Some("upstream-1".into());
    result.usage = Some(json!({"images":1}));
    result.warnings = vec!["fixture warning".into()];
    let request = request::parse(json!({
        "prompt":"draw","count":2,"size":"10x10","output_format":"png","background":"transparent"
    }))
    .unwrap();
    let receipt = tokio::time::timeout(
        Duration::from_secs(3),
        g.finish(
            initial,
            result,
            &request,
            &CancellationToken::new(),
            false,
            false,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(receipt.status(), "partial");
    assert_eq!(receipt.value["generation_outcome"], "completed");
    assert_eq!(receipt.value["storage_outcome"], "partial_or_failed");
    assert_eq!(receipt.value["upstream_model"], "actual-model");
    assert_eq!(receipt.value["upstream_request_id"], "upstream-1");
    assert_eq!(receipt.value["usage"], json!({"images":1}));
    assert_eq!(receipt.value["outputs"].as_array().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(receipt.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
        jpeg
    );
    assert_eq!(receipt.value["errors"].as_array().unwrap().len(), 1);
    assert_eq!(receipt.value["errors"][0]["code"], "invalid_response");
    assert_eq!(receipt.value["warnings"], json!(["fixture warning"]));
    assert_eq!(
        receipt.value["deviations"],
        json!([
            "Output 0 format is image/jpeg, not requested png.",
            "Output 0 dimensions differ from requested size.",
            "Output 0 has no alpha channel.",
            "Requested 2 image(s), saved 1."
        ])
    );
    let store = g.store.lock().unwrap();
    assert_eq!(store.get("request-1").unwrap().value, receipt.value);
    assert!(!store.owns("request-1", None).unwrap());
}

#[tokio::test]
async fn exact_source_direct_finish_aggregate_cancel_and_same_store_recovery_preserve_originals() {
    let workspace = Workspace::new();
    let bytes = original("png");
    let g = generation(&workspace, Some(bytes.len()));
    let initial = prepare(&g, "request-1", "hash");
    let mut result = bytes_result(bytes.clone());
    result.images.push(ResultItem {
        base64: Some(STANDARD.encode(&bytes).into()),
        ..Default::default()
    });
    let signal = CancellationToken::new();
    let request = request::parse(json!({"prompt":"draw","count":2})).unwrap();
    let partial = tokio::time::timeout(
        Duration::from_secs(3),
        g.finish(initial, result, &request, &signal, false, false),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(partial.status(), "partial");
    assert_eq!(partial.value["outputs"].as_array().unwrap().len(), 1);
    assert_eq!(partial.value["errors"].as_array().unwrap().len(), 1);
    assert_eq!(partial.value["errors"][0]["code"], "invalid_response");
    assert_eq!(
        partial.value["errors"][0]["message"],
        "Aggregate output bytes exceeded."
    );
    assert!(!g.store.lock().unwrap().owns("request-1", None).unwrap());
    let initial = prepare(&g, "cancelled", "cancelled-hash");
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let request = request::parse(json!({"prompt":"draw"})).unwrap();
    let failed = tokio::time::timeout(
        Duration::from_secs(3),
        g.finish(
            initial,
            bytes_result(bytes.clone()),
            &request,
            &cancelled,
            false,
            false,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(failed.status(), "failed");
    assert_eq!(failed.value["generation_outcome"], "completed");
    assert_eq!(failed.value["storage_outcome"], "partial_or_failed");
    assert_eq!(failed.value["outputs"], json!([]));
    assert_eq!(failed.value["errors"].as_array().unwrap().len(), 1);
    {
        let store = g.store.lock().unwrap();
        assert_eq!(store.pending("cancelled").unwrap().len(), 1);
        assert!(!store.owns("cancelled", None).unwrap());
    }
    let recovered = tokio::time::timeout(
        Duration::from_secs(3),
        jobs::get(&g, "cancelled", true, &signal, None),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(recovered.status(), "completed");
    assert_eq!(
        std::fs::read(recovered.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
        bytes
    );
    assert_eq!(
        g.store.lock().unwrap().list_outputs(0, 100).unwrap().len(),
        2
    );
}

#[tokio::test]
async fn exact_source_direct_data_url_finish_refresh_and_cancel_keep_terminal_receipt() {
    let workspace = Workspace::new();
    let g = generation(&workspace, None);
    let initial = prepare(&g, "request-1", "hash");
    let bytes = original("png");
    let result = Normalized {
        images: vec![ResultItem {
            url: Some(format!("data:image/png;base64,{}", STANDARD.encode(&bytes)).into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let signal = CancellationToken::new();
    let request = request::parse(json!({"prompt":"draw"})).unwrap();
    let receipt = tokio::time::timeout(
        Duration::from_secs(3),
        g.finish(initial, result, &request, &signal, false, false),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(receipt.status(), "completed");
    assert_eq!(receipt.value["storage_outcome"], "completed");
    assert_eq!(receipt.value["errors"], json!([]));
    assert_eq!(receipt.value["deviations"], json!([]));
    assert_eq!(
        std::fs::read(receipt.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
        bytes
    );
    let refreshed = tokio::time::timeout(
        Duration::from_secs(3),
        jobs::get(&g, "request-1", true, &signal, None),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(refreshed.value, receipt.value);
    let cancelled = tokio::time::timeout(
        Duration::from_secs(3),
        jobs::cancel(&g, "request-1", &signal),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        cancelled,
        json!({
            "request_id":"request-1","local_wait_cancelled":true,"upstream_requested":false,
            "upstream_cancelled":null,"refund_verified":false,"another_submission_may_charge":true
        })
    );
    let store = g.store.lock().unwrap();
    assert!(store.cancelled("request-1").unwrap());
    assert!(!store.owns("request-1", None).unwrap());
}

#[tokio::test]
async fn exact_source_terminal_partial_finish_recovers_on_same_generation_without_capacity_resurrection()
 {
    let workspace = Workspace::new();
    let g = generation(&workspace, None);
    let mut receipt = prepare(&g, "partial", "hash");
    {
        let mut store = g.store.lock().unwrap();
        assert!(store.admit("partial", "local", 1, 1).unwrap());
        receipt.value["status"] = json!("running");
        receipt.value["upstream_job"] = json!({"id":"job","kind":"responses"});
        store.update(&mut receipt).unwrap();
    }
    let bytes = original("png");
    let mut result = bytes_result(bytes.clone());
    result.upstream_id = Some("job".into());
    result.terminal = Some("failed".into());
    let signal = CancellationToken::new();
    signal.cancel();
    let request = request::parse(json!({"prompt":"fixture","count":1})).unwrap();
    let first = tokio::time::timeout(
        Duration::from_secs(3),
        g.finish(receipt, result, &request, &signal, false, false),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(first.value["generation_outcome"], "failed");
    {
        let store = g.store.lock().unwrap();
        assert_eq!(
            store.pending("partial").unwrap()[0].bytes.as_deref(),
            Some(bytes.as_slice())
        );
        assert!(!store.owns("partial", None).unwrap());
    }
    prepare(&g, "fresh", "fresh");
    {
        let mut store = g.store.lock().unwrap();
        assert!(store.admit("fresh", "local", 1, 1).unwrap());
        let mut end = store.get("fresh").unwrap();
        end.value["status"] = json!("failed");
        store.update(&mut end).unwrap();
        store.release("fresh", Some(&end)).unwrap();
    }
    let recovered = tokio::time::timeout(
        Duration::from_secs(3),
        jobs::recover_local(&g, "partial", &CancellationToken::new()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(recovered.value["generation_outcome"], "failed");
    assert_eq!(recovered.status(), "partial");
    assert_eq!(
        std::fs::read(recovered.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
        bytes
    );
    assert!(!g.store.lock().unwrap().owns("partial", None).unwrap());
}

#[tokio::test]
async fn direct_finish_releases_on_error_but_claimed_finish_preserves_outer_writer() {
    let workspace = Workspace::new();
    let g = generation(&workspace, None);
    let request = request::parse(json!({"prompt":"draw"})).unwrap();
    let initial = prepare(&g, "oversized", "hash");
    let result = Normalized {
        images: (0..11).map(|_| ResultItem::default()).collect(),
        ..Default::default()
    };
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        g.finish(
            initial,
            result,
            &request,
            &CancellationToken::new(),
            false,
            false,
        ),
    )
    .await
    .unwrap()
    .err()
    .unwrap();
    assert_eq!(error.code, "invalid_response");
    assert!(!g.store.lock().unwrap().owns("oversized", None).unwrap());
    let initial = prepare(&g, "outer", "outer");
    let current = {
        let mut store = g.store.lock().unwrap();
        assert!(store.claim("outer", Some(&initial), false).unwrap());
        store.get("outer").unwrap()
    };
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        g.finish(
            current,
            bytes_result(original("png")),
            &request,
            &CancellationToken::new(),
            true,
            false,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let mut store = g.store.lock().unwrap();
    assert!(store.owns("outer", Some(&result)).unwrap());
    store.release("outer", Some(&result)).unwrap();
    assert!(!store.owns("outer", None).unwrap());
}

#[tokio::test]
async fn exact_source_cancelled_mixed_result_retains_inline_bytes_without_asset_download() {
    let workspace = Workspace::new();
    let server = Server::new(|_| Reply {
        status: 500,
        bytes: None,
        value: json!({}),
    })
    .await;
    let action = async {
        let mut g = generation(&workspace, None);
        Arc::get_mut(&mut g.config).unwrap().value["files"]["assetAllowances"] =
            json!([{"origin":server.origin,"pathPrefix":"/image"}]);
        let initial = prepare(&g, "cancel-assets", "hash");
        g.store.lock().unwrap().cancel("cancel-assets").unwrap();
        let bytes = original("png");
        let result = Normalized {
            images: vec![
                ResultItem {
                    url: Some(format!("{}/image.png", server.origin).into()),
                    ..Default::default()
                },
                ResultItem {
                    bytes: Some(bytes.clone()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let signal = CancellationToken::new();
        signal.cancel();
        let request = request::parse(json!({"prompt":"draw","count":2})).unwrap();
        let receipt = g
            .finish(initial, result, &request, &signal, false, false)
            .await
            .unwrap();
        assert!(server.requests.lock().unwrap().is_empty());
        assert_eq!(receipt.value["outputs"], json!([]));
        let store = g.store.lock().unwrap();
        let pending = store.pending("cancel-assets").unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[1].bytes.as_deref(), Some(bytes.as_slice()));
        assert_eq!(receipt.value["generation_outcome"], "completed");
    };
    let result = tokio::time::timeout(Duration::from_secs(3), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn exact_source_sql_failure_recovers_published_original_with_same_store() {
    let action = async {
        let workspace = Workspace::new();
        let g = generation(&workspace, None);
        let database = workspace.0.join("state/receipts.sqlite");
        let fault = rusqlite::Connection::open(database).unwrap();
        let bytes = original("png");
        prepare(&g, "sql-fault", "hash");
        fault.execute_batch("CREATE TRIGGER reject_output BEFORE INSERT ON outputs BEGIN SELECT RAISE(FAIL,'Controlled output-index failure'); END;").unwrap();
        let initial = g.store.lock().unwrap().get("sql-fault").unwrap();
        let request = request::parse(json!({"prompt":"draw"})).unwrap();
        let result = g
            .finish(
                initial,
                bytes_result(bytes.clone()),
                &request,
                &CancellationToken::new(),
                false,
                false,
            )
            .await
            .unwrap();
        assert_eq!(result.value["outputs"], json!([]));
        let pending = {
            let store = g.store.lock().unwrap();
            assert!(store.list_outputs(0, 25).unwrap().is_empty());
            store.pending("sql-fault").unwrap()
        };
        assert_eq!(pending.len(), 1);
        assert_eq!(
            std::fs::read(pending[0].target.as_ref().unwrap()).unwrap(),
            bytes
        );
        fault.execute_batch("DROP TRIGGER reject_output").unwrap();
        let recovery = Generation {
            config: g.config.clone(),
            store: g.store.clone(),
            logger: g.logger.clone(),
            runtime: g.runtime.clone(),
            env: g.env.clone(),
            active: Arc::new(Mutex::new(HashMap::new())),
        };
        let recovered = jobs::get(
            &recovery,
            "sql-fault",
            true,
            &CancellationToken::new(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(recovered.status(), "completed");
        assert_eq!(
            g.store.lock().unwrap().list_outputs(0, 25).unwrap().len(),
            1
        );
        assert_eq!(
            std::fs::read(recovered.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            bytes
        );
        drop(fault);
    };
    tokio::time::timeout(Duration::from_secs(3), action)
        .await
        .unwrap();
}

#[test]
fn exact_source_future_state_rejection_preserves_user_version_three() {
    let workspace = Workspace::new();
    let directory = workspace.0.join("state");
    drop(Store::open(&directory).unwrap());
    let fault = rusqlite::Connection::open(directory.join("receipts.sqlite")).unwrap();
    fault.execute_batch("PRAGMA user_version=3").unwrap();
    let error = Store::open(&directory)
        .err()
        .expect("future state must reject");
    assert!(error.message.contains("newer than this build"));
    let version: i64 = fault
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 3);
}
