mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{JsString, Value},
    config::{Config, ConfigReload, ReloadEvent},
    jobs::Store,
    logging::Logger,
    runtime_paths::RuntimePaths,
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

#[test]
fn config_and_reload_keep_authoritative_unknown_values_and_exact_utf16_model_keys() {
    let workspace = Workspace::new();
    let mut raw = workspace.config("https://example.com/v1", "openai-images");
    raw["connections"]["test"]["extensions"] =
        Value::parse(r#"{"\ud800":"\udfff","overflow":1e400}"#).unwrap();
    raw["connections"]["test"]["modelOverrides"] = Value::parse(r#"{"\ud800":{"defaults":{"negative_prompt":"\udfff"},"capabilities":{"overflow":-1e400}}}"#).unwrap();
    // JSON.stringify cannot carry infinity back into JSON; inject its original raw token at ingress.
    let text = raw
        .to_string()
        .replace("\"overflow\":null", "\"overflow\":1e400");
    let file = workspace.0.join("config.json");
    let config = Config::parse(&text, &file, &[], &HashMap::new()).unwrap();
    let retained = &config.value["connections"]["test"]["extensions"];
    assert_eq!(retained["overflow"].as_f64(), Some(f64::INFINITY));
    assert_eq!(
        retained
            .as_object()
            .unwrap()
            .get_utf16(&[0xd800])
            .unwrap()
            .as_utf16(),
        Some([0xdfff].as_slice())
    );
    let model = JsString::from_utf16(vec![0xd800]);
    assert_eq!(config.value["connections"]["test"]["modelOverrides"][&model]["defaults"]["negative_prompt"].as_utf16(), Some([0xdfff].as_slice()));
    let fingerprint = config.value.fingerprint().unwrap();
    let mut reload = ConfigReload::new(config, file, Vec::new(), HashMap::new());
    let first = reload.refresh_text(Some(&text), |_| true).unwrap();
    assert_eq!(first.config.value.fingerprint().unwrap(), fingerprint);
    let again = reload
        .refresh_text(Some(&text), |_| panic!("unchanged text must not reaccept"))
        .unwrap();
    assert_eq!(again.event, ReloadEvent::Unchanged);
    assert_eq!(
        again.config.value["connections"]["test"]["extensions"]["overflow"].as_f64(),
        Some(f64::INFINITY)
    );
}

#[tokio::test]
async fn generation_wire_receipt_restart_and_dedup_preserve_raw_utf16_without_another_post() {
    let workspace = Workspace::new();
    let image = image_fixture::original();
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &image);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let stop = CancellationToken::new();
    let stopped = stop.clone();
    let posts = Arc::new(Mutex::new(Vec::new()));
    let captured = posts.clone();
    let server = async move {
        loop {
            let (mut socket, _) = tokio::select! {
                _ = stopped.cancelled() => break,
                accepted = listener.accept() => accepted.unwrap(),
            };
            let action = async {
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 4096];
                let boundary = loop {
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert!(count > 0, "request must finish");
                    bytes.extend_from_slice(&buffer[..count]);
                    assert!(bytes.len() <= 65536, "bounded request fixture");
                    if let Some(boundary) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..boundary]).unwrap();
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= boundary + 4 + length {
                            break boundary;
                        }
                    }
                };
                assert!(bytes.starts_with(b"POST /v1/images/generations "));
                captured.lock().unwrap().push(
                    Value::parse_with_limit(
                        std::str::from_utf8(&bytes[boundary + 4..]).unwrap(),
                        65536,
                    )
                    .unwrap(),
                );
                let body = format!(
                    r#"{{"data":[{{"b64_json":"{encoded}"}}],"model":"\udfff","id":"\ud800","usage":{{"\ud800":"\udfff"}}}}"#
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            };
            tokio::time::timeout(Duration::from_secs(3), action)
                .await
                .unwrap();
        }
    };
    let mut raw_config = workspace.config(&format!("http://{address}/v1"), "openai-images");
    raw_config["connections"]["test"]["providerOptionKeys"] =
        Value::parse(r#"["\ud800","overflow"]"#).unwrap();
    let config = Config::parse(
        &raw_config.to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let generation = Generation {
        config: Arc::new(config),
        store: Arc::new(Mutex::new(Store::open(&workspace.0.join("state")).unwrap())),
        logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(&workspace.0).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    };
    let action = async {
        let raw = r#"{"prompt":"\ud800","model":"\udfff","request_id":"raw-js","provider_options":{"\ud800":"\udfff","overflow":1e400}}"#;
        let request = Value::parse(raw).unwrap();
        let signal = CancellationToken::new();
        let receipt = generation
            .run(request.clone(), "generate", &signal)
            .await
            .unwrap();
        assert_eq!(receipt.status(), "completed");
        assert_eq!(
            receipt.value["requested_model"].as_utf16(),
            Some([0xdfff].as_slice())
        );
        assert_eq!(
            receipt.value["upstream_model"].as_utf16(),
            Some([0xdfff].as_slice())
        );
        assert_eq!(
            receipt.value["upstream_request_id"].as_utf16(),
            Some([0xd800].as_slice())
        );
        assert_eq!(
            std::fs::read(receipt.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            image
        );
        assert_eq!(
            generation
                .run(request.clone(), "generate", &signal)
                .await
                .unwrap()
                .value["outputs"],
            receipt.value["outputs"]
        );
        let reopened = Store::open(&workspace.0.join("state")).unwrap();
        let persisted = reopened.get("raw-js").unwrap();
        assert_eq!(
            persisted.value["upstream_model"].as_utf16(),
            Some([0xdfff].as_slice())
        );
        assert_eq!(
            persisted.value["usage"]
                .as_object()
                .unwrap()
                .get_utf16(&[0xd800])
                .unwrap()
                .as_utf16(),
            Some([0xdfff].as_slice())
        );
        let mut changed = request;
        changed["provider_options"]["overflow"] = Value::from(f64::NEG_INFINITY);
        // Both infinities stringify as null, so source canonical hashing intentionally binds them equally.
        assert!(generation.run(changed, "generate", &signal).await.is_ok());
        let posts = posts.lock().unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0]["model"].as_utf16(), Some([0xdfff].as_slice()));
        assert_eq!(posts[0]["prompt"].as_utf16(), Some([0xd800].as_slice()));
        assert_eq!(
            posts[0]
                .as_object()
                .unwrap()
                .get_utf16(&[0xd800])
                .unwrap()
                .as_utf16(),
            Some([0xdfff].as_slice())
        );
        assert!(posts[0]["overflow"].is_null());
        assert!(generation.active.lock().unwrap().is_empty());
    };
    let owned_action = async {
        let result = tokio::time::timeout(Duration::from_secs(8), action).await;
        stop.cancel();
        result
    };
    // Both futures are owned by this test: unwinding drops the listener/socket, not a detached task.
    let (_, result) = tokio::time::timeout(Duration::from_secs(12), async {
        tokio::join!(server, owned_action)
    })
    .await
    .expect("owned fixture/action wall bound");
    result.unwrap();
}
