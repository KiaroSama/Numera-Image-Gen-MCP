mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/http.rs"]
mod http_fixture;
use common::Workspace;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    config::{Config, credentials, load_config},
    runtime_paths::RuntimePaths,
    services::{discovery::Discovery, health, request},
};
use std::{collections::HashMap, time::Duration};
use tokio_util::sync::CancellationToken;

// Frozen tests/unit/config.test.ts and simple-config.test.ts own these exact inputs.
// Explicit environment maps keep fixture credentials separate from the host environment.
fn compact() -> Value {
    json!({
        "api_endpoint": "http://127.0.0.1:20100/v1",
        "api_key": "fixture-local-key-not-real",
        "models": [
            {"id": "antigravity/opaque-image", "name": "Image 日本語"},
            {"id": "custom/another", "name": "Second"}
        ]
    })
}

fn load(
    workspace: &Workspace,
    value: &Value,
    extra: &[String],
    env: &HashMap<String, String>,
) -> numera_image_gen_mcp::error::Result<Config> {
    let file = workspace.0.join("config.json");
    std::fs::write(&file, value.to_string().as_bytes()).unwrap();
    let mut args = vec!["--config".into(), file.to_str().unwrap().into()];
    args.extend_from_slice(extra);
    load_config(&args, env, &workspace.0)
}

#[test]
fn whitespace_prompt_count_eleven_and_spaced_connection_name_reject_exact_source_inputs() {
    for value in [
        json!({"prompt": "  "}),
        json!({"prompt": "draw", "count": 11}),
        json!({"prompt": "draw", "count": 0}),
        json!({"prompt": "draw", "request_id": "../id"}),
        json!({"prompt": "draw", "reference_images": [{"type": "path", "path": ""}]}),
        json!({"prompt": "draw", "extra": true}),
    ] {
        assert_eq!(request::parse(value).unwrap_err().code, "invalid_input");
    }
    let workspace = Workspace::new();
    let mut raw = workspace.config("https://api.example/v1", "openai-images");
    raw["connections"] = json!({"bad name": {}});
    assert_eq!(
        Config::parse(
            &raw.to_string(),
            &workspace.0.join("config.json"),
            &[],
            &HashMap::new(),
        )
        .err()
        .expect("spaced connection names must reject")
        .code,
        "invalid_configuration"
    );
}

#[test]
fn flag_default_connection_and_precedence_preserve_unicode_and_original_defaults() {
    let workspace = Workspace::new();
    let mut raw = workspace.config("https://api.example/v1", "openai-images");
    let mut a = raw["connections"]["test"].clone();
    a["requestTimeoutMs"] = json!(300000);
    let mut b = a.clone();
    b["adapter"] = json!("gemini");
    raw["defaultConnection"] = json!("a");
    raw["connections"] = json!({"a": a, "b": b});
    let original = Config::parse(
        &raw.to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap();
    let before = original.value.clone();
    let text = original.value.to_string();
    let loaded = load(
        &workspace,
        &original.value,
        &[
            "--output-dir".into(),
            workspace.0.join("flag").to_str().unwrap().into(),
            "--log-level".into(),
            "DEBUG".into(),
            "--request-timeout-ms".into(),
            "900".into(),
            "--default-connection".into(),
            "b".into(),
        ],
        &HashMap::from([
            (
                "NUMERA_OUTPUT_DIR".into(),
                workspace.0.join("env").to_str().unwrap().into(),
            ),
            (
                "NUMERA_STATE_DIR".into(),
                workspace
                    .0
                    .join("environment-state")
                    .to_str()
                    .unwrap()
                    .into(),
            ),
            ("NUMERA_LOG_LEVEL".into(), "ERROR".into()),
            ("NUMERA_REQUEST_TIMEOUT_MS".into(), "1000".into()),
        ]),
    )
    .unwrap();
    assert_eq!(
        loaded.value["outputDir"],
        workspace.0.join("flag").to_str().unwrap()
    );
    assert_eq!(
        loaded.value["stateDir"],
        workspace.0.join("environment-state").to_str().unwrap()
    );
    assert_eq!(loaded.value["logging"]["level"], "DEBUG");
    let timeouts: Vec<_> = loaded.value["connections"]
        .as_object()
        .unwrap()
        .values()
        .map(|connection| connection["requestTimeoutMs"].as_u64().unwrap())
        .collect();
    assert_eq!(timeouts, [900, 900]);
    let (name, selected) = loaded.select(None).unwrap();
    assert_eq!(name, "b");
    assert_eq!(selected["adapter"], "gemini");
    assert_eq!(
        request::parse(json!({"prompt": "Draw a café"})).unwrap()["prompt"],
        "Draw a café"
    );
    assert_eq!(
        original.value["connections"]["a"]["requestTimeoutMs"],
        300000
    );
    assert_eq!(original.value, before);
    assert_eq!(
        std::fs::read_to_string(workspace.0.join("config.json")).unwrap(),
        text
    );
}

#[tokio::test]
async fn selected_bearer_and_header_profiles_use_only_their_own_environment_secrets() {
    let action = async {
        let workspace = Workspace::new();
        let runtime = RuntimePaths::new(&workspace.0).unwrap();
        let mut raw = workspace.config("https://api.example/v1", "openai-images");
        let mut a = raw["connections"]["test"].clone();
        a["auth"] = json!({"type": "bearer", "secretEnv": "EXAMPLE_A"});
        let mut b = raw["connections"]["test"].clone();
        b["adapter"] = json!("gemini");
        b["auth"] = json!({"type": "header", "header": "x-api-key", "secretEnv": "EXAMPLE_B"});
        raw["defaultConnection"] = json!("a");
        raw["connections"] = json!({"a": a, "b": b});
        let config = load(&workspace, &raw, &[], &HashMap::new()).unwrap();
        let (_, a) = config.select(Some("a")).unwrap();
        let (_, b) = config.select(Some("b")).unwrap();
        let mut env = HashMap::from([
            ("EXAMPLE_A".into(), "fixture-A".into()),
            ("EXAMPLE_B".into(), "fixture-B".into()),
            ("OPENAI_API_KEY".into(), "fixture-unrelated".into()),
        ]);
        assert_eq!(
            credentials(&a, &env, &runtime).await.unwrap(),
            HashMap::from([("Authorization".into(), "Bearer fixture-A".into())])
        );
        assert_eq!(
            credentials(&b, &env, &runtime).await.unwrap(),
            HashMap::from([("x-api-key".into(), "fixture-B".into())])
        );
        env.remove("EXAMPLE_A");
        assert_eq!(
            credentials(&a, &env, &runtime).await.unwrap_err().code,
            "missing_credentials"
        );
        assert_eq!(
            credentials(&b, &env, &runtime).await.unwrap(),
            HashMap::from([("x-api-key".into(), "fixture-B".into())])
        );
    };
    tokio::time::timeout(Duration::from_secs(8), action)
        .await
        .expect("profile credential scenario exceeded 8 seconds");
}

#[test]
fn all_eighteen_unsafe_compact_variants_reject_and_never_display_the_fixture_key() {
    let workspace = Workspace::new();
    let original = compact();
    let changes = [
        json!({"api_key": " "}),
        json!({"api_key": "bad\nkey"}),
        json!({"models": []}),
        json!({"models": [{"id": "duplicate", "name": "One"}, {"id": "duplicate", "name": "Two"}]}),
        json!({"models": [{"id": " ", "name": "Name"}]}),
        json!({"models": [{"id": "native", "name": " "}]}),
        json!({"api_endpoint": "http://user:password@localhost/v1"}),
        json!({"api_endpoint": "http://localhost/v1?key=fixture"}),
        json!({"extra": true}),
        json!({"profile": "guessed"}),
        json!({"profile": "openai-responses"}),
        json!({"orchestration_model": "unused"}),
        json!({"profile": "openai-responses", "orchestration_model": " "}),
        json!({"models": [{"id": "image", "name": "Image", "output_format": "gif"}]}),
        json!({"models": [{"id": "image", "name": "Image", "size": " "}]}),
        json!({"models": [{"id": "image", "name": "Image", "quality": "x".repeat(101)}]}),
        json!({"tools": [{"type": "image_generation"}]}),
        json!({"models": [{"id": "disabled", "name": "Disabled", "enabled": false}]}),
    ];
    assert_eq!(changes.len(), 18);
    for (ordinal, change) in changes.into_iter().enumerate() {
        let mut raw = original.clone();
        raw.as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        let error = load(&workspace, &raw, &[], &HashMap::new())
            .err()
            .unwrap_or_else(|| panic!("unsafe compact source variant {ordinal} was accepted"));
        assert_eq!(
            error.code, "invalid_configuration",
            "source variant {ordinal}"
        );
        assert!(
            !error
                .to_string()
                .contains(original["api_key"].as_str().unwrap()),
            "source variant {ordinal} displayed the original fixture key"
        );
    }
}

#[test]
fn explicit_compact_profiles_apply_the_cli_output_override_without_inferred_gateways() {
    let workspace = Workspace::new();
    for profile in ["omniroute", "9router", "openai-images"] {
        let mut raw = compact();
        raw["profile"] = json!(profile);
        let loaded = load(
            &workspace,
            &raw,
            &[
                "--output-dir".into(),
                workspace.0.join("override").to_str().unwrap().into(),
            ],
            &HashMap::new(),
        )
        .unwrap();
        assert_eq!(
            loaded.value["outputDir"],
            workspace.0.join("override").to_str().unwrap(),
            "{profile}"
        );
        let (_, c) = loaded.select(None).unwrap();
        if profile == "openai-images" {
            assert!(c.get("gateway").is_none());
        } else {
            assert_eq!(c["gateway"], profile);
        }
    }
}

#[test]
fn compact_responses_preserves_exact_per_model_defaults_and_separate_orchestration() {
    let workspace = Workspace::new();
    let mut raw = compact();
    raw["api_endpoint"] = json!("https://images.example.net/custom/api/v1");
    raw["profile"] = json!("openai-responses");
    raw["orchestration_model"] = json!("custom/orchestrator");
    raw["models"] = json!([{
        "id": "opaque/image", "name": "Image", "output_format": "webp",
        "size": "1536x1024", "quality": "high"
    }]);
    let loaded = load(&workspace, &raw, &[], &HashMap::new()).unwrap();
    let (_, c) = loaded.select(None).unwrap();
    assert_eq!(c["adapter"], "openai-responses");
    assert_eq!(c["baseUrl"], "https://images.example.net/custom/api/v1");
    assert_eq!(c["orchestrationModel"], "custom/orchestrator");
    assert_eq!(
        c["configuredModels"],
        json!([{"id": "opaque/image", "name": "Image"}])
    );
    assert_eq!(
        c["modelOverrides"]["opaque/image"]["defaults"],
        json!({"output_format": "webp", "size": "1536x1024", "quality": "high"})
    );
    assert!(c.get("gateway").is_none());
}

#[tokio::test]
async fn three_disabled_and_enabled_slots_keep_only_the_original_active_model_in_offline_catalog() {
    let action = async {
        let workspace = Workspace::new();
        let mut raw = compact();
        raw["models"] = json!([
            {"id": "unused/one", "name": "Unused", "enabled": false,
                "output_format": null, "size": null, "quality": null},
            {"id": "real/model", "name": "Real 日本語", "enabled": true,
                "output_format": null, "size": null, "quality": null},
            {"id": "unused/two", "name": "Unused two", "enabled": false,
                "output_format": null, "size": null, "quality": null}
        ]);
        let mut config = load(&workspace, &raw, &[], &HashMap::new()).unwrap();
        let (_, c) = config.select(None).unwrap();
        assert_eq!(c["defaultModel"], "real/model");
        assert_eq!(
            c["configuredModels"],
            json!([{"id": "real/model", "name": "Real 日本語"}])
        );
        assert_eq!(c["modelOverrides"]["real/model"]["defaults"], json!({}));
        assert_eq!(c["deniedModels"], json!(["unused/one", "unused/two"]));
        // Offline listing must not need a credential or touch even a loopback destination.
        config.value["connections"]["default"]["auth"] =
            json!({"type": "bearer", "secretEnv": "UNSET_OFFLINE_FIXTURE"});
        let signal = CancellationToken::new();
        signal.cancel();
        let runtime = RuntimePaths::new(&workspace.0).unwrap();
        let catalog = Discovery::default()
            .list(&config, &runtime, &HashMap::new(), None, false, &signal)
            .await
            .unwrap();
        let ids: Vec<_> = catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["real/model"]);
        assert_eq!(catalog["models"][0]["name"], "Real 日本語");
        assert_eq!(catalog["models"][0]["configured"], true);
        assert_eq!(catalog["complete"], true);
        assert_eq!(catalog["cached"], false);
        assert_eq!(catalog["warnings"].as_array().unwrap().len(), 1);
        assert_eq!(
            catalog["warnings"][0],
            "Configured model list only; provider availability is unverified. Use refresh for provider discovery."
        );
    };
    tokio::time::timeout(Duration::from_secs(8), action)
        .await
        .expect("disabled-model offline scenario exceeded 8 seconds");
}

#[test]
fn explicit_compatibility_and_file_errors_never_silently_fall_back() {
    let workspace = Workspace::new();
    let env = HashMap::from([
        ("LOCALAPPDATA".into(), workspace.0.to_str().unwrap().into()),
        (
            "XDG_CONFIG_HOME".into(),
            workspace.0.to_str().unwrap().into(),
        ),
        ("USERPROFILE".into(), workspace.0.to_str().unwrap().into()),
        ("HOME".into(), workspace.0.to_str().unwrap().into()),
        ("OPENAI_BASE_URL".into(), "https://compat.example/v1".into()),
        ("OPENAI_MODEL".into(), "image-model".into()),
    ]);
    let config = load_config(&["--openai-compat".into()], &env, &workspace.0).unwrap();
    let keys: Vec<_> = config.value["connections"]
        .as_object()
        .unwrap()
        .keys()
        .map(|key| key.as_str().unwrap())
        .collect();
    assert_eq!(keys, ["openai-compat"]);
    assert_eq!(
        config.value["connections"]["openai-compat"]["baseUrl"],
        "https://compat.example/v1"
    );
    assert_eq!(
        config.value["connections"]["openai-compat"]["defaultModel"],
        "image-model"
    );
    assert_eq!(
        config.value["connections"]["openai-compat"]["auth"]["secretEnv"],
        "OPENAI_API_KEY"
    );
    for path in [
        "relative.json".to_owned(),
        workspace.0.join("missing").to_str().unwrap().into(),
    ] {
        assert_eq!(
            load_config(&["--config".into(), path], &env, &workspace.0)
                .err()
                .unwrap()
                .code,
            "invalid_configuration"
        );
    }
    let path = workspace.0.join("config.json");
    std::fs::write(&path, b"{").unwrap();
    assert_eq!(
        load_config(
            &["--config".into(), path.to_str().unwrap().into()],
            &HashMap::new(),
            &workspace.0
        )
        .err()
        .unwrap()
        .code,
        "invalid_configuration"
    );
    for value in [
        json!(null),
        json!("text"),
        json!([]),
        json!({
            "api_endpoint": "http://127.0.0.1:20100/v1", "api_key": "", "models": [{"id": "only", "name": "Name"}]
        }),
    ] {
        assert_eq!(
            load(&workspace, &value, &[], &HashMap::new())
                .err()
                .unwrap()
                .code,
            "invalid_configuration"
        );
    }
}

#[tokio::test]
async fn exact_no_probe_health_reports_storage_without_any_provider_or_generation_claim() {
    let action = async {
        let workspace = Workspace::new();
        let raw = workspace.config("https://invalid.example/v1", "openai-images");
        let config = load(&workspace, &raw, &[], &HashMap::new()).unwrap();
        let runtime = RuntimePaths::new(&workspace.0).unwrap();
        let signal = CancellationToken::new();
        signal.cancel();
        let result = health::health(&config, &HashMap::new(), &runtime, false, &signal)
            .await
            .unwrap();
        assert_eq!(
            result["storage"],
            json!({"outputs": true, "state": true, "logs": true})
        );
        assert_eq!(
            result["connections"],
            json!([{
                "name": "test", "reachable": null, "authentication_checked": false,
                "generation_verified": false, "error": null
            }])
        );
        assert_eq!(result["generation_verified"], false);
    };
    tokio::time::timeout(Duration::from_secs(8), action)
        .await
        .expect("no-probe health scenario exceeded 8 seconds");
}

#[tokio::test]
async fn exact_offline_catalog_keeps_both_configured_names_after_one_failed_refresh() {
    let server = http_fixture::Server::new(|_| http_fixture::Reply {
        status: 503,
        value: json!({"error": "fixture"}),
        bytes: None,
    })
    .await;
    let origin = server.origin.clone();
    let requests = server.requests.clone();
    let action = async move {
        let workspace = Workspace::new();
        let mut raw = compact();
        raw["api_endpoint"] = json!(format!("{origin}/v1"));
        let config = load(&workspace, &raw, &[], &HashMap::new()).unwrap();
        let runtime = RuntimePaths::new(&workspace.0).unwrap();
        let signal = CancellationToken::new();
        let mut discovery = Discovery::default();
        let offline = discovery
            .list(&config, &runtime, &HashMap::new(), None, false, &signal)
            .await
            .unwrap();
        assert!(requests.lock().unwrap().is_empty());
        let expected = [
            ("antigravity/opaque-image", "Image 日本語"),
            ("custom/another", "Second"),
        ];
        for (model, (id, name)) in offline["models"].as_array().unwrap().iter().zip(expected) {
            assert_eq!(model["id"], id);
            assert_eq!(model["name"], name);
            assert_eq!(model["configured"], true);
        }
        assert_eq!(offline["models"].as_array().unwrap().len(), 2);
        let refreshed = discovery
            .list(&config, &runtime, &HashMap::new(), None, true, &signal)
            .await
            .unwrap();
        assert_eq!(*requests.lock().unwrap(), ["/v1/models"]);
        assert_eq!(refreshed["complete"], false);
        assert_eq!(refreshed["models"].as_array().unwrap().len(), 2);
        for (model, (id, name)) in refreshed["models"].as_array().unwrap().iter().zip(expected) {
            assert_eq!(model["id"], id);
            assert_eq!(model["name"], name);
        }
    };
    let result =
        tokio::spawn(async move { tokio::time::timeout(Duration::from_secs(8), action).await })
            .await;
    tokio::time::timeout(Duration::from_secs(2), server.close())
        .await
        .expect("catalog fixture shutdown exceeded 2 seconds");
    match result {
        Ok(result) => result.expect("offline failed-refresh scenario exceeded 8 seconds"),
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => panic!("catalog fixture action failed: {error}"),
    }
}

#[tokio::test]
async fn exact_bearer_file_credential_trims_private_bytes_and_rejects_source_failure_variants() {
    let action = async {
        let workspace = Workspace::new();
        let path = workspace.0.join("fixture-secret.txt");
        std::fs::write(&path, b"  fixture-secret  \n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let runtime = RuntimePaths::new(&workspace.0).unwrap();
        let profile = json!({"auth": {"type": "bearer", "secretFile": path.to_str().unwrap()}});
        assert_eq!(
            credentials(&profile, &HashMap::new(), &runtime)
                .await
                .unwrap(),
            HashMap::from([("Authorization".into(), "Bearer fixture-secret".into())])
        );
        std::fs::write(&path, vec![b'x'; 65537]).unwrap();
        assert_eq!(
            credentials(&profile, &HashMap::new(), &runtime)
                .await
                .unwrap_err()
                .code,
            "missing_credentials"
        );
        for (file, code) in [
            ("relative.txt", "invalid_configuration"),
            (workspace.0.to_str().unwrap(), "missing_credentials"),
        ] {
            assert_eq!(
                credentials(
                    &json!({"auth": {"type": "bearer", "secretFile": file}}),
                    &HashMap::new(),
                    &runtime
                )
                .await
                .unwrap_err()
                .code,
                code
            );
        }
        let mut raw = workspace.config("https://api.example/v1", "openai-images");
        raw["connections"]["test"]["auth"] = json!({
            "type": "bearer", "secretEnv": "EXAMPLE", "secretFile": path.to_str().unwrap()
        });
        assert_eq!(
            load(&workspace, &raw, &[], &HashMap::new())
                .err()
                .unwrap()
                .code,
            "invalid_configuration"
        );
    };
    tokio::time::timeout(Duration::from_secs(8), action)
        .await
        .expect("bearer file credential scenario exceeded 8 seconds");
}
