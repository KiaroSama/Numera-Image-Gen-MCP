mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/http.rs"]
mod http_fixture;
use common::Workspace;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::value::json;
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

#[tokio::test]
async fn targeted_comfy_cancel_requires_matching_terminal_confirmation_without_refund_claim() {
    for state in ["cancelled", "failed", "running", "wrong"] {
        let workspace = Workspace::new();
        let server = Server::new(move |path| Reply {
            status: 200,
            bytes: None,
            value: if path.ends_with("/cancel") {
                json!({"cancelled":true})
            } else {
                json!({"id":if state=="wrong"{"another-job"}else{"original-job"},"status":state})
            },
        })
        .await;
        let action = async {
            let config = Config::parse(
                &workspace.config(&server.origin, "comfyui").to_string(),
                &workspace.0.join("config.json"),
                &[],
                &HashMap::new(),
            )
            .unwrap();
            let runtime = RuntimePaths::new(&workspace.0).unwrap();
            let (_, c) = config.select(None).unwrap();
            let pinned = snapshot_credentials(&c, &HashMap::new(), &runtime)
                .await
                .unwrap();
            let identity = connection_identity("test", &pinned).unwrap();
            let mut store = Store::open(&workspace.0.join("state")).unwrap();
            let (_,mut receipt)=store.prepare(Receipt::new(json!({"request_id":"existing","connection":"test","adapter":"comfyui","operation":"generate","status":"prepared","generation_outcome":"not_submitted","outputs":[],"errors":[],"warnings":[],"deviations":[]})),"existing",&identity).unwrap();
            assert!(store.admit("existing", "test", 4, 2).unwrap());
            receipt.value["status"] = json!("running");
            receipt.value["generation_outcome"] = json!("running");
            receipt.value["upstream_job"] = json!({"id":"original-job","kind":"comfyui"});
            store.update(&mut receipt).unwrap();
            store.release("existing", Some(&receipt)).unwrap();
            let generation = Generation {
                config: Arc::new(config),
                runtime,
                store: Arc::new(Mutex::new(store)),
                logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
                env: HashMap::new().into(),
                active: Arc::new(Mutex::new(HashMap::new())),
            };
            let result = jobs::cancel(&generation, "existing", &CancellationToken::new()).await;
            if state == "wrong" {
                assert_eq!(result.unwrap_err().code, "invalid_response");
                assert!(
                    generation
                        .store
                        .lock()
                        .unwrap()
                        .get("existing")
                        .unwrap()
                        .value["upstream_terminal"]
                        .is_null()
                );
            } else {
                let result = result.unwrap();
                assert_eq!(result["local_wait_cancelled"], true);
                assert_eq!(result["upstream_requested"], true);
                assert_eq!(result["refund_verified"], false);
                assert_eq!(result["upstream_cancelled"], state == "cancelled");
                let receipt = generation.store.lock().unwrap().get("existing").unwrap();
                if state == "running" {
                    assert_eq!(receipt.status(), "running");
                    assert!(receipt.value["upstream_terminal"].is_null());
                } else {
                    assert_eq!(receipt.value["upstream_terminal"], state);
                }
            }
            assert_eq!(
                *server.requests.lock().unwrap(),
                vec![
                    "/api/jobs/original-job/cancel".to_owned(),
                    "/api/jobs/original-job".to_owned()
                ]
            );
            drop(generation);
        };
        let result = tokio::time::timeout(Duration::from_secs(3), action).await;
        server.close().await;
        result.unwrap();
    }
}
