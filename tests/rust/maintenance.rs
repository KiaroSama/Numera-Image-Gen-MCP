mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/http.rs"]
mod http_fixture;
use common::Workspace;
use http_fixture::{Reply, Server};
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::{cli::readiness, runtime_paths::RuntimePaths};
use std::{collections::HashMap, time::Duration};

#[tokio::test]
async fn readiness_reports_local_probe_and_model_blockers_without_generation() {
    let workspace = Workspace::new();
    let server = Server::new(|_| Reply {
        status: 200,
        value: json!({"data":[{"id":"opaque/image"}]}),
        bytes: None,
    })
    .await;
    let runtime = RuntimePaths::new(&workspace.0).unwrap();
    let file = workspace.0.join("config.json");
    let raw = workspace.config(&format!("{}/v1", server.origin), "openai-images");
    std::fs::write(&file, raw.to_string()).unwrap();
    let bin = workspace.0.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let executable = bin.join(if cfg!(windows) {
        "numera-image-gen-mcp.exe"
    } else {
        "numera-image-gen-mcp"
    });
    std::fs::copy(env!("CARGO_BIN_EXE_numera-image-gen-mcp"), &executable).unwrap();
    let mut env = HashMap::new();
    for key in ["SystemRoot", "WINDIR", "LLVM_PROFILE_FILE"] {
        if let Ok(value) = std::env::var(key) {
            env.insert(key.to_owned(), value);
        }
    }
    let base = vec!["--config".into(), file.to_string_lossy().into_owned()];
    let action = async {
        let (report, ready) = readiness::run(&base, &workspace.0, &env, &runtime, &executable)
            .await
            .unwrap();
        assert!(ready);
        assert_eq!(report["local_ready"], true);
        assert_eq!(report["generation_verified"], false);
        assert!(server.requests.lock().unwrap().is_empty());
        for (model, expected) in [("opaque/image", true), ("missing/image", false)] {
            let mut args = base.clone();
            args.extend(["--probe".into(), "--model".into(), model.into()]);
            let (report, ready) = readiness::run(&args, &workspace.0, &env, &runtime, &executable)
                .await
                .unwrap();
            assert_eq!(ready, expected);
            assert_eq!(report["selected_model_found"], expected);
            assert_eq!(report["generation_verified"], false);
            assert_eq!(report["catalog_count"], 1);
        }
        for args in [
            vec!["--unknown".into()],
            vec!["--config".into()],
            vec!["--config".into(), "relative.json".into()],
        ] {
            assert_eq!(
                readiness::run(&args, &workspace.0, &env, &runtime, &executable)
                    .await
                    .unwrap_err()
                    .code,
                "INVALID_REQUEST"
            );
        }
        assert!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|path| path == "/v1/models")
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(8), action).await;
    server.close().await;
    result.unwrap();
}
