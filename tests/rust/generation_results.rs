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
    compat::canonical_json::fingerprint,
    config::{Config, snapshot_credentials},
    jobs::Store,
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{generation::Generation, identity::connection_identity, jobs},
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
async fn gemini_continuation_and_originals_survive_recovery_without_new_generation() {
    let workspace = Workspace::new();
    let bytes = image_fixture::original();
    let encoded = STANDARD.encode(&bytes);
    let download = bytes.clone();
    let server=Server::new(move |path| Reply {status:200,bytes:path.starts_with("/owned/").then(||download.clone()),value:if path.contains("generateContent"){json!({"model":"actual","candidates":[{"finishReason":"MAX_TOKENS","content":{"parts":[{"thought":true,"thoughtSignature":"synthetic-private"},{"inlineData":{"data":encoded}}]}}]})}else{json!({"data":[{"url":"data:image/png;base64,"}]})}}).await;
    let action = async {
        let raw = workspace.config(&format!("{}/v1beta", server.origin), "gemini");
        let g = generation(&workspace, &raw);
        let signal = CancellationToken::new();
        let receipt = g
            .run(
                json!({"prompt":"Preserve original 日本語","request_id":"gemini-original"}),
                "generate",
                &signal,
            )
            .await
            .unwrap();
        assert_eq!(receipt.status(), "completed");
        assert_eq!(
            std::fs::read(receipt.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
            bytes
        );
        assert!(
            receipt.value["warnings"]
                .as_array()
                .unwrap()
                .contains(&json!("Candidate ended with MAX_TOKENS."))
        );
        let (_, c) = g.config.select(None).unwrap();
        let pinned = snapshot_credentials(&c, &HashMap::new(), &g.runtime)
            .await
            .unwrap();
        let identity = connection_identity("test", &pinned).unwrap();
        let scope = fingerprint(&json!({"identity":identity,"model":"opaque/image"})).unwrap();
        let metadata = g
            .store
            .lock()
            .unwrap()
            .continuation("gemini-original", &scope)
            .unwrap();
        assert_eq!(
            metadata,
            json!({"signatures":[{"part_index":0,"signature":"synthetic-private"}]})
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert_eq!(
            jobs::get(&g, "gemini-original", true, &signal, None)
                .await
                .unwrap()
                .value["outputs"],
            receipt.value["outputs"]
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert!(g.active.lock().unwrap().is_empty());
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn cancelled_publication_retains_completed_originals_and_recovery_preserves_deviations() {
    let workspace = Workspace::new();
    let bytes = image_fixture::original();
    let raw = workspace.config("https://invalid.example/v1", "openai-images");
    let g = generation(&workspace, &raw);
    let mut receipt = numera_image_gen_mcp::jobs::Receipt::new(
        json!({"request_id":"received","connection":"test","status":"prepared","outputs":[],"errors":[],"warnings":[],"deviations":[],"generation_outcome":"not_submitted","created_at":"2026-10-12T00:00:00.000Z"}),
    );
    {
        let mut store = g.store.lock().unwrap();
        receipt = store.prepare(receipt, "effective", "account").unwrap().1;
        assert!(store.admit("received", "test", 4, 2).unwrap());
        assert!(store.claim("received", Some(&receipt), true).unwrap());
        receipt = store.get("received").unwrap();
    }
    let signal = CancellationToken::new();
    signal.cancel();
    let result = numera_image_gen_mcp::adapters::types::Normalized {
        images: vec![numera_image_gen_mcp::adapters::types::ResultItem {
            bytes: Some(bytes.clone()),
            ..Default::default()
        }],
        warnings: vec!["Original requires inspection.".into()],
        upstream_id: Some("original-provider-id".into()),
        usage: Some(json!({"images":1})),
        ..Default::default()
    };
    let failed = tokio::time::timeout(
        Duration::from_secs(3),
        g.finish(
            receipt,
            result,
            &json!({"count":2,"output_format":"jpeg","size":"8x6"}),
            &signal,
            true,
            false,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(failed.status(), "failed");
    assert_eq!(failed.value["generation_outcome"], "completed");
    assert!(g.store.lock().unwrap().has_pending("received").unwrap());
    g.store
        .lock()
        .unwrap()
        .release("received", Some(&failed))
        .unwrap();
    let recovered = tokio::time::timeout(
        Duration::from_secs(3),
        jobs::get(&g, "received", true, &CancellationToken::new(), None),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(recovered.status(), "partial");
    assert_eq!(
        recovered.value["upstream_request_id"],
        "original-provider-id"
    );
    assert_eq!(recovered.value["usage"], json!({"images":1}));
    assert_eq!(
        std::fs::read(recovered.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
        bytes
    );
    assert!(recovered.value["errors"].as_array().unwrap().is_empty());
    assert!(
        recovered.value["deviations"]
            .as_array()
            .unwrap()
            .contains(&json!("Requested 2 image(s), saved 1."))
    );
    assert!(
        recovered.value["deviations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().contains("format"))
    );
}
