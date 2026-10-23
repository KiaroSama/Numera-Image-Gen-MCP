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
use numera_image_gen_mcp::{
    config::Config,
    logging::Logger,
    mcp::{
        application::Application,
        http_server::{Options, serve},
    },
    runtime_paths::RuntimePaths,
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

struct Client {
    http: reqwest::Client,
    url: String,
    token: &'static str,
    session: String,
}
impl Client {
    async fn rpc(&self, id: u32, method: &str, params: Value) -> Value {
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(self.token)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-session-id", &self.session)
            .header("mcp-protocol-version", "2025-11-25")
            .json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let text = response.text().await.unwrap();
        assert!(!text.contains("Cannot encode the protocol response."));
        if text.trim_start().starts_with('{') {
            serde_json::from_str(&text).unwrap()
        } else {
            text.lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
                .find(|value| value["id"] == id)
                .expect("response after optional SSE primer")
        }
    }
    async fn tool(&self, id: u32, name: &str, arguments: Value) -> Value {
        self.rpc(id, "tools/call", json!({"name":name,"arguments":arguments}))
            .await
    }
}

#[tokio::test]
async fn actual_http_tools_preserve_originals_previews_resource_reads_and_error_contracts() {
    let workspace = Workspace::new();
    let original = image_fixture::original();
    let encoded = STANDARD.encode(&original);
    let upstream = Server::new(move |_| Reply {
        status: 200,
        value: numera_image_gen_mcp::compat::value::json!({"id":"remote-owned","data":[{"b64_json":encoded}]}),
        bytes: None,
    })
    .await;
    let config = Config::parse(
        &workspace
            .config(&format!("{}/v1", upstream.origin), "openai-images")
            .to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let stop = CancellationToken::new();
    let app = Arc::new(
        Application::new(
            config,
            None,
            Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
            RuntimePaths::new(&workspace.0).unwrap(),
            HashMap::new(),
            stop.clone(),
        )
        .unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let token = "synthetic-http-output-contract-token-not-provider-key";
    let mut options = Options::new(port, token.into());
    options.shutdown_timeout = Duration::from_secs(2);
    let server = serve(listener, app.clone(), options, stop.clone());
    let action = async {
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let url = format!("http://127.0.0.1:{port}/mcp");
        let init = http.post(&url).bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"output-contract","version":"1"}}}))
            .send().await.unwrap();
        assert_eq!(init.status(), 200);
        let session = init
            .headers()
            .get("mcp-session-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        init.bytes().await.unwrap();
        let initialized = http
            .post(&url)
            .bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-session-id", &session)
            .header("mcp-protocol-version", "2025-11-25")
            .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .send()
            .await
            .unwrap();
        assert_eq!(initialized.status(), 202);
        initialized.bytes().await.unwrap();
        let client = Client {
            http,
            url,
            token,
            session,
        };
        let args = json!({"request_id":"public-http-output","prompt":"fixture","return_mode":"files_and_preview"});
        let generated = client.tool(2, "generate_image", args.clone()).await;
        assert_ne!(generated["result"]["isError"], true);
        let receipt = &generated["result"]["structuredContent"];
        assert_eq!(receipt["status"], "completed");
        assert_eq!(receipt["outputs"].as_array().unwrap().len(), 1);
        assert_eq!(receipt["previews"].as_array().unwrap().len(), 1);
        let output = &receipt["outputs"][0];
        let id = output["output_id"].as_str().unwrap();
        let path = output["path"].as_str().unwrap();
        assert_eq!(std::fs::read(path).unwrap(), original);
        let uri = format!("numera-image://outputs/{id}");
        let content = generated["result"]["content"].as_array().unwrap();
        assert!(
            content
                .iter()
                .any(|item| item["type"] == "resource_link" && item["uri"] == uri)
        );
        let preview = content.iter().find(|item| item["type"] == "image").unwrap();
        assert_eq!(preview["mimeType"], "image/png");
        assert!(
            !STANDARD
                .decode(preview["data"].as_str().unwrap())
                .unwrap()
                .is_empty()
        );
        let info = client
            .tool(3, "get_output_info", json!({"output_id":id,"preview":true}))
            .await;
        assert_eq!(info["result"]["structuredContent"]["verified"], true);
        assert!(
            info["result"]["structuredContent"]["preview"]["width"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(
            info["result"]["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["type"] == "image")
        );
        let templates = client.rpc(4, "resources/templates/list", json!({})).await;
        assert_eq!(
            templates["result"]["resourceTemplates"][0]["uriTemplate"],
            "numera-image://outputs/{id}"
        );
        let resource = client.rpc(5, "resources/read", json!({"uri":uri})).await;
        assert_eq!(
            STANDARD
                .decode(resource["result"]["contents"][0]["blob"].as_str().unwrap())
                .unwrap(),
            original
        );
        for (index, uri) in [
            "file:///unowned",
            "numera-image://outputs/not-uuid",
            "numera-image://outputs/123e4567-e89b-42d3-a456-426614174000",
        ]
        .iter()
        .enumerate()
        {
            let error = client
                .rpc(10 + index as u32, "resources/read", json!({"uri":uri}))
                .await;
            assert_eq!(error["error"]["code"], -32602);
        }
        let unknown = client.tool(20, "unknown", json!({})).await;
        assert_eq!(unknown["error"]["code"], -32602);
        for (index, (name, arguments)) in [
            ("list_connections", json!({"unexpected":true})),
            ("health_check", json!({"probe":"yes"})),
            ("list_models", json!({"offset":-1})),
            ("get_output_info", json!({"output_id":"bad"})),
        ]
        .into_iter()
        .enumerate()
        {
            let error = client.tool(30 + index as u32, name, arguments).await;
            assert_eq!(error["result"]["isError"], true);
            assert_eq!(
                error["result"]["structuredContent"]["error"]["code"],
                "invalid_input"
            );
        }
        std::fs::rename(path, format!("{path}.held")).unwrap();
        let repeated = client.tool(40, "generate_image", args).await;
        assert_ne!(repeated["result"]["isError"], true);
        assert_eq!(
            repeated["result"]["structuredContent"]["outputs"],
            receipt["outputs"]
        );
        assert!(
            !repeated["result"]["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["type"] == "image")
        );
        std::fs::rename(format!("{path}.held"), path).unwrap();
        let generation = app.current().unwrap();
        let (_, connection) = generation.config.select(Some("test")).unwrap();
        let pinned = numera_image_gen_mcp::config::snapshot_credentials(
            &connection,
            &generation.env,
            &generation.runtime,
        )
        .await
        .unwrap();
        let identity =
            numera_image_gen_mcp::services::identity::connection_identity("test", &pinned).unwrap();
        {
            let mut store = app.store.lock().unwrap();
            let seeded = numera_image_gen_mcp::jobs::Receipt::new(
                numera_image_gen_mcp::compat::value::json!({
                    "request_id":"public-http-owned-job","connection":"test","adapter":"openai-images",
                    "operation":"generate","status":"prepared","requested_model":"opaque/image",
                    "outputs":[],"warnings":[],"errors":[],"deviations":[],
                    "generation_outcome":"not_submitted","storage_outcome":"not_started",
                    "created_at":"2026-10-13T00:00:00.000Z",
                    "upstream_job":{"id":"remote-owned","kind":"responses"}
                }),
            );
            let (_, mut receipt) = store.prepare(seeded, "seed", &identity).unwrap();
            assert!(store.admit("public-http-owned-job", "test", 8, 8).unwrap());
            receipt.value["status"] = numera_image_gen_mcp::compat::value::json!("running");
            receipt.value["generation_outcome"] =
                numera_image_gen_mcp::compat::value::json!("running");
            store.update(&mut receipt).unwrap();
            store
                .release("public-http-owned-job", Some(&receipt))
                .unwrap();
        }
        let recovered = client
            .tool(
                50,
                "get_job",
                json!({"request_id":"public-http-owned-job","refresh":true}),
            )
            .await;
        assert_eq!(
            recovered["result"]["structuredContent"]["status"],
            "completed"
        );
        assert_eq!(
            recovered["result"]["structuredContent"]["outputs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let cancelled = client
            .tool(
                51,
                "cancel_job",
                json!({"request_id":"public-http-owned-job"}),
            )
            .await;
        assert_eq!(
            cancelled["result"]["structuredContent"]["upstream_requested"],
            false
        );
        assert!(
            upstream
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|path| path.ends_with("responses/remote-owned"))
        );
        assert_eq!(
            upstream
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|path| path.ends_with("images/generations"))
                .count(),
            1
        );
    };
    let action = async {
        let result = tokio::time::timeout(Duration::from_secs(12), action).await;
        stop.cancel();
        result
    };
    let (served, checked) = tokio::join!(server, action);
    app.shutdown();
    assert_eq!(Arc::strong_count(&app), 1, "all SDK handlers released");
    drop(app);
    upstream.close().await;
    served.unwrap();
    checked.unwrap();
}
