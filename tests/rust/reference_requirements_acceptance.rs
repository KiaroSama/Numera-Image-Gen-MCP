mod common;
#[path = "common/multipart.rs"]
mod multipart;
#[path = "common/wire.rs"]
mod wire;
use base64::{Engine, engine::general_purpose::STANDARD};
use common::Workspace;
use numera_image_gen_mcp::{
    adapters::{build_request, types::Input},
    capabilities,
    compat::value::{Value, json},
    config::Config,
    files::images::{Image, decode_base64, inspect},
    http::api::{self, Body},
    runtime_paths::RuntimePaths,
    services::{discovery::Discovery, request},
};
use reqwest::Method;
use std::{collections::HashMap, path::Path, time::Duration};
use tokio_util::sync::CancellationToken;

fn config(root: &Path, connection: Value) -> numera_image_gen_mcp::error::Result<Config> {
    Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":{"local":connection}
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
}

fn connection(root: &Path, overrides: Value) -> Value {
    let mut c = json!({
        "adapter":"openai-images","baseUrl":"https://api.example/v1",
        "auth":{"type":"none"}
    });
    c.as_object_mut()
        .unwrap()
        .extend(overrides.as_object().unwrap().clone());
    config(root, c).unwrap().select(None).unwrap().1
}

fn original(name: &str, dimensions: (u32, u32)) -> Image {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/image-acceptance.json");
    assert!(std::fs::metadata(&path).unwrap().len() <= 1048576);
    let rows = Value::parse_with_limit(&std::fs::read_to_string(path).unwrap(), 1048576).unwrap();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("Missing exact reference source image: {name}"));
    let bytes = decode_base64(row["base64"].as_str().unwrap(), 1048576).unwrap();
    let image = inspect(bytes.clone(), &json!({}), None).unwrap();
    assert_eq!((image.width, image.height), dimensions);
    assert_eq!(image.mime, "image/png");
    assert!(image.alpha);
    assert_eq!(image.bytes, bytes);
    image
}

fn input<'a>(c: &'a Value, r: &'a Value, references: &'a [Image]) -> Input<'a> {
    static MODEL: std::sync::OnceLock<numera_image_gen_mcp::compat::value::JsString> =
        std::sync::OnceLock::new();
    Input {
        connection: c,
        request: r,
        model: MODEL.get_or_init(|| "model".into()),
        operation: "edit",
        references,
        mask: None,
    }
}

async fn scalar(mode: &str) {
    let workspace = Workspace::new();
    let invalid = config(
        &workspace.0,
        json!({
            "adapter":"openai-images","baseUrl":"https://api.example/v1",
            "auth":{"type":"none"},"edit":{"mode":mode,"encoding":"image","maxReferences":2}
        }),
    );
    let error = invalid.err().unwrap();
    assert!(error.message.contains("incompatible"));
    let c = connection(
        &workspace.0,
        json!({
            "edit":{"mode":mode,"encoding":"image","maxReferences":1},
            "modelOverrides":{"model":{"maxReferences":2}}
        }),
    );
    let r = request::parse(json!({
        "prompt":"draw","reference_images":[
            {"type":"data_url","data_url":"data:image/png;base64,AQID"},
            {"type":"data_url","data_url":"data:image/png;base64,BAUG"}
        ]
    }))
    .unwrap();
    let error = capabilities::effective_request(&c, &r, &"model".into()).unwrap_err();
    assert!(error.message.contains("Scalar image"));
    let references = [original("png", (8, 6)), original("portrait-png", (6, 8))];
    let error = build_request(&input(&c, &r, &references)).err().unwrap();
    assert!(error.message.contains("Scalar image"));
    let encoded = references
        .iter()
        .map(|image| {
            format!(
                "data:{};base64,{}",
                image.mime,
                STANDARD.encode(&image.bytes)
            )
        })
        .collect::<Vec<_>>();
    for encoding in ["images", "image_urls", "input_references"] {
        let c = connection(
            &workspace.0,
            json!({"edit":{"mode":mode,"encoding":encoding,"maxReferences":2}}),
        );
        let built = build_request(&input(&c, &r, &references)).unwrap();
        let Body::Json(body) = built.body else {
            panic!("Explicit array route must retain a JSON body")
        };
        let expected = if encoding == "input_references" {
            json!(
                encoded
                    .iter()
                    .map(|url| json!({"type":"image_url","image_url":{"url":url}}))
                    .collect::<Vec<_>>()
            )
        } else {
            json!(encoded)
        };
        assert_eq!(body[encoding], expected);
    }
    let single = build_request(&input(&c, &r, &references[..1])).unwrap();
    let Body::Json(body) = single.body else {
        panic!("Explicit scalar route must retain a JSON body")
    };
    assert_eq!(body["image"], encoded[0]);
    for encoding in ["image", "image[]"] {
        let (root, r, references) = (&workspace.0, &r, &references);
        wire::serve(
            |request| {
                assert_eq!(request.method, "POST");
                assert_eq!(request.path, "/v1/images/edits");
                let parts = multipart::multipart(request);
                let files = parts
                    .iter()
                    .filter(|part| part.name == encoding)
                    .collect::<Vec<_>>();
                assert_eq!(files.len(), 2);
                for (index, part) in files.into_iter().enumerate() {
                    assert_eq!(part.bytes, references[index].bytes);
                    assert_eq!(
                        part.filename,
                        Some(format!("reference-{index}.png").as_str())
                    );
                    assert_eq!(part.mime, Some("image/png"));
                }
                wire::json_reply(json!({"data":[]}))
            },
            |origin, requests| async move {
                let c = connection(
                    root,
                    json!({
                        "baseUrl":format!("{origin}/v1"),
                        "edit":{"mode":"multipart","encoding":encoding,"maxReferences":2}
                    }),
                );
                let built = build_request(&input(&c, r, references)).unwrap();
                assert!(matches!(&built.body, Body::Multipart(_)));
                api::request(
                    &c,
                    &HashMap::new(),
                    api::Operation {
                        operation: &built.operation,
                        prefix: built.prefix,
                        method: Method::POST,
                        body: built.body,
                        query: None,
                    },
                    &CancellationToken::new(),
                    1048576,
                )
                .await
                .unwrap();
                assert_eq!(requests.lock().unwrap().len(), 1);
            },
        )
        .await;
    }
}

#[tokio::test]
async fn exact_source_json_scalar_loss_refusals_preserve_all_array_and_multipart_references() {
    tokio::time::timeout(Duration::from_secs(8), scalar("json"))
        .await
        .unwrap();
}

#[tokio::test]
async fn exact_source_generation_scalar_loss_refusals_preserve_all_array_and_multipart_references()
{
    tokio::time::timeout(Duration::from_secs(8), scalar("generation"))
        .await
        .unwrap();
}

#[tokio::test]
async fn exact_source_model_limits_remain_separate_from_route_safety_budget() {
    let workspace = Workspace::new();
    let config = config(
        &workspace.0,
        json!({"adapter":"openai-images","baseUrl":"https://api.example/v1","auth":{"type":"none"}}),
    )
    .unwrap();
    let (_, c) = config.select(None).unwrap();
    let runtime = RuntimePaths::new(&workspace.0).unwrap();
    let env = HashMap::new();
    let signal = CancellationToken::new();
    let action = async {
        let model = |id: &str| {
            let id = id.into();
            let (config, runtime, env, signal) = (&config, &runtime, &env, &signal);
            async move {
                Discovery::model(config, runtime, env, None, &id, signal)
                    .await
                    .unwrap()
            }
        };
        let result = model("cx/gpt-image-2.5-sunburst").await;
        let limits = &result["dimensions"]["documented_limits"];
        assert_eq!(limits["max_edge"], 3840);
        assert_eq!(limits["max_pixels"], 8294400);
        assert_eq!(limits["forwarding_guaranteed"], false);
        assert_eq!(
            result["dimensions"]["route_support"]["max_pixels"],
            64000000
        );
        let r = request::parse(json!({"prompt":"draw","size":"4096x4096"})).unwrap();
        assert!(
            capabilities::effective_request(&c, &r, &"cx/gpt-image-2.5-sunburst".into())
                .unwrap_err()
                .message
                .contains("documented")
        );
        let r = request::parse(json!({"prompt":"draw","size":"3840x2160"})).unwrap();
        capabilities::effective_request(&c, &r, &"cx/gpt-image-2.5-sunburst".into()).unwrap();
        let unknown = model("cx/gpt-6.1-sol").await;
        assert!(unknown["dimensions"]["documented_limits"]["max_edge"].is_null());
        let pro = model("antigravity/gemini-3-pro-image-preview").await;
        let dimensions = &pro["dimensions"];
        assert_eq!(
            dimensions["model_support"]["aspect_ratios"]
                .as_array()
                .unwrap()
                .len(),
            10
        );
        assert_eq!(
            dimensions["model_support"]["resolution_tiers"],
            json!(["1K", "2K", "4K"])
        );
        assert_eq!(dimensions["documented_limits"]["max_edge"], 6336);
        for preference in [json!({"aspect_ratio":"1:8"}), json!({"image_size":"8K"})] {
            let mut r = json!({"prompt":"draw"});
            r.as_object_mut()
                .unwrap()
                .extend(preference.as_object().unwrap().clone());
            let r = request::parse(r).unwrap();
            assert!(
                capabilities::effective_request(&c, &r, &"gemini-3-pro-image".into())
                    .unwrap_err()
                    .message
                    .contains("documented model presets")
            );
        }
        assert!(
            dimensions["documented_limits"]["pixel_sizes"]
                .as_array()
                .unwrap()
                .contains(&json!({
                    "aspect_ratio":"1:1","resolution":"4K","width":4096,"height":4096
                }))
        );
        for size in [
            "3841x2160",
            "3840x2304",
            "1008x512",
            "1025x1024",
            "3840x1024",
        ] {
            let r = request::parse(json!({"prompt":"draw","size":size})).unwrap();
            assert!(
                capabilities::effective_request(&c, &r, &"gpt-image-2.5-flare".into())
                    .unwrap_err()
                    .message
                    .contains("documented")
            );
        }
    };
    tokio::time::timeout(Duration::from_secs(3), action)
        .await
        .unwrap();
}

#[tokio::test]
async fn exact_source_multipart_edit_keeps_original_reference_and_alpha_mask_on_wire() {
    let workspace = Workspace::new();
    let image = original("png", (8, 6));
    let (root, image) = (&workspace.0, &image);
    wire::serve(
        |request| {
            assert_eq!(request.method, "POST");
            assert_eq!(request.path, "/v1/images/edits");
            assert!(
                request
                    .headers
                    .to_ascii_lowercase()
                    .contains("multipart/form-data; boundary=")
            );
            assert!(
                request
                    .body
                    .windows(image.bytes.len())
                    .any(|bytes| bytes == image.bytes)
            );
            let parts = multipart::multipart(request);
            for name in ["image", "mask"] {
                let files = parts
                    .iter()
                    .filter(|part| part.name == name)
                    .collect::<Vec<_>>();
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].bytes, image.bytes);
                assert_eq!(files[0].mime, Some("image/png"));
                assert_eq!(
                    files[0].filename,
                    Some(if name == "mask" {
                        "mask.png"
                    } else {
                        "reference-0.png"
                    })
                );
            }
            wire::json_reply(json!({"data":[{"b64_json":STANDARD.encode(&image.bytes)}]}))
        },
        |origin, requests| async move {
            let c = connection(
                root,
                json!({
                    "baseUrl":format!("{origin}/v1"),
                    "edit":{"mode":"multipart","encoding":"image","maxReferences":1,
                        "masks":true,"maskPolarity":"transparent-edit"}
                }),
            );
            let r = request::parse(json!({"prompt":"edit"})).unwrap();
            let built = build_request(&Input {
                connection: &c,
                request: &r,
                model: &"opaque".into(),
                operation: "edit",
                references: std::slice::from_ref(image),
                mask: Some(image),
            })
            .unwrap();
            api::request(
                &c,
                &HashMap::new(),
                api::Operation {
                    operation: &built.operation,
                    prefix: built.prefix,
                    method: Method::POST,
                    body: built.body,
                    query: None,
                },
                &CancellationToken::new(),
                1048576,
            )
            .await
            .unwrap();
            assert_eq!(requests.lock().unwrap().len(), 1);
        },
    )
    .await;
}
