mod common;
#[path = "common/config.rs"]
mod config_fixture;
use common::Workspace;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    config::{
        Config, config_file, credentials, load_config, parse_env_config, read_config_text,
        snapshot_credentials, user_root,
    },
    runtime_paths::RuntimePaths,
};
use std::{collections::HashMap, path::Path};

// Source authority: src/config/{load,env,simple,schema,credentials}.ts and
// tests/unit/{config,env-config,simple-config}.test.ts; this supplements generated parity fixtures.
fn parse(
    workspace: &Workspace,
    value: &Value,
    args: &[String],
) -> numera_image_gen_mcp::error::Result<Config> {
    Config::parse(
        &value.to_string(),
        &workspace.0.join("config.json"),
        args,
        &HashMap::new(),
    )
}
fn invalid<T>(result: numera_image_gen_mcp::error::Result<T>) {
    assert_eq!(
        result.err().expect("unsafe fixture must reject").code,
        "invalid_configuration"
    );
}
fn config(workspace: &Workspace) -> Value {
    workspace.config("https://images.example/v1", "openai-images")
}
fn env_root(root: &Path) -> HashMap<String, String> {
    HashMap::from([
        ("LOCALAPPDATA".into(), root.to_string_lossy().into_owned()),
        (
            "XDG_CONFIG_HOME".into(),
            root.to_string_lossy().into_owned(),
        ),
        ("HOME".into(), root.to_string_lossy().into_owned()),
        ("USERPROFILE".into(), root.to_string_lossy().into_owned()),
    ])
}

#[test]
fn adjacent_discovery_explicit_paths_and_openai_compat_are_isolated_from_caller_state() {
    let workspace = Workspace::new();
    let env = env_root(&workspace.0);
    let user = user_root(&env).unwrap();
    assert!(user.starts_with(&workspace.0));
    assert_eq!(
        config_file(&[], &env, &workspace.0).unwrap(),
        Some(user.join("config.json"))
    );
    let file = workspace.0.join("config.local.json");
    std::fs::write(&file, config(&workspace).to_string()).unwrap();
    assert_eq!(
        config_file(&[], &env, &workspace.0).unwrap(),
        Some(file.clone())
    );
    assert_eq!(
        load_config(&[], &env, &workspace.0)
            .unwrap()
            .select(None)
            .unwrap()
            .0,
        "test"
    );
    let dotenv = workspace.0.join(".env");
    std::fs::write(&dotenv,"API_ENDPOINT=https://images.example/v1\nAPI_KEY=synthetic-fixture\nMODEL_1_ID=opaque/image\nMODEL_1_NAME=Named\n").unwrap();
    assert_eq!(config_file(&[], &env, &workspace.0).unwrap(), Some(dotenv));
    assert_eq!(
        load_config(&[], &env, &workspace.0)
            .unwrap()
            .select(None)
            .unwrap()
            .0,
        "default"
    );
    let mut explicit = env.clone();
    explicit.insert("NUMERA_CONFIG".into(), file.to_string_lossy().into_owned());
    assert_eq!(
        config_file(&[], &explicit, &workspace.0).unwrap(),
        Some(file.clone())
    );
    assert_eq!(
        config_file(
            &["--config".into(), file.to_string_lossy().into_owned()],
            &env,
            &workspace.0
        )
        .unwrap(),
        Some(file)
    );
    invalid(config_file(
        &["--config=relative".into()],
        &env,
        &workspace.0,
    ));
    assert!(
        config_file(&["--openai-compat".into()], &env, &workspace.0)
            .unwrap()
            .is_none()
    );
    let compatible = load_config(&["--openai-compat".into()], &env, &workspace.0).unwrap();
    assert_eq!(
        compatible.value["connections"]["openai-compat"]["baseUrl"],
        "https://api.openai.com/v1"
    );
    assert_eq!(
        compatible.value["connections"]["openai-compat"]["auth"],
        json!({"type":"bearer","secretEnv":"OPENAI_API_KEY"})
    );
    assert!(
        compatible.value["connections"]["openai-compat"]
            .get("defaultModel")
            .is_none()
    );
    let mut chosen = env.clone();
    chosen.insert("OPENAI_BASE_URL".into(), "https://compat.example/v1".into());
    chosen.insert("OPENAI_MODEL".into(), "chosen/image".into());
    let compatible = load_config(&["--openai-compat".into()], &chosen, &workspace.0).unwrap();
    assert_eq!(
        compatible.value["connections"]["openai-compat"]["defaultModel"],
        "chosen/image"
    );
    assert_eq!(
        compatible.value["connections"]["openai-compat"]["baseUrl"],
        "https://compat.example/v1"
    );
    #[cfg(not(windows))]
    {
        let fallback = user_root(&HashMap::from([(
            "HOME".into(),
            workspace.0.to_string_lossy().into_owned(),
        )]))
        .unwrap();
        assert_eq!(fallback, workspace.0.join(".config/numera-image-gen"));
    }
}

#[test]
fn bounded_utf8_config_reads_strip_one_bom_and_refuse_directories_corruption_or_oversize() {
    let workspace = Workspace::new();
    let file = workspace.0.join("bounded.json");
    let text = config(&workspace).to_string();
    std::fs::write(&file, format!("\u{feff}{text}")).unwrap();
    assert_eq!(read_config_text(&file).unwrap(), text);
    let args = vec!["--config".into(), file.to_string_lossy().into_owned()];
    assert_eq!(
        load_config(&args, &HashMap::new(), &workspace.0)
            .unwrap()
            .select(None)
            .unwrap()
            .0,
        "test"
    );
    for bytes in [vec![0xff], vec![b' '; 1024 * 1024 + 1]] {
        std::fs::write(&file, bytes).unwrap();
        invalid(read_config_text(&file));
    }
    invalid(read_config_text(&workspace.0));
    invalid(read_config_text(&workspace.0.join("absent")));
    for text in ["null", "[]", "true", "{} {}"] {
        invalid(Config::parse(text, &file, &[], &HashMap::new()));
    }
    invalid(Config::parse(
        &text,
        Path::new("relative.json"),
        &[],
        &HashMap::new(),
    ));
}

#[test]
fn numeric_flags_use_source_number_semantics_and_strict_cli_option_errors() {
    let workspace = Workspace::new();
    let raw = config(&workspace);
    for (value, expected) in [
        ("0x10", 16),
        ("0o10", 8),
        ("0b10", 2),
        ("+4", 4),
        ("4e0", 4),
        (" 4 ", 4),
    ] {
        let loaded = parse(
            &workspace,
            &raw,
            &[
                format!("--max-concurrent-requests={value}"),
                format!("--request-timeout-ms={value}"),
            ],
        )
        .unwrap();
        assert_eq!(loaded.value["maxConcurrentRequests"], expected);
        assert_eq!(
            loaded.value["connections"]["test"]["requestTimeoutMs"],
            expected
        );
    }
    for value in [
        "", "0", "33", "1.5", "NaN", "Infinity", "0x", "0o8", "0b2", "-1", "1e999", "words",
    ] {
        invalid(parse(
            &workspace,
            &raw,
            &[format!("--max-concurrent-requests={value}")],
        ));
    }
    for value in ["", "0", "3600001", "1.5", "-1", "1e999", "NaN"] {
        invalid(parse(
            &workspace,
            &raw,
            &[format!("--request-timeout-ms={value}")],
        ));
    }
    for (args, code) in [
        (vec!["--unknown"], "ERR_PARSE_ARGS_UNKNOWN_OPTION"),
        (vec!["-x"], "ERR_PARSE_ARGS_UNKNOWN_OPTION"),
        (vec!["positional"], "ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL"),
        (vec!["--", "arg"], "ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL"),
        (
            vec!["--openai-compat=false"],
            "ERR_PARSE_ARGS_INVALID_OPTION_VALUE",
        ),
        (
            vec!["--request-timeout-ms"],
            "ERR_PARSE_ARGS_INVALID_OPTION_VALUE",
        ),
        (
            vec!["--request-timeout-ms", "-1"],
            "ERR_PARSE_ARGS_INVALID_OPTION_VALUE",
        ),
    ] {
        let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(parse(&workspace, &raw, &args).err().unwrap().code, code);
    }
    parse(&workspace, &raw, &["--".into()]).unwrap();
    let mut raw = raw.clone();
    raw["logging"] = Value::Null;
    let loaded = parse(
        &workspace,
        &raw,
        &[format!("--log-dir={}", workspace.0.join("logs").display())],
    )
    .unwrap();
    assert_eq!(loaded.value["logging"]["level"], "INFO");
    raw["logging"] = json!("wrong");
    invalid(parse(&workspace, &raw, &["--log-level=DEBUG".into()]));
}

#[test]
fn connection_cross_validation_rejects_unsafe_origins_headers_queries_proxies_and_edit_routes() {
    let workspace = Workspace::new();
    let raw = config(&workspace);
    for auth in [
        json!({"type":"bearer"}),
        json!({"type":"bearer","apiKey":"synthetic","secretEnv":"OTHER"}),
        json!({"type":"bearer","apiKey":" "}),
        json!({"type":"bearer","apiKey":"line\nbreak"}),
        json!({"type":"bearer","apiKey":"synthetic","origin":"https://other.example"}),
        json!({"type":"bearer","apiKey":"synthetic","origin":"not a URL"}),
        json!({"type":"header","secretEnv":"SELECTED"}),
        json!({"type":"header","secretEnv":"SELECTED","header":"bad header"}),
        json!({"type":"header","secretEnv":"SELECTED","header":"Host"}),
        json!({"type":"header","secretEnv":"SELECTED","header":"Cookie"}),
        json!({"type":"header","secretEnv":"SELECTED","header":"content-length"}),
    ] {
        let mut changed = raw.clone();
        changed["connections"]["test"]["auth"] = auth;
        invalid(parse(&workspace, &changed, &[]));
    }
    for base in [
        "ftp://images.example/v1",
        "https://user@images.example/v1",
        "https://images.example/v1?other=1",
        "https://images.example/v1#fragment",
        "https://images.example/%2fhidden",
        "https://images.example/../v1",
        "https://images.example/v1/images",
        "https://images.example/v1/images/generations",
    ] {
        let mut changed = raw.clone();
        changed["connections"]["test"]["baseUrl"] = json!(base);
        invalid(parse(&workspace, &changed, &[]));
    }
    for proxy in [
        "ftp://proxy.example",
        "https://user:password@proxy.example",
        "https://proxy.example?query=1",
        "https://proxy.example#fragment",
    ] {
        let mut changed = raw.clone();
        changed["connections"]["test"]["proxy"] = json!({"url":proxy});
        invalid(parse(&workspace, &changed, &[]));
    }
    let mut changed = raw.clone();
    changed["connections"]["test"]["baseUrlMode"] = json!("origin");
    invalid(parse(&workspace, &changed, &[]));
    for key in ["api_key", "auth", "token", "signature", "secret"] {
        let mut changed = raw.clone();
        changed["connections"]["test"]["query"] = json!({});
        changed["connections"]["test"]["query"][key] = json!("synthetic");
        invalid(parse(&workspace, &changed, &[]));
    }
    for edit in [
        json!({"mode":"multipart","encoding":"images"}),
        json!({"mode":"json","encoding":"image[]"}),
        json!({"mode":"json","encoding":"image","maxReferences":2}),
    ] {
        let mut changed = raw.clone();
        changed["connections"]["test"]["edit"] = edit;
        invalid(parse(&workspace, &changed, &[]));
    }
    let mut changed = raw.clone();
    changed["connections"]["test"]["configuredModels"] =
        json!([{"id":"same","name":"A"},{"id":"same","name":"B"}]);
    invalid(parse(&workspace, &changed, &[]));
    let mut valid = raw;
    valid["connections"]["test"]["auth"] = json!({"type":"header","secretEnv":"SELECTED","header":"x-api-key","origin":"https://images.example/ignored-path"});
    valid["connections"]["test"]["proxy"] = json!({"url":"http://proxy.example:8080"});
    valid["connections"]["test"]["query"] = json!({"pageToken":"cursor","other":"value"});
    valid["connections"]["test"]["edit"] =
        json!({"mode":"multipart","encoding":"image[]","maxReferences":2});
    parse(&workspace, &valid, &[]).unwrap();
}

#[test]
fn nullable_model_settings_and_utf16_lengths_match_the_public_source_schema() {
    let workspace = Workspace::new();
    let compact = json!({"api_endpoint":"https://images.example/v1","api_key":"synthetic","models":[{"id":"actual","name":"日本語","size":null,"quality":null,"output_format":null}]});
    let loaded = parse(&workspace, &compact, &[]).unwrap();
    assert_eq!(
        loaded.value["connections"]["default"]["modelOverrides"]["actual"]["defaults"],
        json!({})
    );
    for (field, value) in [
        ("size", json!("")),
        ("quality", json!(" ")),
        ("name", json!("🌍".repeat(51))),
        ("id", json!("🌍".repeat(151))),
        ("size", json!("🌍".repeat(51))),
        ("quality", json!("🌍".repeat(51))),
    ] {
        let mut changed = compact.clone();
        changed["models"][0][field] = value;
        invalid(parse(&workspace, &changed, &[]));
    }
    let mut changed = compact.clone();
    changed["models"][0]["name"] = json!("🌍".repeat(50));
    parse(&workspace, &changed, &[]).unwrap();
    let mut disabled = compact;
    disabled["models"][0]["enabled"] = json!(false);
    invalid(parse(&workspace, &disabled, &[]));
    let mut numeric = config(&workspace);
    numeric["connections"]["test"]["defaults"] =
        json!({"signed":-1,"fraction":0.5,"nested":[true,null,{"n":9007199254740993u64}]});
    let loaded = parse(&workspace, &numeric, &[]).unwrap();
    assert_eq!(
        loaded.value["connections"]["test"]["defaults"]["signed"],
        -1
    );
    assert_eq!(
        loaded.value["connections"]["test"]["defaults"]["nested"][2]["n"],
        9007199254740992u64
    );
}

#[test]
fn strict_dotenv_rejects_unknown_slots_controls_and_accepts_quotes_comments_exports() {
    let valid = "API_ENDPOINT=https://images.example/v1\nAPI_KEY=synthetic\nMODEL_1_ID=actual\nMODEL_1_NAME=Name\n";
    for prefix in ["# comment\n", "\n\t# comment\r\n"] {
        assert_eq!(
            parse_env_config(&(prefix.to_owned() + valid)).unwrap()["models"][0]["name"],
            "Name"
        );
    }
    for text in [
        "API_KEY=synthetic\0",
        "1_BAD=value",
        "API_KEY value",
        "export\tAPI_KEY=synthetic",
        "MODEL_0_ID=zero",
        "MODEL_01_ID=zero",
        "MODEL_1000_ID=too-many",
        "MODEL_1_QUALTY=wrong",
        "MODEL_1_ENABLED=1",
        "EDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MAX_REFERENCES=01",
        "EDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MAX_REFERENCES=33",
        "EDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MASKS=1",
    ] {
        invalid(parse_env_config(text));
    }
    for quote in ["'", "`"] {
        let text = format!(
            "API_ENDPOINT=https://images.example/v1\nAPI_KEY=synthetic\nMODEL_1_ID=actual\nexport MODEL_1_NAME={quote}line one\nline two # literal{quote} # tail\n"
        );
        assert_eq!(
            parse_env_config(&text).unwrap()["models"][0]["name"],
            "line one\nline two # literal"
        );
    }
    let text = "API_ENDPOINT=https://images.example/v1\nAPI_KEY=synthetic\nMODEL_1_ID=actual\nMODEL_1_NAME=Name\nPROFILE=gemini\nORCHESTRATION_MODEL=controller\nEDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MAX_REFERENCES=2\nEDIT_MASKS=false";
    let raw = parse_env_config(text).unwrap();
    assert_eq!(raw["profile"], "gemini");
    assert_eq!(raw["orchestration_model"], "controller");
    assert_eq!(raw["edit"]["masks"], false);
    assert_eq!(parse_env_config("API_KEY=\n").unwrap()["api_key"], "");
    assert_eq!(
        parse_env_config("MODEL_1_ID=\nMODEL_1_NAME=\n").unwrap()["models"],
        json!([])
    );
}

#[tokio::test]
async fn inline_header_credentials_pin_independent_copies_and_declared_file_failures_are_redacted()
{
    let workspace = Workspace::new();
    let runtime = RuntimePaths::new(&workspace.0).unwrap();
    let connection =
        json!({"auth":{"type":"header","header":"x-api-key","apiKey":"synthetic-inline"}});
    let pinned = snapshot_credentials(&connection, &HashMap::new(), &runtime)
        .await
        .unwrap();
    let copy = pinned.clone();
    let mut headers = copy.headers();
    headers.insert("x-api-key".into(), "changed".into());
    assert_eq!(pinned.headers()["x-api-key"], "synthetic-inline");
    assert_eq!(copy.headers(), pinned.headers());
    assert_eq!(copy.value(), &connection);
    assert!(
        credentials(&json!({"auth":{"type":"none"}}), &HashMap::new(), &runtime)
            .await
            .unwrap()
            .is_empty()
    );
    for path in [workspace.0.clone(), workspace.0.join("absent")] {
        let error = credentials(
            &json!({"auth":{"type":"bearer","secretFile":path.to_str().expect("fixture path")}}),
            &HashMap::new(),
            &runtime,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "missing_credentials");
        assert!(!error.message.contains("synthetic-inline"));
    }
    let missing = credentials(
        &json!({"auth":{"type":"bearer"}}),
        &HashMap::new(),
        &runtime,
    )
    .await
    .unwrap_err();
    assert_eq!(missing.code, "missing_credentials");
    let missing_header = credentials(
        &json!({"auth":{"type":"header","apiKey":"synthetic-inline"}}),
        &HashMap::new(),
        &runtime,
    )
    .await;
    invalid(missing_header);
    invalid(
        credentials(
            &json!({"auth":{"type":"bearer","secretDpapiFile":"relative"}}),
            &HashMap::new(),
            &runtime,
        )
        .await,
    );
    #[cfg(not(windows))]
    {
        invalid(credentials(&json!({"auth":{"type":"bearer","secretDpapiFile":workspace.0.join("unsupported").to_str().expect("fixture path")}}),&HashMap::new(),&runtime).await);
    }
}
