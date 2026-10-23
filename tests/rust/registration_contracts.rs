mod common;
use common::Workspace;
use numera_image_gen_mcp::{
    cli::registration::{merge, register},
    logging::Logger,
};
use serde_json::{Value, json};

#[test]
fn json_host_merge_rejects_invalid_shapes_and_preserves_unrelated_unicode_fields() {
    let workspace = Workspace::new();
    let binary = workspace.0.join("bin/numera-image-gen-mcp");
    let config = workspace.0.join("config.json");
    for text in [
        "{broken",
        "null",
        "[]",
        "2",
        "{\"mcpServers\":null}",
        "{\"mcpServers\":[]}",
    ] {
        assert_eq!(
            merge(text, "ClaudeDesktop", false, &binary, &config)
                .unwrap_err()
                .code,
            "INVALID_HOST_CONFIG"
        );
    }
    assert_eq!(
        merge("{}", "unknown", false, &binary, &config)
            .unwrap_err()
            .code,
        "INVALID_REQUEST"
    );
    for text in ["", "{}", "{\"model\":\"unchanged 日本語\"}"] {
        assert_eq!(
            merge(text, "ClaudeDesktop", true, &binary, &config).unwrap(),
            text
        );
        let added = merge(text, "ClaudeProject", false, &binary, &config).unwrap();
        let value: Value = serde_json::from_str(&added).unwrap();
        assert_eq!(value["mcpServers"]["numera-image-gen"]["type"], "stdio");
        assert_eq!(
            value["mcpServers"]["numera-image-gen"]["args"],
            json!(["bridge"])
        );
        assert_eq!(
            value["mcpServers"]["numera-image-gen"]["env"]["NUMERA_CONFIG"],
            json!(config)
        );
        assert_eq!(
            merge(&added, "ClaudeProject", false, &binary, &config).unwrap(),
            added
        );
    }
    let unrelated = "{\"model\":\"日本語\",\"mcpServers\":{\"other\":{\"command\":\"stay\"}}}";
    assert_eq!(
        merge(unrelated, "ClaudeDesktop", true, &binary, &config).unwrap(),
        unrelated
    );
}

#[test]
fn conservative_toml_merge_handles_nested_owned_sections_and_rejects_ambiguous_syntax() {
    let workspace = Workspace::new();
    let binary = workspace.0.join("bin/server");
    let config = workspace.0.join("config.json");
    for text in [
        "[missing\n",
        "[a] junk\n",
        "[]\n",
        "['quoted']\n",
        "broken\n",
        "[mcp_servers.numera-image-gen]\nx=1\nx=2\n",
        "[mcp_servers.numera-image-gen.env]\nx=1\n",
        "[mcp_servers]\nnumera-image-gen = {}\n",
        "x = [1\n",
        "x = }\n",
        "x = \"unterminated\n",
    ] {
        assert_eq!(
            merge(text, "Codex", false, &binary, &config)
                .unwrap_err()
                .code,
            "AMBIGUOUS_TOML",
            "{text}"
        );
    }
    let original = "# retain\nmodel = \"日本語\"\n[mcp_servers.other]\ncommand = 'keep' # retain\n";
    let with_owned = format!(
        "{original}[mcp_servers.numera-image-gen]\ncommand = \"obsolete\"\n[mcp_servers.numera-image-gen.env]\nX = 'old'\n[another]\na = [1, 2]\nb = {{ key = \"escaped\\\"value\" }}\n"
    );
    let removed = merge(&with_owned, "Codex", true, &binary, &config).unwrap();
    assert!(removed.starts_with(original));
    assert!(removed.ends_with("b = { key = \"escaped\\\"value\" }\n"));
    assert!(!removed.contains("obsolete"));
    assert!(!removed.contains("X = 'old'"));
    let added = merge("model='keep'", "Codex", false, &binary, &config).unwrap();
    assert!(added.starts_with("model='keep'\n[mcp_servers.numera-image-gen]\n"));
    assert_eq!(
        merge(&added, "Codex", false, &binary, &config).unwrap(),
        added
    );
}

#[test]
fn registration_rejects_unsafe_files_and_creates_only_requested_entry_with_recoverable_backup() {
    let workspace = Workspace::new();
    let binary = workspace.0.join("bin/server");
    let config = workspace.0.join("config.json");
    let logger = Logger::new(&workspace.0.join("logs"), "ERROR");
    assert_eq!(
        register(
            "ClaudeDesktop",
            false,
            std::path::Path::new("relative"),
            &binary,
            &config,
            false,
            &logger
        )
        .unwrap_err()
        .code,
        "INVALID_REQUEST"
    );
    let absent = workspace.0.join("new/host.json");
    register(
        "ClaudeDesktop",
        true,
        &absent,
        &binary,
        &config,
        false,
        &logger,
    )
    .unwrap();
    assert!(!absent.exists());
    register(
        "ClaudeDesktop",
        false,
        &absent,
        &binary,
        &config,
        true,
        &logger,
    )
    .unwrap();
    assert!(!absent.exists());
    register(
        "ClaudeDesktop",
        false,
        &absent,
        &binary,
        &config,
        false,
        &logger,
    )
    .unwrap();
    let added: Value = serde_json::from_slice(&std::fs::read(&absent).unwrap()).unwrap();
    assert_eq!(
        added["mcpServers"]["numera-image-gen"]["command"],
        json!(binary)
    );
    register(
        "ClaudeDesktop",
        true,
        &absent,
        &binary,
        &config,
        false,
        &logger,
    )
    .unwrap();
    let removed: Value = serde_json::from_slice(&std::fs::read(&absent).unwrap()).unwrap();
    assert_eq!(removed, json!({"mcpServers":{}}));
    let backups = std::fs::read_dir(absent.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().contains("backup"))
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&backups[0]).unwrap()).unwrap(),
        added
    );
    let directory = workspace.0.join("directory");
    std::fs::create_dir(&directory).unwrap();
    assert_eq!(
        register(
            "ClaudeDesktop",
            false,
            &directory,
            &binary,
            &config,
            false,
            &logger
        )
        .unwrap_err()
        .code,
        "UNSAFE_HOST_PATH"
    );
    let invalid = workspace.0.join("invalid.json");
    std::fs::write(&invalid, [0xff]).unwrap();
    assert_eq!(
        register(
            "ClaudeDesktop",
            false,
            &invalid,
            &binary,
            &config,
            false,
            &logger
        )
        .unwrap_err()
        .code,
        "INVALID_HOST_CONFIG"
    );
    let linked = workspace.0.join("hard-link.json");
    std::fs::hard_link(&absent, &linked).unwrap();
    assert_eq!(
        register(
            "ClaudeDesktop",
            false,
            &linked,
            &binary,
            &config,
            false,
            &logger
        )
        .unwrap_err()
        .code,
        "UNSAFE_HOST_PATH"
    );
    assert_eq!(std::fs::read(&invalid).unwrap(), vec![0xff]);
    assert!(!workspace.0.join("invalid.json.numera-lock").exists());
}
