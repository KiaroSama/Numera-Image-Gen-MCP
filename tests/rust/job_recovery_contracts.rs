mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/http.rs"]
mod http_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    config::{Config, snapshot_credentials},
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, identity::connection_identity, jobs},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

// Source contracts: integration/{terminal-lifecycle,credential-snapshot,comfy-terminal-recovery,
// cancelled-results,recovery}.test.ts. No new submissions are authorized by recovery.
fn make_generation(workspace: &Workspace, raw: &Value) -> Generation {
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
fn receipt(id: &str, job: Option<&str>) -> Receipt {
    let mut value = json!({"request_id":id,"connection":"test","status":"prepared","operation":"generate","adapter":"openai-responses","requested_model":"opaque/image",
        "outputs":[],"warnings":[],"errors":[],"deviations":[],"generation_outcome":"not_submitted","storage_outcome":"not_started","created_at":"2026-10-12T00:00:00.000Z"});
    if let Some(kind) = job {
        value["upstream_job"] = json!({"kind":kind,"id":"owned"});
    }
    Receipt::new(value)
}
async fn seed(generation: &Generation, id: &str, kind: Option<&str>, release: bool) -> Receipt {
    let (_, connection) = generation.config.select(Some("test")).unwrap();
    let pinned = snapshot_credentials(&connection, &generation.env, &generation.runtime)
        .await
        .unwrap();
    let identity = connection_identity("test", &pinned).unwrap();
    let mut store = generation.store.lock().unwrap();
    let (_, mut receipt) = store.prepare(receipt(id, kind), id, &identity).unwrap();
    assert!(store.admit(id, "test", 16, 16).unwrap());
    receipt.value["status"] = json!("running");
    receipt.value["generation_outcome"] = json!("running");
    store.update(&mut receipt).unwrap();
    if release {
        store.release(id, Some(&receipt)).unwrap();
    }
    receipt
}
fn failure<T>(result: numera_image_gen_mcp::error::Result<T>) -> String {
    result.err().expect("expected rejection").code
}
fn comfy(workspace: &Workspace, origin: &str) -> Value {
    let mut raw = workspace.config(origin, "comfyui");
    raw["connections"]["test"]["baseUrlMode"] = json!("origin");
    raw["connections"]["test"]["workflow"] = json!({"graph":{"save":{"class_type":"Save","inputs":{}}},"bindings":{},"outputNodes":["save"],"requireGpu":false});
    raw
}

#[tokio::test]
async fn inactive_nonrefreshed_no_handle_terminal_and_owned_writer_requests_do_not_probe() {
    let workspace = Workspace::new();
    let server = Server::new(|_| Reply {
        status: 200,
        value: json!({}),
        bytes: None,
    })
    .await;
    let action = async {
        let generation = make_generation(
            &workspace,
            &workspace.config(&format!("{}/v1", server.origin), "openai-responses"),
        );
        let signal = CancellationToken::new();
        seed(&generation, "live", Some("responses"), false).await;
        assert_eq!(
            jobs::get(&generation, "live", false, &signal, None)
                .await
                .unwrap()
                .status(),
            "running"
        );
        let workspace_peer = Workspace::new();
        let mut peer = make_generation(
            &workspace_peer,
            &workspace_peer.config(&format!("{}/v1", server.origin), "openai-responses"),
        );
        peer.store = Arc::new(Mutex::new(Store::open(&workspace.0.join("state")).unwrap()));
        assert_eq!(
            jobs::get(&peer, "live", true, &signal, None)
                .await
                .unwrap()
                .status(),
            "running",
            "another store owner must remain exclusive"
        );
        assert_eq!(
            jobs::recover_local(&generation, "live", &signal)
                .await
                .unwrap()
                .status(),
            "running"
        );
        let local = seed(&generation, "local", None, true).await;
        assert_eq!(
            jobs::get(&generation, "local", true, &signal, None)
                .await
                .unwrap()
                .value,
            local.value
        );
        let cancelled = jobs::cancel(&generation, "local", &signal).await.unwrap();
        assert_eq!(cancelled["upstream_requested"], false);
        assert_eq!(cancelled["upstream_cancelled"], Value::Null);
        let active = CancellationToken::new();
        generation
            .active
            .lock()
            .unwrap()
            .insert("local".into(), active.clone());
        jobs::cancel(&generation, "local", &signal).await.unwrap();
        assert!(active.is_cancelled());
        generation.active.lock().unwrap().clear();
        let mut terminal = seed(&generation, "terminal", Some("responses"), false).await;
        terminal.value["status"] = json!("cancelled");
        terminal.value["upstream_terminal"] = json!("cancelled");
        terminal.value["generation_outcome"] = json!("cancelled");
        generation
            .store
            .lock()
            .unwrap()
            .update(&mut terminal)
            .unwrap();
        assert_eq!(
            jobs::get(&generation, "terminal", true, &signal, None)
                .await
                .unwrap()
                .status(),
            "cancelled"
        );
        let result = jobs::cancel(&generation, "terminal", &signal)
            .await
            .unwrap();
        assert_eq!(result["upstream_cancelled"], true);
        assert_eq!(result["upstream_requested"], false);
        assert!(server.requests.lock().unwrap().is_empty());
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn changed_accounts_or_removed_connections_refuse_recovery_before_any_network_request() {
    let workspace = Workspace::new();
    let server = Server::new(|_| Reply {
        status: 200,
        value: json!({"id":"owned","status":"completed"}),
        bytes: None,
    })
    .await;
    let action = async {
        let mut raw = workspace.config(&format!("{}/v1", server.origin), "openai-responses");
        raw["connections"]["test"]["auth"] = json!({"type":"bearer","secretEnv":"SELECTED"});
        let mut generation = make_generation(&workspace, &raw);
        generation
            .env
            .set("SELECTED", "synthetic-account-one")
            .unwrap();
        seed(&generation, "changed", Some("responses"), true).await;
        generation
            .env
            .set("SELECTED", "synthetic-account-two")
            .unwrap();
        let signal = CancellationToken::new();
        assert_eq!(
            failure(jobs::get(&generation, "changed", true, &signal, None).await),
            "permission_denied"
        );
        assert_eq!(
            failure(jobs::cancel(&generation, "changed", &signal).await),
            "permission_denied"
        );
        assert_eq!(
            generation
                .store
                .lock()
                .unwrap()
                .get("changed")
                .unwrap()
                .status(),
            "running"
        );
        generation.env.remove("SELECTED").unwrap();
        assert_eq!(
            failure(jobs::get(&generation, "changed", true, &signal, None).await),
            "missing_credentials"
        );
        let mut removed = raw.clone();
        removed["connections"] = json!({});
        removed.as_object_mut().unwrap().remove("defaultConnection");
        generation.config = Arc::new(
            Config::parse(
                &removed.to_string(),
                &workspace.0.join("config.json"),
                &[],
                &HashMap::new(),
            )
            .unwrap(),
        );
        assert_eq!(
            failure(jobs::get(&generation, "changed", true, &signal, None).await),
            "invalid_input"
        );
        assert!(server.requests.lock().unwrap().is_empty());
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn pending_comfy_history_and_unsupported_responses_cancel_preserve_running_uncertainty() {
    let server = Server::new(|_| Reply {
        status: 200,
        value: json!({}),
        bytes: None,
    })
    .await;
    let action = async {
        let workspace = Workspace::new();
        let generation = make_generation(&workspace, &comfy(&workspace, &server.origin));
        let signal = CancellationToken::new();
        seed(&generation, "pending", Some("comfyui"), true).await;
        let first = jobs::get(&generation, "pending", true, &signal, None)
            .await
            .unwrap();
        assert_eq!(first.status(), "running");
        assert!(first.value["upstream_terminal"].is_null());
        assert!(first.value["outputs"].as_array().unwrap().is_empty());
        assert_eq!(
            server.requests.lock().unwrap().as_slice(),
            ["/history/owned"]
        );
        let workspace = Workspace::new();
        let generation = make_generation(
            &workspace,
            &workspace.config(&format!("{}/v1", server.origin), "openai-responses"),
        );
        seed(&generation, "unsupported", Some("responses"), true).await;
        let result = jobs::cancel(&generation, "unsupported", &signal)
            .await
            .unwrap();
        assert_eq!(result["upstream_requested"], false);
        assert_eq!(result["upstream_cancelled"], Value::Null);
        assert_eq!(result["refund_verified"], false);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn comfy_authoritative_failed_history_and_absent_cancel_acknowledgement_are_not_equivalent() {
    let server = Server::new(|path| Reply {
        status: 200,
        value: if path.ends_with("/cancel") {
            json!({})
        } else {
            json!({"owned":{"status":{"status_str":"error"}}})
        },
        bytes: None,
    })
    .await;
    let action = async {
        let workspace = Workspace::new();
        let generation = make_generation(&workspace, &comfy(&workspace, &server.origin));
        let signal = CancellationToken::new();
        seed(&generation, "failed", Some("comfyui"), true).await;
        let result = jobs::get(&generation, "failed", true, &signal, None)
            .await
            .unwrap();
        assert_eq!(result.status(), "failed");
        assert_eq!(result.value["generation_outcome"], "failed");
        assert_eq!(result.value["upstream_terminal"], "failed");
        let workspace = Workspace::new();
        let generation = make_generation(&workspace, &comfy(&workspace, &server.origin));
        seed(&generation, "ack", Some("comfyui"), true).await;
        let result = jobs::cancel(&generation, "ack", &signal).await.unwrap();
        assert_eq!(result["upstream_requested"], true);
        assert_eq!(result["upstream_cancelled"], Value::Null);
        assert_eq!(
            generation
                .store
                .lock()
                .unwrap()
                .get("ack")
                .unwrap()
                .status(),
            "running"
        );
        assert_eq!(
            server.requests.lock().unwrap().as_slice(),
            ["/history/owned", "/api/jobs/owned/cancel"]
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn supplied_private_credential_snapshot_survives_rotation_for_existing_handle_recovery() {
    let bytes = image_fixture::original();
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
    let server=Server::new(move |_|Reply{status:200,value:json!({"id":"owned","status":"completed","output":[{"type":"image_generation_call","status":"completed","result":encoded}]}),bytes:None}).await;
    let action = async {
        let workspace = Workspace::new();
        let mut raw = workspace.config(&format!("{}/v1", server.origin), "openai-responses");
        raw["connections"]["test"]["auth"] = json!({"type":"bearer","secretEnv":"SELECTED"});
        let generation = make_generation(&workspace, &raw);
        generation
            .env
            .set("SELECTED", "synthetic-account-one")
            .unwrap();
        seed(&generation, "pinned", Some("responses"), true).await;
        let (_, connection) = generation.config.select(None).unwrap();
        let pinned = snapshot_credentials(&connection, &generation.env, &generation.runtime)
            .await
            .unwrap();
        generation
            .env
            .set("SELECTED", "synthetic-account-two")
            .unwrap();
        let signal = CancellationToken::new();
        let result = jobs::get(&generation, "pinned", true, &signal, Some(&pinned))
            .await
            .unwrap();
        assert_eq!(result.status(), "completed");
        assert_eq!(
            std::fs::read(result.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            bytes
        );
        assert_eq!(
            server.requests.lock().unwrap().as_slice(),
            ["/v1/responses/owned"]
        );
        assert!(!result.value.to_string().contains("synthetic-account"));
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn retained_url_failure_keeps_journal_then_recovers_bytes_without_resubmitting_generation() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let bytes = image_fixture::original();
    let download = bytes.clone();
    let fail = Arc::new(AtomicBool::new(true));
    let failing = fail.clone();
    let server = Server::new(move |_| Reply {
        status: if failing.load(Ordering::SeqCst) {
            500
        } else {
            200
        },
        value: json!({}),
        bytes: Some(download.clone()),
    })
    .await;
    let action = async {
        let workspace = Workspace::new();
        let mut raw = workspace.config("https://invalid.example/v1", "openai-images");
        raw["files"]["assetAllowances"] = json!([{"origin":server.origin,"pathPrefix":"/owned"}]);
        raw["files"]["maxAggregateBytes"] = json!(4096);
        let generation = make_generation(&workspace, &raw);
        let signal = CancellationToken::new();
        let receipt = seed(&generation, "retained", None, false).await;
        {
            let mut store = generation.store.lock().unwrap();
            store.reserve("retained", 4096, 8192).unwrap();
            store
                .retain_url(
                    "retained",
                    0,
                    &format!("{}/owned/image.png", server.origin).into(),
                    Some(&receipt),
                )
                .unwrap();
            store.release("retained", Some(&receipt)).unwrap();
        }
        let failed = jobs::get(&generation, "retained", true, &signal, None)
            .await
            .unwrap();
        assert_eq!(failed.status(), "failed");
        assert_eq!(failed.value["errors"][0]["code"], "input_file_error");
        assert!(
            generation
                .store
                .lock()
                .unwrap()
                .has_pending("retained")
                .unwrap()
        );
        fail.store(false, Ordering::SeqCst);
        let recovered = jobs::get(&generation, "retained", true, &signal, None)
            .await
            .unwrap();
        assert_eq!(recovered.status(), "partial");
        assert_eq!(
            recovered.value["errors"][0]["code"], "input_file_error",
            "source retains download failure history even after successful byte recovery"
        );
        assert_eq!(
            std::fs::read(recovered.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            bytes
        );
        assert!(
            !generation
                .store
                .lock()
                .unwrap()
                .has_pending("retained")
                .unwrap()
        );
        assert_eq!(
            server.requests.lock().unwrap().as_slice(),
            ["/owned/image.png", "/owned/image.png"]
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}
