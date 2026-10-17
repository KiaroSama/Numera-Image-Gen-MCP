mod common;
#[path = "common/config.rs"]
mod config_fixture;

use common::Workspace;
use numera_image_gen_mcp::compat::value::{Value, json};
use std::{
    path::PathBuf,
    process::{ExitStatus, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::Command,
    time::{Instant, timeout, timeout_at},
};
use tokio_util::sync::CancellationToken;

type FixtureResult<T> = std::result::Result<T, String>;

// Frozen fc7da25 tests/integration/readiness.test.ts:44-209 supplies these four scenarios.
// Native readiness is a direct maintenance command, not an inner MCP/stdio child. Reaping below
// proves the actual executable closed; it does not invent the source wrapper's cleanup log marker.
struct Fixture {
    workspace: Workspace,
    executable: PathBuf,
    listener: TcpListener,
    origin: String,
}

impl Fixture {
    async fn new() -> Self {
        let workspace = Workspace::new();
        for directory in ["bin", "caller", "temp"] {
            std::fs::create_dir(workspace.0.join(directory)).unwrap();
        }
        let name = if cfg!(windows) {
            "numera-image-gen-mcp.exe"
        } else {
            "numera-image-gen-mcp"
        };
        let executable = workspace.0.join("bin").join(name);
        std::fs::copy(env!("CARGO_BIN_EXE_numera-image-gen-mcp"), &executable).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        Self {
            workspace,
            executable,
            listener,
            origin,
        }
    }

    fn configuration(&self, auth: Value) -> Vec<String> {
        let mut raw = self
            .workspace
            .config(&format!("{}/v1", self.origin), "openai-images");
        raw["defaultConnection"] = json!("local");
        raw["connections"] = json!({
            "local":{
                "adapter":"openai-images",
                "baseUrl":format!("{}/v1", self.origin),
                "auth":auth
            }
        });
        let file = self.workspace.0.join("config.json");
        std::fs::write(&file, raw.to_string().as_bytes()).unwrap();
        vec!["--config".into(), file.to_str().unwrap().into()]
    }
}

struct Captured {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

async fn captured_bytes(stream: impl AsyncRead + Unpin) -> FixtureResult<Vec<u8>> {
    let mut bytes = Vec::new();
    stream
        .take(1048577)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "Cannot capture owned readiness output.".to_owned())?;
    if bytes.len() > 1048576 {
        return Err("Readiness output exceeded the 1 MiB fixture bound.".into());
    }
    Ok(bytes)
}

async fn readiness(
    fixture: &Fixture,
    args: &[String],
    deadline: Instant,
) -> FixtureResult<Captured> {
    let budget = deadline
        .saturating_duration_since(Instant::now())
        .saturating_sub(Duration::from_secs(2))
        .min(Duration::from_secs(5));
    if budget.is_zero() {
        return Err("Readiness case exhausted its child budget.".into());
    }
    let root = &fixture.workspace.0;
    let mut command = Command::new(&fixture.executable);
    command
        .arg("readiness")
        .args(args)
        .current_dir(root.join("caller"))
        .env_clear()
        .env("LOCALAPPDATA", root)
        .env("XDG_CONFIG_HOME", root)
        .env("HOME", root)
        .env("USERPROFILE", root)
        .env("TEMP", root.join("temp"))
        .env("TMP", root.join("temp"))
        .env("TMPDIR", root.join("temp"))
        .env("NUMERA_LOG_DIR", root.join("logs"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Preserve only OS prerequisites and the CI-owned instrumentation destination, never provider
    // credentials, proxies, host settings or application overrides from the parent environment.
    for key in ["SystemRoot", "WINDIR", "LLVM_PROFILE_FILE"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().map_err(|error| {
        format!(
            "Cannot start owned readiness executable: kind={:?}, os_code={:?}.",
            error.kind(),
            error.raw_os_error()
        )
    })?;
    let stdout = child.stdout.take().expect("owned stdout pipe");
    let stderr = child.stderr.take().expect("owned stderr pipe");
    let capture = async {
        let (status, stdout, stderr) =
            tokio::join!(child.wait(), captured_bytes(stdout), captured_bytes(stderr),);
        Ok::<_, String>((
            status.map_err(|_| "Cannot reap owned readiness executable.".to_owned())?,
            stdout?,
            stderr?,
        ))
    };
    let result = timeout(budget, capture).await;
    match result {
        Ok(result) => {
            if !matches!(child.try_wait(), Ok(Some(_))) {
                timeout(Duration::from_secs(2), child.kill())
                    .await
                    .map_err(|_| "Readiness child fallback reap exceeded 2 seconds.".to_owned())?
                    .map_err(|_| "Cannot reap readiness child after capture failure.".to_owned())?;
                return Err("Readiness capture returned before its child was reaped.".into());
            }
            let (status, stdout, stderr) = result?;
            Ok(Captured {
                status,
                stdout: String::from_utf8(stdout)
                    .map_err(|_| "Readiness stdout is not UTF-8.".to_owned())?,
                stderr: String::from_utf8(stderr)
                    .map_err(|_| "Readiness stderr is not UTF-8.".to_owned())?,
            })
        }
        Err(_) => {
            timeout(Duration::from_secs(2), child.kill())
                .await
                .map_err(|_| "Readiness child kill/reap exceeded 2 seconds.".to_owned())?
                .map_err(|_| "Cannot kill/reap timed-out readiness child.".to_owned())?;
            Err("Owned readiness executable exceeded its at-most-5-second deadline.".into())
        }
    }
}

async fn reply(
    socket: &mut TcpStream,
    status: u16,
    body: &Value,
    captured_method: &mut String,
) -> FixtureResult<()> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    let boundary = loop {
        let count = socket
            .read(&mut buffer)
            .await
            .map_err(|_| "Cannot read bounded readiness fixture request.".to_owned())?;
        if count == 0 {
            return Err("Readiness fixture request ended before its headers.".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > 65536 {
            return Err("Readiness fixture headers exceeded 64 KiB.".into());
        }
        if let Some(boundary) = bytes.windows(4).position(|chunk| chunk == b"\r\n\r\n") {
            break boundary;
        }
    };
    let headers = std::str::from_utf8(&bytes[..boundary])
        .map_err(|_| "Readiness fixture headers are not UTF-8.".to_owned())?;
    let method = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .ok_or_else(|| "Readiness fixture lacks a request method.".to_owned())?
        .to_owned();
    *captured_method = method;
    let body = body.to_string();
    let wire = format!(
        "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    socket
        .write_all(wire.as_bytes())
        .await
        .map_err(|_| "Cannot send readiness fixture response.".to_owned())?;
    socket
        .shutdown()
        .await
        .map_err(|_| "Cannot close readiness fixture socket.".to_owned())?;
    Ok(())
}

async fn serve(
    listener: &TcpListener,
    stop: &CancellationToken,
    reject: &AtomicBool,
    success: &Value,
) -> FixtureResult<Vec<String>> {
    let mut methods = Vec::new();
    loop {
        let accepted = tokio::select! {
            biased;
            accepted = listener.accept() => accepted,
            _ = stop.cancelled() => return Ok(methods),
        };
        let (mut socket, _) =
            accepted.map_err(|_| "Cannot accept owned readiness fixture connection.".to_owned())?;
        let rejected = reject.load(Ordering::SeqCst);
        let failure = json!({"error":"Bearer must-not-print-provider-error"});
        methods.push(String::new());
        let mut method = String::new();
        let result = {
            let response = timeout(
                Duration::from_secs(3),
                reply(
                    &mut socket,
                    if rejected { 503 } else { 200 },
                    if rejected { &failure } else { success },
                    &mut method,
                ),
            );
            tokio::pin!(response);
            tokio::select! {
                result = &mut response => result,
                _ = stop.cancelled() => {
                    timeout(Duration::from_secs(2), &mut response).await
                        .map_err(|_| "Readiness socket shutdown exceeded 2 seconds.".to_owned())?
                }
            }
        };
        *methods.last_mut().expect("accepted connection slot") = method;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(_)) if stop.is_cancelled() => return Ok(methods),
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err("Readiness fixture socket exceeded 3 seconds.".into()),
        }
    }
}

async fn exercise(
    fixture: &Fixture,
    success: Value,
    invocations: Vec<(Vec<String>, bool)>,
) -> FixtureResult<(Vec<Captured>, Vec<String>)> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let stop = CancellationToken::new();
    let reject = AtomicBool::new(false);
    let server = async {
        let result = timeout_at(
            deadline + Duration::from_secs(2),
            serve(&fixture.listener, &stop, &reject, &success),
        )
        .await
        .map_err(|_| "Readiness server shutdown exceeded the 12-second case bound.".to_owned())?;
        stop.cancel();
        result
    };
    let client = async {
        let result = async {
            let mut outputs = Vec::new();
            for (args, rejected) in invocations {
                reject.store(rejected, Ordering::SeqCst);
                outputs.push(readiness(fixture, &args, deadline).await?);
            }
            Ok::<_, String>(outputs)
        }
        .await;
        stop.cancel();
        result
    };
    // Neither branch is spawned. Both children and socket work finish before any test assertion;
    // unwinding or cancellation drops the scoped listener/socket and kill-on-drop child owner.
    let (methods, outputs) = tokio::join!(server, client);
    Ok((outputs?, methods?))
}

fn report(output: &Captured, code: i32) -> Value {
    assert_eq!(output.status.code(), Some(code));
    Value::parse_with_limit(&output.stdout, 1048576)
        .expect("readiness stdout must be one JSON report")
}

#[tokio::test]
async fn executable_readiness_probes_get_only_and_never_discloses_the_exact_catalog_id() {
    let fixture = Fixture::new().await;
    let mut args = fixture.configuration(json!({"type":"none"}));
    args.push("--probe".into());
    let (outputs, methods) = exercise(
        &fixture,
        json!({"data":[{"id":"Bearer do-not-print-fixture"}]}),
        vec![(args, false)],
    )
    .await
    .unwrap();
    assert_eq!(outputs.len(), 1);
    let result = report(&outputs[0], 0);
    assert_eq!(result["local_ready"], true);
    assert_eq!(result["connection_ready"], true);
    assert_eq!(result["catalog_count"], 1);
    assert_eq!(result["generation_verified"], false);
    assert_eq!(result["tool_count"], 10);
    assert!(
        !methods.is_empty(),
        "Read-only probe must contact its local fixture."
    );
    assert!(methods.iter().all(|method| method == "GET"));
    assert!(!outputs[0].stdout.contains("do-not-print-fixture"));
    assert!(!outputs[0].stderr.contains("do-not-print-fixture"));
}

#[tokio::test]
async fn executable_readiness_missing_declared_credential_exits_two_without_provider_calls() {
    let fixture = Fixture::new().await;
    let mut args = fixture.configuration(json!({
        "type":"bearer", "secretEnv":"NUMERA_FIXTURE_MISSING_CREDENTIAL_314"
    }));
    args.push("--probe".into());
    let (outputs, methods) = exercise(&fixture, json!({"data":[]}), vec![(args, false)])
        .await
        .unwrap();
    assert_eq!(outputs.len(), 1);
    let result = report(&outputs[0], 2);
    assert_eq!(result["local_ready"], true);
    assert_eq!(result["credentials_ready"], false);
    assert_eq!(result["connection_ready"], false);
    assert_eq!(result["blockers"], json!(["declared_credentials_missing"]));
    assert!(
        methods.is_empty(),
        "Missing credentials must prevent every provider call."
    );
}

#[tokio::test]
async fn executable_readiness_local_only_has_null_connection_and_relative_config_exits_one() {
    let fixture = Fixture::new().await;
    let args = fixture.configuration(json!({"type":"none"}));
    let (outputs, methods) = exercise(
        &fixture,
        json!({"data":[]}),
        vec![
            (args, false),
            (vec!["--config".into(), "relative.json".into()], false),
        ],
    )
    .await
    .unwrap();
    assert_eq!(outputs.len(), 2);
    let result = report(&outputs[0], 0);
    assert_eq!(result["local_ready"], true);
    assert_eq!(result["connection_ready"], Value::Null);
    assert_eq!(result["generation_verified"], false);
    assert!(
        methods.is_empty(),
        "Local-only checks must never contact the provider."
    );
    assert_eq!(outputs[1].status.code(), Some(1));
}

#[tokio::test]
async fn executable_readiness_unlisted_model_and_failed_catalog_exit_two_without_post_or_error_leak()
 {
    let fixture = Fixture::new().await;
    let mut failed = fixture.configuration(json!({"type":"none"}));
    failed.push("--probe".into());
    let mut unknown = failed.clone();
    unknown.extend(["--model".into(), "missing".into()]);
    let (outputs, methods) = exercise(
        &fixture,
        json!({"data":[{"id":"known"}]}),
        vec![(unknown, false), (failed, true)],
    )
    .await
    .unwrap();
    assert_eq!(outputs.len(), 2);
    let unknown = report(&outputs[0], 2);
    assert_eq!(unknown["selected_model_found"], false);
    assert_eq!(unknown["generation_verified"], false);
    let failed = report(&outputs[1], 2);
    assert_eq!(failed["connection_ready"], false);
    assert_eq!(failed["catalog_complete"], false);
    assert!(!outputs[1].stdout.contains("must-not-print-provider-error"));
    assert!(!outputs[1].stderr.contains("must-not-print-provider-error"));
    assert!(!methods.is_empty());
    assert!(methods.iter().all(|method| method == "GET"));
}
