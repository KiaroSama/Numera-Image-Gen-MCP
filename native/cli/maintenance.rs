use crate::{
    compat::value::{Value, json},
    config,
    error::{NumeraError, Result},
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{discovery::Discovery, health},
};
use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

fn invalid() -> NumeraError {
    NumeraError::new(
        "INVALID_REQUEST",
        "Pass explicit supported maintenance options and absolute paths.",
    )
}
fn flags(args: &[String], booleans: &[&str], strings: &[&str]) -> Result<HashMap<String, String>> {
    let mut result = HashMap::new();
    let mut index = 0;
    while index < args.len() {
        let name = args[index].strip_prefix("--").ok_or_else(invalid)?;
        if booleans.contains(&name) {
            result.insert(name.into(), "true".into());
        } else if strings.contains(&name) {
            index += 1;
            let value = args
                .get(index)
                .filter(|v| !v.starts_with("--"))
                .ok_or_else(invalid)?;
            result.insert(name.into(), value.clone());
        } else {
            return Err(invalid());
        }
        index += 1;
    }
    Ok(result)
}

pub fn register(args: &[String], root: &Path, executable: &Path) -> Result<()> {
    let f = flags(
        args,
        &["dry-run", "remove"],
        &["client", "host-config", "config"],
    )?;
    let client = f.get("client").ok_or_else(invalid)?;
    let host = Path::new(f.get("host-config").ok_or_else(invalid)?);
    let config = Path::new(f.get("config").ok_or_else(invalid)?);
    let logger = Logger::new(&root.join("logs"), "INFO");
    super::registration::register(
        client,
        f.contains_key("remove"),
        host,
        executable,
        config,
        f.contains_key("dry-run"),
        &logger,
    )
}

pub async fn readiness(
    args: &[String],
    root: &Path,
    env: &HashMap<String, String>,
    runtime: &RuntimePaths,
) -> Result<(Value, bool)> {
    let f = flags(args, &["probe"], &["config", "connection", "model"])?;
    let file = f.get("config").ok_or_else(invalid)?;
    if !Path::new(file).is_absolute()
        || f.get("model")
            .is_some_and(|m| m.trim().is_empty() || m.encode_utf16().count() > 1024)
    {
        return Err(invalid());
    }
    let config = Arc::new(config::load_config(
        &["--config".into(), file.clone()],
        env,
        root,
    )?);
    let (name, _) = config.select(f.get("connection").map(String::as_str))?;
    let stop = CancellationToken::new();
    let logger = Logger::new(&root.join("logs"), "INFO");
    let mut blockers = Vec::new();
    let health = health::health(&config, env, runtime, false, &stop).await?;
    let local = health["configuration_valid"] == true
        && health["storage"]
            .as_object()
            .is_some_and(|s| s.len() == 3 && s.values().all(|v| *v == true));
    if !local {
        blockers.push("local_storage_or_configuration_blocked");
    }
    let connections = health::connections(&config, env, runtime).await?;
    let credentials = crate::error::array(&connections)
        .iter()
        .find(|c| c["name"] == name)
        .is_some_and(|c| c["credentials_ready"] == true);
    if !credentials {
        blockers.push("declared_credentials_missing");
    }
    let mut report = json!({"checked_at":chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis,true),"tool_count":crate::mcp::tools::definitions()?.len(),"local_ready":local,"credentials_ready":credentials,
        "connection_ready":null,"catalog_count":null,"catalog_complete":null,"selected_model_found":null,"generation_verified":false});
    if f.contains_key("probe") && credentials {
        let mut discovery = Discovery::default();
        match tokio::time::timeout(
            Duration::from_secs(60),
            discovery.list(&config, runtime, env, Some(&name), true, &stop),
        )
        .await
        {
            Ok(Ok(catalog)) => {
                let models = crate::error::array(&catalog["models"]);
                let ready = catalog["complete"] == true && !models.is_empty();
                report["catalog_count"] = json!(models.len().min(100));
                report["catalog_complete"] = catalog["complete"].clone();
                report["connection_ready"] = json!(ready);
                if !ready {
                    blockers.push("catalog_incomplete_or_empty");
                }
                if let Some(model) = f.get("model") {
                    let found = models.iter().take(100).any(|m| m["id"] == model.as_str());
                    report["selected_model_found"] = json!(found);
                    Discovery::model(
                        &config,
                        runtime,
                        env,
                        Some(&name),
                        &model.as_str().into(),
                        &stop,
                    )
                    .await?;
                    if !found {
                        blockers.push("model_not_in_first_catalog_page");
                    }
                }
            }
            _ => blockers.push("protocol_or_read_only_probe_failed"),
        }
    } else if f.contains_key("probe") {
        report["connection_ready"] = json!(false);
    }
    let ready = local && credentials && blockers.is_empty();
    report["blockers"] = json!(blockers);
    logger.log(if ready{"INFO"}else{"WARNING"},"readiness","Readiness complete; generation was not tested.",json!({"local_ready":local,"connection_ready":report["connection_ready"],"blocker_count":blockers.len()}));
    Ok((report, ready))
}
