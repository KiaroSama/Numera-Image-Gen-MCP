mod common;
#[path = "common/http.rs"]
mod http_fixture;
use common::Workspace;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    config::Config,
    runtime_paths::RuntimePaths,
    services::{discovery::Discovery, model_policy, request},
};
use std::{collections::HashMap, time::Duration};
use tokio_util::sync::CancellationToken;

fn config(workspace: &Workspace, adapter: &str, overrides: Value) -> Config {
    let mut connection = json!({
        "adapter":adapter,"baseUrl":"https://api.example/v1","auth":{"type":"none"}
    });
    connection
        .as_object_mut()
        .unwrap()
        .extend(overrides.as_object().unwrap().clone());
    let raw = json!({
        "schemaVersion":1,"defaultConnection":"local",
        "outputDir":workspace.0.join("outputs").to_str().unwrap(),
        "stateDir":workspace.0.join("state").to_str().unwrap(),
        "logging":{"directory":workspace.0.join("logs").to_str().unwrap()},
        "files":{"allowedInputRoots":[workspace.0.join("inputs").to_str().unwrap()]},
        "connections":{"local":connection}
    });
    Config::parse(
        &raw.to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap()
}

async fn model(workspace: &Workspace, adapter: &str, overrides: Value, id: &str) -> Value {
    let config = config(workspace, adapter, overrides);
    let runtime = RuntimePaths::new(&workspace.0).unwrap();
    tokio::time::timeout(
        Duration::from_secs(3),
        Discovery::model(
            &config,
            &runtime,
            &HashMap::new(),
            None,
            &id.into(),
            &CancellationToken::new(),
        ),
    )
    .await
    .unwrap()
    .unwrap()
}

#[tokio::test]
async fn exact_source_generic_flash_dimensions_separate_native_sizes_from_route_acceptance() {
    let workspace = Workspace::new();
    let result = model(
        &workspace,
        "openai-images",
        json!({}),
        "antigravity/gemini-3.1-flash-image",
    )
    .await;
    let dimensions = &result["dimensions"];
    assert!(
        dimensions["preset_aspect_ratios"]
            .as_array()
            .unwrap()
            .contains(&json!("16:9"))
    );
    assert_eq!(
        dimensions["model_support"]["aspect_ratios"]
            .as_array()
            .unwrap()
            .len(),
        14
    );
    assert_eq!(
        dimensions["model_support"]["resolution_tiers"],
        json!(["0.5K", "1K", "2K", "4K"])
    );
    assert!(
        dimensions["model_support"]["pixel_sizes"]
            .as_array()
            .unwrap()
            .contains(&json!({
                "aspect_ratio":"16:9","resolution":"4K","width":5504,"height":3072
            }))
    );
    assert_eq!(
        dimensions["route_support"]["accepted_parameters"],
        json!(["size"])
    );
    assert_eq!(
        dimensions["model_support"]["custom_dimensions"]["status"],
        "unsupported"
    );
}

#[tokio::test]
async fn exact_source_opaque_model_does_not_invent_native_dimensions() {
    let workspace = Workspace::new();
    let result = model(&workspace, "openai-images", json!({}), "cx/gpt-6.1-sol").await;
    let dimensions = &result["dimensions"];
    assert_eq!(dimensions["model_support"]["aspect_ratios"], Value::Null);
    assert_eq!(dimensions["model_support"]["resolution_tiers"], Value::Null);
    assert_eq!(
        dimensions["model_support"]["custom_dimensions"]["status"],
        "unknown"
    );
    assert!(
        dimensions["preset_aspect_ratios"]
            .as_array()
            .unwrap()
            .contains(&json!("1:1"))
    );
}

#[tokio::test]
async fn exact_source_gateway_tier_restrictions_do_not_establish_native_or_account_support() {
    let workspace = Workspace::new();
    let result = model(
        &workspace,
        "openai-images",
        json!({"gateway":"omniroute"}),
        "antigravity/gemini-3.1-flash-image",
    )
    .await;
    assert_eq!(
        result["dimensions"]["route_support"]["accepted_parameters"],
        json!(["size", "aspect_ratio", "image_size"])
    );
    assert_eq!(
        result["dimensions"]["route_support"]["resolution_tiers"],
        json!(["1K", "2K", "4K"])
    );
    assert_eq!(result["account_availability"], "unknown");
}

#[tokio::test]
async fn exact_source_four_adapter_dimension_inputs_remain_offline_and_unverified_for_accounts() {
    let workspace = Workspace::new();
    for (adapter, overrides, accepted) in [
        ("gemini", json!({}), json!(["aspect_ratio", "image_size"])),
        (
            "gemini-interactions",
            json!({}),
            json!(["aspect_ratio", "image_size"]),
        ),
        ("comfyui", json!({}), json!([])),
        (
            "openai-images",
            json!({"modelOverrides":{"gemini-3.1-flash-image":{"supportedParameters":["size","quality"]}}}),
            json!(["size"]),
        ),
    ] {
        let result = model(&workspace, adapter, overrides, "gemini-3.1-flash-image").await;
        assert_eq!(
            result["dimensions"]["route_support"]["accepted_parameters"], accepted,
            "{adapter}"
        );
        assert_eq!(
            result["dimensions"]["route_support"]["max_pixels"],
            64000000
        );
        assert_eq!(result["dimensions"]["account_verified"], false);
    }
}

#[tokio::test]
async fn exact_source_minimal_endpoint_descriptors_pin_only_compatible_provider() {
    let server = Server::new(|_| Reply {
        status: 200,
        bytes: None,
        value: json!({"endpoints":[
            {"provider_tag":"small","supported_parameters":{"resolution":{"type":"enum","values":["1K"]}}},
            {"provider_tag":"large","supported_parameters":{"resolution":{"type":"enum","values":["2K"]},"n":{"type":"range","min":1,"max":2}}}
        ]}),
    })
    .await;
    let workspace = Workspace::new();
    let config = config(
        &workspace,
        "openrouter-images",
        json!({"baseUrl":format!("{}/api/v1",server.origin)}),
    );
    let (_, connection) = config.select(None).unwrap();
    let signal = CancellationToken::new();
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let (selected, _) = model_policy::validate(
            &connection,
            &HashMap::new(),
            &"family/model".into(),
            request::parse(json!({"prompt":"draw","image_size":"2K","count":2})).unwrap(),
            &signal,
        )
        .await
        .unwrap();
        let error = model_policy::validate(
            &connection,
            &HashMap::new(),
            &"family/model".into(),
            request::parse(json!({"prompt":"draw","quality":"high"})).unwrap(),
            &signal,
        )
        .await
        .unwrap_err();
        (selected, error)
    })
    .await;
    let paths = server.requests.lock().unwrap().clone();
    server.close().await;
    let (selected, error) = result.unwrap();
    assert_eq!(
        selected["provider_options"]["provider"]["only"],
        json!(["large"])
    );
    assert_eq!(
        selected["provider_options"]["provider"]["allow_fallbacks"],
        false
    );
    assert_eq!(error.code, "unsupported_parameter");
    assert_eq!(
        paths.as_slice(),
        [
            "/api/v1/images/models/family/model/endpoints",
            "/api/v1/images/models/family/model/endpoints"
        ]
    );
}

#[tokio::test]
async fn exact_source_failed_catalog_warns_unknown_without_setting_or_replacing_model() {
    let server = Server::new(|_| Reply {
        status: 503,
        bytes: None,
        value: json!({"error":"fixture"}),
    })
    .await;
    let workspace = Workspace::new();
    let config = config(
        &workspace,
        "openrouter-images",
        json!({"baseUrl":format!("{}/api/v1",server.origin)}),
    );
    let (_, connection) = config.select(None).unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        model_policy::validate(
            &connection,
            &HashMap::new(),
            &"chosen".into(),
            request::parse(json!({"prompt":"draw"})).unwrap(),
            &CancellationToken::new(),
        ),
    )
    .await;
    let paths = server.requests.lock().unwrap().clone();
    server.close().await;
    let (request, warnings) = result.unwrap().unwrap();
    assert!(warnings.join(" ").contains("unknown"));
    assert!(request.get("model").is_none());
    assert_eq!(paths.as_slice(), ["/api/v1/images/models/chosen/endpoints"]);
}
