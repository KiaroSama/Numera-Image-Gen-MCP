use super::{
    finalization::{self, SharedStore, locked},
    identity::connection_identity,
    lease::Lease,
    model_policy, request, requirements,
};
use crate::compat::value::{Value, json};
use crate::{
    adapters::{
        self,
        types::{Input, Normalized},
    },
    capabilities,
    compat::canonical_json::fingerprint,
    config::{Config, Environment, snapshot_credentials},
    error::{NumeraError, Result},
    files::{editing, images, output, paths},
    http::assets,
    jobs::Receipt,
    logging::Logger,
    runtime_paths::RuntimePaths,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub struct Generation {
    pub config: Arc<Config>,
    pub store: SharedStore,
    pub logger: Arc<Logger>,
    pub runtime: RuntimePaths,
    pub env: Environment,
    pub active: Arc<Mutex<HashMap<String, CancellationToken>>>,
}
impl Generation {
    pub async fn source(
        &self,
        source: &Value,
        cancel: &CancellationToken,
    ) -> Result<images::Image> {
        let config = &self.config.value;
        let limit = config["files"]["maxInputBytes"]
            .as_u64()
            .unwrap_or(20 * 1024 * 1024) as usize;
        if source["type"] == "output_id" {
            let (_, image) = output::owned(
                config,
                &*locked(&self.store)?,
                source["output_id"]
                    .as_str()
                    .ok_or_else(NumeraError::runtime)?,
            )?;
            if image.bytes.len() > limit {
                return Err(NumeraError::new(
                    "invalid_input",
                    "Owned output exceeds reference byte limit.",
                ));
            }
            return Ok(image);
        }
        let bytes = match source["type"].as_str() {
            Some("path") => paths::input(
                source["path"].as_str().ok_or_else(NumeraError::runtime)?,
                &crate::error::array(&config["files"]["allowedInputRoots"])
                    .iter()
                    .map(|value| {
                        value.as_str().map(PathBuf::from).ok_or_else(|| {
                            NumeraError::new(
                                "invalid_configuration",
                                "An input root cannot be represented as a filesystem path.",
                            )
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
                limit,
            )?,
            Some("data_url") => images::data_url(
                source["data_url"]
                    .as_str()
                    .ok_or_else(NumeraError::runtime)?,
                limit,
            )?,
            Some("url") => {
                assets::fetch(
                    source["url"]
                        .as_js_string()
                        .ok_or_else(NumeraError::runtime)?,
                    config,
                    limit,
                    cancel,
                )
                .await?
            }
            _ => {
                return Err(NumeraError::new(
                    "invalid_input",
                    "Invalid reference source.",
                ));
            }
        };
        images::inspect(bytes, config, Some(limit))
    }

    pub async fn run(
        &self,
        raw: Value,
        operation: &str,
        host_cancel: &CancellationToken,
    ) -> Result<Receipt> {
        let started = std::time::Instant::now();
        let original = editing::request(request::parse(raw)?, operation)?;
        let requested_connection = match original.get("connection") {
            Some(Value::String(value)) => Some(value.checked_str().map_err(|_| {
                NumeraError::new("invalid_input", "Select an enabled configured connection.")
            })?),
            None => None,
            _ => {
                return Err(NumeraError::new(
                    "invalid_input",
                    "Select an enabled configured connection.",
                ));
            }
        };
        let (name, selected) = self.config.select(requested_connection)?;
        let pinned = snapshot_credentials(&selected, &self.env, &self.runtime).await?;
        let c = pinned.value();
        let headers = pinned.headers();
        let model = original.get("model").or_else(|| c.get("defaultModel"));
        let Some(Value::String(model)) = model else {
            return Err(NumeraError::new(
                "invalid_input",
                "Select a model or configure this connection defaultModel.",
            ));
        };
        let preliminary = capabilities::effective_request(c, &original, model)?;
        let (request, policy_warnings) =
            model_policy::validate(c, &headers, model, preliminary, host_cancel).await?;
        let id = request["request_id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let sources = crate::error::array(&request["reference_images"]);
        let config = &self.config.value;
        if sources.len() as u64 > config["files"]["maxReferences"].as_u64().unwrap_or(14) {
            return Err(NumeraError::new(
                "invalid_input",
                "Reference count exceeds configured input limit.",
            ));
        }
        if operation == "edit" && sources.is_empty() {
            return Err(NumeraError::new(
                "invalid_input",
                "edit_image requires at least one reference.",
            ));
        }
        if request["output_subdirectory"].is_string()
            && request["output_subdirectory"].as_str().is_none()
            || request["filename_prefix"].is_string()
                && request["filename_prefix"].as_str().is_none()
        {
            return Err(NumeraError::new(
                "invalid_input",
                "The output path cannot be represented as a filesystem path.",
            ));
        }
        if let Some(path) = request["output_subdirectory"]
            .as_str()
            .filter(|s| !s.is_empty())
        {
            paths::safe_relative(path)?;
        }
        if let Some(prefix) = request["filename_prefix"]
            .as_str()
            .filter(|s| !s.is_empty())
        {
            paths::safe_relative(prefix)?;
            if prefix.contains(['/', '\\']) {
                return Err(NumeraError::new(
                    "invalid_input",
                    "Filename prefix cannot include directories.",
                ));
            }
        }
        if request["wait"] == false && c["adapter"] != "comfyui" {
            return Err(NumeraError::new(
                "unsupported_operation",
                "wait=false requires a recoverable ComfyUI job.",
            ));
        }
        let cancel = host_cancel.child_token();
        let timeout = Duration::from_millis(c["requestTimeoutMs"].as_u64().unwrap_or(300000));
        let deadline = tokio::time::Instant::now() + timeout;
        let mut references = Vec::new();
        let mut total = 0usize;
        for source in sources {
            let image = tokio::time::timeout_at(deadline, self.source(source, &cancel))
                .await
                .map_err(|_| deadline_error())??;
            total = total.checked_add(image.bytes.len()).ok_or_else(|| {
                NumeraError::new("invalid_input", "Aggregate reference bytes exceeded.")
            })?;
            if total as u64
                > config["files"]["maxAggregateBytes"]
                    .as_u64()
                    .unwrap_or(100 * 1024 * 1024)
            {
                return Err(NumeraError::new(
                    "invalid_input",
                    "Aggregate reference bytes exceeded.",
                ));
            }
            references.push(image);
        }
        let mut upscale_source = None;
        if request["upscale"] == true {
            let (w, h) = request["size"]
                .as_str()
                .and_then(editing::dimensions)
                .ok_or_else(NumeraError::runtime)?;
            let image = references.first().ok_or_else(NumeraError::runtime)?;
            if u64::from(w) * u64::from(h)
                > config["files"]["maxPixels"].as_u64().unwrap_or(64000000)
                || w < image.width
                || h < image.height
                || (w == image.width && h == image.height)
            {
                return Err(NumeraError::new(
                    "invalid_input",
                    "Upscale size must enlarge the reference without shrinking either dimension and fit the pixel limit.",
                ));
            }
            upscale_source = Some(json!({"width":image.width,"height":image.height}));
        }
        let mask = if let Some(mask) = request.get("mask") {
            Some(
                tokio::time::timeout_at(deadline, self.source(mask, &cancel))
                    .await
                    .map_err(|_| deadline_error())??,
            )
        } else if request.get("edit_region").is_some() {
            Some(editing::region_mask(
                &request,
                references.first().ok_or_else(|| {
                    NumeraError::new("invalid_input", "Mask needs a reference image.")
                })?,
                c,
                config,
            )?)
        } else {
            None
        };
        if let Some(mask) = &mask {
            if total.checked_add(mask.bytes.len()).is_none_or(|n| {
                n as u64
                    > config["files"]["maxAggregateBytes"]
                        .as_u64()
                        .unwrap_or(100 * 1024 * 1024)
            }) {
                return Err(NumeraError::new(
                    "invalid_input",
                    "Aggregate reference and mask bytes exceeded.",
                ));
            }
            images::validate_mask(
                mask,
                references.first().ok_or_else(|| {
                    NumeraError::new("invalid_input", "Mask needs a reference image.")
                })?,
            )?;
        }
        let input = Input {
            connection: c,
            request: &request,
            model,
            operation,
            references: &references,
            mask: mask.as_ref(),
        };
        let built = if c["adapter"] == "comfyui" {
            None
        } else {
            Some(adapters::build_request(&input)?)
        };
        let workflow = if c["adapter"] == "comfyui" {
            Some(
                tokio::time::timeout_at(
                    deadline,
                    adapters::comfy::prepare(&input, &headers, &cancel),
                )
                .await
                .map_err(|_| deadline_error())??,
            )
        } else {
            None
        };
        let identity = connection_identity(&name, &pinned)?;
        let mut parameters = request
            .as_object()
            .cloned()
            .ok_or_else(NumeraError::runtime)?;
        for key in [
            "request_id",
            "return_mode",
            "wait",
            "reference_images",
            "mask",
        ] {
            parameters.remove(key);
        }
        let mut hashed = json!({"identity":identity,"model":model,"operation":operation,"prompt":request["prompt"],"count":request["count"],"parameters":parameters,
            "reference_hashes":references.iter().map(|i|&i.sha256).collect::<Vec<_>>()});
        if let Some(mask) = &mask {
            hashed["mask_hash"] = json!(mask.sha256);
        }
        if let Some(workflow) = c.get("workflow") {
            hashed["workflow"] = workflow.clone();
        }
        let hash = fingerprint(&hashed)?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut required = json!({"count":request["count"],"output_subdirectory":request["output_subdirectory"].as_str().unwrap_or(&now[..10])});
        for key in ["size", "output_format", "background", "filename_prefix"] {
            if let Some(value) = request.get(key) {
                required[key] = value.clone();
            }
        }
        if let Some(source) = upscale_source {
            required["upscale_source"] = source;
        }
        if let (Some(object), Some(values)) = (
            required.as_object_mut(),
            requirements::snapshot(c, model, &request).as_object(),
        ) {
            object.extend(values.clone());
        }
        let mut warnings = vec![json!(
            "Gateway/provider internal retries and fallback are outside Numera control."
        )];
        warnings.extend(
            policy_warnings
                .into_iter()
                .map(|value| Value::String(value.into())),
        );
        let initial = Receipt::new(
            json!({"request_id":id,"connection":name,"adapter":c["adapter"],"operation":operation,"status":"prepared","requested_model":model,
            "upstream_model":null,"outputs":[],"warnings":warnings,"deviations":[],"errors":[],"generation_outcome":"not_submitted","storage_outcome":"not_started",
            "another_submission_may_charge":true,"output_requirements":required,"created_at":now,"updated_at":now}),
        );
        let preparing = std::time::Instant::now();
        let (fresh, mut receipt) = locked(&self.store)?.prepare(initial, &hash, &identity)?;
        let prepare_ms = preparing.elapsed().as_millis() as u64;
        if !fresh {
            return Ok(receipt);
        }
        self.active
            .lock()
            .map_err(|_| NumeraError::runtime())?
            .insert(id.clone(), cancel.clone());
        let lease = Lease::start(self.store.clone(), receipt.clone(), cancel.clone(), true);
        let result = tokio::time::timeout_at(
            deadline,
            self.submit(super::submission::Submission {
                input: &input,
                built,
                workflow,
                headers: &headers,
                cancel: &cancel,
                id: &id,
                name: &name,
                receipt: &mut receipt,
                lease: &lease,
                identity: &identity,
                before_submit_ms: started.elapsed().as_millis() as u64,
                prepare_ms,
            }),
        )
        .await;
        let result = match result {
            Ok(v) => v,
            Err(_) => Err(NumeraError::new(
                "request_timeout",
                "Waiting stopped; upstream completion and billing may be unknown.",
            )
            .stage("network")),
        };
        let result = match result {
            Ok(receipt) => Ok(receipt),
            Err(error) => self.record_error(&id, receipt, error, cancel.is_cancelled()),
        };
        lease.close().await;
        if let Ok(mut active) = self.active.lock() {
            active.remove(&id);
        }
        if let Ok(receipt) = &result {
            locked(&self.store)?.release(&id, Some(receipt))?;
        } else {
            locked(&self.store)?.release(&id, None)?;
        }
        result
    }

    pub async fn finish(
        &self,
        mut receipt: Receipt,
        result: Normalized,
        request: &Value,
        cancel: &CancellationToken,
        mut claimed: bool,
        recovery: bool,
    ) -> Result<Receipt> {
        let id = receipt.id()?.to_owned();
        let mut terminal_claim = false;
        if let Some(state) = &result.terminal {
            if !locked(&self.store)?.owns(&id, Some(&receipt))? {
                return locked(&self.store)?.get(&id);
            }
            let job = receipt.value.get("upstream_job").cloned();
            receipt = locked(&self.store)?.terminate(
                &id,
                state,
                &json!(
                    NumeraError::new(
                        "provider_rejection",
                        "Provider reported a terminal generation result."
                    )
                    .stage("response")
                ),
                Some(&receipt),
                job.as_ref(),
            )?;
            if result.images.is_empty() && !locked(&self.store)?.has_pending(&id)? {
                return Ok(receipt);
            }
            claimed = true;
            terminal_claim = true;
        }
        let owned = {
            let mut store = locked(&self.store)?;
            if claimed {
                store.owns(&id, Some(&receipt))?
            } else {
                store.claim(&id, Some(&receipt), false)?
            }
        };
        if !owned {
            return locked(&self.store)?.get(&id);
        }
        receipt = locked(&self.store)?.get(&id)?;
        let lease = Lease::start(self.store.clone(), receipt.clone(), cancel.clone(), false);
        let result = finalization::finalize(
            &self.config.value,
            &self.store,
            &self.logger,
            receipt.clone(),
            finalization::Finalization {
                result: Some(result),
                request,
                cancel,
                recovery,
            },
        )
        .await;
        lease.close().await;
        if !claimed || terminal_claim {
            locked(&self.store)?.release(&id, Some(&receipt))?;
        }
        result
    }

    fn record_error(
        &self,
        id: &str,
        mut receipt: Receipt,
        error: NumeraError,
        aborted: bool,
    ) -> Result<Receipt> {
        if !locked(&self.store)?.owns(id, Some(&receipt))? {
            return locked(&self.store)?.get(id);
        }
        receipt = locked(&self.store)?.get(id)?;
        let ambiguous = receipt.status() == "submitting"
            && matches!(
                error.code.as_str(),
                "outcome_unknown"
                    | "request_timeout"
                    | "invalid_response"
                    | "cancelled"
                    | "no_image_returned"
            );
        receipt.value["status"] = json!(if !crate::error::array(&receipt.value["outputs"])
            .is_empty()
        {
            "partial"
        } else if ambiguous {
            "outcome_unknown"
        } else if aborted {
            "cancelled"
        } else {
            "failed"
        });
        receipt.value["errors"]
            .as_array_mut()
            .ok_or_else(NumeraError::runtime)?
            .push(json!(error));
        if ambiguous {
            receipt.value["generation_outcome"] = json!("unknown");
        }
        if !receipt.value["upstream_job"].is_null()
            && receipt.value["upstream_terminal"].is_null()
            && (receipt.value["generation_outcome"] != "completed"
                || (crate::error::array(&receipt.value["outputs"]).is_empty()
                    && !locked(&self.store)?.has_pending(id)?))
        {
            receipt.value["status"] = json!("running");
            if receipt.value["generation_outcome"] != "completed" {
                receipt.value["generation_outcome"] = json!("running");
            }
            receipt.value["warnings"].as_array_mut().ok_or_else(NumeraError::runtime)?.push(json!("Local waiting stopped; existing upstream job may still be running. Use get_job, never resubmit."));
        }
        locked(&self.store)?.update(&mut receipt)?;
        self.logger.log(
            "ERROR",
            "generation",
            "Request did not complete.",
            json!({"requestId":id,"error":error}),
        );
        Ok(receipt)
    }
}
fn deadline_error() -> NumeraError {
    NumeraError::new(
        "request_timeout",
        "Waiting stopped; upstream completion and billing may be unknown.",
    )
    .stage("network")
}
pub fn cancelled() -> NumeraError {
    NumeraError::new(
        "cancelled",
        "Waiting stopped; upstream completion and billing may be unknown.",
    )
    .stage("network")
}
pub async fn wait(cancel: &CancellationToken, millis: u64) -> Result<()> {
    tokio::select! {_=cancel.cancelled()=>Err(cancelled()),_=tokio::time::sleep(Duration::from_millis(millis))=>Ok(())}
}
pub fn validate_job(result: &Normalized, job: &adapters::types::Job) -> Result<()> {
    let matches = result.job.as_ref().map_or_else(
        || result.upstream_id.as_ref() == Some(&job.id),
        |j| j.id == job.id && j.kind == job.kind,
    );
    if !matches {
        return Err(NumeraError::new(
            "invalid_response",
            "Polled response does not match the existing job.",
        )
        .stage("response"));
    }
    Ok(())
}
