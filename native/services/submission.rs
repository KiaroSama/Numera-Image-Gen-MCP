use super::{
    finalization::{self, locked},
    generation::{Generation, cancelled, validate_job, wait},
    lease::Lease,
};
use crate::compat::value::{Value, json};
use crate::{
    adapters::{
        self,
        types::{Input, Prepared},
    },
    compat::canonical_json::fingerprint,
    error::{NumeraError, Result},
    http::api::{self, Body},
    jobs::Receipt,
};
use reqwest::Method;
use std::{collections::HashMap, sync::Mutex, time::Duration};
use tokio_util::sync::CancellationToken;

pub(super) struct Submission<'a, 'input> {
    pub input: &'a Input<'input>,
    pub built: Option<Prepared>,
    pub workflow: Option<Value>,
    pub headers: &'a HashMap<String, String>,
    pub cancel: &'a CancellationToken,
    pub id: &'a str,
    pub name: &'a str,
    pub receipt: &'a mut Receipt,
    pub lease: &'a Lease,
    pub identity: &'a str,
    pub before_submit_ms: u64,
    pub prepare_ms: u64,
}

impl Generation {
    pub(super) async fn submit(&self, submission: Submission<'_, '_>) -> Result<Receipt> {
        let Submission {
            input,
            built,
            workflow,
            headers,
            cancel,
            id,
            name,
            receipt,
            lease,
            identity,
            before_submit_ms,
            prepare_ms,
        } = submission;
        let admitting = std::time::Instant::now();
        let c = input.connection;
        let config = &self.config.value;
        if locked(&self.store)?.queued()? as u64
            > config["maxQueuedRequests"].as_u64().unwrap_or(16)
        {
            return Err(NumeraError::new(
                "rate_limited",
                "Local bounded queue is full.",
            ));
        }
        while !locked(&self.store)?.admit(
            id,
            name,
            config["maxConcurrentRequests"].as_u64().unwrap_or(4) as usize,
            if c["adapter"] == "comfyui" {
                1
            } else {
                c["maxConcurrentRequests"].as_u64().unwrap_or(2) as usize
            },
        )? {
            tokio::select! {_=cancel.cancelled()=>return Err(cancelled()),_=tokio::time::sleep(Duration::from_millis(50))=>{}}
        }
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        let admission_ms = admitting.elapsed().as_millis() as u64;
        let reserving = std::time::Instant::now();
        locked(&self.store)?.reserve(
            id,
            config["files"]["maxAggregateBytes"]
                .as_i64()
                .unwrap_or(100 * 1024 * 1024),
            config["files"]["maxJournalBytes"]
                .as_i64()
                .unwrap_or(256 * 1024 * 1024),
        )?;
        let reserve_ms = reserving.elapsed().as_millis() as u64;
        receipt.value["status"] = json!("submitting");
        receipt.value["generation_outcome"] = json!("unknown");
        let updating = std::time::Instant::now();
        locked(&self.store)?.update(receipt)?;
        let update_ms = updating.elapsed().as_millis() as u64;
        self.logger.log(
            "INFO",
            "generation",
            "Submission intent persisted.",
            json!({"requestId":id,"connection":name,"before_submit_ms":before_submit_ms,
                "prepare_ms":prepare_ms,"admission_ms":admission_ms,
                "reserve_ms":reserve_ms,"update_ms":update_ms}),
        );
        let mut claimed = false;
        let result = if let Some(workflow) = workflow {
            let (job, warnings) = adapters::comfy::submit(input, workflow, headers, cancel).await?;
            receipt.value["upstream_job"] = json!({"id":job.id,"kind":job.kind});
            receipt.value["warnings"]
                .as_array_mut()
                .ok_or_else(NumeraError::runtime)?
                .extend(warnings.into_iter().map(Value::String));
            receipt.value["status"] = json!("running");
            receipt.value["generation_outcome"] = json!("running");
            locked(&self.store)?.update(receipt)?;
            if input.request["wait"] == false {
                return Ok(receipt.clone());
            }
            let shared = Mutex::new((receipt.clone(), claimed));
            let status = super::polling::wait(
                cancel,
                || async {
                    let status = adapters::comfy::status(
                        c,
                        headers,
                        &job.id,
                        cancel,
                        adapters::comfy::OutputLimits {
                            item: config["files"]["maxOutputBytes"]
                                .as_u64()
                                .unwrap_or(50 * 1024 * 1024)
                                as usize,
                            aggregate: config["files"]["maxAggregateBytes"]
                                .as_u64()
                                .unwrap_or(100 * 1024 * 1024)
                                as usize,
                        },
                        |index, bytes| {
                            let mut state = shared.lock().map_err(|_| NumeraError::storage())?;
                            let (current, claimed) = &mut *state;
                            if !*claimed {
                                if !locked(&self.store)?.claim(id, Some(current), false)? {
                                    return Err(NumeraError::new(
                                        "outcome_unknown",
                                        "Another waiter owns output finalization.",
                                    )
                                    .stage("storage"));
                                }
                                *current = locked(&self.store)?.get(id)?;
                                lease.update(current);
                                *claimed = true;
                            }
                            finalization::retain(
                                config,
                                &self.store,
                                id,
                                index,
                                bytes.to_vec(),
                                current,
                            )?;
                            Ok(())
                        },
                        || {
                            let mut state = shared.lock().map_err(|_| NumeraError::storage())?;
                            state.0.value["generation_outcome"] = json!("completed");
                            locked(&self.store)?.update(&mut state.0)
                        },
                    )
                    .await?;
                    Ok(if status.terminal.is_some() || status.result.is_some() {
                        Some(status)
                    } else {
                        None
                    })
                },
                cancelled,
            )
            .await;
            let (current, was_claimed) = shared.into_inner().map_err(|_| NumeraError::storage())?;
            *receipt = current;
            claimed = was_claimed;
            let status = status?;
            if let Some(terminal) = status.terminal {
                return locked(&self.store)?.terminate(
                    id,
                    terminal,
                    &json!(
                        NumeraError::new("provider_rejection", "ComfyUI workflow failed.")
                            .stage("generation")
                    ),
                    Some(receipt),
                    Some(&json!({"id":job.id,"kind":job.kind})),
                );
            }
            status.result.ok_or_else(NumeraError::runtime)?
        } else {
            let built = built.ok_or_else(NumeraError::runtime)?;
            let response = api::request_observed(
                c,
                headers,
                api::Operation {
                    operation: &built.operation,
                    prefix: built.prefix,
                    method: Method::POST,
                    body: built.body,
                    query: None,
                },
                cancel,
                config["files"]["maxAggregateBytes"]
                    .as_u64()
                    .unwrap_or(100 * 1024 * 1024) as usize,
                |client_build_ms, pre_send_ms, timeout_ms| {
                    self.logger.log(
                        "INFO",
                        "http",
                        "Submission transport prepared.",
                        json!({"client_build_ms":client_build_ms,"pre_send_ms":pre_send_ms,"timeout_ms":timeout_ms}),
                    );
                },
            )
            .await?;
            if let Some(id) = response.request_id {
                receipt.value["upstream_request_id"] = json!(id);
            }
            let mut normalized = adapters::normalize::response(c, response.bytes, &response.mime)?;
            if let Some(job) = &normalized.job {
                receipt.value["upstream_job"] = json!({"id":job.id,"kind":job.kind});
                receipt.value["status"] = json!("running");
                locked(&self.store)?.update(receipt)?;
            }
            while let Some(job) = normalized.job.take() {
                wait(cancel, 500).await?;
                let raw = api::json(
                    c,
                    headers,
                    api::Operation {
                        operation: &format!(
                            "{}/{}",
                            job.kind,
                            adapters::encode_component(job.id.checked_str()?)
                        ),
                        prefix: built.prefix,
                        method: Method::GET,
                        body: Body::Empty,
                        query: None,
                    },
                    cancel,
                )
                .await?;
                normalized = adapters::normalize::normalize(c, &raw)?;
                validate_job(&normalized, &job)?;
            }
            normalized
        };
        if !claimed {
            if !locked(&self.store)?.claim(id, Some(receipt), false)? {
                return locked(&self.store)?.get(id);
            }
            *receipt = locked(&self.store)?.get(id)?;
            lease.update(receipt);
        }
        let continuation = result.continuation.clone();
        let mut completed = self
            .finish(receipt.clone(), result, input.request, cancel, true, false)
            .await?;
        if let Some(value) = continuation {
            let stored = locked(&self.store)?.save_continuation(
                id,
                &fingerprint(&json!({"identity":identity,"model":input.model}))?,
                &value,
            );
            if let Err(error) = stored {
                if !locked(&self.store)?.owns(id, Some(&completed))? {
                    return locked(&self.store)?.get(id);
                }
                completed.value["warnings"].as_array_mut().ok_or_else(NumeraError::runtime)?.push(json!("Private continuation metadata could not be retained; original outputs remain available."));
                locked(&self.store)?.update(&mut completed)?;
                self.logger.log(
                    "WARNING",
                    "generation",
                    "Continuation storage failed after retaining outputs.",
                    json!({"code":error.code}),
                );
            }
        }
        Ok(completed)
    }
}
