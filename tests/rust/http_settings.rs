mod common;
use common::Workspace;
use numera_image_gen_mcp::cli::http_settings::{
    port, save_port, split_port_args, token, token_path,
};
use std::{collections::HashMap, io::Write};

#[test]
fn strict_port_precedence_and_argument_split_preserve_image_config_options() {
    let workspace = Workspace::new();
    let env = HashMap::new();
    assert_eq!(port(&workspace.0, &[], &env).unwrap(), 1969);
    save_port(&workspace.0, 2123).unwrap();
    assert_eq!(port(&workspace.0, &[], &env).unwrap(), 2123);
    let env = HashMap::from([("NUMERA_HTTP_PORT".into(), "2345".into())]);
    assert_eq!(port(&workspace.0, &[], &env).unwrap(), 2345);
    let args = vec![
        "--config".into(),
        workspace
            .0
            .join("config.json")
            .to_string_lossy()
            .into_owned(),
        "--port=3456".into(),
        "--log-level".into(),
        "DEBUG".into(),
    ];
    let (selected, remaining) = split_port_args(&args).unwrap();
    assert_eq!(selected, Some(3456));
    assert_eq!(
        remaining,
        vec![
            "--config".to_owned(),
            workspace
                .0
                .join("config.json")
                .to_string_lossy()
                .into_owned(),
            "--log-level".into(),
            "DEBUG".into()
        ]
    );
    assert_eq!(port(&workspace.0, &args, &env).unwrap(), 3456);
    for value in [
        "0",
        "65536",
        "-1",
        "+1",
        "1.5",
        " 1234",
        "1234 ",
        "",
        "0x769",
        "１２３４",
    ] {
        assert!(
            port(&workspace.0, &[format!("--port={value}")], &HashMap::new()).is_err(),
            "{value}"
        );
        assert!(
            port(
                &workspace.0,
                &[],
                &HashMap::from([("NUMERA_HTTP_PORT".into(), value.into())])
            )
            .is_err(),
            "{value}"
        );
    }
    assert!(split_port_args(&["--port".into()]).is_err());
    assert!(split_port_args(&["--port".into(), "--config".into()]).is_err());
    assert_eq!(
        split_port_args(&["--port".into(), "65535".into()])
            .unwrap()
            .0,
        Some(65535)
    );
    assert_eq!(
        split_port_args(&["--port=01969".into()]).unwrap().0,
        Some(1969)
    );
}

#[test]
fn persistent_settings_are_private_bounded_strict_and_never_touch_provider_config() {
    let workspace = Workspace::new();
    let provider = workspace.0.join("config.local.json");
    std::fs::write(&provider, b"owner fixture untouched").unwrap();
    save_port(&workspace.0, 2000).unwrap();
    save_port(&workspace.0, 2001).unwrap();
    assert_eq!(port(&workspace.0, &[], &HashMap::new()).unwrap(), 2001);
    assert_eq!(
        std::fs::read(&provider).unwrap(),
        b"owner fixture untouched"
    );
    let file = workspace.0.join("config/http.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o077,
            0
        );
    }
    for bytes in [
        b"{\"port\":0}".to_vec(),
        b"{\"port\":2123,\"extra\":true}".to_vec(),
        b"{\"port\":\"2123\"}".to_vec(),
        vec![b'x'; 4097],
        vec![0xff],
    ] {
        std::fs::write(&file, bytes).unwrap();
        assert!(port(&workspace.0, &[], &HashMap::new()).is_err());
    }
    assert!(save_port(&workspace.0, 0).is_err());
    std::fs::write(&file, b"{\"port\":2001}\n").unwrap();
    let lock = workspace.0.join("config/.http-settings.lock");
    let mut guard = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock)
        .unwrap();
    guard.try_lock().unwrap();
    guard.write_all(b"fixture lock").unwrap();
    assert!(
        save_port(&workspace.0, 2345).is_err(),
        "concurrent editor must fail closed"
    );
}

#[test]
fn token_is_persistent_random_private_and_environment_override_is_not_written() {
    let workspace = Workspace::new();
    let first = token(&workspace.0, &HashMap::new()).unwrap();
    assert_eq!(first.len(), 64);
    assert!(first.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(token(&workspace.0, &HashMap::new()).unwrap(), first);
    let path = token_path(&workspace.0).unwrap();
    assert!(path.starts_with(std::fs::canonicalize(&workspace.0).unwrap()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
            0
        );
    }
    let env = HashMap::from([(
        "NUMERA_HTTP_TOKEN".into(),
        "synthetic-override-token-0123456789".into(),
    )]);
    assert_eq!(token(&workspace.0, &env).unwrap(), env["NUMERA_HTTP_TOKEN"]);
    assert_eq!(std::fs::read_to_string(path).unwrap(), first);
    for value in [
        "short".into(),
        " ".repeat(32),
        "x".repeat(257),
        format!("{}\n", "x".repeat(32)),
        "日本語".repeat(32),
    ] {
        assert!(
            token(
                &workspace.0,
                &HashMap::from([("NUMERA_HTTP_TOKEN".into(), value)])
            )
            .is_err()
        );
    }
    let other = Workspace::new();
    assert_ne!(token(&other.0, &HashMap::new()).unwrap(), first);
}

#[test]
fn linked_settings_or_token_files_are_rejected_without_changing_the_original() {
    let workspace = Workspace::new();
    save_port(&workspace.0, 2123).unwrap();
    let first = token(&workspace.0, &HashMap::new()).unwrap();
    let settings = workspace.0.join("config/http.json");
    let token_file = token_path(&workspace.0).unwrap();
    std::fs::hard_link(&settings, workspace.0.join("settings-alias")).unwrap();
    std::fs::hard_link(&token_file, workspace.0.join("token-alias")).unwrap();
    assert!(port(&workspace.0, &[], &HashMap::new()).is_err());
    assert!(save_port(&workspace.0, 2345).is_err());
    assert!(token(&workspace.0, &HashMap::new()).is_err());
    assert_eq!(std::fs::read_to_string(&token_file).unwrap(), first);
    assert!(std::fs::read_to_string(settings).unwrap().contains("2123"));
}

#[test]
fn unsafe_roots_directory_targets_corrupt_tokens_and_cli_terminators_fail_closed() {
    let workspace = Workspace::new();
    assert!(port(std::path::Path::new("relative"), &[], &HashMap::new()).is_err());
    assert!(token_path(&workspace.0.join("missing")).is_err());
    assert!(save_port(&workspace.0.join("nested/../escape"), 2123).is_err());
    let directory = workspace.0.join("config");
    std::fs::write(&directory, b"not a directory").unwrap();
    assert!(port(&workspace.0, &[], &HashMap::new()).is_err());
    std::fs::remove_file(&directory).unwrap();
    save_port(&workspace.0, 2123).unwrap();
    let path = token_path(&workspace.0).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(token(&workspace.0, &HashMap::new()).is_err());
    std::fs::remove_dir(&path).unwrap();
    let first = token(&workspace.0, &HashMap::new()).unwrap();
    for bytes in [vec![b'x'; 257], vec![0xff], b"short".to_vec()] {
        std::fs::write(&path, bytes).unwrap();
        assert!(token(&workspace.0, &HashMap::new()).is_err());
    }
    std::fs::write(&path, first).unwrap();
    let args = vec!["--port=2123".into(), "--".into(), "--port=2345".into()];
    let (selected, remaining) = split_port_args(&args).unwrap();
    assert_eq!(selected, Some(2123));
    assert_eq!(remaining, vec!["--".to_owned(), "--port=2345".to_owned()]);
}

#[cfg(unix)]
#[test]
fn settings_lock_and_token_symlinks_or_public_permissions_are_rejected() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let workspace = Workspace::new();
    let target = workspace.0.join("original");
    std::fs::write(&target, b"preserve original").unwrap();
    symlink(&target, workspace.0.join("config")).unwrap();
    assert!(save_port(&workspace.0, 2123).is_err());
    std::fs::remove_file(workspace.0.join("config")).unwrap();
    save_port(&workspace.0, 2123).unwrap();
    let token_file = token_path(&workspace.0).unwrap();
    symlink(&target, &token_file).unwrap();
    assert!(token_path(&workspace.0).is_err());
    assert!(token(&workspace.0, &HashMap::new()).is_err());
    std::fs::remove_file(&token_file).unwrap();
    let first = token(&workspace.0, &HashMap::new()).unwrap();
    std::fs::set_permissions(&token_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(token(&workspace.0, &HashMap::new()).is_err());
    assert_eq!(std::fs::read_to_string(&token_file).unwrap(), first);
    let lock = workspace.0.join("config/.http-settings.lock");
    std::fs::remove_file(&lock).unwrap();
    std::fs::hard_link(&target, &lock).unwrap();
    assert!(save_port(&workspace.0, 2345).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"preserve original");
}
