mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/devtool_signed.rs"]
mod signed_fixture;
use common::Workspace;
use numera_image_gen_mcp::{compat::value::json, devtools::setup};
use std::{collections::HashMap, fs};

#[test]
fn every_setup_dry_run_preserves_host_and_skips_build_smoke_and_state() {
    let workspace = Workspace::new();
    use numera_image_gen_mcp::{
        devtools::supervisor::{Bounds, run},
        logging::Logger,
    };
    use std::{process::Command, sync::atomic::AtomicBool};
    let root = workspace.0.join("installation 日本語");
    fs::create_dir(&root).unwrap();
    let logger = Logger::new(&workspace.0.join("logs"), "INFO");
    let config = root.join("config 日本語.json");
    let mut value = workspace.config("https://invalid.example/v1", "openai-images");
    value["outputDir"] = json!(root.join("outputs").to_str().unwrap());
    value["stateDir"] = json!(root.join("state").to_str().unwrap());
    value["logging"]["directory"] = json!(root.join("logs").to_str().unwrap());
    fs::write(&config, value.to_string()).unwrap();
    let host = root.join("host.json");
    let before =
        json!({"retained":true,"mcpServers":{"other":{"command":"unchanged"}}}).to_string();
    fs::write(&host, &before).unwrap();
    let artifact = root.join("prepared");
    fs::create_dir(&artifact).unwrap();
    // Dry-run validates the chosen directory, never treats it as a signed installation.
    for action in ["Install", "Update", "Diagnose", "Register", "Remove"] {
        let mut args = vec![
            action.into(),
            "--root".into(),
            root.to_string_lossy().into_owned(),
            "--dry-run".into(),
        ];
        match action {
            "Install" | "Update" => {
                args.extend(["--artifact".into(), artifact.to_string_lossy().into_owned()])
            }
            "Diagnose" => args.extend(["--config".into(), config.to_string_lossy().into_owned()]),
            _ => args.extend([
                "--client".into(),
                "ClaudeDesktop".into(),
                "--host-config".into(),
                host.to_string_lossy().into_owned(),
                "--config".into(),
                config.to_string_lossy().into_owned(),
            ]),
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_numera-dev"));
        command.arg("setup").args(&args);
        let result = run(
            &mut command,
            &workspace.0,
            Bounds::new(5000, 2000).unwrap(),
            &AtomicBool::new(false),
            &logger,
        )
        .unwrap();
        assert_eq!(result.code, 0, "Dry-run failed for {action}.");
        assert!(result.cleanup_verified);
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(
            report,
            serde_json::json!({"action":action,"dry_run":true,"changed":false})
        );
        assert!(
            !String::from_utf8(result.stderr)
                .unwrap()
                .contains(root.to_str().unwrap())
        );
        assert_eq!(fs::read_to_string(&host).unwrap(), before);
        for forbidden in [
            "node_modules",
            "dist",
            "state",
            "outputs",
            ".runtime",
            ".numera-install.lock",
        ] {
            assert!(
                !root.join(forbidden).exists(),
                "Dry-run created a forbidden side effect."
            );
        }
        assert!(!fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("numera-backup")
        }));
        assert!(!host.with_file_name("host.json.numera-lock").exists());
    }
}

#[test]
fn setup_rejects_unknown_duplicate_and_inapplicable_options_before_mutation() {
    let workspace = Workspace::new();
    for arguments in [
        vec!["Unknown".into()],
        vec![
            "Diagnose".into(),
            "--root".into(),
            "relative".into(),
            "--dry-run".into(),
        ],
        vec![
            "Install".into(),
            "--root".into(),
            workspace.0.to_string_lossy().into_owned(),
            "--dry-run".into(),
            "--dry-run".into(),
        ],
        vec![
            "Remove".into(),
            "--root".into(),
            workspace.0.to_string_lossy().into_owned(),
            "--probe".into(),
        ],
    ] {
        assert!(setup::execute(&arguments, &workspace.0.join("app"), &HashMap::new()).is_err());
    }
    assert_eq!(fs::read_dir(&workspace.0).unwrap().count(), 0);
}

#[test]
fn signed_prepared_install_and_update_preserve_private_files_without_compilation() {
    let workspace = Workspace::new();
    let source = workspace.0.join("prepared");
    let target = workspace.0.join("installed");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    signed_fixture::prepared(&source);
    fs::write(
        target.join("config.local.json"),
        b"private fixture unchanged",
    )
    .unwrap();
    let executable = target.join("bin/numera-image-gen-mcp");
    for action in ["Install", "Update"] {
        if action == "Update" {
            signed_fixture::variant(&source, true);
        }
        let args = vec![
            action.into(),
            "--root".into(),
            target.to_string_lossy().into_owned(),
            "--artifact".into(),
            source.to_string_lossy().into_owned(),
        ];
        let report = setup::execute(&args, &executable, &HashMap::new()).unwrap();
        assert!(!report.dry_run);
        assert_eq!(
            fs::read(target.join("config.local.json")).unwrap(),
            b"private fixture unchanged"
        );
        assert_eq!(
            fs::read(target.join("manifest.json")).unwrap(),
            fs::read(source.join("manifest.json")).unwrap()
        );
        let installed = numera_image_gen_mcp::devtools::artifact::verify(&target).unwrap();
        assert_eq!(
            installed.manifest.version,
            if action == "Update" { "0.2.0" } else { "0.1.0" }
        );
        let binary = if cfg!(windows) {
            "bin/numera-image-gen-mcp.exe"
        } else {
            "bin/numera-image-gen-mcp"
        };
        assert_eq!(
            fs::read(target.join(binary)).unwrap(),
            fs::read(source.join(binary)).unwrap()
        );
        assert!(!target.join("node_modules").exists());
        assert!(!target.join(".numera-install.lock").exists());
        assert!(!fs::read_dir(&target).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".numera-staging-")
        }));
    }
}

#[test]
fn actual_diagnose_cli_reports_only_local_readiness_and_never_generation() {
    use numera_image_gen_mcp::{
        devtools::supervisor::{Bounds, run},
        logging::Logger,
    };
    use std::{process::Command, sync::atomic::AtomicBool};
    let workspace = Workspace::new();
    let config = workspace.0.join("config.json");
    fs::write(
        &config,
        workspace
            .config("https://invalid.example/v1", "openai-images")
            .to_string(),
    )
    .unwrap();
    let bin = workspace.0.join("bin");
    fs::create_dir(&bin).unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_numera-image-gen-mcp"),
        bin.join(if cfg!(windows) {
            "numera-image-gen-mcp.exe"
        } else {
            "numera-image-gen-mcp"
        }),
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_numera-dev"));
    command
        .args(["setup", "Diagnose", "--root"])
        .arg(&workspace.0)
        .arg("--config")
        .arg(&config);
    let logger = Logger::new(&workspace.0.join("logs"), "INFO");
    let result = run(
        &mut command,
        &workspace.0,
        Bounds::new(5000, 2000).unwrap(),
        &AtomicBool::new(false),
        &logger,
    )
    .unwrap();
    assert_eq!(result.code, 0);
    assert!(result.cleanup_verified);
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["local_ready"], true);
    assert_eq!(report["credentials_ready"], true);
    assert_eq!(report["generation_verified"], false);
    assert!(report["connection_ready"].is_null());
    assert_eq!(report["tool_count"], 10);
    assert_eq!(report["blockers"], serde_json::json!([]));
    assert!(!workspace.0.join("node_modules").exists());
    assert!(!workspace.0.join("manifest.json").exists());
}

#[test]
fn installation_lock_and_unowned_collision_preserve_destination_bytes() {
    let workspace = Workspace::new();
    let source = workspace.0.join("prepared");
    let target = workspace.0.join("installed");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    signed_fixture::prepared(&source);
    let executable = target.join("bin/numera-image-gen-mcp");
    let args = vec![
        "Install".into(),
        "--root".into(),
        target.to_string_lossy().into_owned(),
        "--artifact".into(),
        source.to_string_lossy().into_owned(),
    ];
    let lock = target.join(".numera-install.lock");
    fs::write(&lock, b"another installer owns this lock").unwrap();
    let error = setup::execute(&args, &executable, &HashMap::new())
        .err()
        .unwrap();
    assert_eq!(error.code, "installation_blocked");
    assert_eq!(
        fs::read(&lock).unwrap(),
        b"another installer owns this lock"
    );
    assert!(!target.join("manifest.json").exists());
    fs::remove_file(&lock).unwrap();
    fs::create_dir_all(target.join("docs/assets")).unwrap();
    fs::write(target.join("docs/assets/logo.png"), b"unowned user file").unwrap();
    assert!(setup::execute(&args, &executable, &HashMap::new()).is_err());
    assert_eq!(
        fs::read(target.join("docs/assets/logo.png")).unwrap(),
        b"unowned user file"
    );
    assert!(!target.join("bin").exists());
    assert!(!target.join("manifest.json").exists());
    assert!(!lock.exists());
    assert!(!fs::read_dir(&target).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".numera-staging-")
    }));
}

#[cfg(windows)]
#[test]
fn locked_second_update_member_rolls_back_and_releases_owned_staging() {
    use std::os::windows::fs::OpenOptionsExt;
    let workspace = Workspace::new();
    let source = workspace.0.join("prepared");
    let target = workspace.0.join("installed");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&target).unwrap();
    signed_fixture::variant(&source, true);
    signed_fixture::prepared(&target);
    let names = [
        "bin/numera-image-gen-mcp.exe",
        "docs/assets/logo.png",
        "manifest.json",
        "manifest.sig",
    ];
    let before = names.map(|name| fs::read(target.join(name)).unwrap());
    // Sharing READ permits authenticity checks but excludes DELETE/rename on the second member.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(target.join(names[1]))
        .unwrap();
    let args = vec![
        "Update".into(),
        "--root".into(),
        target.to_string_lossy().into_owned(),
        "--artifact".into(),
        source.to_string_lossy().into_owned(),
    ];
    let error = setup::execute(&args, &target.join(names[0]), &HashMap::new())
        .err()
        .unwrap();
    assert_eq!(error.code, "installation_blocked");
    assert!(error.message.contains("rollback required"));
    for (name, bytes) in names.iter().zip(before) {
        assert_eq!(fs::read(target.join(name)).unwrap(), bytes);
    }
    drop(held);
    assert!(!target.join(".numera-install.lock").exists());
    assert!(!fs::read_dir(&target).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".numera-staging-")
    }));
    assert!(numera_image_gen_mcp::devtools::artifact::verify(&target).is_ok());
}

#[test]
fn actual_setup_cli_registers_selected_installation_not_unpacked_sibling() {
    use numera_image_gen_mcp::{
        devtools::supervisor::{Bounds, run},
        logging::Logger,
    };
    use std::{process::Command, sync::atomic::AtomicBool};
    let workspace = Workspace::new();
    let root = workspace.0.join("installed 日本語");
    fs::create_dir(&root).unwrap();
    signed_fixture::prepared(&root);
    let config = workspace.0.join("config.json");
    fs::write(
        &config,
        workspace
            .config("https://invalid.example/v1", "openai-images")
            .to_string(),
    )
    .unwrap();
    let host = workspace.0.join("host.json");
    fs::write(&host, b"{\"retained\":true}").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_numera-dev"));
    command
        .args(["setup", "Register", "--root"])
        .arg(&root)
        .args(["--client", "ClaudeDesktop", "--host-config"])
        .arg(&host)
        .arg("--config")
        .arg(&config);
    let logger = Logger::new(&workspace.0.join("logs"), "INFO");
    let result = run(
        &mut command,
        &workspace.0,
        Bounds::new(5000, 2000).unwrap(),
        &AtomicBool::new(false),
        &logger,
    )
    .unwrap();
    assert_eq!(result.code, 0);
    assert!(result.cleanup_verified);
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&host).unwrap()).unwrap();
    let expected = fs::canonicalize(&root)
        .unwrap()
        .join("bin")
        .join(if cfg!(windows) {
            "numera-image-gen-mcp.exe"
        } else {
            "numera-image-gen-mcp"
        });
    assert_eq!(
        value["mcpServers"]["numera-image-gen"]["command"],
        serde_json::json!(expected)
    );
    assert_eq!(value["retained"], true);
}

#[test]
fn native_register_and_remove_change_only_selected_fixture_entry() {
    let workspace = Workspace::new();
    let root = &workspace.0;
    let host = root.join("host.json");
    let config = root.join("config.json");
    fs::write(
        &config,
        workspace
            .config("https://invalid.example/v1", "openai-images")
            .to_string(),
    )
    .unwrap();
    fs::write(
        &host,
        "{\"retained\":true,\"mcpServers\":{\"other\":{\"command\":\"unchanged\"}}}",
    )
    .unwrap();
    let args = |action: &str| {
        vec![
            action.into(),
            "--root".into(),
            root.to_string_lossy().into_owned(),
            "--client".into(),
            "ClaudeDesktop".into(),
            "--host-config".into(),
            host.to_string_lossy().into_owned(),
            "--config".into(),
            config.to_string_lossy().into_owned(),
        ]
    };
    signed_fixture::prepared(root);
    let executable = root.join("bin").join(if cfg!(windows) {
        "numera-image-gen-mcp.exe"
    } else {
        "numera-image-gen-mcp"
    });
    setup::execute(&args("Register"), &executable, &HashMap::new()).unwrap();
    let registered: serde_json::Value = serde_json::from_slice(&fs::read(&host).unwrap()).unwrap();
    assert_eq!(registered["retained"], true);
    assert_eq!(registered["mcpServers"]["other"]["command"], "unchanged");
    assert_eq!(
        registered["mcpServers"]["numera-image-gen"]["args"],
        serde_json::json!(["bridge"])
    );
    setup::execute(&args("Remove"), &executable, &HashMap::new()).unwrap();
    let removed: serde_json::Value = serde_json::from_slice(&fs::read(&host).unwrap()).unwrap();
    assert_eq!(
        removed,
        serde_json::json!({"retained":true,"mcpServers":{"other":{"command":"unchanged"}}})
    );
}
