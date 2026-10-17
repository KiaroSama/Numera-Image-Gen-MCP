mod common;
#[path = "common/normalized.rs"]
mod normalized_fixture;
use common::Workspace;
use normalized_fixture::normalized;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    adapters::{build_request, normalize::normalize, types::Input},
    capabilities::effective_request,
    error::NumeraError,
    files::images::{decode_base64, inspect},
    http::api::Body,
    jobs::{Receipt, Store},
};
use std::{collections::HashSet, path::Path};

const ADAPTERS: [&str; 6] = [
    "openai-images",
    "openrouter-images",
    "openai-responses",
    "gemini",
    "gemini-interactions",
    "chat-images",
];

fn oracle() -> Value {
    let file =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/normalization-acceptance.json");
    let bytes = std::fs::read(file).expect("exact frozen normalization oracle required");
    assert!(bytes.len() <= 4 * 1024 * 1024, "bounded oracle bundle");
    Value::parse_with_limit(std::str::from_utf8(&bytes).unwrap(), 4 * 1024 * 1024).unwrap()
}

fn error(error: NumeraError) -> Value {
    json!({"error": {"code": error.code, "message": error.message}})
}

#[test]
fn six_exact_persian_prompt_only_wires_match_source_body_and_distinct_protocol_fields() {
    let oracle = oracle();
    let cases = oracle["wires"].as_array().unwrap();
    assert_eq!(cases.len(), 6);
    let mut names = HashSet::new();
    for case in cases {
        let adapter = case["name"].as_str().unwrap();
        assert!(ADAPTERS.contains(&adapter));
        assert!(names.insert(adapter));
        assert_eq!(case["request"]["prompt"], "تصویر 123");
        assert_eq!(case["request"]["model"], "family/image");
        assert_eq!(case["connection"]["orchestrationModel"], "chat-model");
        let built = build_request(&Input {
            connection: &case["connection"],
            request: &case["request"],
            model: case["model"].as_js_string().unwrap(),
            operation: "generate",
            references: &[],
            mask: None,
        })
        .unwrap();
        let Body::Json(body) = built.body else {
            panic!("exact source prompt-only request must be JSON")
        };
        let (operation, field) = match adapter {
            "openai-images" => ("images/generations", "model"),
            "gemini" => ("models/family%2Fimage:generateContent", "contents"),
            "gemini-interactions" => ("interactions", "input"),
            "openai-responses" => ("responses", "tools"),
            "openrouter-images" => ("images", "input_references"),
            "chat-images" => ("chat/completions", "messages"),
            _ => unreachable!(),
        };
        assert_eq!(built.operation, operation);
        assert_eq!(case["required_operation"], operation);
        assert!(body.get(field).is_some());
        let serialized = body.to_string();
        assert!(serialized.contains("تصویر 123"));
        assert_eq!(body, case["expected"]["value"]["body"]);
        assert_eq!(
            built.prefix,
            case["expected"]["value"]["prefix"].as_str().unwrap()
        );
        assert_eq!(
            built.operation,
            case["expected"]["value"]["operation"].as_str().unwrap()
        );
        let source_body =
            Value::parse(case["expected"]["value"]["serialized"].as_str().unwrap()).unwrap();
        assert_eq!(body, source_body);
    }
    assert_eq!(names, ADAPTERS.into_iter().collect::<HashSet<_>>());
}

#[test]
fn both_antigravity_gateway_url_references_reject_with_the_exact_source_error() {
    let oracle = oracle();
    let cases = oracle["gateways"].as_array().unwrap();
    assert_eq!(cases.len(), 2);
    let mut names = HashSet::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        assert!(names.insert(name));
        assert_eq!(case["model"], "antigravity/gemini-image");
        assert_eq!(
            case["request"]["reference_images"],
            json!([{"type": "url", "url": "https://public.example/x.png"}])
        );
        let rejected = effective_request(
            &case["connection"],
            &case["request"],
            case["model"].as_js_string().unwrap(),
        )
        .unwrap_err();
        assert_eq!(rejected.code, "unsupported_operation");
        assert!(rejected.message.contains("Reference"));
        assert_eq!(error(rejected), case["expected"]);
    }
    assert_eq!(
        names,
        ["omniroute", "9router"].into_iter().collect::<HashSet<_>>()
    );
}

#[test]
fn all_twenty_four_source_sibling_pairs_keep_two_exact_errors_and_the_original_middle_image() {
    let oracle = oracle();
    let cases = oracle["pairs"].as_array().unwrap();
    assert_eq!(cases.len(), 24);
    let bytes = decode_base64(oracle["original"].as_str().unwrap(), 1048576).unwrap();
    let image = inspect(bytes.clone(), &json!({}), None).unwrap();
    assert_eq!((image.width, image.height), (8, 6));
    assert_eq!(image.mime, "image/png");
    assert!(image.alpha);
    let expected_names: HashSet<String> = ADAPTERS
        .iter()
        .flat_map(|adapter| {
            ["null", "number", "string", "array"].map(|kind| format!("{adapter}-{kind}"))
        })
        .collect();
    let mut names = HashSet::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        assert!(names.insert(name.to_owned()));
        let adapter = case["connection"]["adapter"].as_str().unwrap();
        let raw = &case["raw"];
        let items = match adapter {
            "openai-images" | "openrouter-images" => &raw["data"],
            "openai-responses" => &raw["output"],
            "gemini" => &raw["candidates"][0]["content"]["parts"],
            "gemini-interactions" => &raw["steps"][0]["content"],
            "chat-images" => &raw["choices"][0]["message"]["images"],
            _ => panic!("unexpected source pair adapter"),
        };
        assert_eq!(items.as_array().unwrap().len(), 3);
        let invalid = match case["kind"].as_str().unwrap() {
            "null" => Value::Null,
            "number" => json!(42),
            "string" => json!("malformed"),
            "array" => json!([]),
            _ => panic!("unexpected source malformed sibling kind"),
        };
        assert_eq!(items[0], invalid);
        assert_eq!(items[2], invalid);
        let result = normalize(&case["connection"], raw).unwrap();
        let actual = normalized(result);
        assert_eq!(actual["images"].as_array().unwrap().len(), 3, "{name}");
        assert_eq!(
            actual["images"][0],
            json!({"error": "Malformed image result item."})
        );
        assert_eq!(
            actual["images"][2],
            json!({"error": "Malformed image result item."})
        );
        let middle = &actual["images"][1];
        assert!(middle.get("error").is_none());
        let encoded = if adapter == "chat-images" {
            middle["url"]
                .as_str()
                .unwrap()
                .strip_prefix("data:image/png;base64,")
                .unwrap()
        } else {
            middle["base64"].as_str().unwrap()
        };
        assert_eq!(decode_base64(encoded, 1048576).unwrap(), bytes, "{name}");
        assert_eq!(json!({"value": actual}), case["expected"], "{name}");
    }
    assert_eq!(names, expected_names);
}

#[test]
fn five_exact_source_envelopes_refusals_and_text_only_outputs_remain_fatal() {
    let oracle = oracle();
    let cases = oracle["fatal"].as_array().unwrap();
    assert_eq!(cases.len(), 5);
    let mut names = HashSet::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        assert!(names.insert(name));
        let rejected = normalize(&case["connection"], &case["raw"])
            .err()
            .expect("source fatal input cannot return an image");
        let code = match name {
            "null-envelope" => "invalid_response",
            "all-invalid-images" | "gemini-text-only" => "no_image_returned",
            "gemini-safety" | "chat-refusal" => "provider_rejection",
            _ => panic!("unexpected source fatal name"),
        };
        assert_eq!(rejected.code, code);
        if name == "gemini-text-only" {
            assert!(rejected.message.contains("final image"));
        }
        assert_eq!(error(rejected), case["expected"], "{name}");
    }
    assert_eq!(
        names,
        [
            "null-envelope",
            "all-invalid-images",
            "gemini-safety",
            "chat-refusal",
            "gemini-text-only"
        ]
        .into_iter()
        .collect::<HashSet<_>>()
    );
}

#[test]
fn exact_gemini_signatures_stay_in_private_sqlite_and_out_of_the_public_receipt() {
    let oracle = oracle();
    assert_eq!(oracle["continuation"].as_array().unwrap().len(), 2);
    let case = oracle["continuation"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "gemini-signatures")
        .unwrap();
    let actual = normalized(normalize(&case["connection"], &case["raw"]).unwrap());
    assert_eq!(actual["images"], json!([{"base64": "aW1hZ2U="}]));
    let continuation = json!({"signatures": [
        {"part_index": 0, "signature": "opaque-private"},
        {"part_index": 1, "signature": "final-private"}
    ]});
    assert_eq!(actual["continuation"], continuation);
    assert_eq!(json!({"value": actual}), case["expected"]);
    let workspace = Workspace::new();
    let mut store = Store::open(&workspace.0.join("state")).unwrap();
    let (fresh, receipt) = store
        .prepare(
            Receipt::new(json!({
                "request_id": "request-1", "connection": "local", "adapter": "openai-images",
                "operation": "generate", "status": "prepared", "requested_model": "image-model",
                "upstream_model": null, "outputs": [], "warnings": [], "errors": [], "deviations": [],
                "generation_outcome": "not_submitted", "storage_outcome": "not_started",
                "another_submission_may_charge": false, "created_at": "2026-10-02T00:00:00.000Z",
                "updated_at": "2026-10-02T00:00:00.000Z"
            })),
            "hash",
            "scope",
        )
        .unwrap();
    assert!(fresh);
    store
        .save_continuation("request-1", "scope", &continuation)
        .unwrap();
    let public = store.get("request-1").unwrap();
    assert!(public.value.get("continuation").is_none());
    assert!(!public.value.to_string().contains("opaque-private"));
    assert!(!public.value.to_string().contains("final-private"));
    assert_eq!(
        store.continuation("request-1", "scope").unwrap(),
        continuation
    );
    let mismatch = store.continuation("request-1", "different").unwrap_err();
    assert_eq!(mismatch.code, "permission_denied");
    assert!(mismatch.message.to_ascii_lowercase().contains("scope"));
    store.release("request-1", Some(&receipt)).unwrap();
}

#[test]
fn exact_interactions_hidden_image_summary_is_not_promoted_and_signature_is_separate() {
    let oracle = oracle();
    assert_eq!(oracle["continuation"].as_array().unwrap().len(), 2);
    let names: HashSet<_> = oracle["continuation"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| case["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["gemini-signatures", "interactions-signatures"]
            .into_iter()
            .collect::<HashSet<_>>()
    );
    let case = oracle["continuation"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "interactions-signatures")
        .unwrap();
    assert_eq!(
        case["raw"]["steps"][0]["summary"][0],
        json!({"type": "image", "data": "not-final"})
    );
    let actual = normalized(normalize(&case["connection"], &case["raw"]).unwrap());
    assert_eq!(actual["images"], json!([{"base64": "ZmluYWw="}]));
    assert_eq!(
        actual["continuation"],
        json!({"signatures": [{"step_type": "thought", "signature": "encrypted"}]})
    );
    assert_eq!(actual["upstreamId"], "interaction");
    assert_eq!(json!({"value": actual}), case["expected"]);
}
