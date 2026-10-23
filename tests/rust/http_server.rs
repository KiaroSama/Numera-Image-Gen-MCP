mod common;
#[path = "common/config.rs"]
mod config_fixture;
use common::Workspace;
use numera_image_gen_mcp::{
    config::Config,
    logging::Logger,
    mcp::{
        application::Application,
        http_server::{Options, bind, serve},
    },
    runtime_paths::RuntimePaths,
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn real_http_endpoint_rejects_unsafe_clients_and_serves_the_existing_ten_tools() {
    let workspace = Workspace::new();
    let stop = CancellationToken::new();
    let config = Config::parse(
        &workspace
            .config("https://invalid.example/v1", "openai-images")
            .to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
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
    assert_eq!(bind(port).await.unwrap_err().code, "port_unavailable");
    let token = "synthetic-local-http-token-not-provider-key";
    let options = Options {
        port,
        token: token.into(),
        max_body_bytes: 1024,
        max_connections: 4,
        max_sessions: 2,
        read_timeout: Duration::from_secs(1),
        shutdown_timeout: Duration::from_secs(2),
    };
    let server = serve(listener, app.clone(), options, stop.clone());
    let action = async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let url = format!("http://127.0.0.1:{port}/mcp");
        let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}});
        assert_eq!(
            client.post(&url).json(&init).send().await.unwrap().status(),
            401
        );
        assert_eq!(
            client
                .post(&url)
                .bearer_auth("wrong")
                .json(&init)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        for (header, value) in [
            ("origin", "https://evil.example"),
            ("origin", "null"),
            ("host", "evil.example"),
        ] {
            assert_eq!(
                client
                    .post(&url)
                    .bearer_auth(token)
                    .header(header, value)
                    .json(&init)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                403
            );
        }
        assert_eq!(
            client
                .post(format!("{url}?key=not-allowed"))
                .bearer_auth(token)
                .json(&init)
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
        assert_eq!(
            client
                .put(&url)
                .bearer_auth(token)
                .send()
                .await
                .unwrap()
                .status(),
            405
        );
        assert_eq!(
            client
                .post(&url)
                .bearer_auth(token)
                .body("x".repeat(1025))
                .send()
                .await
                .unwrap()
                .status(),
            413
        );
        assert_eq!(
            client
                .post(format!("http://127.0.0.1:{port}/wrong"))
                .bearer_auth(token)
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        let mut stalled = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        stalled.write_all(format!("POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{").as_bytes()).await.unwrap();
        let mut rejected = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), stalled.read_to_end(&mut rejected))
            .await
            .unwrap()
            .unwrap();
        assert!(String::from_utf8(rejected).unwrap().contains("408"));
        let response = client
            .post(&url)
            .bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .json(&init)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let session = response
            .headers()
            .get("mcp-session-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let text = response.text().await.unwrap();
        assert!(text.contains("numera-image-gen-mcp"));
        let response = client
            .post(&url)
            .bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-session-id", &session)
            .header("mcp-protocol-version", "2025-11-25")
            .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 202);
        let response = client
            .post(&url)
            .bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-session-id", &session)
            .header("mcp-protocol-version", "2025-11-25")
            .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let text = response.text().await.unwrap();
        let value: Value = if !text.trim_start().starts_with('{') {
            text.lines()
                .filter_map(|s| s.strip_prefix("data: "))
                .filter_map(|s| serde_json::from_str::<Value>(s).ok())
                .find(|v| v["id"] == 2)
                .unwrap()
        } else {
            serde_json::from_str(&text).unwrap()
        };
        assert_eq!(value["result"]["tools"].as_array().unwrap().len(), 10);
        let response = client.post(&url).bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-session-id", &session)
            .header("mcp-protocol-version", "2025-11-25")
            .body(r#"{"jsonrpc":"2.0","id":"\ud800","method":"tools/call","params":{"name":"list_connections","arguments":{}}}"#)
            .send().await.unwrap();
        assert_eq!(response.status(), 200);
        let text = response.text().await.unwrap();
        assert!(!text.contains("Cannot encode the protocol response."));
        let raw = if text.trim_start().starts_with('{') {
            numera_image_gen_mcp::compat::js_json::parse(&text).expect("exact HTTP JSON")
        } else {
            text.lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .filter_map(|data| numera_image_gen_mcp::compat::js_json::parse(data.trim()).ok())
                .find(|message| {
                    message
                        .get("id")
                        .and_then(numera_image_gen_mcp::compat::js_json::JsValue::as_utf16)
                        == Some([0xd800].as_slice())
                })
                .expect("exact response after optional SSE priming event")
        };
        assert_eq!(
            raw.get("id")
                .and_then(numera_image_gen_mcp::compat::js_json::JsValue::as_utf16),
            Some([0xd800].as_slice())
        );
        assert!(
            raw.get("result")
                .unwrap()
                .get("structuredContent")
                .is_some()
        );
        let second = client
            .post(&url)
            .bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .json(&init)
            .send()
            .await
            .unwrap();
        assert_eq!(second.status(), 200);
        let second_id = second.headers().get("mcp-session-id").unwrap().clone();
        second.bytes().await.unwrap();
        assert_eq!(
            client
                .post(&url)
                .bearer_auth(token)
                .header("accept", "application/json, text/event-stream")
                .json(&json!({"jsonrpc":"2.0","id":3,"method":"initialize","params":{"protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"modern-init","version":"1"}}}))
                .send()
                .await
                .unwrap()
                .status(),
            429
        );
        let response = client
            .delete(&url)
            .bearer_auth(token)
            .header("mcp-session-id", &session)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        let mut stream = client
            .get(&url)
            .bearer_auth(token)
            .header("accept", "text/event-stream")
            .header("mcp-session-id", second_id)
            .header("mcp-protocol-version", "2025-11-25")
            .send()
            .await
            .unwrap();
        assert_eq!(stream.status(), 200);
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(2), async {
            while stream.chunk().await.unwrap().is_some() {}
        })
        .await
        .unwrap();
    };
    let client = async {
        let result = tokio::time::timeout(Duration::from_secs(8), action).await;
        stop.cancel();
        result.unwrap();
    };
    let (server, ()) = tokio::join!(server, client);
    server.unwrap();
    app.shutdown();
    assert_eq!(
        Arc::strong_count(&app),
        1,
        "SDK handlers must be released before serve returns"
    );
    drop(app);
}
