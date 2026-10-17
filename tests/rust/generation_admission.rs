mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/http.rs"]
mod http_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::Workspace;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    config::Config,
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::generation::Generation,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
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
        runtime: RuntimePaths::new(&workspace.0).unwrap(),
        store: Arc::new(Mutex::new(Store::open(&workspace.0.join("state")).unwrap())),
        logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    }
}

#[tokio::test]
async fn blocked_admission_cancellation_never_submits_and_comfy_waiting_reuses_one_job() {
    let original = image_fixture::original();
    let encoded = STANDARD.encode(&original);
    let download = original.clone();
    let server=Server::new(move |path|Reply {status:200,bytes:path.starts_with("/view?").then(||download.clone()),value:match path {
        "/object_info"=>json!({"Text":{"input":{"required":{"text":["STRING"]}}},"Save":{"input":{"required":{"images":["STRING"]}},"output_node":true}}),
        "/prompt"=>json!({"prompt_id":"one-job","node_errors":{}}),
        "/history/one-job"=>json!({"one-job":{"status":{"completed":true},"outputs":{"save":{"images":[{"filename":"original.png","type":"output"}]}}}}),
        _=>json!({"data":[{"b64_json":encoded}]}),
    }}).await;
    let action = async {
        let workspace = Workspace::new();
        let mut raw = workspace.config(&format!("{}/v1", server.origin), "openai-images");
        raw["maxConcurrentRequests"] = json!(1);
        let g = generation(&workspace, &raw);
        {
            let mut store = g.store.lock().unwrap();
            let (_,mut receipt)=store.prepare(Receipt::new(json!({"request_id":"blocking","connection":"test","status":"prepared","outputs":[],"generation_outcome":"not_submitted"})),"blocking","identity").unwrap();
            assert!(store.admit("blocking", "test", 1, 1).unwrap());
            receipt.value["status"] = json!("running");
            receipt.value["upstream_job"] = json!({"id":"existing","kind":"responses"});
            store.update(&mut receipt).unwrap();
            store.release("blocking", Some(&receipt)).unwrap();
        }
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let cancelled = async {
            loop {
                if g.store.lock().unwrap().get("queued").is_ok() {
                    stop.cancel();
                    break;
                }
                tokio::task::yield_now().await;
            }
        };
        let request = g.run(
            json!({"prompt":"fixture","request_id":"queued"}),
            "generate",
            &cancel,
        );
        let (receipt, ()) = tokio::join!(request, cancelled);
        let receipt = receipt.unwrap();
        assert_eq!(receipt.status(), "cancelled");
        assert_eq!(receipt.value["generation_outcome"], "not_submitted");
        assert!(server.requests.lock().unwrap().is_empty());
        assert!(g.active.lock().unwrap().is_empty());
        let workspace = Workspace::new();
        let mut raw = workspace.config(&server.origin, "comfyui");
        raw["connections"]["test"]["workflow"] = json!({"graph":{"text":{"class_type":"Text","inputs":{"text":""}},"save":{"class_type":"Save","inputs":{"images":"fixture"}}},"bindings":{"prompt":{"node":"text","input":"text"}},"outputNodes":["save"],"requireGpu":false});
        let g = generation(&workspace, &raw);
        let receipt = g
            .run(
                json!({"prompt":"Preserve original","request_id":"comfy-wait"}),
                "generate",
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(receipt.status(), "completed");
        assert_eq!(
            std::fs::read(receipt.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            original
        );
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|p| p.as_str() == "/prompt")
                .count(),
            1
        );
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|p| p.starts_with("/view?"))
                .count(),
            1
        );
        let repeated = g
            .run(
                json!({"prompt":"Preserve original","request_id":"comfy-wait"}),
                "generate",
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(repeated.value["outputs"], receipt.value["outputs"]);
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}
