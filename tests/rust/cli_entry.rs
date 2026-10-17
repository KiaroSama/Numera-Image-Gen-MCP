mod common;
#[path = "common/config.rs"]
mod config_fixture;
use common::Workspace;
use numera_image_gen_mcp::compat::value::{Value, json};
use std::{collections::HashMap, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

async fn run(
    root: &std::path::Path,
    args: &[String],
    input: &[u8],
    env: &HashMap<String, String>,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_numera-image-gen-mcp"));
    command
        .args(args)
        .current_dir(root)
        .env_remove("NUMERA_CONFIG")
        .env_remove("NUMERA_HTTP_PORT")
        .env_remove("NUMERA_HTTP_TOKEN")
        .envs(env)
        .env("TEMP", root)
        .env("TMP", root)
        .env("TMPDIR", root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().unwrap();
    let pid = child.id().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input).await.unwrap();
    drop(stdin);
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .unwrap();
        assert!(!status.success());
    }
    #[cfg(windows)]
    let _ = pid;
    output
}

#[tokio::test]
async fn actual_native_menu_maintenance_and_invalid_http_flags_preserve_host_config() {
    let workspace = Workspace::new();
    let action = async {
        for flag in ["--version", "--help"] {
            let output = run(&workspace.0, &[flag.into()], b"", &HashMap::new()).await;
            assert!(output.status.success());
            assert!(
                String::from_utf8(output.stdout)
                    .unwrap()
                    .contains("numera-image-gen-mcp")
            );
        }
        let output = run(&workspace.0, &["menu".into()], b"4\n", &HashMap::new()).await;
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("1. Start HTTP server"));
        let config = workspace.0.join("config.json");
        std::fs::write(
            &config,
            workspace
                .config("https://invalid.example/v1", "openai-images")
                .to_string(),
        )
        .unwrap();
        let config_path = config.to_string_lossy().into_owned();
        let output = run(
            &workspace.0,
            &["readiness".into(), "--config".into(), config_path.clone()],
            b"",
            &HashMap::new(),
        )
        .await;
        assert!(output.status.success());
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["generation_verified"], false);
        assert_eq!(report["tool_count"], 10);
        for args in [
            vec!["http".into(), "--port=0".into()],
            vec!["readiness".into(), "--unknown".into()],
            vec!["bridge".into(), "--unknown".into()],
        ] {
            let output = run(&workspace.0, &args, b"", &HashMap::new()).await;
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .contains("Server cannot start")
            );
        }
        let host = workspace.0.join("host.json");
        let before =
            json!({"mcpServers":{"other":{"command":"unchanged"}},"model":"unchanged 日本語"});
        std::fs::write(&host, before.to_string()).unwrap();
        let output = run(
            &workspace.0,
            &[
                "register".into(),
                "--client".into(),
                "ClaudeDesktop".into(),
                "--host-config".into(),
                host.to_string_lossy().into_owned(),
                "--config".into(),
                config_path,
                "--dry-run".into(),
            ],
            b"",
            &HashMap::new(),
        )
        .await;
        assert!(output.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(host).unwrap()).unwrap(),
            before
        );
    };
    tokio::time::timeout(Duration::from_secs(20), action)
        .await
        .unwrap();
}
