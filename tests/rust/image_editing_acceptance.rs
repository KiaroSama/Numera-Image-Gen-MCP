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

fn generation(root: &Path, origin: &str, polarity: &str) -> Generation {
    let config = Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":{"local":{
                "adapter":"openai-images","baseUrl":format!("{origin}/v1"),
                "auth":{"type":"none"},"defaultModel":"comic-model",
                "edit":{"mode":"json","encoding":"image","masks":true,"maskPolarity":polarity}
            }}
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let (_, connection) = config.select(None).unwrap();
    assert_eq!(connection["requestTimeoutMs"], 300000);
    assert_eq!(connection["discoveryTimeoutMs"], 15000);
    assert_eq!(connection["edit"]["maxReferences"], 1);
    Generation {
        config: Arc::new(config),
        store: Arc::new(Mutex::new(Store::open(&root.join("state")).unwrap())),
        logger: Arc::new(Logger::new(&root.join("logs"), "INFO")),
        runtime: RuntimePaths::new(root).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    }
}

fn original() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/image-acceptance.json");
    assert!(std::fs::metadata(&path).unwrap().len() <= 1048576);
    let rows = Value::parse_with_limit(&std::fs::read_to_string(path).unwrap(), 1048576).unwrap();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "png")
        .unwrap();
    let bytes = decode_base64(row["base64"].as_str().unwrap(), 1048576).unwrap();
    let image = inspect(bytes.clone(), &json!({}), None).unwrap();
    assert_eq!((image.width, image.height), (8, 6));
    assert_eq!(image.mime, "image/png");
    assert!(image.alpha);
    bytes
}

async fn exact_region(polarity: &str) {
    let workspace = Workspace::new();
    let source = original();
    let reference = format!("data:image/png;base64,{}", STANDARD.encode(&source));
    let wire_reference = reference.clone();
    let response = STANDARD.encode(&source);
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
            let prompt = body["prompt"].as_str().unwrap();
            assert!(prompt.contains("Preserve the comic panels 日本語"));
            assert!(prompt.contains("Translate the text inside the selected region into Persian"));
            let mask = numera_image_gen_mcp::files::images::data_url(
                body["mask"].as_str().unwrap(),
                1048576,
            )
            .unwrap();
            let decoded = image::load_from_memory(&mask).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (8, 6));
            assert_eq!(decoded.color(), image::ColorType::Rgba8);
            let pixels = decoded.to_rgba8();
            assert_eq!(pixels.get_pixel(0, 0).0[3], 255);
            let alpha = if polarity == "transparent-edit" {
                0
            } else {
                255
            };
            assert_eq!(pixels.get_pixel(2, 1).0[3], alpha);
            assert_eq!(pixels.get_pixel(4, 2).0[3], alpha);
            assert_eq!(pixels.get_pixel(5, 2).0[3], 255);
            if polarity != "transparent-edit" {
                assert_eq!(
                    pixels.get_pixel(0, 0).0[0],
                    if polarity == "white-edit" { 0 } else { 255 }
                );
                assert_eq!(
                    pixels.get_pixel(2, 1).0[0],
                    if polarity == "white-edit" { 255 } else { 0 }
                );
            }
            wire::json_reply(json!({"data":[{"b64_json":response}]}))
        },
        |origin, requests| async move {
            let g = generation(&workspace.0, &origin, polarity);
            let signal = CancellationToken::new();
            let input = json!({
                "prompt":"Preserve the comic panels 日本語","request_id":"comic-region",
                "target_language":"Persian",
                "reference_images":[{"type":"data_url","data_url":reference}],
                "edit_region":{"x":2,"y":1,"width":3,"height":2}
            });
            let result = g.run(input.clone(), "edit", &signal).await.unwrap();
            assert_eq!(result.status(), "completed");
            assert_eq!(result.value["outputs"][0]["width"], 8);
            assert_eq!(result.value["outputs"][0]["height"], 6);
            assert_eq!(
                std::fs::read(result.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
                source
            );
            let mut outside = input.clone();
            outside["request_id"] = json!("outside");
            outside["edit_region"] = json!({"x":7,"y":0,"width":2,"height":1});
            let mut conflict = input.clone();
            conflict["request_id"] = json!("conflict");
            conflict["mask"] = input["reference_images"][0].clone();
            let mut generate = input.clone();
            generate["request_id"] = json!("generate-region");
            for (invalid, operation) in [
                (outside, "edit"),
                (conflict, "edit"),
                (generate, "generate"),
            ] {
                let error = g.run(invalid, operation, &signal).await.unwrap_err();
                assert_eq!(error.code, "invalid_input");
            }
            assert_eq!(requests.lock().unwrap().len(), 1);
        },
    )
    .await;
    assert_eq!(posts, 1);
}

#[tokio::test]
async fn exact_source_transparent_region_edit_preserves_mask_pixels_and_original() {
    exact_region("transparent-edit").await;
}

#[tokio::test]
async fn exact_source_white_region_edit_preserves_mask_pixels_and_original() {
    exact_region("white-edit").await;
}

#[tokio::test]
async fn exact_source_black_region_edit_preserves_mask_pixels_and_original() {
    exact_region("black-edit").await;
}
