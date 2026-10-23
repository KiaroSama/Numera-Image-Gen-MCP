use crate::{
    compat::value::{Value, json},
    config,
    devtools::Owned,
    error::{NumeraError, Result},
    logging::Logger,
    runtime_paths::RuntimePaths,
};
use rmcp::{
    RoleClient, ServiceExt,
    model::{
        CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation, ProtocolVersion,
    },
    service::{RunningService, RxJsonRpcMessage, TxJsonRpcMessage},
    transport::Transport,
};
use std::{
    collections::HashMap,
    io,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout},
    sync::Mutex,
    time::timeout,
};

const LIMIT: usize = 1024 * 1024;
type Client = RunningService<RoleClient, ClientConfig>;

fn unavailable() -> NumeraError {
    NumeraError::new("upstream_error", "Read-only readiness protocol failed.").stage("protocol")
}

// The SDK's default reader is uncapped and can trace raw malformed messages.
struct Pipes {
    input: BufReader<ChildStdout>,
    output: Arc<Mutex<Option<ChildStdin>>>,
    pending: Vec<u8>,
}

impl Transport<RoleClient> for Pipes {
    type Error = io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        let output = self.output.clone();
        async move {
            let mut bytes = serde_json::to_vec(&item)
                .map_err(|_| io::Error::other("Cannot encode readiness request."))?;
            if bytes.len() >= LIMIT {
                return Err(io::Error::other("Readiness request exceeded its bound."));
            }
            bytes.push(b'\n');
            timeout(Duration::from_secs(10), async {
                let mut guard = output.lock().await;
                let stream = guard
                    .as_mut()
                    .ok_or_else(|| io::Error::other("Readiness transport is closed."))?;
                stream.write_all(&bytes).await?;
                stream.flush().await
            })
            .await
            .map_err(|_| io::Error::other("Readiness write deadline exceeded."))?
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
        loop {
            let bytes = self.input.fill_buf().await.ok()?;
            if bytes.is_empty() {
                return None;
            }
            let count = bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |index| index + 1);
            if self
                .pending
                .len()
                .checked_add(count)
                .is_none_or(|size| size > LIMIT)
            {
                return None;
            }
            self.pending.extend_from_slice(&bytes[..count]);
            let complete = bytes[count - 1] == b'\n';
            self.input.consume(count);
            if complete {
                let result = serde_json::from_slice(&self.pending).ok();
                self.pending.clear();
                return result;
            }
        }
    }

    async fn close(&mut self) -> io::Result<()> {
        self.pending.clear();
        if let Some(mut stream) = self.output.lock().await.take() {
            timeout(Duration::from_secs(2), stream.shutdown())
                .await
                .map_err(|_| io::Error::other("Readiness close deadline exceeded."))??;
        }
        Ok(())
    }
}

async fn call(
    client: &Client,
    name: &'static str,
    args: serde_json::Value,
    blockers: &mut Vec<&'static str>,
) -> Result<Value> {
    let arguments = args.as_object().cloned().ok_or_else(unavailable)?;
    let result = timeout(
        Duration::from_secs(10),
        client.call_tool(CallToolRequestParams::new(name).with_arguments(arguments)),
    )
    .await
    .map_err(|_| unavailable())?
    .map_err(|_| unavailable())?;
    if result.is_error == Some(true) || result.structured_content.is_none() {
        blockers.push("read_only_tool_failed");
        return Ok(json!({}));
    }
    Value::from_serde(&result.structured_content.ok_or_else(unavailable)?)
}

async fn inspect(
    client: &Client,
    flags: &HashMap<String, String>,
    name: &str,
    report: &mut Value,
    blockers: &mut Vec<&'static str>,
) -> Result<()> {
    let tools = timeout(Duration::from_secs(10), client.list_tools(None))
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())?;
    report["tool_count"] = json!(tools.tools.len());
    if tools.tools.len() != 10
        || [
            "health_check",
            "list_connections",
            "list_models",
            "get_model_capabilities",
        ]
        .iter()
        .any(|name| !tools.tools.iter().any(|tool| tool.name == *name))
    {
        blockers.push("tool_contract_mismatch");
    }
    let health = call(
        client,
        "health_check",
        serde_json::json!({"probe":false}),
        blockers,
    )
    .await?;
    let local = health["configuration_valid"] == true
        && health["storage"]
            .as_object()
            .is_some_and(|values| values.len() == 3 && values.values().all(|value| *value == true));
    report["local_ready"] = json!(local);
    if !local {
        blockers.push("local_storage_or_configuration_blocked");
    }
    let connections = call(client, "list_connections", serde_json::json!({}), blockers).await?;
    let credentials = crate::error::array(&connections["connections"])
        .iter()
        .find(|connection| connection["name"] == name)
        .is_some_and(|connection| connection["credentials_ready"] == true);
    report["credentials_ready"] = json!(credentials);
    if !credentials {
        blockers.push("declared_credentials_missing");
    }
    if flags.contains_key("probe") && credentials {
        let catalog = call(
            client,
            "list_models",
            serde_json::json!({
                "connection":name,"refresh":true,"limit":100,"offset":0
            }),
            blockers,
        )
        .await?;
        let models = crate::error::array(&catalog["models"]);
        let complete = catalog["complete"] == true;
        let ready = complete && !models.is_empty();
        report["catalog_count"] = json!(models.len());
        report["catalog_complete"] = json!(complete);
        report["connection_ready"] = json!(ready);
        if !ready {
            blockers.push("catalog_incomplete_or_empty");
        }
        if let Some(model) = flags.get("model") {
            let found = models.iter().any(|item| item["id"] == model.as_str());
            report["selected_model_found"] = json!(found);
            call(
                client,
                "get_model_capabilities",
                serde_json::json!({
                    "connection":name,"model":model
                }),
                blockers,
            )
            .await?;
            if !found {
                blockers.push("model_not_in_first_catalog_page");
            }
        }
    } else if flags.contains_key("probe") {
        report["connection_ready"] = json!(false);
    }
    Ok(())
}

pub async fn run(
    args: &[String],
    root: &Path,
    env: &HashMap<String, String>,
    runtime: &RuntimePaths,
    executable: &Path,
) -> Result<(Value, bool)> {
    let flags = super::maintenance::flags(args, &["probe"], &["config", "connection", "model"])?;
    let file = flags.get("config").ok_or_else(|| {
        NumeraError::new("INVALID_REQUEST", "An absolute configuration is required.")
    })?;
    if !Path::new(file).is_absolute()
        || flags
            .get("model")
            .is_some_and(|model| model.trim().is_empty() || model.encode_utf16().count() > 1024)
    {
        return Err(NumeraError::new(
            "INVALID_REQUEST",
            "Absolute configuration and valid model are required.",
        ));
    }
    let config = config::load_config(&["--config".into(), file.clone()], env, root)?;
    let (name, _) = config.select(flags.get("connection").map(String::as_str))?;
    let mut report = json!({
        "checked_at":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis,true),
        "tool_count":0,"local_ready":false,"credentials_ready":false,
        "connection_ready":null,"catalog_count":null,"catalog_complete":null,
        "selected_model_found":null,"generation_verified":false
    });
    let mut blockers = Vec::new();
    let logger = Logger::new(&root.join("logs"), "INFO");
    let mut command = Command::new(executable);
    command
        .args(["bridge", "--config", file])
        .current_dir(root)
        .env_clear()
        .envs(env)
        .envs(runtime.child_environment())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let spawned = Owned::spawn_direct(&mut command);
    let mut child = match spawned {
        Ok(child) => child,
        Err(_) => {
            report["blockers"] = json!(["protocol_or_read_only_probe_failed"]);
            return Ok((report, false));
        }
    };
    let pid = child.id();
    logger.log(
        "INFO",
        "readiness",
        "Owned stdio readiness child started.",
        json!({"pid":pid}),
    );
    // The OS owner remains on this stack across every await. Its bounded Drop reaps the
    // exact child if this future is cancelled; neither diagnostics nor cleanup is detached.
    let operation = async {
        let pipes = child.pipes().map_err(|_| unavailable())?;
        let input = ChildStdout::from_std(pipes.stdout).map_err(|_| unavailable())?;
        let output = ChildStdin::from_std(pipes.stdin.ok_or_else(unavailable)?)
            .map_err(|_| unavailable())?;
        let mut stderr =
            tokio::process::ChildStderr::from_std(pipes.stderr).map_err(|_| unavailable())?;
        let protocol = async {
            let transport = Pipes {
                input: BufReader::new(input),
                output: Arc::new(Mutex::new(Some(output))),
                pending: Vec::new(),
            };
            let client_config = ClientConfig::new(
                ClientCapabilities::default(),
                Implementation::new("numera-readiness", "1.0.0"),
            )
            .with_protocol_version(ProtocolVersion::V_2025_11_25);
            let mut client = timeout(Duration::from_secs(10), client_config.serve(transport))
                .await
                .map_err(|_| unavailable())?
                .map_err(|_| unavailable())?;
            let inspected = inspect(&client, &flags, &name, &mut report, &mut blockers).await;
            let closed = timeout(Duration::from_secs(2), client.close()).await;
            drop(client);
            if !matches!(closed, Ok(Ok(_))) {
                return Err(unavailable());
            }
            inspected
        };
        let diagnostics = async {
            let mut total = 0usize;
            let mut bytes = [0u8; 4096];
            loop {
                let count = stderr.read(&mut bytes).await.map_err(|_| unavailable())?;
                if count == 0 {
                    return Ok::<(), NumeraError>(());
                }
                total += count;
                if total > LIMIT {
                    return Err(unavailable());
                }
            }
        };
        tokio::try_join!(protocol, diagnostics)?;
        Ok::<(), NumeraError>(())
    };
    let verified = matches!(
        timeout(Duration::from_secs(60), operation).await,
        Ok(Ok(()))
    );
    let waiting = async {
        while !child.exited().map_err(|_| unavailable())? {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok::<(), NumeraError>(())
    };
    let graceful = matches!(timeout(Duration::from_secs(2), waiting).await, Ok(Ok(())));
    let status = child.stop(std::time::Instant::now() + Duration::from_secs(2));
    let clean = status.is_ok();
    let exited = matches!(status, Ok(0));
    logger.log(
        if clean { "INFO" } else { "ERROR" },
        "readiness",
        "Owned stdio readiness child closed.",
        json!({"pid":pid,"cleanup_verified":clean,"exit_success":exited,
            "stdio_verified":verified,"tool_count":report["tool_count"]}),
    );
    if !verified || !graceful || !clean || !exited {
        blockers.push("protocol_or_read_only_probe_failed");
    }
    let ready =
        report["local_ready"] == true && report["credentials_ready"] == true && blockers.is_empty();
    report["blockers"] = json!(blockers);
    logger.log(
        if ready { "INFO" } else { "WARNING" },
        "readiness",
        "Readiness complete; generation was not tested.",
        json!({"local_ready":report["local_ready"],"connection_ready":report["connection_ready"],"blocker_count":blockers.len()}),
    );
    Ok((report, ready))
}
