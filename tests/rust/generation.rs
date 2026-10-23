mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    config::Config, jobs::Store, logging::Logger, runtime_paths::RuntimePaths,
    services::generation::Generation,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn actual_loopback_generation_preserves_bytes_and_deduplicates_paid_post() {
    let workspace = Workspace::new();
    let image = image_fixture::original();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let stop = CancellationToken::new();
    let done = stop.clone();
    let posts = Arc::new(Mutex::new(Vec::new()));
    let requests = posts.clone();
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &image);
    let server = tokio::spawn(async move {
        loop {
            let accepted = tokio::select! {_=done.cancelled()=>break,result=listener.accept()=>result.unwrap()};
            let (mut socket, _) = accepted;
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let n = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut buffer))
                    .await
                    .unwrap()
                    .unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..n]);
                assert!(bytes.len() < 65536);
                if let Some(boundary) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..boundary]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= boundary + 4 + length {
                        break;
                    }
                }
            }
            let boundary = bytes.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
            let headers = String::from_utf8_lossy(&bytes[..boundary]);
            assert!(headers.starts_with("POST /v1/images/generations "));
            requests
                .lock()
                .unwrap()
                .push(serde_json::from_slice::<Value>(&bytes[boundary + 4..]).unwrap());
            let body = json!({"data":[{"b64_json":encoded}],"model":"opaque/image"}).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        }
    });
    let config = Config::parse(
        &workspace
            .config(&format!("http://{address}/v1"), "openai-images")
            .to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let generation = Generation {
        store: Arc::new(Mutex::new(Store::open(&workspace.0.join("state")).unwrap())),
        logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(&workspace.0).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
        config: Arc::new(config),
    };
    let action = async {
        let input = json!({"prompt":"Draw a café 日本語 🌍","request_id":"same-image"});
        let cancel = CancellationToken::new();
        let result = generation
            .run(input.clone(), "generate", &cancel)
            .await
            .unwrap();
        assert_eq!(result.status(), "completed");
        let output = &result.value["outputs"][0];
        let saved = std::fs::read(output["path"].as_str().unwrap()).unwrap();
        assert_eq!(saved, image);
        let repeated = generation.run(input, "generate", &cancel).await.unwrap();
        assert_eq!(repeated.value["outputs"], result.value["outputs"]);
        let changed = generation
            .run(
                json!({"prompt":"different","request_id":"same-image"}),
                "generate",
                &cancel,
            )
            .await
            .unwrap_err();
        assert_eq!(changed.code, "request_id_conflict");
        assert_eq!(posts.lock().unwrap().len(), 1);
        assert_eq!(posts.lock().unwrap()[0]["prompt"], "Draw a café 日本語 🌍");
        assert!(generation.active.lock().unwrap().is_empty());
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    stop.cancel();
    server.await.unwrap();
    result.unwrap();
}
