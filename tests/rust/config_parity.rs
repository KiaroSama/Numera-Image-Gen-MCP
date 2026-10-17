use numera_image_gen_mcp::compat::value::Value;
use numera_image_gen_mcp::config::{
    Config, ConfigReload, ReloadEvent, credentials, parse_env_config, snapshot_credentials,
};
use numera_image_gen_mcp::runtime_paths::RuntimePaths;
use std::{collections::HashMap, path::Path};

fn oracle() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/config.json");
    let data =
        std::fs::read(path).expect("CI must generate config oracle from the source baseline");
    assert!(
        data.len() <= 16 * 1024 * 1024,
        "source fixture bundle must stay bounded"
    );
    let cases = Value::parse_with_limit(
        std::str::from_utf8(&data).expect("UTF8 oracle"),
        16 * 1024 * 1024,
    )
    .expect("valid generated config oracle");
    for group in ["parse", "select", "dotenv"] {
        assert!(
            !cases[group]
                .as_array()
                .expect("required oracle group")
                .is_empty(),
            "empty {group} must not pass parity"
        );
    }
    cases
}

fn parse(case: &Value) -> numera_image_gen_mcp::error::Result<Config> {
    let args: Vec<String> =
        serde_json::from_value(case["args"].to_serde().expect("Unicode fixture args"))
            .expect("fixture args");
    let env: HashMap<String, String> =
        serde_json::from_value(case["env"].to_serde().expect("Unicode fixture env"))
            .expect("fixture env");
    Config::parse(
        case["text"].as_str().expect("source text"),
        Path::new(case["file"].as_str().expect("source path")),
        &args,
        &env,
    )
}

fn outcome(result: numera_image_gen_mcp::error::Result<Value>) -> Value {
    match result {
        Ok(value) => numera_image_gen_mcp::compat::value::json!({"value": value}),
        Err(error) => numera_image_gen_mcp::compat::value::json!({"error": error.code}),
    }
}

#[test]
fn source_generated_defaults_compact_advanced_and_overrides_match() {
    let cases = oracle();
    for case in cases["parse"].as_array().unwrap() {
        assert_eq!(
            outcome(parse(case).map(Config::into_value)),
            case["expected"],
            "{}",
            case["name"]
        );
    }
}

#[test]
fn generated_deep_unknown_records_survive_parse_selection_clone_and_release() {
    let cases = oracle();
    let case = cases["parse"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "nested-unknown-512")
        .expect("required deep source oracle");
    let config = parse(case).expect("deep source configuration");
    let copy = config.clone();
    let (_, connection) = copy
        .select(Some("default"))
        .expect("deep connection selection");
    assert_eq!(
        connection["extensions"],
        case["expected"]["value"]["connections"]["default"]["extensions"]
    );
    numera_image_gen_mcp::config::drop_json(connection);
    drop(copy);
    assert_eq!(config.value, case["expected"]["value"]);
    let path = Path::new(case["file"].as_str().unwrap());
    let oversized = " ".repeat(1024 * 1024 + 1);
    let error = Config::parse(&oversized, path, &[], &HashMap::new())
        .err()
        .expect("bounded config rejection");
    assert_eq!(error.code, "invalid_configuration");
}

#[test]
fn source_generated_strict_dotenv_matches() {
    let cases = oracle();
    for case in cases["dotenv"].as_array().unwrap() {
        assert_eq!(
            outcome(parse_env_config(case["text"].as_str().unwrap())),
            case["expected"],
            "{}",
            case["name"]
        );
    }
}

#[test]
fn source_generated_connection_selection_matches() {
    let cases = oracle();
    for case in cases["select"].as_array().unwrap() {
        let input = cases["parse"]
            .as_array()
            .unwrap()
            .iter()
            .find(|input| input["name"] == case["config"])
            .expect("named config fixture exists");
        let result = parse(input).and_then(|config| config.select(case["connection"].as_str()));
        let actual = match result {
            Ok((id, value)) => {
                numera_image_gen_mcp::compat::value::json!({"id": id, "value": value})
            }
            Err(error) => numera_image_gen_mcp::compat::value::json!({"error": error.code}),
        };
        assert_eq!(actual, case["expected"], "{}", case["name"]);
    }
}

struct Workspace(std::path::PathBuf);
impl Workspace {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".ci-work/config-tests")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).expect("isolated project-contained state");
        Self(root)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn generated_valid() -> Config {
    let cases = oracle();
    cases["parse"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|case| parse(case).ok())
        .expect("oracle must contain a valid source config")
}

#[test]
fn reload_retains_last_good_fixed_policy_and_sixty_four_snapshots() {
    let workspace = Workspace::new();
    let file = workspace.0.join("config.json");
    let mut original = generated_valid();
    original.file = file.clone();
    let mut reload = ConfigReload::new(original.clone(), file, vec![], HashMap::new());
    assert_eq!(
        reload.refresh_text(Some("{"), |_| true).unwrap().event,
        ReloadEvent::Invalid
    );
    assert_eq!(reload.current().value, original.value);
    assert_eq!(
        reload.refresh_text(Some("{"), |_| true).unwrap().event,
        ReloadEvent::Unchanged
    );
    assert_eq!(
        reload.refresh_text(None, |_| true).unwrap().event,
        ReloadEvent::Invalid
    );
    assert_eq!(
        reload.refresh_text(None, |_| true).unwrap().event,
        ReloadEvent::Unchanged
    );
    let mut refusal = original.value.clone();
    refusal["returnMode"] = numera_image_gen_mcp::compat::value::json!("files_and_preview");
    assert_eq!(
        reload
            .refresh_text(Some(&refusal.to_string()), |_| false)
            .unwrap()
            .event,
        ReloadEvent::SnapshotLimit
    );
    assert_eq!(reload.snapshots().len(), 1);
    let mut fixed = original.value.clone();
    fixed["stateDir"] = numera_image_gen_mcp::compat::value::json!(
        workspace.0.join("changed").to_str().expect("fixture path")
    );
    assert_eq!(
        reload
            .refresh_text(Some(&fixed.to_string()), |_| true)
            .unwrap()
            .event,
        ReloadEvent::RestartRequired
    );
    for index in 1..64 {
        let mut candidate = original.value.clone();
        candidate["connections"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap()["gatewayVersion"] =
            numera_image_gen_mcp::compat::value::json!(index.to_string());
        assert_eq!(
            reload
                .refresh_text(Some(&candidate.to_string()), |_| true)
                .unwrap()
                .event,
            ReloadEvent::Accepted
        );
    }
    let mut overflow = original.value.clone();
    overflow["returnMode"] = numera_image_gen_mcp::compat::value::json!("files_and_preview");
    assert_eq!(
        reload
            .refresh_text(Some(&overflow.to_string()), |_| true)
            .unwrap()
            .event,
        ReloadEvent::SnapshotLimit
    );
    assert_eq!(reload.snapshots().len(), 64);
    assert_ne!(reload.current().value, overflow);
}

#[tokio::test]
async fn operation_credentials_are_private_pinned_and_do_not_fall_back() {
    let workspace = Workspace::new();
    let runtime = RuntimePaths::new(&workspace.0).unwrap();
    let connection = numera_image_gen_mcp::compat::value::json!({"auth":{"type":"bearer", "secretEnv":"SELECTED"}});
    let mut env = HashMap::from([
        ("SELECTED".into(), "synthetic-one".into()),
        ("OPENAI_API_KEY".into(), "unrelated".into()),
    ]);
    let pinned = snapshot_credentials(&connection, &env, &runtime)
        .await
        .unwrap();
    env.insert("SELECTED".into(), "synthetic-two".into());
    let mut copy = pinned.headers();
    copy.clear();
    assert_eq!(pinned.headers()["Authorization"], "Bearer synthetic-one");
    assert_eq!(
        credentials(&connection, &env, &runtime).await.unwrap()["Authorization"],
        "Bearer synthetic-two"
    );
    assert_eq!(pinned.value(), &connection);
    assert!(!pinned.value().to_string().contains("synthetic-one"));
    env.remove("SELECTED");
    assert_eq!(
        credentials(&connection, &env, &runtime)
            .await
            .unwrap_err()
            .code,
        "missing_credentials"
    );
    for key in ["", "x\nx", "x\rx", "x\0x"] {
        env.insert("SELECTED".into(), key.into());
        assert_eq!(
            credentials(&connection, &env, &runtime)
                .await
                .unwrap_err()
                .code,
            "missing_credentials"
        );
    }
}

#[tokio::test]
async fn declared_secret_files_are_bounded_private_and_pinned_across_changes() {
    let workspace = Workspace::new();
    let runtime = RuntimePaths::new(&workspace.0).unwrap();
    let path = workspace.0.join("private fixture 日本語.txt");
    std::fs::write(&path, "  synthetic-file-one  \n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let connection = numera_image_gen_mcp::compat::value::json!({"auth":{"type":"header", "header":"x-api-key", "secretFile":path.to_str().expect("fixture path")}});
    let pinned = snapshot_credentials(&connection, &HashMap::new(), &runtime)
        .await
        .unwrap();
    std::fs::write(&path, "synthetic-file-two").unwrap();
    assert_eq!(pinned.headers()["x-api-key"], "synthetic-file-one");
    assert_eq!(
        credentials(&connection, &HashMap::new(), &runtime)
            .await
            .unwrap()["x-api-key"],
        "synthetic-file-two"
    );
    std::fs::write(&path, vec![b'x'; 65537]).unwrap();
    assert_eq!(
        credentials(&connection, &HashMap::new(), &runtime)
            .await
            .unwrap_err()
            .code,
        "missing_credentials"
    );
    let relative = numera_image_gen_mcp::compat::value::json!({"auth":{"type":"bearer", "secretFile":"relative"}});
    assert_eq!(
        credentials(&relative, &HashMap::new(), &runtime)
            .await
            .unwrap_err()
            .code,
        "invalid_configuration"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&path, "synthetic-file-three").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            credentials(&connection, &HashMap::new(), &runtime)
                .await
                .unwrap_err()
                .code,
            "missing_credentials"
        );
    }
}

#[cfg(windows)]
#[tokio::test]
async fn source_generated_current_user_dpapi_format_round_trips_without_a_shell() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let data = std::fs::read(root.join(".ci-work/parity/config-dpapi.json"))
        .expect("CI must generate same-user PowerShell DPAPI oracle");
    let case: Value = serde_json::from_slice(&data).unwrap();
    let workspace = Workspace::new();
    let runtime = RuntimePaths::new(&workspace.0).unwrap();
    let source = Path::new(case["file"].as_str().expect("declared fixture file"));
    assert!(
        source.starts_with(root.join(".ci-work")),
        "DPAPI fixture must stay inside CI workspace"
    );
    let connection = numera_image_gen_mcp::compat::value::json!({"auth":{"type":"bearer", "secretDpapiFile":source.to_str().expect("fixture path")}});
    let actual = credentials(&connection, &HashMap::new(), &runtime)
        .await
        .unwrap();
    assert_eq!(
        numera_image_gen_mcp::compat::value::json!(actual),
        case["expected"]
    );
    let corrupt = workspace.0.join("corrupt-dpapi.txt");
    std::fs::write(&corrupt, "not-dpapi").unwrap();
    let connection = numera_image_gen_mcp::compat::value::json!({"auth":{"type":"bearer", "secretDpapiFile":corrupt.to_str().expect("fixture path")}});
    assert_eq!(
        credentials(&connection, &HashMap::new(), &runtime)
            .await
            .unwrap_err()
            .code,
        "missing_credentials"
    );
}
