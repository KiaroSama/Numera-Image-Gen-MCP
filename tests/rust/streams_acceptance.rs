#[path = "common/normalized.rs"]
mod normalized_fixture;
use normalized_fixture::normalized;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    adapters::{
        build_request,
        normalize::{normalize, response},
        types::Input,
    },
    error::Result,
    files::images::{Image, decode_base64, inspect},
    http::api::Body,
};
use std::{collections::HashSet, path::Path};

// Exact frozen tests/contract/streams.test.ts literals, not the broad 7x5 stream oracle.
// The parent-owned incremental Reader and actual multipart capture remain separate seams.
fn oracle() -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/stream-acceptance.json");
    let bytes = std::fs::read(file).expect("exact frozen stream acceptance oracle required");
    assert!(
        bytes.len() <= 4 * 1024 * 1024,
        "bounded stream oracle bundle"
    );
    Value::parse_with_limit(std::str::from_utf8(&bytes).unwrap(), 4 * 1024 * 1024).unwrap()
}

fn outcome(result: Result<Value>) -> Value {
    match result {
        Ok(value) => json!({"value": value}),
        Err(error) => json!({"error": error.code}),
    }
}

fn group<'a>(oracle: &'a Value, name: &str, count: usize) -> &'a [Value] {
    let cases = oracle[name]
        .as_array()
        .expect("required exact source group");
    assert_eq!(cases.len(), count, "{name}");
    let names: HashSet<_> = cases
        .iter()
        .map(|case| {
            let name = case["name"].as_str().expect("named exact source case");
            assert!(!name.is_empty());
            name
        })
        .collect();
    assert_eq!(names.len(), count, "{name} names must be unique");
    cases
}

fn original(oracle: &Value) -> Image {
    let bytes = decode_base64(oracle["original"].as_str().unwrap(), 1048576).unwrap();
    let image = inspect(bytes.clone(), &json!({}), None).unwrap();
    assert_eq!((image.width, image.height), (8, 6));
    assert_eq!(image.mime, "image/png");
    assert!(image.alpha);
    assert_eq!(image.bytes, bytes);
    image
}

#[test]
fn exact_source_normalization_preserves_metadata_siblings_jobs_and_all_terminal_statuses() {
    let oracle = oracle();
    let image = original(&oracle);
    let mut jobs = HashSet::new();
    let mut terminal = HashSet::new();
    let mut empty = HashSet::new();
    let mut text = HashSet::new();
    for case in group(&oracle, "normalization", 37) {
        let name = case["name"].as_str().unwrap();
        let adapter = case["connection"]["adapter"].as_str().unwrap();
        let raw = &case["raw"];
        let actual = outcome(normalize(&case["connection"], raw).map(normalized));
        assert_eq!(actual, case["expected"], "{name}");
        if name.starts_with("images-final-") {
            let result = &actual["value"];
            assert_eq!(raw["data"][0]["b64_json"], oracle["original"]);
            assert_eq!(result["upstreamModel"], "actual-model");
            assert_eq!(result["upstreamId"], "owned-upstream");
            assert_eq!(result["usage"], json!({"images": 2}));
            assert_eq!(result["images"].as_array().unwrap().len(), 3);
            assert_eq!(
                result["images"][1]["url"],
                "https://fixture.example/output.png"
            );
            assert!(result["images"][2]["error"].is_string());
            assert_eq!(result["warnings"], json!([]));
            assert_eq!(
                decode_base64(result["images"][0]["base64"].as_str().unwrap(), 1048576).unwrap(),
                image.bytes
            );
        } else if name.starts_with("async-") {
            let status = raw["status"].as_str().unwrap();
            assert!(jobs.insert((adapter, status)));
            assert_eq!(raw["id"], "job-123");
            assert_eq!(raw["model"], "actual");
            assert_eq!(actual["value"]["images"], json!([]));
            assert_eq!(
                actual["value"]["job"],
                json!({
                    "id": "job-123",
                    "kind": if adapter == "gemini-interactions" { "interactions" } else { "responses" }
                })
            );
        } else if name.starts_with("terminal-") {
            let status = raw["status"].as_str().unwrap();
            assert!(terminal.insert((adapter, status)));
            assert_eq!(raw["id"], "job-123");
            assert_eq!(actual["value"]["terminal"], status);
            assert_eq!(actual["value"]["upstreamId"], "job-123");
            assert_eq!(actual["value"]["images"], json!([]));
        } else if name.starts_with("empty-") {
            assert!(empty.insert(adapter));
            assert_eq!(*raw, json!({}));
            assert_eq!(actual["error"], "no_image_returned");
        } else if name.starts_with("text-only-") {
            assert!(text.insert(adapter));
            assert_eq!(raw["data"], json!([{"b64_json": "", "url": ""}]));
            assert_eq!(
                raw["candidates"][0]["content"]["parts"],
                json!([{"text": "description"}])
            );
            assert_eq!(raw["output"], json!([{"type": "message"}]));
            assert_eq!(raw["choices"][0]["message"]["content"], "text");
            assert_eq!(actual["error"], "no_image_returned");
        }
    }
    assert_eq!(jobs.len(), 4);
    assert_eq!(terminal.len(), 6);
    assert_eq!(empty.len(), 6);
    assert_eq!(text.len(), 6);
}

#[test]
fn exact_source_json_binary_and_sse_results_require_finals_and_never_promote_partial_frames() {
    let oracle = oracle();
    let image = original(&oracle);
    let mut binary = HashSet::new();
    let mut suffixes = HashSet::new();
    for case in group(&oracle, "responses", 19) {
        let name = case["name"].as_str().unwrap();
        let bytes = if let Some(encoded) = case["base64"].as_str() {
            decode_base64(encoded, 1048576).unwrap()
        } else {
            case["text"].as_str().unwrap().as_bytes().to_vec()
        };
        let actual = response(
            &case["connection"],
            bytes.clone(),
            case["mime"].as_str().unwrap(),
        );
        if name.starts_with("binary-") {
            let format = name.strip_prefix("binary-").unwrap();
            assert!(binary.insert(format));
            let result = actual
                .as_ref()
                .expect("binary source output must be preserved");
            assert_eq!(result.images.len(), 1);
            assert_eq!(result.images[0].bytes.as_ref().unwrap(), &bytes);
            assert!(result.upstream_model.is_none());
            assert!(result.warnings.is_empty());
            let inspected = inspect(bytes.clone(), &json!({}), None).unwrap();
            assert_eq!((inspected.width, inspected.height), (8, 6));
            assert_eq!(inspected.mime, format!("image/{format}"));
            assert_eq!(case["mime"], format!("image/{format}; charset=binary"));
            if format == "png" {
                assert_eq!(bytes, image.bytes);
            }
        }
        let actual = outcome(actual.map(normalized));
        assert_eq!(actual, case["expected"], "{name}");
        if name.starts_with("responses-after-") {
            let kind = name.strip_prefix("responses-after-").unwrap();
            assert!(suffixes.insert(kind));
            assert_eq!(actual["error"], "outcome_unknown");
            let frames = numera_image_gen_mcp::http::sse::events(
                case["text"].as_str().unwrap().as_bytes(),
                1048576,
            )
            .unwrap();
            assert_eq!(frames.len(), 2);
            assert_eq!(frames[0].kind, "response.completed");
            assert_eq!(frames[1].kind, kind);
            assert!(frames[0].data["response"].get("id").is_none());
            assert_eq!(
                frames[0].data["response"]["output"][0]["result"],
                oracle["original"]
            );
            assert_eq!(frames[1].data, json!({"error": "fixture"}));
        }
    }
    assert_eq!(
        binary,
        ["png", "jpeg", "webp"].into_iter().collect::<HashSet<_>>()
    );
    assert_eq!(
        suffixes,
        ["error", "response.failed", "response.incomplete"]
            .into_iter()
            .collect::<HashSet<_>>()
    );
}

#[test]
fn exact_source_json_edits_settings_and_all_unsupported_options_and_masks_match() {
    let oracle = oracle();
    let image = original(&oracle);
    let mut successes = 0;
    let mut errors = 0;
    let mut masks = 0;
    for case in group(&oracle, "wires", 30) {
        let name = case["name"].as_str().unwrap();
        assert_eq!(case["request"]["prompt"], "A café");
        assert_eq!(case["request"]["count"], 1);
        assert!(case["request"]["provider_options"].is_object());
        let references = vec![image.clone(); case["reference_count"].as_u64().unwrap() as usize];
        let mask = (case["mask"] == true).then_some(&image);
        let built = build_request(&Input {
            connection: &case["connection"],
            request: &case["request"],
            model: case["model"].as_js_string().unwrap(),
            operation: case["operation"].as_str().unwrap(),
            references: &references,
            mask,
        });
        if name == "omniroute-clamped-tier" {
            let Err(error) = &built else {
                panic!("unsupported tier must fail");
            };
            assert!(error.message.contains("1K, 2K or 4K"));
        }
        let actual = outcome(built.map(|prepared| {
            let Body::Json(body) = prepared.body else {
                panic!("pure source JSON case {name} unexpectedly built multipart")
            };
            json!({"operation": prepared.operation, "prefix": prepared.prefix, "body": body})
        }));
        assert_eq!(actual, case["expected"], "{name}");
        if case["expected"].get("error").is_some() {
            errors += 1;
            if name.starts_with("mask-") {
                masks += 1;
                assert_eq!(case["reference_count"], 1);
                assert_eq!(case["mask"], true);
                assert_eq!(case["connection"]["orchestrationModel"], "text");
                assert_eq!(case["connection"]["chatImageOutput"], true);
                assert_eq!(actual["error"], "unsupported_operation");
            }
        } else {
            successes += 1;
            let body = &actual["value"]["body"];
            if name.starts_with("json-reference-") {
                let encoding = name.strip_prefix("json-reference-").unwrap();
                assert_eq!(case["reference_count"], 1);
                assert_eq!(case["mask"], true);
                assert!(
                    body[encoding]
                        .to_string()
                        .contains(oracle["original"].as_str().unwrap())
                );
                assert_eq!(
                    body["mask"],
                    format!(
                        "data:image/png;base64,{}",
                        oracle["original"].as_str().unwrap()
                    )
                );
                assert_eq!(actual["value"]["operation"], "images/edits");
            } else if name == "9router-two-references" {
                let url = format!(
                    "data:image/png;base64,{}",
                    oracle["original"].as_str().unwrap()
                );
                assert_eq!(case["reference_count"], 2);
                assert_eq!(body["images"], json!([url, url]));
            } else if name == "openrouter-fallback" {
                assert_eq!(body["seed"], 42);
                assert_eq!(body["resolution"], "2K");
                assert_eq!(
                    body["provider"],
                    json!({"allow_fallbacks": false, "order": ["fixture-provider"]})
                );
                assert!(
                    body["input_references"]
                        .to_string()
                        .contains(oracle["original"].as_str().unwrap())
                );
            }
        }
    }
    assert_eq!((successes, errors, masks), (12, 18, 5));
    // Actual wire and original-file assertions run in the separate multipart target.
    let multipart = group(&oracle, "multipart", 2);
    assert_eq!(multipart[0]["name"], "multipart-image-array-mask");
    assert_eq!(multipart[0]["reference_count"], 2);
    assert_eq!(multipart[0]["mask"], true);
    assert_eq!(multipart[1]["name"], "omniroute-codex-multipart");
}
