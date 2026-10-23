use super::requirements;
use crate::compat::value::{Value, json};
use crate::{
    adapters::types::Normalized,
    error::{NumeraError, Result},
    files::{
        images::{self, Image},
        output,
    },
    http::assets,
    jobs::{Receipt, Store},
    logging::Logger,
};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

pub type SharedStore = Arc<Mutex<Store>>;
pub fn locked(store: &SharedStore) -> Result<std::sync::MutexGuard<'_, Store>> {
    store.lock().map_err(|_| NumeraError::storage())
}
fn push(receipt: &mut Receipt, key: &str, value: Value) -> Result<()> {
    receipt.value[key]
        .as_array_mut()
        .ok_or_else(NumeraError::storage)?
        .push(value);
    Ok(())
}

pub fn retain(
    config: &Value,
    store: &SharedStore,
    id: &str,
    index: u32,
    bytes: Vec<u8>,
    receipt: &Receipt,
) -> Result<Image> {
    let image = images::inspect(bytes, config, None)?;
    locked(store)?.retain(id, index, &image.bytes, Some(receipt))?;
    Ok(image)
}

pub struct Finalization<'a> {
    pub result: Option<Normalized>,
    pub request: &'a Value,
    pub cancel: &'a CancellationToken,
    pub recovery: bool,
}

pub async fn finalize(
    config: &Value,
    store: &SharedStore,
    logger: &Logger,
    receipt: Receipt,
    input: Finalization<'_>,
) -> Result<Receipt> {
    let Finalization {
        result,
        request,
        cancel,
        recovery,
    } = input;
    let mut receipt = receipt;
    let id = receipt.id()?.to_owned();
    locked(store)?.reserve(
        &id,
        config["files"]["maxAggregateBytes"]
            .as_i64()
            .unwrap_or(100 * 1024 * 1024),
        config["files"]["maxJournalBytes"]
            .as_i64()
            .unwrap_or(256 * 1024 * 1024),
    )?;
    if !locked(store)?.owns(&id, Some(&receipt))? {
        return Err(NumeraError::new(
            "outcome_unknown",
            "Request writer generation changed before output recovery.",
        )
        .stage("storage"));
    }
    if receipt.value.get("output_requirements").is_none() {
        let mut required = json!({"count":request["count"]});
        for key in [
            "size",
            "output_format",
            "background",
            "output_subdirectory",
            "filename_prefix",
        ] {
            if let Some(value) = request.get(key) {
                required[key] = value.clone();
            }
        }
        if !recovery
            && let (Some(connection), Some(Value::String(model))) = (
                receipt.value["connection"]
                    .as_str()
                    .and_then(|s| config["connections"].get(s)),
                receipt.value.get("requested_model"),
            )
            && let (Some(object), Some(values)) = (
                required.as_object_mut(),
                requirements::snapshot(connection, model, request).as_object(),
            )
        {
            object.extend(values.clone());
        }
        receipt.value["output_requirements"] = required;
    }
    if receipt.value["output_requirements"]["output_subdirectory"].is_null() {
        let date = receipt.value["created_at"]
            .as_str()
            .and_then(|s| s.get(..10))
            .ok_or_else(NumeraError::storage)?
            .to_owned();
        receipt.value["output_requirements"]["output_subdirectory"] = json!(date);
    }
    receipt.value["status"] = json!("finalizing");
    receipt.value["generation_outcome"] = if receipt.value["upstream_terminal"].is_null() {
        json!("completed")
    } else {
        receipt.value["upstream_terminal"].clone()
    };
    receipt.value["storage_outcome"] = json!("writing");
    if let Some(errors) = receipt.value["errors"].as_array_mut() {
        errors.retain(|e| {
            !matches!(
                e["code"].as_str(),
                Some("output_file_error" | "cancelled" | "request_timeout" | "upstream_error")
            )
        });
    }
    if let Some(values) = receipt.value["deviations"].as_array_mut() {
        values.retain(|v| !v.as_str().is_some_and(|s| s.starts_with("Requested ")));
    }
    if let Some(result) = result.as_ref() {
        receipt.value["upstream_model"] = json!(result.upstream_model);
        if let Some(id) = &result.upstream_id {
            receipt.value["upstream_request_id"] = json!(id);
        }
        if let Some(usage) = &result.usage {
            receipt.value["usage"] = usage.clone();
        } else if let Some(object) = receipt.value.as_object_mut() {
            object.remove("usage");
        }
        for warning in &result.warnings {
            if !crate::error::array(&receipt.value["warnings"])
                .iter()
                .any(|v| v == warning)
            {
                push(&mut receipt, "warnings", json!(warning))?;
            }
        }
    }
    for warning in requirements::warnings(&receipt) {
        if !crate::error::array(&receipt.value["warnings"])
            .iter()
            .any(|v| v == &warning)
        {
            push(&mut receipt, "warnings", json!(warning))?;
        }
    }
    let max = config["files"]["maxOutputBytes"]
        .as_u64()
        .unwrap_or(50 * 1024 * 1024) as usize;
    if let Some(result) = result {
        if result.images.len() > 10 {
            return Err(NumeraError::new(
                "invalid_response",
                "Provider returned too many final image items.",
            )
            .stage("response"));
        }
        // Completed inline bytes are journaled before observing cancellation or publishing files.
        for (index, item) in result.images.into_iter().enumerate() {
            let action = || -> Result<()> {
                if let Some(error) = item.error {
                    return Err(NumeraError::new("invalid_response", &error).stage("response"));
                }
                if let Some(bytes) = item.bytes {
                    retain(config, store, &id, index as u32, bytes, &receipt)?;
                } else if let Some(value) = item.base64 {
                    retain(
                        config,
                        store,
                        &id,
                        index as u32,
                        images::decode_base64(
                            value.checked_str().map_err(|_| {
                                NumeraError::new(
                                    "invalid_response",
                                    "Invalid Base64 image encoding.",
                                )
                            })?,
                            max,
                        )?,
                        &receipt,
                    )?;
                } else if let Some(url) = item.url {
                    if url.starts_with("data:") {
                        retain(
                            config,
                            store,
                            &id,
                            index as u32,
                            images::data_url(
                                url.checked_str().map_err(|_| {
                                    NumeraError::new("invalid_input", "Invalid image data URL.")
                                })?,
                                max,
                            )?,
                            &receipt,
                        )?;
                    } else {
                        locked(store)?.retain_url(&id, index as u32, &url, Some(&receipt))?;
                    }
                } else {
                    return Err(NumeraError::new(
                        "invalid_response",
                        "Image item has no usable bytes or continuation URL.",
                    )
                    .stage("response"));
                }
                Ok(())
            };
            if let Err(error) = action() {
                if !locked(store)?.owns(&id, Some(&receipt))? {
                    return Err(error);
                }
                push(&mut receipt, "errors", json!(error))?;
            }
        }
    }
    locked(store)?.update(&mut receipt)?;
    let pending = locked(store)?.pending(&id)?;
    for item in pending {
        let action = async {
            if cancel.is_cancelled() {
                return Err(NumeraError::new(
                    "cancelled",
                    "Waiting stopped; upstream completion and billing may be unknown.",
                )
                .stage("network"));
            }
            if !recovery && locked(store)?.cancelled(&id)? {
                return Err(NumeraError::new(
                    "cancelled",
                    "Local output recovery was cancelled; completed image bytes remain retained.",
                )
                .stage("storage"));
            }
            let bytes = if let Some(bytes) = item.bytes {
                bytes
            } else {
                let url = item.url.as_ref().ok_or_else(|| {
                    NumeraError::new(
                        "output_file_error",
                        "Retained image item has no recoverable bytes or URL.",
                    )
                    .stage("storage")
                })?;
                let bytes = assets::fetch(url, config, max, cancel).await?;
                retain(config, store, &id, item.index, bytes.clone(), &receipt)?;
                bytes
            };
            let image = images::inspect(bytes, config, None)?;
            let deviations = requirements::deviations(&receipt, &image, item.index);
            output::save(
                &image,
                config,
                &mut *locked(store)?,
                output::Publication {
                    id: &id,
                    index: item.index,
                    subdir: receipt.value["output_requirements"]["output_subdirectory"].as_str(),
                    prefix: receipt.value["output_requirements"]["filename_prefix"].as_str(),
                    deviations: &deviations,
                    expected: Some(&receipt),
                    stable: true,
                },
            )?;
            Ok::<(), NumeraError>(())
        }
        .await;
        match action {
            Ok(()) => {
                let current = locked(store)?.get(&id)?;
                receipt.value["outputs"] = current.value["outputs"].clone();
                receipt.value["deviations"] = current.value["deviations"].clone();
            }
            Err(error) => {
                if !locked(store)?.owns(&id, Some(&receipt))? {
                    return Err(error);
                }
                push(&mut receipt, "errors", json!(error))?;
                if cancel.is_cancelled() || (!recovery && locked(store)?.cancelled(&id)?) {
                    break;
                }
            }
        }
    }
    let count = crate::error::array(&receipt.value["outputs"]).len();
    let expected = request["count"].as_u64().unwrap_or(1) as usize;
    if count != expected {
        push(
            &mut receipt,
            "deviations",
            json!(format!("Requested {expected} image(s), saved {count}.")),
        )?;
    }
    let pending = locked(store)?.has_pending(&id)?;
    let errors = !crate::error::array(&receipt.value["errors"]).is_empty();
    let deviations = !crate::error::array(&receipt.value["deviations"]).is_empty();
    receipt.value["status"] = json!(if count == 0 {
        "failed"
    } else if errors || deviations || pending {
        "partial"
    } else {
        "completed"
    });
    receipt.value["storage_outcome"] = json!(if errors || pending {
        "partial_or_failed"
    } else {
        "completed"
    });
    locked(store)?.update(&mut receipt)?;
    logger.log(
        "INFO",
        "generation",
        "Outputs processed.",
        json!({"requestId":id,"status":receipt.status(),"count":count,"retained":pending}),
    );
    Ok(receipt)
}
