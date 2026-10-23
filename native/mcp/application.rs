use crate::{
    compat::{
        canonical_json::fingerprint,
        value::{JsString, Value, json},
    },
    config::{Config, ConfigReload, snapshot_credentials},
    error::{NumeraError, Result},
    files::{images, output},
    jobs::Store,
    logging::Logger,
    runtime_paths::RuntimePaths,
    services::{
        discovery::Discovery,
        finalization::{SharedStore, locked},
        generation::Generation,
        health,
        identity::connection_identity,
        jobs,
    },
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

struct Snapshots {
    reload: Option<ConfigReload>,
    generations: Vec<Arc<Generation>>,
    current: Arc<Generation>,
    owners: HashMap<String, Arc<Generation>>,
}
pub struct Application {
    snapshots: Mutex<Snapshots>,
    discoveries: tokio::sync::Mutex<HashMap<usize, Discovery>>,
    pub store: SharedStore,
    pub logger: Arc<Logger>,
    pub stop: CancellationToken,
}
impl Application {
    pub fn new(
        config: Config,
        reload: Option<ConfigReload>,
        logger: Arc<Logger>,
        runtime: RuntimePaths,
        env: HashMap<String, String>,
        stop: CancellationToken,
    ) -> Result<Self> {
        let store = Arc::new(Mutex::new(Store::open(std::path::Path::new(
            config.value["stateDir"]
                .as_str()
                .ok_or_else(NumeraError::runtime)?,
        ))?));
        let generation = Arc::new(Generation {
            config: Arc::new(config),
            store: store.clone(),
            logger: logger.clone(),
            runtime,
            env: env.into(),
            active: Arc::new(Mutex::new(HashMap::new())),
        });
        Ok(Self {
            snapshots: Mutex::new(Snapshots {
                reload,
                generations: vec![generation.clone()],
                current: generation,
                owners: HashMap::new(),
            }),
            discoveries: tokio::sync::Mutex::new(HashMap::new()),
            store,
            logger,
            stop,
        })
    }
    pub fn current(&self) -> Result<Arc<Generation>> {
        let mut snapshots = self.snapshots.lock().map_err(|_| NumeraError::runtime())?;
        if let Some(reload) = snapshots.reload.as_mut() {
            let config = reload.refresh(&self.logger, |_| true)?;
            if !Arc::ptr_eq(&config, &snapshots.current.config)
                && fingerprint(&config.value)? != fingerprint(&snapshots.current.config.value)?
            {
                let old = &snapshots.current;
                let generation = Arc::new(Generation {
                    config,
                    store: self.store.clone(),
                    logger: self.logger.clone(),
                    runtime: old.runtime.clone(),
                    env: old.env.clone(),
                    active: Arc::new(Mutex::new(HashMap::new())),
                });
                snapshots.generations.push(generation.clone());
                snapshots.current = generation;
            }
        }
        Ok(snapshots.current.clone())
    }
    pub fn shutdown(&self) {
        self.stop.cancel();
        if let Ok(snapshots) = self.snapshots.lock() {
            for g in &snapshots.generations {
                if let Ok(active) = g.active.lock() {
                    for token in active.values() {
                        token.cancel();
                    }
                }
            }
        }
    }
    async fn owner(&self, id: &str, upstream: bool) -> Result<Arc<Generation>> {
        let (generations, preferred, current) = {
            let snapshots = self.snapshots.lock().map_err(|_| NumeraError::runtime())?;
            let active = snapshots
                .generations
                .iter()
                .find(|g| g.active.lock().is_ok_and(|a| a.contains_key(id)))
                .cloned();
            (
                snapshots.generations.clone(),
                snapshots.owners.get(id).cloned().or(active),
                snapshots.current.clone(),
            )
        };
        if !upstream {
            return Ok(preferred.unwrap_or(current));
        }
        let receipt = locked(&self.store)?.get(id)?;
        let identity = locked(&self.store)?.identity(id)?;
        let name = receipt.value["connection"]
            .as_str()
            .ok_or_else(NumeraError::runtime)?;
        let candidates = preferred.map_or(generations, |g| vec![g]);
        let mut matches = Vec::new();
        let mut routes = HashSet::new();
        for g in candidates {
            if let Ok((_, c)) = g.config.select(Some(name))
                && let Ok(pinned) = snapshot_credentials(&c, &g.env, &g.runtime).await
                && connection_identity(name, &pinned)? == identity
            {
                let mut route =
                    json!({"paths":c["paths"],"query":c["query"],"baseUrlMode":c["baseUrlMode"]});
                for key in ["proxy", "workflow"] {
                    if let Some(value) = c.get(key) {
                        route[key] = value.clone();
                    }
                }
                routes.insert(fingerprint(&route)?);
                matches.push(g);
            }
        }
        if routes.len() == 1
            && let Some(g) = matches.into_iter().next()
        {
            return Ok(g);
        }
        Err(NumeraError::new(
            "permission_denied",
            "Existing job connection identity is unavailable or ambiguous; no upstream request was sent.",
        ))
    }

    pub async fn call(&self, name: &str, args: Value, cancel: &CancellationToken) -> Result<Value> {
        self.call_snapshot(self.current()?, name, args, cancel)
            .await
    }
    pub async fn call_snapshot(
        &self,
        generation: Arc<Generation>,
        name: &str,
        args: Value,
        cancel: &CancellationToken,
    ) -> Result<Value> {
        let config = &generation.config;
        match name {
            "health_check" => {
                health::health(
                    config,
                    &generation.env,
                    &generation.runtime,
                    boolean(&args, "probe", false)?,
                    cancel,
                )
                .await
            }
            "list_connections" => Ok(
                json!({"connections":health::connections(config,&generation.env,&generation.runtime).await?}),
            ),
            "list_models" => {
                let (offset, limit) = pagination(&args)?;
                let mut discoveries = self.discoveries.lock().await;
                let discovery = discoveries.entry(Arc::as_ptr(config) as usize).or_default();
                let mut result = discovery
                    .list(
                        config,
                        &generation.runtime,
                        &generation.env,
                        optional_string(&args, "connection")?,
                        boolean(&args, "refresh", false)?,
                        cancel,
                    )
                    .await?;
                let all = result["models"]
                    .as_array()
                    .ok_or_else(NumeraError::runtime)?;
                let next = if all.len() > offset + limit {
                    json!(offset + limit)
                } else {
                    Value::Null
                };
                result["models"] = json!(
                    all.iter()
                        .skip(offset)
                        .take(limit)
                        .cloned()
                        .collect::<Vec<_>>()
                );
                result["next_offset"] = next;
                Ok(result)
            }
            "get_model_capabilities" => {
                Discovery::model(
                    config,
                    &generation.runtime,
                    &generation.env,
                    optional_string(&args, "connection")?,
                    required_js_string(&args, "model")?,
                    cancel,
                )
                .await
            }
            "generate_image" | "edit_image" => {
                let receipt = generation
                    .run(
                        args,
                        if name == "generate_image" {
                            "generate"
                        } else {
                            "edit"
                        },
                        cancel,
                    )
                    .await?;
                if !receipt.value["upstream_job"].is_null() {
                    self.snapshots
                        .lock()
                        .map_err(|_| NumeraError::runtime())?
                        .owners
                        .insert(receipt.id()?.into(), generation);
                }
                Ok(receipt.value)
            }
            "get_job" => {
                let id = required_string(&args, "request_id")?;
                let refresh = boolean(&args, "refresh", false)?;
                let mut receipt = locked(&self.store)?.get(id)?;
                let local = refresh && { locked(&self.store)?.needs_local_recovery(id)? };
                if local {
                    let owner = self.owner(id, false).await?;
                    receipt = jobs::recover_local(owner.as_ref(), id, cancel).await?;
                }
                let pending = { locked(&self.store)?.needs_local_recovery(id)? };
                let upstream = refresh
                    && !pending
                    && !receipt.value["upstream_job"].is_null()
                    && !matches!(receipt.status(), "completed" | "partial" | "failed");
                let owner = self.owner(id, upstream).await?;
                Ok(jobs::get(owner.as_ref(), id, refresh, cancel, None)
                    .await?
                    .value)
            }
            "cancel_job" => {
                let id = required_string(&args, "request_id")?;
                let receipt = locked(&self.store)?.get(id)?;
                locked(&self.store)?.cancel(id)?;
                let owner = self
                    .owner(id, receipt.value["upstream_job"]["kind"] == "comfyui")
                    .await?;
                jobs::cancel(&owner, id, cancel).await
            }
            "list_outputs" => {
                let (offset, limit) = pagination(&args)?;
                let all = locked(&self.store)?.list_outputs(offset as u32, limit as u32)?;
                Ok(
                    json!({"outputs":all.iter().take(limit).collect::<Vec<_>>(),"next_offset":if all.len()>limit{json!(offset+limit)}else{Value::Null}}),
                )
            }
            "get_output_info" => {
                let id = required_string(&args, "output_id")?;
                if uuid::Uuid::parse_str(id).is_err() {
                    return Err(invalid());
                }
                let (mut result, _) = output::owned(&config.value, &*locked(&self.store)?, id)?;
                result["verified"] = json!(true);
                Ok(result)
            }
            _ => Err(NumeraError::new("invalid_input", "Unknown tool name.")),
        }
    }

    pub fn original(&self, id: &str) -> Result<images::Image> {
        let generation = self.current()?;
        Ok(output::owned(&generation.config.value, &*locked(&self.store)?, id)?.1)
    }
    pub fn preview(&self, id: &str) -> Result<Value> {
        let generation = self.current()?;
        let image = output::owned(&generation.config.value, &*locked(&self.store)?, id)?.1;
        images::preview(&image, &generation.config.value)
    }
    pub fn return_mode(&self) -> Result<String> {
        Ok(self.current()?.config.value["returnMode"]
            .as_str()
            .unwrap_or("files")
            .into())
    }
}
fn invalid() -> NumeraError {
    NumeraError::new(
        "invalid_input",
        "Tool arguments do not match the public schema.",
    )
}
pub fn optional_string<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.checked_str().map_err(|_| invalid())?)),
        _ => Err(invalid()),
    }
}
pub fn required_js_string<'a>(args: &'a Value, key: &str) -> Result<&'a JsString> {
    match args.get(key) {
        Some(Value::String(value)) if !value.is_empty() => Ok(value),
        _ => Err(invalid()),
    }
}
pub fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    optional_string(args, key)?
        .filter(|s| !s.is_empty())
        .ok_or_else(invalid)
}
pub fn boolean(args: &Value, key: &str, default: bool) -> Result<bool> {
    match args.get(key) {
        None => Ok(default),
        Some(Value::Bool(v)) => Ok(*v),
        _ => Err(invalid()),
    }
}
fn pagination(args: &Value) -> Result<(usize, usize)> {
    let parse = |key: &str, default: u64, min: u64, max: u64| -> Result<usize> {
        let n = args
            .get(key)
            .map_or(Some(default as f64), Value::as_f64)
            .ok_or_else(invalid)?;
        if n.fract() != 0.0 || n < min as f64 || n > max as f64 {
            return Err(invalid());
        }
        Ok(n as usize)
    };
    Ok((parse("offset", 0, 0, 100000)?, parse("limit", 25, 1, 100)?))
}
