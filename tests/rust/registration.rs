mod common;
use common::Workspace;
use numera_image_gen_mcp::{
    cli::registration::{merge, register},
    logging::Logger,
};
use serde_json::json;

#[test]
fn owned_native_entry_preserves_unrelated_host_settings_and_toml_bytes() {
    let workspace = Workspace::new();
    let executable = workspace.0.join("numera-image-gen-mcp");
    let config = workspace.0.join("config.json");
    let before =
        "{\"model\":\"unchanged 日本語\",\"mcpServers\":{\"other\":{\"command\":\"stay\"}}}\n";
    let after = merge(before, "ClaudeDesktop", false, &executable, &config).unwrap();
    let value: serde_json::Value = serde_json::from_str(&after).unwrap();
    assert_eq!(value["model"], "unchanged 日本語");
    assert_eq!(value["mcpServers"]["other"]["command"], "stay");
    assert_eq!(
        value["mcpServers"]["numera-image-gen"]["args"],
        json!(["bridge"])
    );
    assert_eq!(
        merge(&after, "ClaudeDesktop", false, &executable, &config).unwrap(),
        after
    );
    let removed = merge(&after, "ClaudeDesktop", true, &executable, &config).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&removed).unwrap(),
        serde_json::from_str::<serde_json::Value>(before).unwrap()
    );
    let toml = "# retained\r\nmodel = \"unchanged\"\r\n[mcp_servers.other]\r\ncommand = \"existing\" # retained\r\n";
    let after = merge(toml, "Codex", false, &executable, &config).unwrap();
    assert!(after.starts_with(toml));
    assert!(after.contains("args = [\"bridge\"]"));
    assert_eq!(
        merge(&after, "Codex", false, &executable, &config).unwrap(),
        after
    );
    assert!(
        merge(&after, "Codex", true, &executable, &config)
            .unwrap()
            .starts_with(toml)
    );
    for ambiguous in [
        "[mcp_servers.\"numera-image-gen\"]\ncommand=\"other\"\n",
        "[mcp_servers.numera-image-gen]\ncommand=\"a\"\n[mcp_servers.numera-image-gen]\ncommand=\"b\"\n",
        "description = \"\"\"\n[mcp_servers.numera-image-gen]\n\"\"\"\n",
        "mcp_servers = { \"numera-image-gen\" = { command = \"other\" } }\n",
    ] {
        assert_eq!(
            merge(ambiguous, "Codex", false, &executable, &config)
                .unwrap_err()
                .code,
            "AMBIGUOUS_TOML"
        );
    }
}

#[test]
fn registration_dry_run_backups_idempotence_and_active_lock_are_safe() {
    let workspace = Workspace::new();
    let executable = workspace.0.join("numera-image-gen-mcp");
    let config = workspace.0.join("config.json");
    let file = workspace.0.join("host.json");
    let before = b"{\"mcpServers\":{\"other\":{\"command\":\"stay\"}}}\n";
    std::fs::write(&file, before).unwrap();
    let logger = Logger::new(&workspace.0.join("logs"), "ERROR");
    register(
        "ClaudeDesktop",
        false,
        &file,
        &executable,
        &config,
        true,
        &logger,
    )
    .unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), before);
    register(
        "ClaudeDesktop",
        false,
        &file,
        &executable,
        &config,
        false,
        &logger,
    )
    .unwrap();
    let after = std::fs::read(&file).unwrap();
    register(
        "ClaudeDesktop",
        false,
        &file,
        &executable,
        &config,
        false,
        &logger,
    )
    .unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), after);
    let backups = std::fs::read_dir(&workspace.0)
        .unwrap()
        .flatten()
        .filter(|v| v.file_name().to_string_lossy().contains("backup"))
        .collect::<Vec<_>>();
    assert_eq!(backups.len(), 1);
    assert_eq!(std::fs::read(backups[0].path()).unwrap(), before);
    let lock = workspace.0.join("host.json.numera-lock");
    std::fs::write(&lock, b"other request").unwrap();
    assert!(
        register(
            "ClaudeDesktop",
            false,
            &file,
            &executable,
            &config,
            false,
            &logger
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&file).unwrap(), after);
    assert_eq!(std::fs::read(&lock).unwrap(), b"other request");
}
