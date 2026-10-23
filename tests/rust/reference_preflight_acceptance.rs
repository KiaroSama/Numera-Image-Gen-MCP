mod common;
#[path = "common/multipart.rs"]
mod multipart_fixture;
#[path = "common/wire.rs"]
mod wire;
use common::Workspace;
use multipart_fixture::multipart;
use numera_image_gen_mcp::{
    adapters::{
        build_request, comfy,
        types::{Input, mime_data},
        workflow,
    },
    capabilities,
    compat::value::{JsString, Value, json},
    config::Config,
    files::images::{Image, decode_base64, inspect},
    http::api::{self, Body},
    services::request,
};
use reqwest::Method;
use std::{collections::HashMap, path::Path, time::Duration};
use tokio_util::sync::CancellationToken;

fn configuration(root: &Path, connections: Value) -> Config {
    let default = connections
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let config = Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":default,
            "outputDir":root.join("outputs").to_str().unwrap(),
            "stateDir":root.join("state").to_str().unwrap(),
            "logging":{"directory":root.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[root.join("inputs").to_str().unwrap()]},
            "connections":connections
        })
        .to_string(),
        &root.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    assert_eq!(config.value["defaultConnection"], "local");
    config
}

fn parsed_connection(root: &Path, raw: Value) -> Value {
    let (_, connection) = configuration(root, json!({"local":raw}))
        .select(None)
        .unwrap();
    assert_eq!(connection["enabled"], true);
    assert_eq!(connection["baseUrlMode"], "api-prefix");
    assert_eq!(connection["auth"], json!({"type":"none"}));
    assert_eq!(connection["requestTimeoutMs"], 300000);
    assert_eq!(connection["discoveryTimeoutMs"], 15000);
    assert_eq!(connection["maxConcurrentRequests"], 2);
    assert!(connection.get("defaultModel").is_none());
    assert!(connection.get("orchestrationModel").is_none());
    connection
}

fn image_configuration(root: &Path) -> Config {
    configuration(
        root,
        json!({"local":{
            "adapter":"openai-images","baseUrl":"https://api.example/v1","auth":{"type":"none"}
        }}),
    )
}

fn comfy_connection(root: &Path, origin: &str) -> Value {
    parsed_connection(
        root,
        json!({
            "adapter":"comfyui","baseUrl":origin,"auth":{"type":"none"},
            "workflow":{
                "graph":{"1":{"class_type":"Fixture","inputs":{"prompt":"","image":"","image2":"","mask":""}}},
                "bindings":{
                    "prompt":{"node":"1","input":"prompt"},
                    "image":{"node":"1","input":"image"},
                    "image_2":{"node":"1","input":"image2"}
                },"outputNodes":["1"],"requireGpu":false
            }
        }),
    )
}

fn original(config: &Config, name: &str) -> Image {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/image-acceptance.json");
    assert!(std::fs::metadata(&file).unwrap().len() <= 1048576);
    let text = std::fs::read_to_string(file).unwrap();
    let rows = Value::parse_with_limit(&text, 1048576).unwrap();
    // Opaque JPEG uses the source's three-channel recipe, not the alpha=true JPEG row.
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("Missing exact source image oracle row: {name}"));
    let bytes = decode_base64(row["base64"].as_str().unwrap(), 1048576).unwrap();
    let image = inspect(bytes.clone(), &config.value, None).unwrap();
    assert_eq!((image.width, image.height), (8, 6));
    assert_eq!(
        image.mime,
        if name == "png" {
            "image/png"
        } else {
            "image/jpeg"
        }
    );
    assert_eq!(image.alpha, name == "png");
    assert_eq!(image.bytes, bytes);
    image
}

fn fixture_request() -> Value {
    let value = request::parse(json!({"prompt":"fixture"})).unwrap();
    assert_eq!(
        value,
        json!({"prompt":"fixture","count":1,"provider_options":{},"reference_images":[],"wait":true})
    );
    value
}

async fn invalid_case(mode: &str) {
    let workspace = Workspace::new();
    let root = &workspace.0;
    wire::serve(
        |_| wire::json_reply(json!({"name":"fixture.png","subfolder":"","prompt_id":"owned"})),
        |origin, requests| async move {
            let config = image_configuration(root);
            let references = [original(&config, "png"), original(&config, "opaque-jpeg")];
            let mut connection = comfy_connection(root, &origin);
            let request = fixture_request();
            let model: JsString = "fixture".into();
            let prepared = if mode == "changed-after-preflight" {
                Some(
                    workflow::prepare(&Input {
                        connection: &connection,
                        request: &request,
                        model: &model,
                        operation: "edit",
                        references: &references,
                        mask: None,
                    })
                    .unwrap(),
                )
            } else {
                None
            };
            match mode {
                "second" | "changed-after-preflight" => {
                    connection["workflow"]["bindings"]
                        .as_object_mut()
                        .unwrap()
                        .remove("image_2");
                }
                "mask" => {}
                "node" => {
                    connection["workflow"]["bindings"]["image_2"] =
                        json!({"node":"missing","input":"image"});
                }
                "slot" => {
                    connection["workflow"]["bindings"]["image_2"] =
                        json!({"node":"1","input":"imagge"});
                }
                "collision" => {
                    connection["workflow"]["bindings"]["image_2"] =
                        connection["workflow"]["bindings"]["image"].clone();
                }
                _ => panic!("Unknown frozen reference variant"),
            }
            let input = Input {
                connection: &connection,
                request: &request,
                model: &model,
                operation: "edit",
                references: &references,
                mask: (mode == "mask").then_some(&references[0]),
            };
            let signal = CancellationToken::new();
            let result = tokio::time::timeout(Duration::from_secs(2), async {
                let graph = match prepared {
                    Some(graph) => Ok(graph),
                    None => workflow::prepare(&input),
                }?;
                comfy::submit(&input, graph, &HashMap::new(), &signal).await
            })
            .await
            .expect("invalid reference action exceeded source two-second bound");
            assert!(result.is_err(), "{mode} must reject");
            assert!(
                requests.lock().unwrap().is_empty(),
                "{mode} must precede every HTTP request"
            );
        },
    )
    .await;
}

#[tokio::test]
async fn exact_source_second_binding_refuses_before_upload() {
    invalid_case("second").await;
}

#[tokio::test]
async fn exact_source_mask_binding_refuses_before_upload() {
    invalid_case("mask").await;
}

#[tokio::test]
async fn exact_source_missing_node_refuses_before_upload() {
    invalid_case("node").await;
}

#[tokio::test]
async fn exact_source_misspelled_slot_refuses_before_upload() {
    invalid_case("slot").await;
}

#[tokio::test]
async fn exact_source_colliding_targets_refuse_before_upload() {
    invalid_case("collision").await;
}

#[tokio::test]
async fn exact_source_changed_after_preflight_revalidates_before_upload() {
    invalid_case("changed-after-preflight").await;
}

#[tokio::test]
async fn exact_source_unmasked_two_distinct_references_keep_bytes_targets_and_direct_submit_order()
{
    let workspace = Workspace::new();
    let root = &workspace.0;
    let mut uploads = 0;
    wire::serve(
        |request| {
            if request.path == "/upload/image" {
                uploads += 1;
                wire::json_reply(json!({"name":format!("reference-{uploads}"),"subfolder":"owned"}))
            } else {
                wire::json_reply(json!({"prompt_id":"owned","node_errors":{}}))
            }
        },
        |origin, requests| async move {
            let config = image_configuration(root);
            let references = [original(&config, "png"), original(&config, "opaque-jpeg")];
            let connection = comfy_connection(root, &origin);
            let request = fixture_request();
            let model: JsString = "fixture".into();
            let input = Input {
                connection: &connection,
                request: &request,
                model: &model,
                operation: "edit",
                references: &references,
                mask: None,
            };
            let signal = CancellationToken::new();
            let (job, _) = tokio::time::timeout(Duration::from_secs(2), async {
                comfy::submit(&input, workflow::prepare(&input).unwrap(), &HashMap::new(), &signal).await
            })
            .await
            .expect("accepted reference action exceeded source two-second bound")
            .unwrap();
            assert_eq!(job.id, "owned");
            assert_ne!(references[0].sha256, references[1].sha256);
            let captured = requests.lock().unwrap();
            let paths = captured.iter().map(|item| (item.method.as_str(), item.path.as_str())).collect::<Vec<_>>();
            assert_eq!(paths, [("POST", "/upload/image"), ("POST", "/upload/image"), ("POST", "/prompt")]);
            for (wire, image) in captured[..2].iter().zip(&references) {
                assert!(wire.body.windows(image.bytes.len()).any(|bytes| bytes == image.bytes));
                let parts = multipart(wire);
                assert_eq!(parts.len(), 2);
                assert_eq!(parts[0].name, "image");
                assert!(parts[0].filename.unwrap().ends_with(&format!(".{}", image.extension)));
                assert_eq!(parts[0].mime, Some(image.mime.as_str()));
                assert_eq!(parts[0].bytes, image.bytes);
                assert_eq!(parts[1].name, "type");
                assert_eq!(parts[1].bytes, b"input");
            }
            let body = Value::parse(std::str::from_utf8(&captured[2].body).unwrap()).unwrap();
            assert!(uuid::Uuid::parse_str(body["client_id"].as_str().unwrap()).is_ok());
            assert_eq!(body["prompt"]["1"]["inputs"], json!({
                "prompt":"fixture","image":"owned/reference-1","image2":"owned/reference-2","mask":""
            }));
        },
    )
    .await;
}

fn gateway_inputs(root: &Path, gateway: &str) -> (Value, Value, Image) {
    let config = image_configuration(root);
    let image = original(&config, "png");
    let connection = parsed_connection(
        root,
        json!({
            "adapter":"openai-images","baseUrl":"https://api.example/v1","auth":{"type":"none"},"gateway":gateway
        }),
    );
    let request = request::parse(json!({
        "prompt":"fixture","reference_images":[{"type":"data_url","data_url":mime_data(&image)}]
    }))
    .unwrap();
    (connection, request, image)
}

#[tokio::test]
async fn exact_source_omniroute_aliases_preserve_full_multipart_model_and_refuse_paid_fanout() {
    let workspace = Workspace::new();
    let (mut connection, request, image) = gateway_inputs(&workspace.0, "omniroute");
    wire::serve(
        |_| wire::json_reply(json!({})),
        |origin, requests| async move {
            let references = [image];
            for prefix in ["codex", "cx"] {
                let model: JsString = format!("{prefix}/fixture").into();
                assert_eq!(
                    capabilities::capabilities(&connection, &model)["gateway_forwarding"]["image_to_image"],
                    "supported"
                );
                let effective = capabilities::effective_request(&connection, &request, &model).unwrap();
                let built = build_request(&Input {
                    connection: &connection,
                    request: &effective,
                    model: &model,
                    operation: "edit",
                    references: &references,
                    mask: None,
                })
                .unwrap();
                assert!(matches!(&built.body, Body::Multipart(_)));
                assert_eq!(built.operation, "images/edits");
                // Redirect only serialization transport after building the unchanged source profile.
                let mut transport = connection.clone();
                transport["baseUrl"] = json!(format!("{origin}/v1"));
                api::json(
                    &transport,
                    &HashMap::new(),
                    api::Operation {
                        operation: &built.operation,
                        prefix: built.prefix,
                        method: Method::POST,
                        body: built.body,
                        query: None,
                    },
                    &CancellationToken::new(),
                )
                .await
                .unwrap();
                let captured = requests.lock().unwrap();
                let parts = multipart(captured.last().unwrap());
                assert_eq!(parts.len(), 4);
                for (name, expected) in [
                    ("model", model.as_str().unwrap()),
                    ("prompt", "fixture"),
                    ("n", "1"),
                ] {
                    let values = parts.iter().filter(|part| part.name == name).collect::<Vec<_>>();
                    assert_eq!(values.len(), 1);
                    assert!(values[0].filename.is_none());
                    assert_eq!(values[0].bytes, expected.as_bytes());
                }
                let files = parts.iter().filter(|part| part.name == "image").collect::<Vec<_>>();
                assert_eq!(files.len(), 1);
                assert_eq!(files[0].mime, Some("image/png"));
                assert!(files[0].filename.is_some());
                assert_eq!(files[0].bytes, references[0].bytes);
            }
            assert_eq!(
                capabilities::capabilities(&connection, &"cxx/fixture".into())["gateway_forwarding"]["image_to_image"],
                "unknown"
            );
            for prefix in ["codex", "cx"] {
                let model: JsString = format!("{prefix}/fixture").into();
                connection["modelOverrides"][&model] = json!({"defaults":{},"capabilities":{},"maxCount":2});
                let batch = request::parse(json!({"prompt":"fixture","count":2})).unwrap();
                let error = capabilities::effective_request(&connection, &batch, &model).unwrap_err();
                assert!(error.message.contains("fans out paid requests"));
            }
            let captured = requests.lock().unwrap();
            let paths = captured.iter().map(|item| (item.method.as_str(), item.path.as_str())).collect::<Vec<_>>();
            assert_eq!(paths, [("POST", "/v1/images/edits"), ("POST", "/v1/images/edits")]);
        },
    )
    .await;
}

#[test]
fn exact_source_nine_router_aliases_keep_json_model_ids_and_negative_prefix_unknown() {
    let workspace = Workspace::new();
    let (connection, request, image) = gateway_inputs(&workspace.0, "9router");
    let references = [image];
    for prefix in ["codex", "cx"] {
        let model: JsString = format!("{prefix}/fixture").into();
        assert_eq!(
            capabilities::capabilities(&connection, &model)["gateway_forwarding"]["image_to_image"],
            "supported"
        );
        let effective = capabilities::effective_request(&connection, &request, &model).unwrap();
        let built = build_request(&Input {
            connection: &connection,
            request: &effective,
            model: &model,
            operation: "edit",
            references: &references,
            mask: None,
        })
        .unwrap();
        let Body::Json(body) = built.body else {
            panic!("9router must retain JSON reference forwarding");
        };
        assert_eq!(body["model"], json!(model));
        assert_eq!(body["prompt"], "fixture");
        assert_eq!(body["image"], mime_data(&references[0]));
    }
    assert_eq!(
        capabilities::capabilities(&connection, &"cxx/fixture".into())["gateway_forwarding"]["image_to_image"],
        "unknown"
    );
}
