mod common;
#[path = "common/config.rs"]
mod config_fixture;

use common::Workspace;
use numera_image_gen_mcp::{
    compat::value::json,
    config::{Config, ConfigReload, load_config},
    error::Result,
    logging::Logger,
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

// Frozen fc7da25 tests/unit/env-config.test.ts: the preamble and suffixes are exact source inputs.
const VALID: &str = "API_ENDPOINT=https://api.example/v1\nAPI_KEY=\"synthetic-test-key\"\nMODEL_1_ID=family/image\nMODEL_1_NAME=\"تصویر #1\"\n";
const INVALID_SUFFIXES: [&str; 13] = [
    "MODEL_1_ENABLED=yes\n",
    "MODEL_101_ID=overflow\n",
    "MODEL_01_ID=zero\n",
    "MODEL_1_QUALTY=typo\n",
    "PROFILEE=typo\n",
    "API_KEY=duplicate\n",
    "broken line\n",
    "MODEL_1_SIZE=\"unterminated\n",
    "MODEL_1_SIZE=\"512x512\" garbage\n",
    "EDIT_MASKS=true\n",
    "EDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MAX_REFERENCES=1.5\n",
    "EDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MAX_REFERENCES=33\n",
    "EDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MASKS=1\n",
];

fn environment(workspace: &Workspace) -> HashMap<String, String> {
    ["LOCALAPPDATA", "XDG_CONFIG_HOME", "HOME", "USERPROFILE"]
        .into_iter()
        .map(|key| {
            (
                key.to_owned(),
                workspace
                    .0
                    .to_str()
                    .expect("Unicode fixture root")
                    .to_owned(),
            )
        })
        .collect()
}

fn load_file(workspace: &Workspace, name: &str, bytes: &[u8]) -> Result<Config> {
    let file = workspace.0.join(name);
    assert!(file.is_absolute(), "Fixture config path must be absolute.");
    std::fs::write(&file, bytes).expect("write actual fixture config bytes");
    load_config(
        &[
            "--config".into(),
            file.to_str().expect("Unicode fixture path").into(),
        ],
        &environment(workspace),
        &workspace.0,
    )
}

fn invalid_configuration(result: Result<Config>) {
    let error = result.err().expect("Source-invalid file must reject.");
    assert_eq!(error.code, "invalid_configuration");
    assert!(
        !error.to_string().contains("synthetic-test-key"),
        "Configuration error Display must not contain the fixture key."
    );
}

#[test]
fn quoted_env_slots_preserve_source_defaults_and_process_environment() {
    let workspace = Workspace::new();
    let before = std::env::vars_os().collect::<BTreeMap<_, _>>();
    let text = format!(
        "{VALID}MODEL_1_SIZE=\nMODEL_1_QUALITY=\"\"\nMODEL_1_OUTPUT_FORMAT=\nMODEL_2_ID=disabled\nMODEL_2_NAME=Disabled\nMODEL_2_ENABLED=false\n"
    );
    let config = load_file(&workspace, ".env", text.as_bytes()).unwrap();
    let connection = &config.value["connections"]["default"];
    assert_eq!(connection["defaultModel"], "family/image");
    assert_eq!(
        connection["configuredModels"],
        json!([{"id":"family/image","name":"تصویر #1"}])
    );
    assert_eq!(connection["deniedModels"], json!(["disabled"]));
    assert_eq!(
        connection["modelOverrides"]["family/image"]["defaults"],
        json!({})
    );
    assert!(
        std::env::vars_os().collect::<BTreeMap<_, _>>() == before,
        "File loading must not mutate the process environment."
    );
}

#[test]
fn all_thirteen_invalid_env_suffixes_reject_file_loads_without_key_disclosure() {
    let workspace = Workspace::new();
    for suffix in INVALID_SUFFIXES {
        let text = format!("{VALID}{suffix}");
        invalid_configuration(load_file(&workspace, ".env", text.as_bytes()));
    }
}

#[test]
fn gemini_env_declares_exact_header_and_four_reference_edit_contract() {
    let workspace = Workspace::new();
    let text = format!(
        "{VALID}PROFILE=gemini\nEDIT_MODE=json\nEDIT_ENCODING=images\nEDIT_MAX_REFERENCES=4\nEDIT_MASKS=true\nEDIT_MASK_POLARITY=white-edit\n"
    );
    let config = load_file(&workspace, "settings.env", text.as_bytes()).unwrap();
    let connection = &config.value["connections"]["default"];
    assert_eq!(connection["adapter"], "gemini");
    assert_eq!(connection["auth"]["type"], "header");
    assert_eq!(connection["auth"]["header"], "x-goog-api-key");
    assert_eq!(
        connection["edit"],
        json!({
            "mode":"json",
            "encoding":"images",
            "maxReferences":4,
            "masks":true,
            "maskPolarity":"white-edit"
        })
    );
}

#[test]
fn env_file_loader_rejects_source_byte_limits_and_preserves_exported_multiline_name() {
    let workspace = Workspace::new();
    invalid_configuration(load_file(&workspace, ".env", &vec![b' '; 1024 * 1024 + 1]));
    // Deliberately invalid UTF-8: the frozen file-loader rejection fixture is the single byte ff.
    invalid_configuration(load_file(&workspace, ".env", &[0xff]));
    let text = VALID.replace(
        "MODEL_1_NAME=\"تصویر #1\"",
        "export MODEL_1_NAME=\"line one\nline two\" # comment",
    );
    let config = load_file(&workspace, ".env", text.as_bytes()).unwrap();
    assert_eq!(
        config.value["connections"]["default"]["configuredModels"][0]["name"],
        "line one\nline two"
    );
}

#[test]
fn file_reload_retains_initial_arc_for_unchanged_storage_change_and_snapshot_refusal() {
    let workspace = Workspace::new();
    let file = workspace.0.join("config.json");
    let mut raw = workspace.config("https://api.example/v1", "openai-images");
    raw["defaultConnection"] = json!("local");
    raw["connections"] = json!({
        "local":{
            "adapter":"openai-images",
            "baseUrl":"https://api.example/v1",
            "auth":{"type":"none"}
        }
    });
    let config = load_file(&workspace, "config.json", raw.to_string().as_bytes()).unwrap();
    let original_value = config.value.clone();
    let logger = Logger::new(&workspace.0.join("logs"), "ERROR");
    let args = vec![
        "--config".into(),
        file.to_str().expect("Unicode fixture path").into(),
    ];
    let env = environment(&workspace);
    let mut reload = ConfigReload::new(config, file.clone(), args, env);
    let original = reload.current();
    assert_eq!(original.file, file);
    assert_eq!(original.value, original_value);

    let unchanged = reload
        .refresh(&logger, |_| {
            panic!("Unchanged file must not request snapshot admission.")
        })
        .unwrap();
    assert!(Arc::ptr_eq(&unchanged, &original));
    assert_eq!(unchanged.value, original_value);

    let mut changed = original_value.clone();
    changed["stateDir"] = json!(
        workspace
            .0
            .join("different")
            .to_str()
            .expect("Unicode fixture path")
    );
    let loaded_change =
        load_file(&workspace, "config.json", changed.to_string().as_bytes()).unwrap();
    assert_eq!(loaded_change.value["stateDir"], changed["stateDir"]);
    assert_ne!(loaded_change.value["stateDir"], original_value["stateDir"]);
    let fixed_storage = reload
        .refresh(&logger, |_| {
            panic!("Fixed storage changes must reject before admission.")
        })
        .unwrap();
    assert!(Arc::ptr_eq(&fixed_storage, &original));
    assert_eq!(fixed_storage.value, original_value);

    let mut refused = original_value.clone();
    refused["returnMode"] = json!("files_and_preview");
    let loaded_refusal =
        load_file(&workspace, "config.json", refused.to_string().as_bytes()).unwrap();
    assert_eq!(loaded_refusal.value["returnMode"], "files_and_preview");
    let mut admission_checked = false;
    let snapshot_limit = reload
        .refresh(&logger, |candidate| {
            admission_checked = true;
            assert_eq!(candidate.value, loaded_refusal.value);
            false
        })
        .unwrap();
    assert!(
        admission_checked,
        "Snapshot refusal must reach the acceptance callback."
    );
    assert!(Arc::ptr_eq(&snapshot_limit, &original));
    assert_eq!(snapshot_limit.value, original_value);
    assert!(Arc::ptr_eq(&reload.current(), &original));
    assert_eq!(reload.snapshots().len(), 1);
    assert!(Arc::ptr_eq(&reload.snapshots()[0], &original));
    drop(logger);
}
