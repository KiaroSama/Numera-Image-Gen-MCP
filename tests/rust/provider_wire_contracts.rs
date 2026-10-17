mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::Workspace;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    adapters::{build_request, normalize, types::Input},
    config::{Config, snapshot_credentials},
    files::images,
    http::api,
    runtime_paths::RuntimePaths,
    services::request,
};
use reqwest::Method;
use std::{collections::HashMap, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

// Frozen tests/contract/wire-contracts.test.ts:16-149 and src/adapters/{images,gemini,
// openai,openrouter}.ts supply the literal bodies; expected values never call a native builder.
// Existing adapters.json rows use different prompts/models/size or edit references.
async fn wire(adapter: &str, path: &str, expected: Value) {
    let action = async {
        let workspace = Workspace::new();
        let bytes = image_fixture::original();
        let encoded = STANDARD.encode(&bytes);
        let reply = match adapter {
            "gemini" => json!({"candidates": [{"content": {"parts": [
                {"inlineData": {"data": encoded, "mimeType": "image/png"}}
            ]}}]}),
            "gemini-interactions" => json!({"status": "completed", "steps": [
                {"type": "model_output", "content": [{"type": "image", "data": encoded}]}
            ]}),
            "openai-responses" => json!({"status": "completed", "output": [
                {"type": "image_generation_call", "status": "completed", "result": encoded}
            ]}),
            "chat-images" => json!({"choices": [{"message": {"images": [
                {"image_url": {"url": format!("data:image/png;base64,{encoded}")}}
            ]}}]}),
            "openai-images" | "openrouter-images" => json!({"data": [{"b64_json": encoded}]}),
            _ => panic!("Unexpected wire fixture adapter."),
        }
        .to_string();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let mut raw = workspace.config(&origin, adapter);
        raw["connections"]["test"]["baseUrlMode"] = json!("origin");
        raw["connections"]["test"]["auth"] =
            json!({"type": "bearer", "secretEnv": "NUMERA_WIRE_FIXTURE"});
        raw["connections"]["test"]["orchestrationModel"] = json!("orchestrator");
        raw["connections"]["test"]["chatImageOutput"] = json!(true);
        raw["connections"]["test"]["discoveryTimeoutMs"] = json!(3000);
        let config = Config::parse(
            &raw.to_string(),
            &workspace.0.join("config.json"),
            &[],
            &HashMap::new(),
        )
        .unwrap();
        let (_, selected) = config.select(Some("test")).unwrap();
        let env = HashMap::from([("NUMERA_WIRE_FIXTURE".into(), "fixture-key".into())]);
        let runtime = RuntimePaths::new(&workspace.0).unwrap();
        let pinned = snapshot_credentials(&selected, &env, &runtime)
            .await
            .unwrap();
        let c = pinned.value();
        let headers = pinned.headers();
        let request = request::parse(json!({"prompt": "اصل Unicode 123"})).unwrap();
        let model = "family/image".into();
        let built = build_request(&Input {
            connection: c,
            request: &request,
            model: &model,
            operation: "generate",
            references: &[],
            mask: None,
        })
        .unwrap();
        let server = async {
            let capture = async {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut wire = Vec::new();
                let mut buffer = [0u8; 4096];
                let (boundary, length) = loop {
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0, "Fixture request ended before its declared body.");
                    wire.extend_from_slice(&buffer[..n]);
                    assert!(wire.len() <= 1048576, "Fixture request exceeds 1 MiB.");
                    if let Some(boundary) = wire.windows(4).position(|b| b == b"\r\n\r\n") {
                        assert!(boundary <= 65536, "Fixture headers exceed 64 KiB.");
                        let header = std::str::from_utf8(&wire[..boundary]).unwrap();
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .expect("JSON submissions declare bounded Content-Length");
                        assert!(length <= 1048576 - boundary - 4);
                        if wire.len() >= boundary + 4 + length {
                            break (boundary, length);
                        }
                    } else {
                        assert!(wire.len() <= 65536, "Fixture headers exceed 64 KiB.");
                    }
                };
                assert_eq!(wire.len(), boundary + 4 + length);
                let header = std::str::from_utf8(&wire[..boundary]).unwrap();
                let mut lines = header.lines();
                assert_eq!(lines.next().unwrap(), format!("POST {path} HTTP/1.1"));
                let fields: HashMap<_, _> = lines
                    .map(|line| {
                        let (name, value) = line.split_once(':').unwrap();
                        (name.to_ascii_lowercase(), value.trim())
                    })
                    .collect();
                assert_eq!(fields.get("authorization"), Some(&"Bearer fixture-key"));
                assert_eq!(fields.get("content-type"), Some(&"application/json"));
                assert!(!fields.contains_key("transfer-encoding"));
                let body = std::str::from_utf8(&wire[boundary + 4..]).unwrap();
                assert!(body.contains("اصل Unicode 123"));
                assert_eq!(Value::parse_with_limit(body, 1048576).unwrap(), expected);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            };
            tokio::time::timeout(Duration::from_secs(3), capture)
                .await
                .expect("Wire fixture socket exceeded 3 seconds");
        };
        let cancel = CancellationToken::new();
        // Scoped join owns both futures: timeout/panic drops their socket and listener;
        // no spawned server task can outlive this scenario or keep its workspace open.
        let (response, ()) = tokio::join!(
            api::request(
                c,
                &headers,
                api::Operation {
                    operation: &built.operation,
                    prefix: built.prefix,
                    method: Method::POST,
                    body: built.body,
                    query: None,
                },
                &cancel,
                1048576,
            ),
            server
        );
        let response = response.unwrap();
        assert_eq!(response.mime, "application/json");
        let mut normalized = normalize::response(c, response.bytes, &response.mime).unwrap();
        assert_eq!(normalized.images.len(), 1);
        assert!(normalized.job.is_none());
        assert!(normalized.terminal.is_none());
        let image = normalized.images.pop().unwrap();
        assert!(image.error.is_none());
        assert!(image.bytes.is_none());
        let returned = if adapter == "chat-images" {
            assert!(image.base64.is_none());
            let url = image.url.unwrap();
            assert_eq!(url, format!("data:image/png;base64,{encoded}"));
            images::data_url(url.as_str().unwrap(), 1048576).unwrap()
        } else {
            assert!(image.url.is_none());
            let data = image.base64.unwrap();
            assert_eq!(data, encoded);
            images::decode_base64(data.as_str().unwrap(), 1048576).unwrap()
        };
        assert_eq!(returned, bytes);
        let original = images::inspect(returned, &config.value, None).unwrap();
        assert_eq!(original.bytes, bytes);
        assert_eq!(original.mime, "image/png");
        assert_eq!((original.width, original.height), (7, 5));
    };
    tokio::time::timeout(Duration::from_secs(8), action)
        .await
        .expect("Provider wire scenario exceeded 8 seconds");
}

#[tokio::test]
async fn openai_images_posts_exact_utf8_json_and_normalizes_original() {
    wire(
        "openai-images",
        "/v1/images/generations",
        json!({"model": "family/image", "prompt": "اصل Unicode 123", "n": 1}),
    )
    .await;
}

#[tokio::test]
async fn gemini_posts_encoded_model_path_and_exact_modalities_and_normalizes_original() {
    wire(
        "gemini",
        "/v1beta/models/family%2Fimage:generateContent",
        json!({
            "contents": [{"role": "user", "parts": [{"text": "اصل Unicode 123"}]}],
            "generationConfig": {"responseModalities": ["TEXT", "IMAGE"]}
        }),
    )
    .await;
}

#[tokio::test]
async fn gemini_interactions_posts_stateless_image_format_and_normalizes_original() {
    wire(
        "gemini-interactions",
        "/v1beta/interactions",
        json!({
            "model": "family/image", "store": false,
            "input": [{"type": "text", "text": "اصل Unicode 123"}],
            "response_format": {"type": "image"}
        }),
    )
    .await;
}

#[tokio::test]
async fn responses_posts_separate_orchestrator_and_image_tool_and_normalizes_original() {
    wire(
        "openai-responses",
        "/v1/responses",
        json!({
            "model": "orchestrator", "store": false,
            "input": [{"role": "user", "content": [{"type": "input_text", "text": "اصل Unicode 123"}]}],
            "tools": [{"type": "image_generation", "model": "family/image", "action": "generate"}],
            "tool_choice": {"type": "image_generation"}
        }),
    )
    .await;
}

#[tokio::test]
async fn openrouter_posts_dedicated_images_route_without_fallback_and_normalizes_original() {
    wire(
        "openrouter-images",
        "/api/v1/images",
        json!({
            "model": "family/image", "prompt": "اصل Unicode 123", "n": 1,
            "input_references": [], "provider": {"allow_fallbacks": false}
        }),
    )
    .await;
}

#[tokio::test]
async fn chat_images_posts_opted_in_image_modalities_and_normalizes_original() {
    wire(
        "chat-images",
        "/v1/chat/completions",
        json!({
            "model": "family/image",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "اصل Unicode 123"}]}],
            "modalities": ["image", "text"], "image_config": {}
        }),
    )
    .await;
}
