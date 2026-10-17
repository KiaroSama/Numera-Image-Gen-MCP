mod common;
#[path = "common/multipart.rs"]
mod multipart_fixture;
#[path = "common/comfy_source.rs"]
mod source;
#[path = "common/wire.rs"]
mod wire;
use common::Workspace;
use numera_image_gen_mcp::{
    adapters::{comfy, workflow},
    capabilities,
    compat::value::{JsString, Value, json},
    config::Config,
    files::images::{Image, decode_base64, inspect},
    services::request,
};
use source::{input, workflow as configured_workflow};
use std::{collections::HashMap, time::Duration};
use tokio_util::sync::CancellationToken;
use wire::{Reply, json_reply as reply, serve};

fn config(workspace: &Workspace, origin: &str, configured: Value) -> Config {
    source::config(workspace, origin, json!({"workflow":configured}))
}

fn original() -> Image {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".ci-work/parity/image-acceptance.json");
    let rows = Value::parse_with_limit(&std::fs::read_to_string(path).unwrap(), 1048576).unwrap();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "png")
        .unwrap();
    let bytes = decode_base64(row["base64"].as_str().unwrap(), 1048576).unwrap();
    let image = inspect(bytes, &json!({}), None).unwrap();
    assert_eq!((image.width, image.height), (8, 6));
    assert_eq!(image.mime, "image/png");
    image
}

fn installed() -> Value {
    json!({"CLIPTextEncode":{},"KSampler":{},"LoadImage":{},"SaveImage":{}})
}

#[tokio::test]
async fn exact_source_full_comfy_edit_preserves_three_uploads_and_only_bound_final_originals() {
    let workspace = Workspace::new();
    let image = original();
    let output = image.bytes.clone();
    let uploads = std::cell::Cell::new(0);
    serve(
        |request| {
            if request.path == "/object_info" {
                reply(installed())
            } else if request.path == "/system_stats" {
                reply(json!({"devices":[{"type":"cuda"}]}))
            } else if request.path == "/upload/image" {
                uploads.set(uploads.get() + 1);
                reply(json!({"name":format!("upload-{}.png",uploads.get()),"subfolder":"fixtures"}))
            } else if request.path == "/prompt" {
                reply(json!({"prompt_id":"owned-job"}))
            } else if request.path == "/history/owned-job" {
                reply(json!({"owned-job":{
                    "status":{"completed":true,"status_str":"success"},
                    "outputs":{
                        "save":{"images":[{"filename":"final.png","subfolder":"batch","type":"output"},
                            {"filename":"ignored-input.png","type":"input"},{"filename":"temp.png","type":"temp"}]},
                        "unbound":{"images":[{"filename":"unowned.png","type":"output"}]}
                    }
                }}))
            } else if request.path.starts_with("/view?") {
                Reply { status: 200, mime: "image/png", body: output.clone() }
            } else {
                Reply { status: 404, ..reply(json!({})) }
            }
        },
        |origin, requests| async move {
            let config = config(&workspace, &origin, configured_workflow());
            let (_, connection) = config.select(None).unwrap();
            let request = request::parse(json!({"prompt":"Draw a café","seed":123})).unwrap();
            let references = [image.clone(), image.clone()];
            let adapter = input(&connection, &request, &references, Some(&image), "edit");
            let signal = CancellationToken::new();
            let prepared = comfy::prepare(&adapter, &HashMap::new(), &signal).await.unwrap();
            let (job, warnings) = comfy::submit(&adapter, prepared, &HashMap::new(), &signal)
                .await
                .unwrap();
            assert_eq!(job.id, "owned-job");
            assert_eq!(job.kind, "comfyui");
            assert!(warnings.is_empty());
            let result = comfy::status(
                &connection,
                &HashMap::new(),
                &job.id,
                &signal,
                comfy::OutputLimits { item: 100 * 1024 * 1024, aggregate: 100 * 1024 * 1024 },
                |_, _| Ok(()),
                || Ok(()),
            )
            .await
            .unwrap();
            assert!(result.terminal.is_none());
            let result = result.result.unwrap();
            assert_eq!(result.images.len(), 2);
            for item in &result.images {
                assert_eq!(item.bytes.as_ref().unwrap(), &image.bytes);
                assert!(item.base64.is_none() && item.url.is_none() && item.error.is_none());
            }
            assert!(result.upstream_model.is_none());
            assert_eq!(result.upstream_id.as_ref().unwrap(), "owned-job");
            assert!(result.warnings.is_empty());
            let requests = requests.lock().unwrap();
            let uploads = requests.iter().filter(|r| r.path == "/upload/image").collect::<Vec<_>>();
            assert_eq!(uploads.len(), 3);
            for upload in uploads {
                assert!(upload.body.windows(image.bytes.len()).any(|window| window == image.bytes));
                assert!(upload.body.windows(b"Content-Type: image/png".len())
                    .any(|window| window == b"Content-Type: image/png"));
                assert!(upload.headers.to_ascii_lowercase().contains("multipart/form-data"));
                let parts = multipart_fixture::multipart(upload);
                let files = parts.iter().filter(|part| part.name == "image").collect::<Vec<_>>();
                assert_eq!(files.len(), 1);
                assert!(files[0].filename.is_some());
                assert_eq!(files[0].mime, Some("image/png"));
                assert_eq!(files[0].bytes, image.bytes);
            }
            let submitted = requests.iter().find(|r| r.path == "/prompt").unwrap();
            let submitted = Value::parse_with_limit(
                std::str::from_utf8(&submitted.body).unwrap(), 1048576,
            ).unwrap();
            assert!(submitted["client_id"].is_string());
            assert_eq!(submitted["prompt"]["text"]["inputs"]["text"], "Draw a café");
            assert_eq!(submitted["prompt"]["sampler"]["inputs"]["seed"], 123);
            assert_eq!(submitted["prompt"]["load"]["inputs"]["image"], "fixtures/upload-1.png");
            assert_eq!(submitted["prompt"]["load"]["inputs"]["mask"], "fixtures/upload-3.png");
            assert_eq!(submitted["prompt"]["second"]["inputs"]["image"], "fixtures/upload-2.png");
            assert_eq!(connection["workflow"]["graph"]["load"]["inputs"]["image"], "original.png");
            let queries = requests.iter().filter_map(|r| r.path.strip_prefix("/view")).collect::<Vec<_>>();
            assert_eq!(queries, [
                "?filename=final.png&subfolder=batch&type=output",
                "?filename=temp.png&subfolder=&type=temp"
            ]);
            let paths = requests.iter()
                .map(|r| format!("{} {}", r.method, r.path.split('?').next().unwrap()))
                .collect::<Vec<_>>();
            assert_eq!(paths, [
                "GET /object_info", "GET /system_stats", "POST /upload/image",
                "POST /upload/image", "POST /upload/image", "POST /prompt",
                "GET /history/owned-job", "GET /view", "GET /view"
            ]);
        },
    )
    .await;
}

async fn unavailable(scenario: &'static str) {
    let workspace = Workspace::new();
    serve(
        |request| {
            if request.path == "/object_info" {
                reply(if scenario == "missing-node" { json!({}) } else { installed() })
            } else {
                reply(json!({"devices":if scenario == "no-devices" {json!([])} else {json!([{"type":"cpu"}])}}))
            }
        },
        |origin, requests| async move {
            let config = config(&workspace, &origin, configured_workflow());
            let (_, connection) = config.select(None).unwrap();
            let request = request::parse(json!({"prompt":"Draw a café"})).unwrap();
            let error = tokio::time::timeout(
                Duration::from_secs(5),
                comfy::prepare(
                    &input(&connection, &request, &[], None, "generate"),
                    &HashMap::new(),
                    &CancellationToken::new(),
                ),
            )
            .await
            .unwrap()
            .unwrap_err();
            assert_eq!(error.code, "unsupported_operation");
            assert!(!requests.lock().unwrap().iter()
                .any(|r| r.path.contains("upload") || r.path.contains("prompt")));
        },
    )
    .await;
}

#[tokio::test]
async fn exact_source_missing_node_refuses_before_any_upload_or_prompt() {
    unavailable("missing-node").await;
}

#[tokio::test]
async fn exact_source_cpu_refuses_before_any_upload_or_prompt() {
    unavailable("cpu").await;
}

#[tokio::test]
async fn exact_source_no_devices_refuses_before_any_upload_or_prompt() {
    unavailable("no-devices").await;
}

#[tokio::test]
async fn exact_source_cpu_optout_only_requests_installed_nodes() {
    let workspace = Workspace::new();
    serve(
        |_| reply(installed()),
        |origin, requests| async move {
            let mut configured = configured_workflow();
            configured["requireGpu"] = json!(false);
            let config = config(&workspace, &origin, configured);
            let (_, connection) = config.select(None).unwrap();
            let request = request::parse(json!({"prompt":"Draw a café"})).unwrap();
            comfy::prepare(
                &input(&connection, &request, &[], None, "generate"),
                &HashMap::new(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
            let paths = requests
                .lock()
                .unwrap()
                .iter()
                .map(|r| r.path.clone())
                .collect::<Vec<_>>();
            assert_eq!(paths, ["/object_info"]);
        },
    )
    .await;
}

#[tokio::test]
async fn exact_source_missing_image_and_mask_bindings_refuse_both_calls() {
    let workspace = Workspace::new();
    let mut configured = configured_workflow();
    let bindings = configured["bindings"].as_object_mut().unwrap();
    bindings.remove("image");
    bindings.remove("mask");
    let config = config(&workspace, "https://fixture.example", configured);
    let (_, connection) = config.select(None).unwrap();
    let request = request::parse(json!({"prompt":"Draw a café"})).unwrap();
    let image = original();
    let references = [image.clone()];
    assert_eq!(
        workflow::prepare(&input(&connection, &request, &references, None, "generate"))
            .unwrap_err()
            .code,
        "unsupported_operation"
    );
    assert_eq!(
        workflow::prepare(&input(&connection, &request, &[], Some(&image), "generate"))
            .unwrap_err()
            .code,
        "unsupported_operation"
    );
}

#[tokio::test]
async fn exact_source_bad_reference_identity_then_empty_submission_never_retries() {
    let workspace = Workspace::new();
    serve(
        |request| {
            reply(if request.path == "/upload/image" {
                json!({"name":42,"subfolder":""})
            } else {
                json!({})
            })
        },
        |origin, requests| async move {
            let config = config(&workspace, &origin, configured_workflow());
            let (_, connection) = config.select(None).unwrap();
            let request = request::parse(json!({"prompt":"Draw a café"})).unwrap();
            let image = original();
            let references = [image];
            let signal = CancellationToken::new();
            let adapter = input(&connection, &request, &references, None, "generate");
            let error = comfy::submit(
                &adapter,
                workflow::prepare(&adapter).unwrap(),
                &HashMap::new(),
                &signal,
            )
            .await
            .err()
            .unwrap();
            assert_eq!(
                (error.code.as_str(), error.stage.as_str()),
                ("invalid_response", "upload")
            );
            assert_eq!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r.path == "/upload/image")
                    .count(),
                1
            );
            assert_eq!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r.path == "/prompt")
                    .count(),
                0
            );
            let adapter = input(&connection, &request, &[], None, "generate");
            let error = comfy::submit(
                &adapter,
                workflow::prepare(&adapter).unwrap(),
                &HashMap::new(),
                &signal,
            )
            .await
            .err()
            .unwrap();
            assert_eq!(
                (error.code.as_str(), error.stage.as_str()),
                ("outcome_unknown", "submission")
            );
            assert_eq!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r.path == "/prompt")
                    .count(),
                1
            );
        },
    )
    .await;
}

#[tokio::test]
async fn exact_source_empty_mask_upload_identity_never_submits_sentinel_prompt() {
    let workspace = Workspace::new();
    serve(
        |request| {
            reply(if request.path == "/upload/image" {
                json!({})
            } else {
                json!({"prompt_id":"must-not-submit"})
            })
        },
        |origin, requests| async move {
            let config = config(&workspace, &origin, configured_workflow());
            let (_, connection) = config.select(None).unwrap();
            let request = request::parse(json!({"prompt":"Draw a café"})).unwrap();
            let image = original();
            let adapter = input(&connection, &request, &[], Some(&image), "generate");
            let error = comfy::submit(
                &adapter,
                workflow::prepare(&adapter).unwrap(),
                &HashMap::new(),
                &CancellationToken::new(),
            )
            .await
            .err()
            .unwrap();
            assert_eq!(
                (error.code.as_str(), error.stage.as_str()),
                ("invalid_response", "upload")
            );
            assert_eq!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r.path == "/prompt")
                    .count(),
                0
            );
        },
    )
    .await;
}

#[tokio::test]
async fn exact_source_partial_branch_warning_keeps_only_bad_node_and_mask_evidence() {
    let workspace = Workspace::new();
    serve(
        |_| reply(json!({"prompt_id":"accepted","node_errors":{"bad":{"errors":["private detail"]}}})),
        |origin, _| async move {
            let config = source::config(&workspace, &origin, json!({
                "workflow":{
                    "graph":{"text":{"class_type":"Text","inputs":{"text":""}}},
                    "bindings":{"prompt":{"node":"text","input":"text"},
                        "image":{"node":"text","input":"image"},"mask":{"node":"text","input":"mask"}},
                    "outputNodes":["text"]
                },
                "requestTimeoutMs":300000,"discoveryTimeoutMs":15000,
                "edit":{"mode":"json","encoding":"image","masks":true,
                    "maskPolarity":"transparent-edit"}
            }));
            let (_, connection) = config.select(None).unwrap();
            let request = request::parse(json!({"prompt":"draw"})).unwrap();
            let adapter = input(&connection, &request, &[], None, "generate");
            let (job, warnings) = comfy::submit(
                &adapter,
                workflow::prepare(&adapter).unwrap(),
                &HashMap::new(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(job.id, "accepted");
            assert_eq!(warnings, vec![JsString::from(
                "ComfyUI accepted only valid workflow branches; rejected node IDs: bad"
            )]);
            assert!(!warnings.iter().any(|warning| warning.to_string().contains("private detail")));
            let model = "workflow".into();
            assert_eq!(
                capabilities::capabilities(&connection, &model)["gateway_forwarding"]["masks"],
                "supported"
            );
            let effective = capabilities::effective_request(
                &connection,
                &request::parse(json!({
                    "prompt":"edit","reference_images":[{"type":"path","path":"/input.png"}],
                    "mask":{"type":"path","path":"/mask.png"}
                })).unwrap(),
                &model,
            ).unwrap();
            assert!(effective.get("mask").is_some());
        },
    )
    .await;
}

async fn history_case(state: &str) {
    let workspace = Workspace::new();
    serve(
        |_| {
            let status = match state {
                "running" => json!({"completed":false}),
                "error" => json!({"status_str":"error"}),
                _ => json!({"completed":true}),
            };
            reply(if state == "absent" {
                json!({})
            } else {
                json!({"job":{"status":status,"outputs":{"save":{"images":[
                    {"filename":"not-output.png","type":"input"}
                ]}}}})
            })
        },
        |origin, requests| async move {
            let config = config(&workspace, &origin, configured_workflow());
            let (_, connection) = config.select(None).unwrap();
            let signal = CancellationToken::new();
            let result = tokio::time::timeout(
                Duration::from_secs(5),
                comfy::status(
                    &connection,
                    &HashMap::new(),
                    &"job".into(),
                    &signal,
                    comfy::OutputLimits {
                        item: 100 * 1024 * 1024,
                        aggregate: 100 * 1024 * 1024,
                    },
                    |_, _| Ok(()),
                    || Ok(()),
                ),
            )
            .await
            .unwrap();
            match state {
                "empty" => assert_eq!(result.err().unwrap().code, "no_image_returned"),
                "error" => {
                    let result = result.unwrap();
                    assert!(result.result.is_none());
                    assert_eq!(result.terminal, Some("failed"));
                }
                _ => {
                    let result = result.unwrap();
                    assert!(result.result.is_none());
                    assert!(result.terminal.is_none());
                }
            }
            assert_eq!(
                requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r.path.starts_with("/view"))
                    .count(),
                0
            );
        },
    )
    .await;
}

#[tokio::test]
async fn exact_source_absent_history_fetches_no_unrelated_image() {
    history_case("absent").await;
}

#[tokio::test]
async fn exact_source_running_input_only_history_fetches_no_unrelated_image() {
    history_case("running").await;
}

#[tokio::test]
async fn exact_source_error_input_only_history_retains_terminal_flag_without_view() {
    history_case("error").await;
}

#[tokio::test]
async fn exact_source_empty_input_only_history_returns_no_image_without_view() {
    history_case("empty").await;
}
