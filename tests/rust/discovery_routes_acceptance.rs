mod common;
#[path = "common/http.rs"]
mod http_fixture;
use common::Workspace;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::Config,
    runtime_paths::RuntimePaths,
    services::discovery::{Discovery, prefix},
};
use std::{collections::HashMap, future::Future, path::Path, time::Duration};
use tokio_util::sync::CancellationToken;

fn configuration(root: &Path, connection: Value) -> Config {
    Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":{"local":connection}
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap()
}

fn connection(origin: &str, adapter: &str, suffix: &str) -> Value {
    json!({
        "adapter":adapter,"baseUrl":format!("{origin}{suffix}"),
        "auth":{"type":"none"},"discoveryTimeoutMs":2000
    })
}

fn ids(value: &Value) -> Vec<&str> {
    value["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["id"].as_str().unwrap())
        .collect()
}

async fn checked(server: Server, action: impl Future<Output = ()> + Send + 'static) {
    let result =
        tokio::spawn(async move { tokio::time::timeout(Duration::from_secs(8), action).await })
            .await;
    tokio::time::timeout(Duration::from_secs(2), server.close())
        .await
        .expect("Discovery route fixture shutdown exceeded two seconds");
    match result {
        Ok(result) => result.expect("Discovery route action exceeded eight seconds"),
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => panic!("Owned discovery route action failed: {error}"),
    }
}

async fn specialty(
    adapter: &'static str,
    gateway: Option<&'static str>,
    path: &'static str,
    expected_prefix: &'static str,
    payload: Value,
) {
    let workspace = Workspace::new();
    let root = workspace.0.clone();
    let server = Server::new(move |_| Reply {
        status: 200,
        value: payload.clone(),
        bytes: None,
    })
    .await;
    let origin = server.origin.clone();
    let paths = server.requests.clone();
    checked(server, async move {
        let mut profile = connection(&origin, adapter, "");
        profile["baseUrlMode"] = json!("origin");
        if let Some(gateway) = gateway {
            profile["gateway"] = json!(gateway);
        }
        let config = configuration(&root, profile);
        let runtime = RuntimePaths::new(&root).unwrap();
        let result = Discovery::default()
            .list(
                &config,
                &runtime,
                &HashMap::new(),
                None,
                false,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(result["complete"], true);
        assert_eq!(result["cached"], false);
        assert_eq!(result["warnings"], json!([]));
        assert_eq!(ids(&result), ["image-a"]);
        assert_eq!(paths.lock().unwrap().as_slice(), [path]);
        let (_, profile) = config.select(None).unwrap();
        assert_eq!(prefix(&profile), expected_prefix);
    })
    .await;
}

#[tokio::test]
async fn exact_source_images_specialty_route_preserves_complete_uncached_evidence() {
    specialty(
        "openai-images",
        None,
        "/v1/models",
        "/v1",
        json!({"data":[{"id":"image-a"}]}),
    )
    .await;
}

#[tokio::test]
async fn exact_source_ninerouter_specialty_route_preserves_complete_uncached_evidence() {
    specialty(
        "openai-images",
        Some("9router"),
        "/v1/models/image",
        "/v1",
        json!({"data":[{"id":"image-a"}]}),
    )
    .await;
}

#[tokio::test]
async fn exact_source_gemini_specialty_route_preserves_complete_uncached_evidence() {
    specialty(
        "gemini",
        None,
        "/v1beta/models",
        "/v1beta",
        json!({"models":[{"name":"image-a"}]}),
    )
    .await;
}

#[tokio::test]
async fn exact_source_interactions_specialty_route_preserves_complete_uncached_evidence() {
    specialty(
        "gemini-interactions",
        None,
        "/v1beta/models",
        "/v1beta",
        json!({"models":[{"name":"image-a"}]}),
    )
    .await;
}

#[tokio::test]
async fn exact_source_openrouter_specialty_route_preserves_complete_uncached_evidence() {
    specialty(
        "openrouter-images",
        None,
        "/api/v1/images/models",
        "/api/v1",
        json!({"data":[{"id":"image-a"}]}),
    )
    .await;
}

#[tokio::test]
async fn exact_source_comfy_specialty_route_preserves_complete_uncached_evidence() {
    specialty(
        "comfyui",
        None,
        "/object_info",
        "",
        json!({"image-a":{"input":{}}}),
    )
    .await;
}

#[tokio::test]
async fn exact_source_omniroute_specialty_supplements_only_paginated_unified_images() {
    let workspace = Workspace::new();
    let root = workspace.0.clone();
    let server = Server::new(|path| Reply {
        status: 200,
        bytes: None,
        value: if path == "/v1/images/generations" {
            json!({"data":[{"id":"specialty"}]})
        } else if !path.contains("after=") {
            json!({
                "data":[{"id":"specialty","type":"image"},
                    {"id":"text-model","type":"text"},{"id":"unified-1","type":"image"}],
                "has_more":true,"last_id":"cursor-1"
            })
        } else {
            json!({"data":[{"id":"unified-2","type":"image"}]})
        },
    })
    .await;
    let origin = server.origin.clone();
    let paths = server.requests.clone();
    checked(server, async move {
        let mut profile = connection(&origin, "openai-images", "/v1");
        profile["gateway"] = json!("omniroute");
        let config = configuration(&root, profile);
        let runtime = RuntimePaths::new(&root).unwrap();
        let result = Discovery::default()
            .list(
                &config,
                &runtime,
                &HashMap::new(),
                None,
                false,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(ids(&result), ["specialty", "unified-1", "unified-2"]);
        assert_eq!(result["complete"], true);
        assert_eq!(result["warnings"].as_array().unwrap().len(), 1);
        assert_eq!(
            paths.lock().unwrap().as_slice(),
            [
                "/v1/images/generations",
                "/v1/models?limit=100",
                "/v1/models?after=cursor-1&limit=100"
            ]
        );
    })
    .await;
}

#[tokio::test]
async fn exact_source_gemini_page_token_refresh_preserves_config_and_resets_cursor() {
    let workspace = Workspace::new();
    let root = workspace.0.clone();
    let server = Server::new(|path| Reply {
        status: 200,
        bytes: None,
        value: if path.contains("pageToken=") {
            json!({"models":[{"name":"second"}]})
        } else {
            json!({"models":[{"name":"first"}],"nextPageToken":"token-2"})
        },
    })
    .await;
    let origin = server.origin.clone();
    let paths = server.requests.clone();
    checked(server, async move {
        let mut profile = connection(&origin, "gemini", "/v1beta");
        profile["query"] = json!({"scope":"images"});
        let config = configuration(&root, profile);
        let runtime = RuntimePaths::new(&root).unwrap();
        let mut discovery = Discovery::default();
        for refresh in [false, true] {
            let result = discovery
                .list(
                    &config,
                    &runtime,
                    &HashMap::new(),
                    None,
                    refresh,
                    &CancellationToken::new(),
                )
                .await
                .unwrap();
            assert_eq!(ids(&result), ["first", "second"]);
        }
        assert_eq!(
            config.value["connections"]["local"]["query"],
            json!({"scope":"images"})
        );
        let paths = paths.lock().unwrap();
        let queries = paths
            .iter()
            .map(|path| path.strip_prefix("/v1beta/models").unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            queries,
            [
                "?scope=images",
                "?scope=images&pageToken=token-2",
                "?scope=images",
                "?scope=images&pageToken=token-2"
            ]
        );
    })
    .await;
}

#[tokio::test]
async fn exact_source_gateway_metadata_preserves_encoded_ids_and_unknown_availability() {
    let workspace = Workspace::new();
    let root = workspace.0.clone();
    let server = Server::new(|_| Reply {
        status: 200,
        value: json!({"supported":true}),
        bytes: None,
    })
    .await;
    let origin = server.origin.clone();
    let paths = server.requests.clone();
    checked(server, async move {
        let mut profile = connection(&origin, "openai-images", "/v1");
        profile["gateway"] = json!("9router");
        let config = configuration(&root, profile);
        let runtime = RuntimePaths::new(&root).unwrap();
        let result = Discovery::model(
            &config,
            &runtime,
            &HashMap::new(),
            None,
            &"codex/image model".into(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result["connection"], "local");
        assert_eq!(result["model_id"], "codex/image model");
        assert_eq!(result["account_availability"], "unknown");
        assert_eq!(result["advertised"], json!({"supported":true}));
        let config = configuration(&root, connection(&origin, "openrouter-images", "/api/v1"));
        let result = Discovery::model(
            &config,
            &runtime,
            &HashMap::new(),
            None,
            &"provider/image model".into(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result["advertised"], json!({"supported":true}));
        assert_eq!(
            paths.lock().unwrap().as_slice(),
            [
                "/v1/models/info?id=codex%2Fimage+model",
                "/api/v1/images/models/provider/image%20model/endpoints"
            ]
        );
    })
    .await;
    let root = workspace.0.clone();
    let server = Server::new(|_| Reply {
        status: 404,
        value: json!({}),
        bytes: None,
    })
    .await;
    let origin = server.origin.clone();
    checked(server, async move {
        let mut profile = connection(&origin, "openai-images", "/v1");
        profile["gateway"] = json!("9router");
        let config = configuration(&root, profile);
        let runtime = RuntimePaths::new(&root).unwrap();
        let result = Discovery::model(
            &config,
            &runtime,
            &HashMap::new(),
            None,
            &"missing".into(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result["advertised"], Value::Null);
    })
    .await;
}
