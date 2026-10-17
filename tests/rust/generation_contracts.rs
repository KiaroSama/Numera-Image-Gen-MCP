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
use tokio_util::sync::CancellationToken;

// Public Generation seams from integration/image-editing.test.ts, terminal-lifecycle.test.ts,
// reservation-lifecycle.test.ts and contract/comfyui.test.ts. HTTP wire capture is a separate tier.
fn make_generation(workspace: &Workspace, raw: &Value) -> Generation {
    let config = Config::parse(
        &raw.to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    Generation {
        config: Arc::new(config),
        store: Arc::new(Mutex::new(Store::open(&workspace.0.join("state")).unwrap())),
        logger: Arc::new(Logger::new(&workspace.0.join("logs"), "ERROR")),
        runtime: RuntimePaths::new(&workspace.0).unwrap(),
        env: HashMap::new().into(),
        active: Arc::new(Mutex::new(HashMap::new())),
    }
}
fn edit_config(workspace: &Workspace, origin: &str) -> Value {
    let mut config = workspace.config(&format!("{origin}/v1"), "openai-images");
    config["connections"]["test"]["edit"] = json!({"mode":"json","encoding":"image","maxReferences":1,"masks":true,"maskPolarity":"transparent-edit"});
    config
}
fn alpha_mask() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/images.json");
    let cases: Vec<Value> =
        serde_json::from_slice(&std::fs::read(path).expect("source image oracle")).unwrap();
    let case = cases
        .iter()
        .find(|case| case["metadata"]["mime"] == "image/png" && case["metadata"]["alpha"] == true)
        .expect("source-generated alpha PNG mask fixture");
    STANDARD.decode(case["base64"].as_str().unwrap()).unwrap()
}
fn data_source(bytes: &[u8]) -> Value {
    json!({"type":"data_url","data_url":format!("data:image/png;base64,{}",STANDARD.encode(bytes))})
}
fn saved(receipt: &Receipt, original: &[u8]) {
    assert_eq!(receipt.value["outputs"].as_array().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(receipt.value["outputs"][0]["path"].as_str().unwrap()).unwrap(),
        original
    );
    assert_eq!(receipt.value["outputs"][0]["mime_type"], "image/png");
}
fn error<T>(result: numera_image_gen_mcp::error::Result<T>) -> String {
    result.err().expect("fixture must reject").code
}
fn initial(id: &str) -> Receipt {
    Receipt::new(
        json!({"request_id":id,"connection":"test","status":"prepared","outputs":[],"warnings":[],"errors":[],"deviations":[],"generation_outcome":"not_submitted"}),
    )
}

#[tokio::test]
async fn source_types_and_edits_keep_originals_and_never_replay_the_same_request() {
    let workspace = Workspace::new();
    let bytes = image_fixture::original();
    let encoded = STANDARD.encode(&bytes);
    let download = bytes.clone();
    let server = Server::new(move |path| Reply {
        status: 200,
        value: json!({"data":[{"b64_json":encoded}]}),
        bytes: path.starts_with("/owned/").then(|| download.clone()),
    })
    .await;
    let action = async {
        let mut raw = edit_config(&workspace, &server.origin);
        raw["files"]["assetAllowances"] = json!([{"origin":server.origin,"pathPrefix":"/owned"}]);
        let generation = make_generation(&workspace, &raw);
        let signal = CancellationToken::new();
        let root = workspace.0.join("inputs");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("café-日本語.png");
        std::fs::write(&path, &bytes).unwrap();
        let seed = generation
            .run(
                json!({"prompt":"seed original","request_id":"seed"}),
                "generate",
                &signal,
            )
            .await
            .unwrap();
        saved(&seed, &bytes);
        let output_id = seed.value["outputs"][0]["output_id"].as_str().unwrap();
        let mut bounded = raw.clone();
        bounded["files"]["maxInputBytes"] = json!(1);
        let limited = make_generation(&workspace, &bounded);
        assert_eq!(
            error(
                limited
                    .source(&json!({"type":"output_id","output_id":output_id}), &signal)
                    .await
            ),
            "invalid_input"
        );
        drop(limited);
        for (name, source) in [
            ("data", data_source(&bytes)),
            (
                "path",
                json!({"type":"path","path":path.to_str().expect("fixture path")}),
            ),
            ("owned", json!({"type":"output_id","output_id":output_id})),
            (
                "url",
                json!({"type":"url","url":format!("{}/owned/source.png",server.origin)}),
            ),
        ] {
            let loaded = generation.source(&source, &signal).await.unwrap();
            assert_eq!(loaded.bytes, bytes, "{name}");
            let request = json!({"prompt":"Preserve source artwork 日本語","request_id":format!("edit-{name}"),"reference_images":[source]});
            let first = generation
                .run(request.clone(), "edit", &signal)
                .await
                .unwrap();
            assert_eq!(first.status(), "completed", "{name}");
            saved(&first, &bytes);
            let repeated = generation.run(request, "edit", &signal).await.unwrap();
            assert_eq!(repeated.value["outputs"], first.value["outputs"], "{name}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|path| path.ends_with("images/edits"))
                .count(),
            4
        );
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|path| path.ends_with("images/generations"))
                .count(),
            1
        );
        assert_eq!(
            error(
                generation
                    .source(&json!({"type":"unsupported"}), &signal)
                    .await
            ),
            "invalid_input"
        );
        assert_eq!(
            error(
                generation
                    .source(
                        &json!({"type":"path","path":workspace.0.join("absent").to_str().expect("fixture path")}),
                        &signal
                    )
                    .await
            ),
            "input_file_error"
        );
        assert!(generation.active.lock().unwrap().is_empty());
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn explicit_masks_region_translation_and_upscale_report_native_results_not_resized_images() {
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
        let raw = edit_config(&workspace, &server.origin);
        let generation = make_generation(&workspace, &raw);
        let signal = CancellationToken::new();
        let reference = data_source(&bytes);
        let explicit_mask = data_source(&alpha_mask());
        let explicit = generation.run(json!({"prompt":"Preserve drawing","request_id":"explicit-mask","reference_images":[reference],"mask":explicit_mask}),"edit",&signal).await.unwrap();
        assert_eq!(explicit.status(), "completed");
        saved(&explicit, &bytes);
        let region = generation.run(json!({"prompt":"Preserve the comic panels 日本語","request_id":"translated-region","reference_images":[reference],
            "edit_region":{"x":2,"y":1,"width":3,"height":2},"target_language":"Persian"}),"edit",&signal).await.unwrap();
        assert_eq!(region.status(), "completed");
        saved(&region, &bytes);
        let upscaled = generation.run(json!({"prompt":"Preserve drawing","request_id":"upscale","reference_images":[reference],"upscale":true,"size":"32x32"}),"edit",&signal).await.unwrap();
        assert_eq!(
            upscaled.status(),
            "partial",
            "unchanged native dimensions must not claim enlargement"
        );
        saved(&upscaled, &bytes);
        let deviations = upscaled.value["deviations"].as_array().unwrap();
        assert!(deviations.contains(&json!("Output 0 dimensions differ from requested size.")));
        assert!(deviations.contains(&json!(
            "Output 0 was not upscaled beyond the reference dimensions."
        )));
        for request in [
            json!({"prompt":"fixture","reference_images":[reference],"upscale":true,"size":"1x1"}),
            json!({"prompt":"fixture","reference_images":[reference],"edit_region":{"x":7,"y":0,"width":2,"height":1}}),
            json!({"prompt":"fixture","reference_images":[reference],"mask":reference,"edit_region":{"x":0,"y":0,"width":1,"height":1}}),
            json!({"prompt":"fixture","filename_prefix":"folder/name","reference_images":[reference]}),
            json!({"prompt":"fixture","output_subdirectory":"../outside","reference_images":[reference]}),
        ] {
            assert_eq!(
                error(generation.run(request, "edit", &signal).await),
                "invalid_input"
            );
        }
        assert_eq!(
            server.requests.lock().unwrap().len(),
            3,
            "rejected requirements must not submit"
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn input_reference_mask_pixel_and_owned_byte_limits_reject_before_submission() {
    let bytes = image_fixture::original();
    let server = Server::new(|_| Reply {
        status: 200,
        value: json!({"data":[]}),
        bytes: None,
    })
    .await;
    let action = async {
        let reference = data_source(&bytes);
        let signal = CancellationToken::new();
        for mode in [
            "input",
            "aggregate",
            "mask-aggregate",
            "references",
            "pixels",
        ] {
            let workspace = Workspace::new();
            let mut raw = edit_config(&workspace, &server.origin);
            let mut request = json!({"prompt":"fixture","reference_images":[reference]});
            match mode {
                "input" => raw["files"]["maxInputBytes"] = json!(1),
                "aggregate" => raw["files"]["maxAggregateBytes"] = json!(1),
                "mask-aggregate" => {
                    raw["files"]["maxAggregateBytes"] = json!(bytes.len());
                    request["mask"] = reference.clone();
                }
                "references" => {
                    raw["files"]["maxReferences"] = json!(1);
                    raw["connections"]["test"]["edit"]["encoding"] = json!("images");
                    raw["connections"]["test"]["edit"]["maxReferences"] = json!(2);
                    request["reference_images"] = json!([reference, reference]);
                }
                "pixels" => raw["files"]["maxPixels"] = json!(1),
                _ => unreachable!(),
            }
            let generation = make_generation(&workspace, &raw);
            let expected = if mode == "input" {
                "invalid_response"
            } else {
                "invalid_input"
            };
            assert_eq!(
                error(generation.run(request, "edit", &signal).await),
                expected,
                "{mode}"
            );
            assert!(generation.active.lock().unwrap().is_empty());
        }
        assert!(server.requests.lock().unwrap().is_empty());
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn failed_and_ambiguous_submission_receipts_persist_and_never_trigger_paid_retry() {
    let workspace = Workspace::new();
    let server = Server::new(|path| Reply {
        status: if path.starts_with("/rejected/") {
            500
        } else {
            200
        },
        value: json!({"data":[]}),
        bytes: None,
    })
    .await;
    let action = async {
        for (mode, status, code) in [
            ("rejected", "failed", "upstream_error"),
            ("empty", "outcome_unknown", "no_image_returned"),
        ] {
            let case_workspace = Workspace::new();
            let raw =
                case_workspace.config(&format!("{}/{mode}/v1", server.origin), "openai-images");
            let generation = make_generation(&case_workspace, &raw);
            let signal = CancellationToken::new();
            let request = json!({"prompt":"fixture","request_id":mode});
            let first = generation
                .run(request.clone(), "generate", &signal)
                .await
                .unwrap();
            assert_eq!(first.status(), status);
            assert!(first.value["outputs"].as_array().unwrap().is_empty());
            assert_eq!(first.value["errors"][0]["code"], code);
            assert_eq!(first.value["generation_outcome"], "unknown");
            assert_eq!(first.value["another_submission_may_charge"], true);
            let repeated = generation.run(request, "generate", &signal).await.unwrap();
            assert_eq!(repeated.value["errors"], first.value["errors"]);
            assert!(generation.active.lock().unwrap().is_empty());
        }
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        let generation = make_generation(
            &workspace,
            &workspace.config(&format!("{}/v1", server.origin), "openai-images"),
        );
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = generation
            .run(
                json!({"prompt":"fixture","request_id":"pre-cancelled"}),
                "generate",
                &cancel,
            )
            .await
            .unwrap();
        assert_eq!(result.status(), "outcome_unknown");
        assert_eq!(result.value["generation_outcome"], "unknown");
        assert_eq!(result.value["errors"][0]["code"], "cancelled");
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        assert_eq!(
            error(
                generation
                    .run(
                        json!({"prompt":"fixture","wait":false}),
                        "generate",
                        &CancellationToken::new()
                    )
                    .await
            ),
            "unsupported_operation"
        );
        assert_eq!(
            error(
                generation
                    .run(
                        json!({"prompt":"fixture"}),
                        "edit",
                        &CancellationToken::new()
                    )
                    .await
            ),
            "invalid_input"
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

#[tokio::test]
async fn bounded_local_queue_and_missing_credentials_create_no_provider_submission() {
    let workspace = Workspace::new();
    let server = Server::new(|_| Reply {
        status: 200,
        value: json!({}),
        bytes: None,
    })
    .await;
    let action = async {
        let mut raw = workspace.config(&format!("{}/v1", server.origin), "openai-images");
        raw["maxQueuedRequests"] = json!(1);
        let generation = make_generation(&workspace, &raw);
        generation
            .store
            .lock()
            .unwrap()
            .prepare(initial("blocked"), "fixture", "fixture")
            .unwrap();
        let result = generation
            .run(
                json!({"prompt":"fixture","request_id":"queue-refusal"}),
                "generate",
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(result.status(), "failed");
        assert_eq!(result.value["errors"][0]["code"], "rate_limited");
        assert_eq!(result.value["generation_outcome"], "not_submitted");
        assert!(server.requests.lock().unwrap().is_empty());
        assert!(generation.active.lock().unwrap().is_empty());
        drop(generation);
        let credentials_workspace = Workspace::new();
        let mut raw =
            credentials_workspace.config(&format!("{}/v1", server.origin), "openai-images");
        raw["connections"]["test"]["auth"] =
            json!({"type":"bearer","secretEnv":"MISSING_FIXTURE_KEY"});
        let generation = make_generation(&credentials_workspace, &raw);
        assert_eq!(
            error(
                generation
                    .run(
                        json!({"prompt":"fixture","request_id":"no-key"}),
                        "generate",
                        &CancellationToken::new()
                    )
                    .await
            ),
            "missing_credentials"
        );
        assert_eq!(
            error(generation.store.lock().unwrap().get("no-key")),
            "invalid_input"
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}

async fn responses_and_interactions_poll_one_existing_handle_and_persist_terminal_results(
    adapter: &str,
    mode: &str,
) {
    let bytes = image_fixture::original();
    let encoded = STANDARD.encode(&bytes);
    let server = Server::new(move |path| {
        let interactions = path.contains("interactions");let terminal = path.contains("failed") || path.contains("incomplete");
        let id = if path.contains("wrong") && path.ends_with("/job") {"another"} else {"job"};
        let status = if path.ends_with("/job") {if path.contains("failed"){"failed"}else if path.contains("incomplete"){"incomplete"}else{"completed"}} else {"in_progress"};
        let image = if interactions {json!({"steps":[{"type":"model_output","content":[{"type":"image","data":encoded}]}]})}
            else {json!({"output":[{"type":"image_generation_call","status":"completed","result":encoded}]})};
        let mut value = json!({"id":id,"status":status});
        if status == "completed" || (terminal && status == "incomplete") {value.as_object_mut().unwrap().extend(image.as_object().unwrap().clone());}
        Reply {status:200,value,bytes:None}
    }).await;
    let signal = CancellationToken::new();
    let action = async {
        let workspace = Workspace::new();
        let raw = workspace.config(&format!("{}/{mode}/v1", server.origin), adapter);
        let generation = make_generation(&workspace, &raw);
        let request = json!({"prompt":"fixture","request_id":format!("{adapter}-{mode}")});
        let result = generation
            .run(request.clone(), "generate", &signal)
            .await
            .unwrap();
        let status = match mode {
            "complete" => "completed",
            "failed" => "failed",
            "incomplete" => "partial",
            "wrong" => "running",
            _ => unreachable!(),
        };
        assert_eq!(result.status(), status, "{adapter}-{mode}");
        assert_eq!(result.value["upstream_job"]["id"], "job");
        if matches!(mode, "complete" | "incomplete") {
            saved(&result, &bytes);
        }
        if mode == "wrong" {
            assert_eq!(result.value["errors"][0]["code"], "invalid_response");
        }
        let repeat = generation.run(request, "generate", &signal).await.unwrap();
        assert_eq!(repeat.value["outputs"], result.value["outputs"]);
        assert_eq!(
            server.requests.lock().unwrap().len(),
            2,
            "one POST then one GET, no new submission"
        );
        assert!(generation.active.lock().unwrap().is_empty());
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    signal.cancel();
    server.close().await;
    result.unwrap_or_else(|_| panic!("{adapter}-{mode} exceeded its polling deadline"));
}

macro_rules! polling_case {
    ($name:ident, $adapter:literal, $mode:literal) => {
        #[tokio::test]
        async fn $name() {
            responses_and_interactions_poll_one_existing_handle_and_persist_terminal_results(
                $adapter, $mode,
            )
            .await;
        }
    };
}

polling_case!(responses_completed, "openai-responses", "complete");
polling_case!(responses_failed, "openai-responses", "failed");
polling_case!(responses_incomplete, "openai-responses", "incomplete");
polling_case!(responses_wrong_handle, "openai-responses", "wrong");
polling_case!(interactions_completed, "gemini-interactions", "complete");
polling_case!(interactions_failed, "gemini-interactions", "failed");
polling_case!(interactions_incomplete, "gemini-interactions", "incomplete");
polling_case!(interactions_wrong_handle, "gemini-interactions", "wrong");

#[tokio::test]
async fn comfy_wait_false_retains_one_handle_and_get_job_recovers_original_bytes() {
    let workspace = Workspace::new();
    let bytes = image_fixture::original();
    let download = bytes.clone();
    let server = Server::new(move |path| {
        let value = if path == "/object_info" {json!({"Text":{"input":{"required":{"text":["STRING"]}}},"Save":{"input":{"required":{"text":["STRING"]}},"output_node":true}})}
            else if path == "/system_stats" {json!({"devices":[{"type":"cuda"}]})}
            else if path == "/prompt" {json!({"prompt_id":"comfy-owned","node_errors":{}})}
            else if path == "/history/comfy-owned" {json!({"comfy-owned":{"status":{"completed":true,"status_str":"success"},"outputs":{"save":{"images":[{"filename":"original.png","subfolder":"","type":"output"}]}}}})}
            else {json!({})};
        Reply {status:200,value,bytes:path.starts_with("/view?").then(||download.clone())}
    }).await;
    let action = async {
        let mut raw = workspace.config(&server.origin, "comfyui");
        raw["connections"]["test"]["baseUrlMode"] = json!("origin");
        raw["connections"]["test"]["workflow"] = json!({"graph":{"text":{"class_type":"Text","inputs":{"text":""}},"save":{"class_type":"Save","inputs":{"text":"output"}}},
            "bindings":{"prompt":{"node":"text","input":"text"}},"outputNodes":["save"],"requireGpu":true});
        let generation = make_generation(&workspace, &raw);
        let signal = CancellationToken::new();
        let request = json!({"prompt":"fixture","request_id":"comfy-async","wait":false});
        let first = generation
            .run(request.clone(), "generate", &signal)
            .await
            .unwrap();
        assert_eq!(first.status(), "running");
        assert_eq!(
            first.value["upstream_job"],
            json!({"id":"comfy-owned","kind":"comfyui"})
        );
        let repeated = generation.run(request, "generate", &signal).await.unwrap();
        assert_eq!(repeated.value["upstream_job"], first.value["upstream_job"]);
        let recovered = jobs::get(&generation, "comfy-async", true, &signal, None)
            .await
            .unwrap();
        assert_eq!(recovered.status(), "completed");
        saved(&recovered, &bytes);
        let again = jobs::get(&generation, "comfy-async", true, &signal, None)
            .await
            .unwrap();
        assert_eq!(again.value["outputs"], recovered.value["outputs"]);
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|path| path.as_str() == "/prompt")
                .count(),
            1
        );
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|path| path.starts_with("/view?"))
                .count(),
            1
        );
        assert!(generation.active.lock().unwrap().is_empty());
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}
