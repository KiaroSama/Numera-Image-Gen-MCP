mod common;
#[path = "common/wire.rs"]
mod wire;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::{Value, json},
    config::Config,
    files::images::{decode_base64, inspect},
    jobs::Store,
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::generation::Generation,
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

fn generation(root: &Path, origin: &str) -> Generation {
    let config = Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":{"local":{
                "adapter":"openai-images","baseUrl":format!("{origin}/v1"),
                "auth":{"type":"none"},"defaultModel":"image-model",
                "edit":{"mode":"json","encoding":"image"}
            }}
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let (_, connection) = config.select(None).unwrap();
    assert_eq!(connection["baseUrlMode"], "api-prefix");
    assert_eq!(connection["requestTimeoutMs"], 300000);
    assert_eq!(connection["discoveryTimeoutMs"], 15000);
    assert_eq!(connection["edit"]["maxReferences"], 1);
    assert_eq!(connection["edit"]["masks"], false);
    assert!(connection.get("orchestrationModel").is_none());
    Generation {
        config: Arc::new(config),
        store: Arc::new(Mutex::new(Store::open(&root.join("state")).unwrap())),
        logger: Arc::new(Logger::new(&root.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(root).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    }
}

fn original(name: &str, expected: (u32, u32)) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/image-acceptance.json");
    assert!(std::fs::metadata(&path).unwrap().len() <= 1048576);
    let rows = Value::parse_with_limit(&std::fs::read_to_string(path).unwrap(), 1048576).unwrap();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("Missing exact source upscale image oracle row: {name}"));
    let bytes = decode_base64(row["base64"].as_str().unwrap(), 1048576).unwrap();
    let image = inspect(bytes.clone(), &json!({}), None).unwrap();
    assert_eq!((image.width, image.height), expected);
    assert_eq!(image.mime, "image/png");
    assert!(image.alpha);
    assert_eq!(image.bytes, bytes);
    bytes
}

#[tokio::test]
async fn exact_source_native_upscale_saves_larger_original_then_marks_unchanged_output_partial() {
    let workspace = Workspace::new();
    let root = &workspace.0;
    let original = original("png", (8, 6));
    let larger = self::original("upscale-png", (16, 12));
    let reference = format!("data:image/png;base64,{}", STANDARD.encode(&original));
    let wire_reference = reference.clone();
    let larger_response = STANDARD.encode(&larger);
    let unchanged_response = STANDARD.encode(&original);
    let mut posts = 0;
    wire::serve(
        |request| {
            posts += 1;
            assert_eq!(request.method, "POST");
            assert_eq!(request.path, "/v1/images/edits");
            assert!(
                request
                    .headers
                    .to_ascii_lowercase()
                    .contains("content-type: application/json")
            );
            let body =
                Value::parse_with_limit(std::str::from_utf8(&request.body).unwrap(), 1048576)
                    .unwrap();
            assert_eq!(body["image"], wire_reference);
            assert!(
                body["prompt"]
                    .as_str()
                    .unwrap()
                    .contains("Upscale the reference image to 16x12")
            );
            assert_eq!(body["size"], "16x12");
            assert!(body.get("upscale").is_none());
            wire::json_reply(json!({"data":[{
                "b64_json":if posts == 1 { &larger_response } else { &unchanged_response }
            }]}))
        },
        |origin, requests| async move {
            let g = generation(root, &origin);
            let signal = CancellationToken::new();
            let input = json!({
                "prompt":"Preserve the composition","upscale":true,"size":"16x12",
                "reference_images":[{"type":"data_url","data_url":reference}]
            });
            let mut first_input = input.clone();
            first_input["request_id"] = json!("native-upscale");
            let first = g.run(first_input, "edit", &signal).await.unwrap();
            assert_eq!(first.status(), "completed");
            assert_eq!(first.value["outputs"][0]["width"], 16);
            assert_eq!(first.value["outputs"][0]["height"], 12);
            assert_eq!(
                std::fs::read(first.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
                larger
            );
            let mut unchanged_input = input.clone();
            unchanged_input["request_id"] = json!("unchanged-output");
            let mismatch = g.run(unchanged_input, "edit", &signal).await.unwrap();
            assert_eq!(mismatch.status(), "partial");
            assert!(
                mismatch.value["deviations"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(
                        "Output 0 was not upscaled beyond the reference dimensions."
                    ))
            );
            assert_eq!(
                std::fs::read(mismatch.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
                original
            );
            for size in ["8x6", "4x3", "auto", "100000x100000"] {
                let mut invalid = input.clone();
                invalid["size"] = json!(size);
                invalid["request_id"] = json!(format!("invalid-{size}"));
                let error = g.run(invalid, "edit", &signal).await.unwrap_err();
                assert_eq!(error.code, "invalid_input", "{size}");
            }
            let captured = requests.lock().unwrap();
            assert_eq!(captured.len(), 2);
            assert!(
                captured.iter().all(|request| {
                    request.method == "POST" && request.path == "/v1/images/edits"
                })
            );
        },
    )
    .await;
    assert_eq!(posts, 2);
}
