#[cfg(any(target_os = "linux", target_os = "macos"))]
mod common;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod posix {
    use super::common::Workspace;
    use numera_image_gen_mcp::{
        compat::value::{Value, json},
        config::{Config, credentials, load_config},
        files::{images::decode_base64, paths},
        runtime_paths::RuntimePaths,
    };
    use std::{
        collections::HashMap,
        fs::{self, OpenOptions, Permissions},
        io::Write,
        os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
        path::Path,
    };

    fn private_file(path: &Path, text: &str) {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    fn original() -> Vec<u8> {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/image-acceptance.json");
        assert!(fs::metadata(&path).unwrap().len() <= 1048576);
        let rows = Value::parse_with_limit(&fs::read_to_string(path).unwrap(), 1048576).unwrap();
        let row = rows
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "png")
            .unwrap();
        assert_eq!(row["metadata"]["width"], 8);
        assert_eq!(row["metadata"]["height"], 6);
        decode_base64(row["base64"].as_str().unwrap(), 1048576).unwrap()
    }

    #[test]
    fn exact_source_owner_only_credentials_and_unicode_config_paths() {
        let workspace = Workspace::new();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let file = workspace.0.join("private key 日本語.txt");
        private_file(&file, "posix-fixture-key");
        let config = Config::parse(
            &json!({
                "schemaVersion":1,"defaultConnection":"local",
                "outputDir":workspace.0.join("outputs").to_str().unwrap(),
                "stateDir":workspace.0.join("state").to_str().unwrap(),
                "logging":{"directory":workspace.0.join("logs").to_str().unwrap()},
                "files":{"allowedInputRoots":[workspace.0.join("inputs").to_str().unwrap()]},
                "connections":{"local":{
                    "adapter":"openai-images","baseUrl":"https://api.example/v1",
                    "auth":{"type":"bearer","secretFile":file.to_str().unwrap()}
                }}
            })
            .to_string(),
            &workspace.0.join("config.json"),
            &[],
            &HashMap::new(),
        )
        .unwrap();
        let (_, connection) = config.select(None).unwrap();
        let paths = RuntimePaths::new(&workspace.0).unwrap();
        let env = HashMap::new();
        let headers = runtime
            .block_on(credentials(&connection, &env, &paths))
            .unwrap();
        assert_eq!(
            headers,
            HashMap::from([(
                "Authorization".to_owned(),
                "Bearer posix-fixture-key".to_owned(),
            )])
        );
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(&file, Permissions::from_mode(0o644)).unwrap();
        let error = runtime
            .block_on(credentials(&connection, &env, &paths))
            .unwrap_err();
        assert_eq!(error.code, "missing_credentials");
        let config_file = workspace.0.join("config 日本語.json");
        private_file(
            &config_file,
            &json!({
                "api_endpoint":"https://fixture.example/v1",
                "api_key":"fixture-inline",
                "models":[{"id":"image/opaque","name":"日本語"}]
            })
            .to_string(),
        );
        let loaded = load_config(
            &["--config".into(), config_file.to_str().unwrap().into()],
            &HashMap::new(),
            &workspace.0,
        )
        .unwrap();
        assert_eq!(
            loaded.value["outputDir"].as_str().unwrap(),
            workspace.0.join("outputs").to_str().unwrap(),
        );
    }

    #[test]
    fn exact_source_posix_escape_symlink_rejects_and_safe_symlink_reads_original() {
        let workspace = Workspace::new();
        let input = workspace.0.join("inputs");
        fs::create_dir(&input).unwrap();
        let outside = workspace.0.join("outside.png");
        let bytes = original();
        fs::write(&outside, &bytes).unwrap();
        let alias = input.join("escape.png");
        symlink(&outside, &alias).unwrap();
        let error =
            paths::input(alias.to_str().unwrap(), std::slice::from_ref(&input), 1024).unwrap_err();
        assert_eq!(error.code, "permission_denied");
        let inside = input.join("inside.png");
        fs::write(&inside, &bytes).unwrap();
        let safe = input.join("safe.png");
        symlink(&inside, &safe).unwrap();
        assert_eq!(
            paths::input(safe.to_str().unwrap(), &[input], 1024).unwrap(),
            bytes,
        );
    }
}
