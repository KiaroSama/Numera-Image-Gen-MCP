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
    files::output,
    jobs::{Receipt, Store},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, jobs},
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
async fn mixed_provider_results_preserve_valid_originals_and_report_partial_storage_without_retry()
{
    let workspace = Workspace::new();
    let bytes = image_fixture::original();
    let encoded = STANDARD.encode(&bytes);
    let server = Server::new(move |_| Reply {
        status: 200, bytes: None,
        value: json!({"model":"actual","usage":{"images":4},"data":[{"b64_json":encoded},{"b64_json":"%%%"},{"url":"data:image/png;base64,AA=="},null]}),
    }).await;
    let action = async {
        let g = generation(
            &workspace,
            &workspace.config(&format!("{}/v1", server.origin), "openai-images"),
        );
        let request = json!({"prompt":"Preserve original","request_id":"mixed"});
        let receipt = g
            .run(request.clone(), "generate", &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(receipt.status(), "partial");
        assert_eq!(receipt.value["generation_outcome"], "completed");
        assert_eq!(receipt.value["storage_outcome"], "partial_or_failed");
        assert_eq!(receipt.value["outputs"].as_array().unwrap().len(), 1);
        assert_eq!(receipt.value["errors"].as_array().unwrap().len(), 3);
        assert_eq!(receipt.value["upstream_model"], "actual");
        assert_eq!(receipt.value["usage"], json!({"images":4}));
        let id = receipt.value["outputs"][0]["output_id"].as_str().unwrap();
        let (_, image) = output::owned(&g.config.value, &g.store.lock().unwrap(), id).unwrap();
        assert_eq!(image.bytes, bytes);
        let repeated = g
            .run(request, "generate", &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(repeated.value["outputs"], receipt.value["outputs"]);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        std::fs::write(
            receipt.value["outputs"][0]["path"].as_str().unwrap(),
            b"changed",
        )
        .unwrap();
        assert!(
            output::owned(&g.config.value, &g.store.lock().unwrap(), id).is_err(),
            "changed originals must not verify"
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn received_bytes_survive_output_directory_failure_and_recover_with_the_same_uuid_after_reopen()
 {
    let workspace = Workspace::new();
    let bytes = image_fixture::original();
    let encoded = STANDARD.encode(&bytes);
    let server = Server::new(move |_| Reply {
        status: 200,
        value: json!({"data":[{"b64_json":encoded}]}),
        bytes: None,
    })
    .await;
    let action = async {
        let raw = workspace.config(&format!("{}/v1", server.origin), "openai-images");
        let blocked = workspace.0.join("outputs");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("blocked"), b"fixture blocker, not user data").unwrap();
        let g = generation(&workspace, &raw);
        let request = json!({"prompt":"fixture","request_id":"storage-recovery","output_subdirectory":"blocked"});
        let failed = g
            .run(request.clone(), "generate", &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(failed.status(), "failed");
        assert_eq!(failed.value["generation_outcome"], "completed");
        assert!(failed.value["outputs"].as_array().unwrap().is_empty());
        let item = g
            .store
            .lock()
            .unwrap()
            .pending("storage-recovery")
            .unwrap()
            .remove(0);
        assert_eq!(item.bytes.as_ref().unwrap(), &bytes);
        let output_id = item.output_id;
        drop(g);
        std::fs::remove_file(blocked.join("blocked")).unwrap();
        let g = generation(&workspace, &raw);
        let recovered = jobs::get(
            &g,
            "storage-recovery",
            true,
            &CancellationToken::new(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(recovered.status(), "completed");
        assert_eq!(recovered.value["outputs"][0]["output_id"], output_id);
        assert_eq!(
            std::fs::read(recovered.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            bytes
        );
        assert!(
            g.store
                .lock()
                .unwrap()
                .pending("storage-recovery")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            g.run(request, "generate", &CancellationToken::new())
                .await
                .unwrap()
                .value["outputs"],
            recovered.value["outputs"]
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        let cancelled = jobs::cancel(&g, "storage-recovery", &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(cancelled["upstream_requested"], false);
        assert_eq!(cancelled["refund_verified"], false);
        assert_eq!(
            g.store
                .lock()
                .unwrap()
                .get("storage-recovery")
                .unwrap()
                .status(),
            "completed"
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn exact_source_two_item_partial_receipt_saves_original_and_repeated_request_posts_once() {
    let action = async {
        let workspace = Workspace::new();
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".ci-work/parity/normalization-acceptance.json");
        let oracle = Value::parse_with_limit(
            &std::fs::read_to_string(file).expect("frozen normalization oracle required"),
            1048576,
        )
        .unwrap();
        let bytes = numera_image_gen_mcp::files::images::decode_base64(
            oracle["original"].as_str().unwrap(),
            1048576,
        )
        .unwrap();
        let raw = &oracle["mixed"]["raw"];
        assert_eq!(raw["data"].as_array().unwrap().len(), 2);
        assert!(raw["data"][1].is_null());
        let reply = raw.to_string();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let mut config = workspace.config(&format!("{origin}/v1"), "openai-images");
        config["connections"]["test"]["defaultModel"] = json!("fixture");
        config["connections"]["test"]["modelOverrides"] = json!({"fixture": {"maxCount": 2}});
        let g = generation(&workspace, &config);
        let request = json!({"prompt": "fixture", "count": 2, "request_id": "mixed"});
        assert_eq!(oracle["mixed"]["request"], request);
        let client = async {
            let receipt = g
                .run(request.clone(), "generate", &CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(receipt.status(), "partial");
            assert_eq!(receipt.value["outputs"].as_array().unwrap().len(), 1);
            assert_eq!(
                std::fs::read(receipt.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
                bytes
            );
            assert!(
                receipt.value["errors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|error| { error["code"] == "invalid_response" })
            );
            assert_eq!(
                g.run(request, "generate", &CancellationToken::new())
                    .await
                    .unwrap()
                    .value["outputs"],
                receipt.value["outputs"]
            );
            assert!(g.active.lock().unwrap().is_empty());
        };
        let server = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut wire = Vec::new();
            let mut buffer = [0u8; 4096];
            let boundary = loop {
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0, "partial-result request ended before its body");
                wire.extend_from_slice(&buffer[..count]);
                assert!(wire.len() <= 65536, "partial-result fixture request bound");
                if let Some(boundary) = wire.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&wire[..boundary]).unwrap();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .expect("Images fixture must declare its JSON body length");
                    assert!(length <= 65536 - boundary - 4);
                    if wire.len() >= boundary + 4 + length {
                        assert_eq!(wire.len(), boundary + 4 + length);
                        break boundary;
                    }
                }
            };
            let headers = std::str::from_utf8(&wire[..boundary]).unwrap();
            assert_eq!(
                headers.lines().next().unwrap(),
                "POST /v1/images/generations HTTP/1.1"
            );
            let body =
                Value::parse_with_limit(std::str::from_utf8(&wire[boundary + 4..]).unwrap(), 65536)
                    .unwrap();
            assert_eq!(body["prompt"], "fixture");
            assert_eq!(body["model"], "fixture");
            assert_eq!(body["n"], 2);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
            // Keep the listener owned until deduplication finishes; an extra POST stays
            // queued and is detected directly, without a sleep or a permissive retry.
            listener
        };
        let ((), listener) = tokio::join!(client, async {
            tokio::time::timeout(Duration::from_secs(3), server)
                .await
                .unwrap()
        });
        assert!(
            matches!(listener.into_std().unwrap().accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    };
    tokio::time::timeout(Duration::from_secs(8), action)
        .await
        .expect("two-item partial publication scenario exceeded 8 seconds");
}

#[tokio::test]
async fn retained_continuation_url_is_downloaded_locally_without_credentials_or_new_submission() {
    let workspace = Workspace::new();
    let bytes = image_fixture::original();
    let download = bytes.clone();
    let server = Server::new(move |_| Reply {
        status: 200,
        value: json!({}),
        bytes: Some(download.clone()),
    })
    .await;
    let action = async {
        let mut raw = workspace.config("https://invalid.example/v1", "openai-images");
        raw["files"]["assetAllowances"] = json!([{"origin":server.origin,"pathPrefix":"/owned"}]);
        let g = generation(&workspace, &raw);
        {
            let mut store = g.store.lock().unwrap();
            let initial = Receipt::new(
                json!({"request_id":"retained-url","connection":"removed","status":"prepared","outputs":[],"warnings":[],"errors":[],"deviations":[],"generation_outcome":"not_submitted","storage_outcome":"not_started","created_at":"2026-10-11T00:00:00.000Z","output_requirements":{"count":1,"filename_prefix":"original"}}),
            );
            let (_, mut receipt) = store
                .prepare(initial, "effective", "original-account")
                .unwrap();
            assert!(store.admit("retained-url", "removed", 4, 2).unwrap());
            assert!(store.claim("retained-url", Some(&receipt), false).unwrap());
            receipt = store.get("retained-url").unwrap();
            store.reserve("retained-url", 1048576, 2097152).unwrap();
            store
                .retain_url(
                    "retained-url",
                    0,
                    &format!("{}/owned/original.png", server.origin).into(),
                    Some(&receipt),
                )
                .unwrap();
            store.release("retained-url", Some(&receipt)).unwrap();
        }
        let recovered = jobs::get(&g, "retained-url", true, &CancellationToken::new(), None)
            .await
            .unwrap();
        assert_eq!(recovered.status(), "completed");
        assert_eq!(
            std::fs::read(recovered.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            bytes
        );
        assert_eq!(
            g.store.lock().unwrap().identity("retained-url").unwrap(),
            "original-account"
        );
        assert_eq!(
            *server.requests.lock().unwrap(),
            vec!["/owned/original.png".to_owned()]
        );
        let repeated = jobs::get(&g, "retained-url", true, &CancellationToken::new(), None)
            .await
            .unwrap();
        assert_eq!(repeated.value["outputs"], recovered.value["outputs"]);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}
