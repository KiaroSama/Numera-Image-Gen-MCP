mod common;
#[path = "common/http.rs"]
mod http_fixture;
use common::Workspace;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    config::Config,
    runtime_paths::RuntimePaths,
    services::{discovery::Discovery, health},
};
use std::{
    collections::HashMap,
    future::Future,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

// Frozen configuration() defaults, not common/config.rs's distinct model/timeouts.
fn configuration(root: &Path, connections: Value) -> Config {
    let default = connections
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    let raw = json!({
        "schemaVersion":1,"defaultConnection":default,
        "outputDir":root.join("outputs").to_str().unwrap(),
        "stateDir":root.join("state").to_str().unwrap(),
        "logging":{"directory":root.join("logs").to_str().unwrap()},
        "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
        "connections":connections
    });
    Config::parse(
        &raw.to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap()
}

fn local(origin: &str) -> Value {
    json!({"local":{
        "adapter":"openai-images","baseUrl":format!("{origin}/v1"),
        "auth":{"type":"none"},"discoveryTimeoutMs":2000
    }})
}

async fn checked(server: Server, action: impl Future<Output = ()> + Send + 'static) {
    // Join assertion panics before releasing the socket owner and propagating the result.
    let result =
        tokio::spawn(async move { tokio::time::timeout(Duration::from_secs(8), action).await })
            .await;
    tokio::time::timeout(Duration::from_secs(2), server.close())
        .await
        .expect("Discovery fixture shutdown exceeded two seconds");
    match result {
        Ok(result) => result.expect("Discovery lifecycle action exceeded eight seconds"),
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => panic!("Owned discovery action failed: {error}"),
    }
}

#[tokio::test]
async fn exact_source_profile_caches_stay_isolated_and_failed_refresh_retains_stale_evidence() {
    let workspace = Workspace::new();
    let root = workspace.0.clone();
    let failing = Arc::new(AtomicBool::new(false));
    let failed = failing.clone();
    let server = Server::new(move |path| Reply {
        status: if failed.load(Ordering::SeqCst) {
            503
        } else {
            200
        },
        bytes: None,
        value: if failed.load(Ordering::SeqCst) {
            json!({})
        } else {
            json!({"data":[{"id":if path.starts_with("/a/") { "a-model" } else { "b-model" }}]})
        },
    })
    .await;
    let origin = server.origin.clone();
    let requests = server.requests.clone();
    checked(server, async move {
        let config = configuration(
            &root,
            json!({
                "a":{"adapter":"openai-images","baseUrl":format!("{origin}/a/v1"),
                    "auth":{"type":"none"},"discoveryTimeoutMs":2000},
                "b":{"adapter":"openai-images","baseUrl":format!("{origin}/b/v1"),
                    "auth":{"type":"none"},"discoveryTimeoutMs":2000}
            }),
        );
        assert_eq!(config.value["defaultConnection"], "a");
        let runtime = RuntimePaths::new(&root).unwrap();
        let env = HashMap::new();
        let signal = CancellationToken::new();
        let mut discovery = Discovery::default();
        let first = discovery
            .list(&config, &runtime, &env, Some("a"), false, &signal)
            .await
            .unwrap();
        assert_eq!(first["models"][0]["id"], "a-model");
        let second = discovery
            .list(&config, &runtime, &env, Some("b"), false, &signal)
            .await
            .unwrap();
        assert_eq!(second["models"][0]["id"], "b-model");
        let cached = discovery
            .list(&config, &runtime, &env, Some("a"), false, &signal)
            .await
            .unwrap();
        assert_eq!(cached["cached"], true);
        assert_eq!(requests.lock().unwrap().len(), 2);
        failing.store(true, Ordering::SeqCst);
        let stale = discovery
            .list(&config, &runtime, &env, Some("a"), true, &signal)
            .await
            .unwrap();
        let ids = stale["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["a-model"]);
        assert_eq!(stale["cached"], false);
        assert_eq!(stale["complete"], false);
        let warnings = stale["warnings"].as_array().unwrap();
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0]["code"], "upstream_error");
        assert_eq!(warnings[0]["http_status"], 503);
        assert_eq!(warnings[1], "Using stale catalog; availability is unknown.");
    })
    .await;
}

#[tokio::test]
async fn exact_source_repeated_cursor_and_twenty_page_refresh_remain_incomplete() {
    let workspace = Workspace::new();
    let root = workspace.0.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let repeat = Arc::new(AtomicBool::new(true));
    let counted = calls.clone();
    let repeated = repeat.clone();
    let server = Server::new(move |_| {
        let index = counted.fetch_add(1, Ordering::SeqCst) + 1;
        Reply {
            status: 200,
            bytes: None,
            value: json!({
                "data":[{"id":format!("model-{index}")}],"has_more":true,
                "last_id":if repeated.load(Ordering::SeqCst) {"same".to_owned()} else {format!("cursor-{index}")}
            }),
        }
    })
    .await;
    let origin = server.origin.clone();
    checked(server, async move {
        let config = configuration(&root, local(&origin));
        let runtime = RuntimePaths::new(&root).unwrap();
        let env = HashMap::new();
        let signal = CancellationToken::new();
        let mut discovery = Discovery::default();
        let repeated = discovery
            .list(&config, &runtime, &env, None, false, &signal)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(repeated["complete"], false);
        assert!(
            repeated["warnings"]
                .as_array()
                .unwrap()
                .contains(&json!("Repeated catalog cursor."))
        );
        repeat.store(false, Ordering::SeqCst);
        calls.store(0, Ordering::SeqCst);
        let bounded = discovery
            .list(&config, &runtime, &env, None, true, &signal)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 20);
        assert_eq!(bounded["complete"], false);
        assert_eq!(bounded["models"].as_array().unwrap().len(), 20);
        assert!(
            bounded["warnings"]
                .as_array()
                .unwrap()
                .contains(&json!("Catalog page limit reached."))
        );
    })
    .await;
}

#[tokio::test]
async fn exact_source_malformed_first_page_is_incomplete_with_one_response_warning() {
    let workspace = Workspace::new();
    let root = workspace.0.clone();
    let server = Server::new(|_| Reply {
        status: 200,
        bytes: None,
        value: json!({"data":[null]}),
    })
    .await;
    let origin = server.origin.clone();
    checked(server, async move {
        let config = configuration(&root, local(&origin));
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
        assert_eq!(result["models"], json!([]));
        assert_eq!(result["complete"], false);
        let warnings = result["warnings"].as_array().unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0]["code"], "invalid_response");
    })
    .await;
}

#[tokio::test]
async fn exact_source_enabled_and_disabled_health_never_disclose_declared_secret_sources() {
    let workspace = Workspace::new();
    let root = workspace.0.clone();
    let server = Server::new(|_| Reply {
        status: 200,
        bytes: None,
        value: json!({"data":[]}),
    })
    .await;
    let origin = server.origin.clone();
    checked(server, async move {
        let mut connections = local(&origin);
        connections["disabled"] = json!({
            "adapter":"gemini","baseUrl":"https://api.example/v1","enabled":false,
            "auth":{"type":"bearer","secretFile":root.join("absent-fixture-secret.txt").to_str().unwrap()}
        });
        let config = configuration(&root, connections);
        let runtime = RuntimePaths::new(&root).unwrap();
        let env = HashMap::new();
        let listed = health::connections(&config, &env, &runtime).await.unwrap();
        assert_eq!(
            listed,
            json!([
                {"name":"local","adapter":"openai-images","gateway":null,"enabled":true,
                    "credentials_ready":true,"default_model":null},
                {"name":"disabled","adapter":"gemini","gateway":null,"enabled":false,
                    "credentials_ready":false,"default_model":null}
            ])
        );
        let result = health::health(&config, &env, &runtime, true, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            result["connections"],
            json!([
                {"name":"local","reachable":true,"authentication_checked":false,"generation_verified":false,"error":null},
                {"name":"disabled","reachable":null,"authentication_checked":false,"generation_verified":false,"error":null}
            ])
        );
        assert!(!result.to_string().contains("absent-fixture-secret.txt"));
        assert_eq!(result["generation_verified"], false);
    })
    .await;
}
