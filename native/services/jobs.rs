use super::{
    finalization::{self, locked},
    generation::{Generation, validate_job},
    identity::connection_identity,
    lease::Lease,
    request,
};
use crate::compat::value::{Value, json};
use crate::{
    adapters::{self, types::Job},
    config::{PinnedConnection, snapshot_credentials},
    error::{NumeraError, Result, object},
    http::api::{self, Body},
    jobs::Receipt,
};
use reqwest::Method;
use std::{sync::Mutex, time::Duration};
use tokio_util::sync::CancellationToken;

fn recovery_request(receipt: &Receipt) -> Result<Value> {
    let mut value = json!({"prompt":"recovered"});
    if let Some(required) = receipt.value["output_requirements"].as_object() {
        for (key, item) in required {
            if key
                .as_str()
                .is_none_or(|key| !["upscale_source", "dimension_evidence"].contains(&key))
            {
                value[key] = item.clone();
            }
        }
    }
    request::parse(value)
}
fn job(receipt: &Receipt) -> Result<Job> {
    let j = &receipt.value["upstream_job"];
    Ok(Job {
        id: j["id"]
            .as_utf16()
            .map(|v| crate::compat::value::JsString::from_utf16(v.to_vec()))
            .ok_or_else(NumeraError::runtime)?,
        kind: j["kind"].as_str().ok_or_else(NumeraError::runtime)?.into(),
    })
}
fn terminal(receipt: &Receipt) -> bool {
    matches!(
        receipt.status(),
        "completed" | "partial" | "failed" | "cancelled"
    )
}

pub async fn recover_local(
    generation: &Generation,
    id: &str,
    cancel: &CancellationToken,
) -> Result<Receipt> {
    let pending = { locked(&generation.store)?.needs_local_recovery(id)? };
    if !pending {
        return locked(&generation.store)?.get(id);
    }
    let claimed = { locked(&generation.store)?.claim(id, None, false)? };
    if !claimed {
        return locked(&generation.store)?.get(id);
    }
    let receipt = locked(&generation.store)?.get(id)?;
    let request = recovery_request(&receipt)?;
    let lease = Lease::start(
        generation.store.clone(),
        receipt.clone(),
        cancel.clone(),
        false,
    );
    let result = finalization::finalize(
        &generation.config.value,
        &generation.store,
        &generation.logger,
        receipt.clone(),
        finalization::Finalization {
            result: None,
            request: &request,
            cancel,
            recovery: true,
        },
    )
    .await;
    lease.close().await;
    locked(&generation.store)?.release(id, Some(&receipt))?;
    result
}

pub async fn get(
    generation: &Generation,
    id: &str,
    refresh: bool,
    cancel: &CancellationToken,
    pinned: Option<&PinnedConnection>,
) -> Result<Receipt> {
    if refresh {
        locked(&generation.store)?.queued()?;
    }
    let mut receipt = locked(&generation.store)?.get(id)?;
    if !refresh {
        return Ok(receipt);
    }
    if locked(&generation.store)?.needs_local_recovery(id)? {
        return recover_local(generation, id, cancel).await;
    }
    if receipt.value["upstream_job"].is_null() || terminal(&receipt) {
        return Ok(receipt);
    }
    let name = receipt.value["connection"]
        .as_str()
        .ok_or_else(NumeraError::runtime)?;
    let (_, selected) = generation.config.select(Some(name))?;
    let owned;
    let pinned = if let Some(pinned) = pinned {
        pinned
    } else {
        owned = snapshot_credentials(&selected, &generation.env, &generation.runtime).await?;
        &owned
    };
    if connection_identity(name, pinned)? != locked(&generation.store)?.identity(id)? {
        return Err(NumeraError::new(
            "permission_denied",
            "Existing job destination/account identity changed; network recovery is not authorized.",
        ));
    }
    if !locked(&generation.store)?.claim(id, None, false)? {
        return locked(&generation.store)?.get(id);
    }
    receipt = locked(&generation.store)?.get(id)?;
    let job = job(&receipt)?;
    let c = pinned.value();
    let headers = pinned.headers();
    let cancel = cancel.child_token();
    let lease = Lease::start(
        generation.store.clone(),
        receipt.clone(),
        cancel.clone(),
        false,
    );
    let action = async {
        let config = &generation.config.value;
        locked(&generation.store)?.reserve(
            id,
            config["files"]["maxAggregateBytes"]
                .as_i64()
                .unwrap_or(100 * 1024 * 1024),
            config["files"]["maxJournalBytes"]
                .as_i64()
                .unwrap_or(256 * 1024 * 1024),
        )?;
        let result = if job.kind == "comfyui" {
            let current = Mutex::new(receipt.clone());
            let status = adapters::comfy::status(
                c,
                &headers,
                &job.id,
                &cancel,
                adapters::comfy::OutputLimits {
                    item: config["files"]["maxOutputBytes"]
                        .as_u64()
                        .unwrap_or(50 * 1024 * 1024) as usize,
                    aggregate: config["files"]["maxAggregateBytes"]
                        .as_u64()
                        .unwrap_or(100 * 1024 * 1024) as usize,
                },
                |index, bytes| {
                    let receipt = current.lock().map_err(|_| NumeraError::storage())?;
                    finalization::retain(
                        config,
                        &generation.store,
                        id,
                        index,
                        bytes.to_vec(),
                        &receipt,
                    )?;
                    Ok(())
                },
                || {
                    let mut receipt = current.lock().map_err(|_| NumeraError::storage())?;
                    receipt.value["generation_outcome"] = json!("completed");
                    locked(&generation.store)?.update(&mut receipt)
                },
            )
            .await;
            receipt = current.into_inner().map_err(|_| NumeraError::storage())?;
            let status = status?;
            if let Some(state) = status.terminal {
                return locked(&generation.store)?.terminate(
                    id,
                    state,
                    &json!(
                        NumeraError::new("provider_rejection", "ComfyUI workflow failed.")
                            .stage("generation")
                    ),
                    Some(&receipt),
                    Some(&json!({"id":job.id,"kind":job.kind})),
                );
            }
            status.result
        } else {
            let raw = api::json(
                c,
                &headers,
                api::Operation {
                    operation: &format!(
                        "{}/{}",
                        job.kind,
                        adapters::encode_component(job.id.checked_str()?)
                    ),
                    prefix: super::discovery::prefix(c),
                    method: Method::GET,
                    body: Body::Empty,
                    query: None,
                },
                &cancel,
            )
            .await?;
            Some(adapters::normalize::normalize(c, &raw)?)
        };
        if let Some(result) = result {
            validate_job(&result, &job)?;
            if result.job.is_none() {
                return generation
                    .finish(
                        receipt.clone(),
                        result,
                        &recovery_request(&receipt)?,
                        &cancel,
                        true,
                        true,
                    )
                    .await;
            }
        }
        receipt.value["status"] = json!("running");
        locked(&generation.store)?.update(&mut receipt)?;
        Ok(receipt.clone())
    };
    let result = tokio::time::timeout(
        Duration::from_millis(c["requestTimeoutMs"].as_u64().unwrap_or(300000)),
        action,
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
        Ok(value) => Ok(value),
        Err(error) => {
            if !locked(&generation.store)?.owns(id, Some(&receipt))? {
                locked(&generation.store)?.get(id)
            } else {
                receipt = locked(&generation.store)?.get(id)?;
                receipt.value["errors"]
                    .as_array_mut()
                    .ok_or_else(NumeraError::runtime)?
                    .push(json!(error));
                receipt.value["status"] = json!(if locked(&generation.store)?.has_pending(id)? {
                    "failed"
                } else {
                    "running"
                });
                locked(&generation.store)?.update(&mut receipt)?;
                Ok(receipt.clone())
            }
        }
    };
    lease.close().await;
    locked(&generation.store)?.release(id, Some(&receipt))?;
    result
}

pub async fn cancel(
    generation: &Generation,
    id: &str,
    signal: &CancellationToken,
) -> Result<Value> {
    let receipt = locked(&generation.store)?.get(id)?;
    locked(&generation.store)?.cancel(id)?;
    if let Some(cancel) = generation
        .active
        .lock()
        .map_err(|_| NumeraError::runtime())?
        .get(id)
    {
        cancel.cancel();
    }
    if terminal(&receipt) {
        return Ok(
            json!({"request_id":id,"local_wait_cancelled":true,"upstream_requested":false,
        "upstream_cancelled":if receipt.value["upstream_terminal"]=="cancelled"{json!(true)}else{Value::Null},"refund_verified":false,"another_submission_may_charge":true}),
        );
    }
    let mut requested = false;
    let mut cancelled = Value::Null;
    if !receipt.value["upstream_job"].is_null() {
        let name = receipt.value["connection"]
            .as_str()
            .ok_or_else(NumeraError::runtime)?;
        let (_, selected) = generation.config.select(Some(name))?;
        let pinned = snapshot_credentials(&selected, &generation.env, &generation.runtime).await?;
        if connection_identity(name, &pinned)? != locked(&generation.store)?.identity(id)? {
            return Err(NumeraError::new(
                "permission_denied",
                "Existing job destination/account identity changed; cancellation is not authorized.",
            ));
        }
        let job = job(&receipt)?;
        if job.kind == "comfyui" {
            let c = pinned.value();
            let headers = pinned.headers();
            let encoded = adapters::encode_component(job.id.checked_str()?);
            let value = api::json(
                c,
                &headers,
                api::Operation {
                    operation: &format!("api/jobs/{encoded}/cancel"),
                    prefix: "",
                    method: Method::POST,
                    body: Body::Json(json!({})),
                    query: None,
                },
                signal,
            )
            .await?;
            object(&value)?;
            requested = true;
            cancelled = if value["cancelled"].is_boolean() {
                json!(false)
            } else {
                Value::Null
            };
            if value["cancelled"] == true {
                let status = api::json(
                    c,
                    &headers,
                    api::Operation {
                        operation: &format!("api/jobs/{encoded}"),
                        prefix: "",
                        method: Method::GET,
                        body: Body::Empty,
                        query: None,
                    },
                    signal,
                )
                .await?;
                object(&status)?;
                if status["id"] != job.id {
                    return Err(NumeraError::new(
                        "invalid_response",
                        "Cancellation status does not match the existing job.",
                    ));
                }
                if status["status"] == "completed" {
                    get(generation, id, true, signal, Some(&pinned)).await?;
                }
                if let Some(state) = status["status"]
                    .as_str()
                    .filter(|s| ["cancelled", "failed"].contains(s))
                {
                    locked(&generation.store)?.terminate(
                        id,
                        state,
                        &json!(
                            NumeraError::new(
                                "provider_rejection",
                                "Provider confirmed terminal job state."
                            )
                            .stage("generation")
                        ),
                        None,
                        Some(&json!({"id":job.id,"kind":job.kind})),
                    )?;
                    cancelled = json!(
                        locked(&generation.store)?.get(id)?.value["upstream_terminal"]
                            == "cancelled"
                    );
                }
            }
        }
    }
    Ok(
        json!({"request_id":id,"local_wait_cancelled":true,"upstream_requested":requested,"upstream_cancelled":cancelled,"refund_verified":false,"another_submission_may_charge":true}),
    )
}
