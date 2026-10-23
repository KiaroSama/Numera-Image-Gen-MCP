mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/http.rs"]
mod http_fixture;
use common::Workspace;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::{
    config::Config,
    runtime_paths::RuntimePaths,
    services::{discovery::Discovery, health},
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn config(workspace: &Workspace, origin: &str, adapter: &str, gateway: Option<&str>) -> Config {
    let mut raw = workspace.config(origin, adapter);
    raw["connections"]["test"]["baseUrlMode"] = json!("origin");
    if let Some(gateway) = gateway {
        raw["connections"]["test"]["gateway"] = json!(gateway);
    }
    Config::parse(
        &raw.to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap()
}

#[tokio::test]
async fn specialty_catalog_routes_cache_health_and_metadata_preserve_evidence() {
    let workspace = Workspace::new();
    let runtime = RuntimePaths::new(&workspace.0).unwrap();
    let cancel = CancellationToken::new();
    let server = Server::new(|path| Reply {
        bytes: None,
        status: 200,
        value: if path == "/object_info" {
            json!({"image-a":{"input":{}}})
        } else if path.contains("endpoints") || path.contains("models/info") {
            json!({"supported":true})
        } else if path.contains("v1beta") {
            json!({"models":[{"name":"image-a"}]})
        } else {
            json!({"data":[{"id":"image-a"}]})
        },
    })
    .await;
    let action = async {
        for (adapter, gateway, path) in [
            ("openai-images", None, "/v1/models"),
            ("openai-images", Some("9router"), "/v1/models/image"),
            ("gemini", None, "/v1beta/models"),
            ("gemini-interactions", None, "/v1beta/models"),
            ("openrouter-images", None, "/api/v1/images/models"),
            ("comfyui", None, "/object_info"),
        ] {
            let config = config(&workspace, &server.origin, adapter, gateway);
            let mut discovery = Discovery::default();
            let catalog = discovery
                .list(&config, &runtime, &HashMap::new(), None, false, &cancel)
                .await
                .unwrap();
            assert_eq!(catalog["complete"], true);
            assert_eq!(catalog["models"][0]["id"], "image-a");
            assert_eq!(server.requests.lock().unwrap().last().unwrap(), path);
            assert_eq!(
                discovery
                    .list(&config, &runtime, &HashMap::new(), None, false, &cancel)
                    .await
                    .unwrap()["cached"],
                true
            );
        }
        for (adapter, gateway, model, path) in [
            (
                "openai-images",
                Some("9router"),
                "codex/image model",
                "/v1/models/info?id=codex%2Fimage+model",
            ),
            (
                "openrouter-images",
                None,
                "provider/image model",
                "/api/v1/images/models/provider/image%20model/endpoints",
            ),
        ] {
            let config = config(&workspace, &server.origin, adapter, gateway);
            let cap = Discovery::model(
                &config,
                &runtime,
                &HashMap::new(),
                None,
                &model.into(),
                &cancel,
            )
            .await
            .unwrap();
            assert_eq!(cap["account_availability"], "unknown");
            assert_eq!(cap["advertised"], json!({"supported":true}));
            assert_eq!(server.requests.lock().unwrap().last().unwrap(), path);
        }
        let config = config(&workspace, &server.origin, "openai-images", None);
        let health = health::health(&config, &HashMap::new(), &runtime, true, &cancel)
            .await
            .unwrap();
        assert_eq!(health["configuration_valid"], true);
        assert_eq!(health["generation_verified"], false);
        assert_eq!(health["connections"][0]["reachable"], true);
        assert_eq!(health["connections"][0]["generation_verified"], false);
        assert_eq!(
            health::connections(&config, &HashMap::new(), &runtime)
                .await
                .unwrap()[0]["credentials_ready"],
            true
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn catalog_pagination_partial_errors_and_stale_cache_are_bounded() {
    let workspace = Workspace::new();
    let runtime = RuntimePaths::new(&workspace.0).unwrap();
    let cancel = CancellationToken::new();
    let failures = Arc::new(AtomicBool::new(false));
    let repeat = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(AtomicUsize::new(0));
    let (failed, repeated, counted) = (failures.clone(), repeat.clone(), calls.clone());
    let server = Server::new(move |_| {
        let index = counted.fetch_add(1, Ordering::SeqCst) + 1;
        Reply { bytes: None, status: if failed.load(Ordering::SeqCst) { 503 } else { 200 }, value: json!({"data":[{"id":format!("model-{index}")}],"has_more":true,"last_id":if repeated.load(Ordering::SeqCst){"same".to_owned()}else{format!("cursor-{index}")}}) }
    }).await;
    let action = async {
        let config = config(&workspace, &server.origin, "openai-images", None);
        let mut discovery = Discovery::default();
        let first = discovery
            .list(&config, &runtime, &HashMap::new(), None, true, &cancel)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(first["complete"], false);
        assert!(
            first["warnings"]
                .as_array()
                .unwrap()
                .contains(&json!("Repeated catalog cursor."))
        );
        repeat.store(false, Ordering::SeqCst);
        calls.store(0, Ordering::SeqCst);
        let pages = discovery
            .list(&config, &runtime, &HashMap::new(), None, true, &cancel)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 20);
        assert_eq!(pages["models"].as_array().unwrap().len(), 20);
        assert_eq!(pages["complete"], false);
        failures.store(true, Ordering::SeqCst);
        let stale = discovery
            .list(&config, &runtime, &HashMap::new(), None, true, &cancel)
            .await
            .unwrap();
        assert_eq!(stale["models"], pages["models"]);
        assert_eq!(stale["warnings"][0]["http_status"], 503);
        assert!(
            stale["warnings"]
                .as_array()
                .unwrap()
                .contains(&json!("Using stale catalog; availability is unknown."))
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}
