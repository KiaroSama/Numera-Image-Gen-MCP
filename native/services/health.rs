use crate::compat::value::{Value, json};
use crate::{
    config::{Config, EnvironmentSource, credentials, snapshot_credentials},
    error::{NumeraError, Result},
    http::api::{self, Body},
    runtime_paths::{RuntimePaths, private_directory},
};
use reqwest::Method;
use std::path::Path;
use tokio_util::sync::CancellationToken;

pub async fn connections(
    config: &Config,
    env: &impl EnvironmentSource,
    runtime: &RuntimePaths,
) -> Result<Value> {
    let mut results = Vec::new();
    for (name, c) in config.value["connections"]
        .as_object()
        .ok_or_else(NumeraError::runtime)?
    {
        results.push(json!({"name":name,"adapter":c["adapter"],"gateway":c.get("gateway"),"enabled":c["enabled"],
            "credentials_ready":credentials(c,env,runtime).await.is_ok(),"default_model":c.get("defaultModel")}));
    }
    Ok(json!(results))
}

pub async fn health(
    config: &Config,
    env: &impl EnvironmentSource,
    runtime: &RuntimePaths,
    probe: bool,
    cancel: &CancellationToken,
) -> Result<Value> {
    let mut storage = json!({});
    for (key, field) in [
        ("outputs", "outputDir"),
        ("state", "stateDir"),
        ("logs", "logging"),
    ] {
        let value = if field == "logging" {
            &config.value[field]["directory"]
        } else {
            &config.value[field]
        };
        let writable = value.as_str().is_some_and(|s| {
            let path = Path::new(s);
            if private_directory(path).is_err() {
                return false;
            }
            // Probe creates only its own exclusive short-lived file, never modifies existing data.
            let name = path.join(format!(".numera-health-{}", uuid::Uuid::new_v4()));
            let result = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&name);
            match result {
                Ok(file) => {
                    drop(file);
                    std::fs::remove_file(name).is_ok()
                }
                Err(_) => false,
            }
        });
        storage[key] = json!(writable);
    }
    let mut connections = Vec::new();
    for (name, c) in config.value["connections"]
        .as_object()
        .ok_or_else(NumeraError::runtime)?
    {
        let mut reachable = Value::Null;
        let mut checked = false;
        let mut error = Value::Null;
        if probe && c["enabled"] == true {
            let action = async {
                let pinned = snapshot_credentials(c, env, runtime).await?;
                api::json(
                    pinned.value(),
                    &pinned.headers(),
                    api::Operation {
                        operation: if c["adapter"] == "comfyui" {
                            "system_stats"
                        } else {
                            "models"
                        },
                        prefix: super::discovery::prefix(c),
                        method: Method::GET,
                        body: Body::Empty,
                        query: None,
                    },
                    cancel,
                )
                .await
            }
            .await;
            match action {
                Ok(_) => {
                    reachable = json!(true);
                    checked = c["auth"]["type"] != "none";
                }
                Err(e) => {
                    reachable = json!(false);
                    error = json!(e);
                }
            }
        }
        connections.push(json!({"name":name,"reachable":reachable,"authentication_checked":checked,"generation_verified":false,"error":error}));
    }
    Ok(
        json!({"configuration_valid":true,"runtime":"Rust 1.99.1","storage":storage,"connections":connections,"generation_verified":false,"caveats":["Passing probes does not prove generation/editing entitlement.","Gateway/provider retries cannot establish global exactly-once billing."]}),
    )
}
