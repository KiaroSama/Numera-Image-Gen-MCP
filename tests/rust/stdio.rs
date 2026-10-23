use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

struct Host {
    child: std::process::Child,
    lines: mpsc::Receiver<String>,
    reader: Option<std::thread::JoinHandle<()>>,
    root: PathBuf,
}
impl Host {
    fn start() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".ci-work/bridge-tests")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("caller")).unwrap();
        let name = if cfg!(windows) {
            "numera-image-gen-mcp.exe"
        } else {
            "numera-image-gen-mcp"
        };
        let executable = root.join("bin").join(name);
        std::fs::copy(env!("CARGO_BIN_EXE_numera-image-gen-mcp"), &executable).unwrap();
        std::fs::write(root.join("config.local.json"),json!({"schemaVersion":1,"defaultConnection":"offline","outputDir":root.join("outputs"),"stateDir":root.join("state"),"logging":{"directory":root.join("logs")},"connections":{"offline":{"adapter":"openai-images","baseUrl":"https://invalid.example/v1","auth":{"type":"none"},"configuredModels":[{"id":"opaque/image","name":"Offline"}]}}}).to_string()).unwrap();
        let mut command = Command::new(executable);
        command
            .arg("bridge")
            .current_dir(root.join("caller"))
            .env_remove("NUMERA_CONFIG")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, lines) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if send.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            child,
            lines,
            reader: Some(reader),
            root,
        }
    }
    fn rpc(&mut self, value: Value) -> Value {
        writeln!(self.child.stdin.as_mut().unwrap(), "{value}").unwrap();
        serde_json::from_str(
            &self
                .lines
                .recv_timeout(Duration::from_secs(8))
                .expect("bounded native protocol response"),
        )
        .expect("stdout only JSON-RPC")
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn bridge_runs_without_launcher_from_foreign_cwd_and_closes_on_eof() {
    let mut host = Host::start();
    let response=host.rpc(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"native-ci","version":"1"}}}));
    assert_eq!(
        response["result"]["serverInfo"]["name"],
        "numera-image-gen-mcp"
    );
    writeln!(
        host.child.stdin.as_mut().unwrap(),
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    let response = host.rpc(json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}));
    let tools = response["result"]["tools"].as_array().expect("ten tools");
    assert_eq!(tools.len(), 10);
    for name in [
        "health_check",
        "list_connections",
        "list_models",
        "get_model_capabilities",
        "generate_image",
        "edit_image",
        "get_job",
        "cancel_job",
        "list_outputs",
        "get_output_info",
    ] {
        assert!(tools.iter().any(|t| t["name"] == name));
    }
    let response=host.rpc(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_connections","arguments":{}}}));
    assert_eq!(
        response["result"]["structuredContent"]["connections"][0]["name"],
        "offline"
    );
    let request = r#"{"jsonrpc":"2.0","id":"\ud800","method":"tools/call","params":{"name":"generate_image","arguments":{"prompt":"\udfff","provider_options":{"\ud800":1e400},"reference_images":[{"type":"path","path":"missing-fixture"}]}}}"#;
    writeln!(host.child.stdin.as_mut().unwrap(), "{request}").unwrap();
    let text = host
        .lines
        .recv_timeout(Duration::from_secs(8))
        .expect("raw tool response");
    let raw = numera_image_gen_mcp::compat::js_json::parse_bounded(&text, 128 * 1024 * 1024)
        .expect("authoritative response");
    assert_eq!(
        raw.get("id")
            .and_then(numera_image_gen_mcp::compat::js_json::JsValue::as_utf16),
        Some([0xd800].as_slice())
    );
    let result = raw.get("result").expect("tool result");
    let error = result
        .get("structuredContent")
        .expect("structured result")
        .get("error")
        .expect("preflight failure");
    assert_ne!(
        error
            .get("message")
            .expect("error message")
            .checked_string()
            .expect("message"),
        "Image request does not match the public request schema."
    );
    assert_eq!(
        result
            .get("isError")
            .and_then(numera_image_gen_mcp::compat::js_json::JsValue::as_bool),
        Some(true)
    );
    assert!(host.root.join(".runtime/temp").is_dir());
    assert!(host.root.join(".runtime/cache").is_dir());
    host.child.stdin.take();
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = host.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "EOF shutdown exceeded deadline"
        );
        // Child exit is not an event on std::process; bounded observation only.
        std::thread::sleep(Duration::from_millis(10));
    }
}
