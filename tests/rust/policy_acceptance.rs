mod common;
#[path = "common/config.rs"]
mod config_fixture;
use common::Workspace;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    capabilities::{capabilities, effective_request, safe_options},
    config::Config,
    services::request,
};
use std::collections::HashMap;

fn connection(adapter: &str, changes: Value) -> Value {
    let workspace = Workspace::new();
    let mut raw = workspace.config("https://api.example/v1", adapter);
    raw["connections"]["test"]
        .as_object_mut()
        .unwrap()
        .extend(changes.as_object().unwrap().clone());
    let config = Config::parse(
        &raw.to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    config.select(None).unwrap().1
}

fn input(changes: Value) -> Value {
    let mut raw = json!({"prompt": "Draw a café"});
    raw.as_object_mut()
        .unwrap()
        .extend(changes.as_object().unwrap().clone());
    request::parse(raw).unwrap()
}

#[test]
fn exact_profile_model_request_precedence_keeps_shared_defaults_and_extensions_immutable() {
    let profile = connection(
        "openai-images",
        json!({
            "defaults": {"quality": "low", "size": "small"},
            "modelOverrides": {"model": {"defaults": {"quality": "high"}, "maxCount": 3}},
            "extensions": {"strength": 0.2}, "providerOptionKeys": ["strength"]
        }),
    );
    let before = profile.clone();
    let result = effective_request(
        &profile,
        &input(json!({"size": "large", "count": 2, "provider_options": {"strength": 0.8}})),
        &"model".into(),
    )
    .unwrap();
    assert_eq!(result["size"], "large");
    assert_eq!(result["quality"], "high");
    assert_eq!(result["count"], 2);
    assert_eq!(result["provider_options"], json!({"strength": 0.8}));
    assert_eq!(
        profile["defaults"],
        json!({"quality": "low", "size": "small"})
    );
    assert_eq!(profile["extensions"], json!({"strength": 0.2}));
    assert_eq!(profile, before);
}

#[test]
fn all_six_source_reserved_option_names_reject_at_the_nested_boundary() {
    for key in [
        "model",
        "authorization",
        "api_key",
        "endpoint",
        "upload",
        "constructor",
    ] {
        let mut nested = json!({});
        nested[key] = json!("fixture");
        assert!(
            safe_options(&json!({"nested": nested}), None, 0).is_err(),
            "{key}"
        );
    }
}

#[test]
fn exact_scalar_allowlist_array_object_and_depth_limits_preserve_the_source_options_contract() {
    let value = json!({"strength": 0.5, "choices": [1, "two", null], "nested": {"enabled": false}});
    assert_eq!(
        safe_options(&value, Some(&json!(["strength", "choices", "nested"])), 0).unwrap(),
        value
    );
    assert!(safe_options(&json!({"strength": 0.5}), Some(&json!([])), 0).is_err());
    assert!(safe_options(&json!({"choices": [{"value": 1}]}), None, 0).is_err());
    assert!(safe_options(&json!({"choices": vec![1; 33]}), None, 0).is_err());
    let mut keys = json!({});
    for index in 0..51 {
        keys[&format!("k{index}")] = json!(index);
    }
    assert!(safe_options(&keys, None, 0).is_err());
    let mut nested = json!({"value": 1});
    for _ in 0..8 {
        nested = json!({"nested": nested});
    }
    assert!(safe_options(&nested, None, 0).is_err());
}

#[test]
fn exact_unconfigured_url_reference_and_mask_inputs_reject_without_forwarding() {
    let profile = connection("openai-images", json!({}));
    for changes in [
        json!({"reference_images": [{"type": "url", "url": "https://fixture.example/image.png"}]}),
        json!({"mask": {"type": "path", "path": "/mask.png"}}),
    ] {
        assert_eq!(
            effective_request(&profile, &input(changes), &"model".into())
                .unwrap_err()
                .code,
            "unsupported_operation"
        );
    }
}

#[test]
fn exact_model_allow_deny_reference_limit_and_native_mask_polarity_are_enforced() {
    for changes in [
        json!({"allowedModels": ["another"]}),
        json!({"deniedModels": ["model"]}),
    ] {
        assert_eq!(
            effective_request(
                &connection("gemini", changes),
                &input(json!({})),
                &"model".into()
            )
            .unwrap_err()
            .code,
            "model_unavailable"
        );
    }
    let mut profile = connection(
        "openai-images",
        json!({
            "edit": {"mode": "multipart", "encoding": "image", "masks": true, "maxReferences": 1}
        }),
    );
    let mask = input(json!({"mask": {"type": "path", "path": "/mask.png"}}));
    assert!(
        effective_request(&profile, &mask, &"model".into())
            .unwrap_err()
            .message
            .contains("polarity")
    );
    let references = input(json!({"reference_images": [
        {"type": "path", "path": "/a.png"}, {"type": "path", "path": "/b.png"}
    ]}));
    assert!(
        effective_request(&profile, &references, &"model".into())
            .unwrap_err()
            .message
            .contains("Too many")
    );
    profile["edit"]["maskPolarity"] = json!("transparent-edit");
    assert_eq!(
        effective_request(&profile, &mask, &"model".into()).unwrap()["mask"],
        mask["mask"]
    );
}

#[test]
fn exact_configured_capability_override_and_codex_batch_message_keep_evidence_layers_separate() {
    let profile = connection(
        "gemini",
        json!({"modelOverrides": {"model": {
            "maxCount": 4, "supportedParameters": ["quality"],
            "capabilities": {"text_to_image": "supported"}, "evidence": "fixture docs"
        }}}),
    );
    let cap = capabilities(&profile, &"model".into());
    assert_eq!(cap["max_count"], 4);
    assert_eq!(cap["parameters"], json!(["quality"]));
    assert_eq!(cap["evidence"][0]["kind"], "configured");
    assert_eq!(cap["account_availability"], "unknown");
    let omni = connection(
        "openai-images",
        json!({
            "gateway": "omniroute", "modelOverrides": {"codex/model": {"maxCount": 3}}
        }),
    );
    assert!(
        effective_request(&omni, &input(json!({"count": 2})), &"codex/model".into())
            .unwrap_err()
            .message
            .contains("fans out")
    );
}
