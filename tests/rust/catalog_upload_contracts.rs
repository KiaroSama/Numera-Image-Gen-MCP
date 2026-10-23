mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use numera_image_gen_mcp::{
    adapters::{comfy, types::Input, workflow},
    capabilities,
    compat::value::{Value, json},
    config::Config,
    files::images,
    runtime_paths::RuntimePaths,
    services::discovery::Discovery,
};
use std::{
    collections::HashMap,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Request {
    path: String,
    headers: String,
    body: Vec<u8>,
}

async fn serve<F, Fut>(reply: impl Fn(&Request) -> Value, action: F)
where
    F: FnOnce(String, Arc<Mutex<Vec<Request>>>) -> Fut,
    Fut: Future<Output = ()>,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let stop = CancellationToken::new();
    let server_stop = stop.clone();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let server = async {
        loop {
            let (mut socket, _) = tokio::select! {
                _ = server_stop.cancelled() => break,
                accepted = listener.accept() => accepted.unwrap(),
            };
            let operation = async {
                let mut bytes = Vec::new();
                let mut buffer = [0u8; 4096];
                let (boundary, length) = loop {
                    let read = socket.read(&mut buffer).await.unwrap();
                    assert!(read > 0, "request must finish");
                    bytes.extend_from_slice(&buffer[..read]);
                    assert!(bytes.len() <= 1048576, "bounded multipart fixture");
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
                            break (boundary, length);
                        }
                    }
                };
                let headers = String::from_utf8(bytes[..boundary].to_vec()).unwrap();
                let path = headers
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                let request = Request {
                    path,
                    headers,
                    body: bytes[boundary + 4..boundary + 4 + length].to_vec(),
                };
                let value = reply(&request);
                captured.lock().unwrap().push(request);
                let body = value.to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            };
            tokio::time::timeout(Duration::from_secs(3), operation)
                .await
                .unwrap();
        }
    };
    let client = async {
        let result = tokio::time::timeout(Duration::from_secs(8), action(origin, requests)).await;
        stop.cancel();
        result.expect("bounded contract action");
    };
    tokio::time::timeout(Duration::from_secs(12), async {
        tokio::join!(server, client)
    })
    .await
    .expect("owned server/action lifetime");
}

fn config(workspace: &Workspace, raw: Value) -> Config {
    Config::parse(
        &raw.to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap()
}

fn ids(value: &Value) -> Vec<&str> {
    value["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn omniroute_specialty_catalog_is_supplemented_by_paginated_image_only_deduplication() {
    let workspace = Workspace::new();
    serve(|request| {
        if request.path == "/v1/images/generations" { json!({"data":[{"id":"specialty"}]}) }
        else if !request.path.contains("after=") {
            json!({"data":[{"id":"specialty","type":"image"},{"id":"text-model","type":"text"},{"id":"unified-1","type":"image"}],"has_more":true,"last_id":"cursor-1"})
        } else { json!({"data":[{"id":"unified-2","type":"image"}]}) }
    }, |origin, requests| async move {
        let mut raw = workspace.config(&format!("{origin}/v1"), "openai-images");
        raw["connections"]["test"]["gateway"] = json!("omniroute");
        let config = config(&workspace, raw);
        let runtime = RuntimePaths::new(&workspace.0).unwrap();
        let result = Discovery::default().list(&config, &runtime, &HashMap::new(), None, false, &CancellationToken::new()).await.unwrap();
        assert_eq!(ids(&result), ["specialty", "unified-1", "unified-2"]);
        assert_eq!(result["complete"], true);
        assert_eq!(result["warnings"].as_array().unwrap().len(), 1);
        let requests = requests.lock().unwrap();
        let paths: Vec<_> = requests.iter().map(|request| request.path.as_str()).collect();
        assert_eq!(paths, ["/v1/images/generations", "/v1/models?limit=100", "/v1/models?after=cursor-1&limit=100"]);
        assert!(requests.iter().all(|request| request.headers.starts_with("GET ") && request.body.is_empty()));
    }).await;
}

#[tokio::test]
async fn gemini_page_tokens_do_not_mutate_configuration_or_leak_into_fresh_refresh() {
    let workspace = Workspace::new();
    serve(
        |request| {
            if request.path.contains("pageToken=") {
                json!({"models":[{"name":"second"}]})
            } else {
                json!({"models":[{"name":"first"}],"nextPageToken":"token-2"})
            }
        },
        |origin, requests| async move {
            let mut raw = workspace.config(&format!("{origin}/v1beta"), "gemini");
            raw["connections"]["test"]["query"] = json!({"scope":"images"});
            let config = config(&workspace, raw);
            let runtime = RuntimePaths::new(&workspace.0).unwrap();
            let mut discovery = Discovery::default();
            for refresh in [false, true] {
                let result = discovery
                    .list(
                        &config,
                        &runtime,
                        &HashMap::new(),
                        None,
                        refresh,
                        &CancellationToken::new(),
                    )
                    .await
                    .unwrap();
                assert_eq!(ids(&result), ["first", "second"]);
            }
            assert_eq!(
                config.value["connections"]["test"]["query"],
                json!({"scope":"images"})
            );
            let requests = requests.lock().unwrap();
            let paths: Vec<_> = requests
                .iter()
                .map(|request| request.path.as_str())
                .collect();
            assert_eq!(
                paths,
                [
                    "/v1beta/models?scope=images",
                    "/v1beta/models?scope=images&pageToken=token-2",
                    "/v1beta/models?scope=images",
                    "/v1beta/models?scope=images&pageToken=token-2"
                ]
            );
        },
    )
    .await;
}

#[tokio::test]
async fn refreshed_catalog_retains_configured_names_native_ids_and_cached_metadata() {
    let workspace = Workspace::new();
    serve(|_| json!({"data":[{"id":"custom/another","type":"image"},{"id":"upstream/new","type":"image"}]}),
        |origin, requests| async move {
        let raw = json!({"api_endpoint":format!("{origin}/v1"),"api_key":"synthetic-local-key-not-real","models":[{"id":"antigravity/opaque-image","name":"Image 日本語"},{"id":"custom/another","name":"Second"}]});
        let config = config(&workspace, raw);
        let runtime = RuntimePaths::new(&workspace.0).unwrap();
        let mut discovery = Discovery::default();
        let refreshed = discovery.list(&config, &runtime, &HashMap::new(), None, true, &CancellationToken::new()).await.unwrap();
        assert_eq!(ids(&refreshed), ["antigravity/opaque-image", "custom/another", "upstream/new"]);
        assert_eq!(refreshed["models"][1]["name"], "Second");
        assert_eq!(refreshed["models"][1]["metadata"]["advertised"]["id"], "custom/another");
        let model = Discovery::model(&config, &runtime, &HashMap::new(), None, &"custom/another".into(), &CancellationToken::new()).await.unwrap();
        assert_eq!(model["model_id"], "custom/another");
        assert_eq!(model["display_name"], "Second");
        assert_eq!(model["account_availability"], "unknown");
        assert_eq!(discovery.list(&config, &runtime, &HashMap::new(), None, false, &CancellationToken::new()).await.unwrap()["cached"], true);
        assert_eq!(requests.lock().unwrap().len(), 1, "cached metadata cannot trigger extra discovery");
    }).await;
}

fn comfy_config(origin: &str) -> Value {
    json!({"adapter":"comfyui","baseUrl":origin,"baseUrlMode":"origin","requestTimeoutMs":3000,
        "workflow":{"graph":{"text":{"class_type":"Text","inputs":{"text":"","image":"","mask":""}}},
        "bindings":{"prompt":{"node":"text","input":"text"},"image":{"node":"text","input":"image"},"mask":{"node":"text","input":"mask"}},
        "outputNodes":["text"],"requireGpu":false},
        "edit":{"mode":"json","encoding":"image","maxReferences":1,"masks":true,"maskPolarity":"transparent-edit"}})
}

#[tokio::test]
async fn malformed_image_and_mask_upload_identity_prevent_prompt_submission_and_unknown_ids_never_retry()
 {
    let image = images::inspect(image_fixture::original(), &json!({}), None).unwrap();
    serve(
        |request| {
            if request.path == "/upload/image" {
                json!({"name":42,"subfolder":""})
            } else {
                json!({})
            }
        },
        |origin, requests| async move {
            let connection = comfy_config(&origin);
            let request = json!({"prompt":"draw","count":1,"provider_options":{}});
            let model = "workflow".into();
            for mask in [false, true] {
                let references = if mask {
                    Vec::new()
                } else {
                    vec![image.clone()]
                };
                let input = Input {
                    connection: &connection,
                    request: &request,
                    model: &model,
                    operation: "generate",
                    references: &references,
                    mask: mask.then_some(&image),
                };
                let error = comfy::submit(
                    &input,
                    workflow::prepare(&input).unwrap(),
                    &HashMap::new(),
                    &CancellationToken::new(),
                )
                .await
                .err()
                .unwrap();
                assert_eq!(error.code, "invalid_response");
                assert_eq!(error.stage, "upload");
                assert!(
                    requests
                        .lock()
                        .unwrap()
                        .iter()
                        .all(|request| request.path == "/upload/image")
                );
            }
            let input = Input {
                connection: &connection,
                request: &request,
                model: &model,
                operation: "generate",
                references: &[],
                mask: None,
            };
            let error = comfy::submit(
                &input,
                workflow::prepare(&input).unwrap(),
                &HashMap::new(),
                &CancellationToken::new(),
            )
            .await
            .err()
            .unwrap();
            assert_eq!(error.code, "outcome_unknown");
            assert_eq!(error.stage, "submission");
            let requests = requests.lock().unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request.path == "/upload/image")
                    .count(),
                2
            );
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request.path == "/prompt")
                    .count(),
                1
            );
            for request in requests
                .iter()
                .filter(|request| request.path == "/upload/image")
            {
                assert!(
                    request
                        .headers
                        .to_ascii_lowercase()
                        .contains("multipart/form-data")
                );
                assert!(
                    request
                        .body
                        .windows(image.bytes.len())
                        .any(|window| window == image.bytes)
                );
            }
        },
    )
    .await;
}

#[tokio::test]
async fn distinct_reference_and_mask_uploads_preserve_bytes_targets_and_gpu_preflight() {
    let first = images::inspect(image_fixture::original(), &json!({}), None).unwrap();
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/images.json");
    let fixtures: Vec<Value> = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    let jpeg = fixtures
        .iter()
        .find(|fixture| fixture["metadata"]["mime"] == "image/jpeg")
        .unwrap();
    let second = images::inspect(
        images::decode_base64(jpeg["base64"].as_str().unwrap(), 1048576).unwrap(),
        &json!({}),
        None,
    )
    .unwrap();
    assert_ne!(first.sha256, second.sha256);
    let uploads = Arc::new(AtomicUsize::new(0));
    let mode = Arc::new(AtomicUsize::new(0));
    let (uploaded, scenario) = (uploads.clone(), mode.clone());
    serve(
        move |request| match request.path.as_str() {
            "/object_info" if scenario.load(Ordering::SeqCst) == 1 => json!({}),
            "/object_info" => json!({"Text":{}}),
            "/system_stats" if scenario.load(Ordering::SeqCst) == 3 => json!({"devices":[]}),
            "/system_stats" => json!({"devices":[{"type":if scenario.load(Ordering::SeqCst)==2 {"cpu"}else{"cuda"}}]}),
            "/upload/image" => {
                let index = uploaded.fetch_add(1, Ordering::SeqCst) + 1;
                json!({"name":format!("reference-{index}"),"subfolder":"owned"})
            }
            "/prompt" => json!({"prompt_id":"owned","node_errors":{}}),
            _ => panic!("unexpected Comfy upload path"),
        },
        |origin, requests| async move {
            let mut connection = comfy_config(&origin);
            connection["workflow"]["graph"]["text"]["inputs"]["image2"] = json!("");
            connection["workflow"]["bindings"]["image_2"] = json!({"node":"text","input":"image2"});
            connection["workflow"]["requireGpu"] = json!(true);
            let before = connection.clone();
            let request = json!({"prompt":"Draw a café","count":1,"provider_options":{}});
            let model = "workflow".into();
            let references = [first.clone(), second.clone()];
            let input = Input {
                connection: &connection,
                request: &request,
                model: &model,
                operation: "edit",
                references: &references,
                mask: Some(&first),
            };
            let signal = CancellationToken::new();
            let graph = comfy::prepare(&input, &HashMap::new(), &signal)
                .await
                .unwrap();
            let (job, warnings) = comfy::submit(&input, graph, &HashMap::new(), &signal)
                .await
                .unwrap();
            assert_eq!(job.id, "owned");
            assert!(warnings.is_empty());
            assert_eq!(connection, before);
            {
                let captured = requests.lock().unwrap();
                let paths: Vec<_> = captured.iter().map(|item| item.path.as_str()).collect();
                assert_eq!(
                    paths,
                    [
                        "/object_info",
                        "/system_stats",
                        "/upload/image",
                        "/upload/image",
                        "/upload/image",
                        "/prompt"
                    ]
                );
                assert!(captured[..2].iter().all(|item| item.headers.starts_with("GET ")));
                let uploaded: Vec<_> = captured
                    .iter()
                    .filter(|item| item.path == "/upload/image")
                    .collect();
                for (upload, image) in uploaded.iter().zip([&first, &second, &first]) {
                    assert!(upload.headers.starts_with("POST "));
                    assert!(
                        upload
                            .body
                            .windows(image.bytes.len())
                            .any(|bytes| bytes == image.bytes)
                    );
                    let mime = format!("Content-Type: {}", image.mime);
                    assert!(
                        upload
                            .body
                            .windows(mime.len())
                            .any(|bytes| bytes.eq_ignore_ascii_case(mime.as_bytes()))
                    );
                }
                let prompt = captured.last().unwrap();
                assert!(prompt.headers.starts_with("POST "));
                let body = Value::parse(std::str::from_utf8(&prompt.body).unwrap()).unwrap();
                assert!(uuid::Uuid::parse_str(body["client_id"].as_str().unwrap()).is_ok());
                assert_eq!(body["prompt"]["text"]["inputs"], json!({"text":"Draw a café","image":"owned/reference-1","image2":"owned/reference-2","mask":"owned/reference-3"}));
            }
            for scenario in 1..=3 {
                let before = requests.lock().unwrap().len();
                mode.store(scenario, Ordering::SeqCst);
                let error = comfy::prepare(&input, &HashMap::new(), &signal)
                    .await
                    .unwrap_err();
                assert_eq!(error.code, "unsupported_operation");
                assert_eq!(uploads.load(Ordering::SeqCst), 3);
                let captured = requests.lock().unwrap();
                let paths: Vec<_> = captured[before..]
                    .iter()
                    .map(|item| item.path.as_str())
                    .collect();
                assert_eq!(
                    paths,
                    if scenario == 1 {
                        vec!["/object_info"]
                    } else {
                        vec!["/object_info", "/system_stats"]
                    }
                );
            }
            connection["workflow"]["requireGpu"] = json!(false);
            let input = Input {
                connection: &connection,
                request: &request,
                model: &model,
                operation: "edit",
                references: &references,
                mask: Some(&first),
            };
            let before = requests.lock().unwrap().len();
            comfy::prepare(&input, &HashMap::new(), &signal)
                .await
                .unwrap();
            assert_eq!(
                requests.lock().unwrap()[before..]
                    .iter()
                    .map(|item| item.path.as_str())
                    .collect::<Vec<_>>(),
                ["/object_info"]
            );
        },
    )
    .await;
}

#[tokio::test]
async fn invalid_reference_bindings_refuse_all_six_source_variants_before_any_upload() {
    let image = images::inspect(image_fixture::original(), &json!({}), None).unwrap();
    serve(
        |_| panic!("invalid reference bindings must not reach HTTP"),
        |origin, requests| async move {
            let request = json!({"prompt":"draw","count":1,"provider_options":{}});
            let model = "workflow".into();
            let references = [image.clone(), image.clone()];
            for mode in [
                "second",
                "mask",
                "node",
                "slot",
                "collision",
                "changed-after-preflight",
            ] {
                let mut connection = comfy_config(&origin);
                connection["workflow"]["graph"]["text"]["inputs"]["image2"] = json!("");
                connection["workflow"]["bindings"]["image_2"] =
                    json!({"node":"text","input":"image2"});
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
                    "mask" => {
                        connection["workflow"]["bindings"]
                            .as_object_mut()
                            .unwrap()
                            .remove("mask");
                    }
                    "node" => {
                        connection["workflow"]["bindings"]["image_2"]["node"] = json!("missing")
                    }
                    "slot" => {
                        connection["workflow"]["bindings"]["image_2"]["input"] = json!("imagge")
                    }
                    "collision" => {
                        connection["workflow"]["bindings"]["image_2"] =
                            connection["workflow"]["bindings"]["image"].clone()
                    }
                    _ => unreachable!(),
                }
                let input = Input {
                    connection: &connection,
                    request: &request,
                    model: &model,
                    operation: "edit",
                    references: &references,
                    mask: (mode == "mask").then_some(&image),
                };
                let result = match prepared
                    .map(Ok)
                    .unwrap_or_else(|| workflow::prepare(&input))
                {
                    Ok(graph) => {
                        comfy::submit(&input, graph, &HashMap::new(), &CancellationToken::new())
                            .await
                            .map(|_| ())
                    }
                    Err(error) => Err(error),
                };
                let error = result.unwrap_err();
                assert_eq!(
                    error.code,
                    if matches!(mode, "second" | "mask" | "changed-after-preflight") {
                        "unsupported_operation"
                    } else {
                        "invalid_configuration"
                    },
                    "{mode}"
                );
                assert!(requests.lock().unwrap().is_empty(), "{mode}");
            }
        },
    )
    .await;
}

#[tokio::test]
async fn accepted_partial_workflow_warns_only_rejected_node_ids_and_preserves_mask_evidence() {
    serve(|_| Value::parse(r#"{"prompt_id":"accepted","node_errors":{"bad":{"errors":["private detail"]},"\ud800":{"errors":["private detail"]}}}"#).unwrap(),
        |origin, requests| async move {
        let connection = comfy_config(&origin);
        let request = json!({"prompt":"draw","count":1,"provider_options":{}});
        let model = "workflow".into();
        let input = Input { connection:&connection, request:&request, model:&model, operation:"generate", references:&[], mask:None };
        let (job, warnings) = comfy::submit(&input, workflow::prepare(&input).unwrap(), &HashMap::new(), &CancellationToken::new()).await.unwrap();
        assert_eq!(job.id, "accepted");
        assert_eq!(warnings.len(), 1);
        let mut expected: Vec<u16> = "ComfyUI accepted only valid workflow branches; rejected node IDs: bad, "
            .encode_utf16().collect();
        expected.push(0xd800);
        assert_eq!(warnings[0].as_utf16(), expected);
        assert!(!warnings.iter().any(|warning| warning.to_string().contains("private detail")));
        assert_eq!(capabilities::capabilities(&connection, &model)["gateway_forwarding"]["masks"], "supported");
        let effective = capabilities::effective_request(&connection, &json!({"prompt":"edit","count":1,"provider_options":{},"reference_images":[{"type":"path","path":"/input.png"}],"mask":{"type":"path","path":"/mask.png"}}), &model).unwrap();
        assert!(effective.get("mask").is_some());
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let body = Value::parse(std::str::from_utf8(&requests[0].body).unwrap()).unwrap();
        assert_eq!(body["prompt"]["text"]["inputs"]["text"], "draw");
    }).await;
}
