use super::{
    types::{Input, Job, Normalized, ResultItem},
    workflow,
};
use crate::compat::value::{JsString, Value, json};
use crate::{
    error::{NumeraError, Result, array, object},
    http::api::{self, Body},
};
use reqwest::{
    Method,
    multipart::{Form, Part},
};
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct Status {
    pub result: Option<Normalized>,
    pub terminal: Option<&'static str>,
}
fn invalid(message: &str) -> NumeraError {
    NumeraError::new("invalid_response", message).stage("response")
}

pub async fn prepare(
    input: &Input<'_>,
    headers: &HashMap<String, String>,
    cancel: &CancellationToken,
) -> Result<Value> {
    let graph = workflow::prepare(input)?;
    let c = input.connection;
    let info = api::json(
        c,
        headers,
        api::Operation {
            operation: "object_info",
            prefix: "",
            method: Method::GET,
            body: Body::Empty,
            query: None,
        },
        cancel,
    )
    .await?;
    object(&info)?;
    workflow::validate(&graph, &c["workflow"], &info)?;
    if c["workflow"]["requireGpu"] == true {
        let stats = api::json(
            c,
            headers,
            api::Operation {
                operation: "system_stats",
                prefix: "",
                method: Method::GET,
                body: Body::Empty,
                query: None,
            },
            cancel,
        )
        .await?;
        object(&stats)?;
        let first = array(&stats["devices"]).first().ok_or_else(|| {
            NumeraError::new(
                "unsupported_operation",
                "Configured ComfyUI reports no primary GPU; CPU inference is not authorized.",
            )
        })?;
        object(first)?;
        if first["type"] == "cpu" {
            return Err(NumeraError::new(
                "unsupported_operation",
                "Configured ComfyUI reports no primary GPU; CPU inference is not authorized.",
            ));
        }
    }
    Ok(graph)
}

pub async fn submit(
    input: &Input<'_>,
    mut graph: Value,
    headers: &HashMap<String, String>,
    cancel: &CancellationToken,
) -> Result<(Job, Vec<JsString>)> {
    workflow::references(input)?;
    let c = input.connection;
    for (index, image) in input.references.iter().enumerate() {
        let key = if index == 0 {
            "image".into()
        } else {
            format!("image_{}", index + 1)
        };
        let part = Part::bytes(image.bytes.clone())
            .file_name(format!("{}.{}", Uuid::new_v4(), image.extension))
            .mime_str(&image.mime)
            .map_err(|_| invalid("Invalid image upload type."))?;
        let upload = api::json(
            c,
            headers,
            api::Operation {
                operation: "upload/image",
                prefix: "",
                method: Method::POST,
                body: Body::Multipart(Form::new().part("image", part).text("type", "input")),
                query: None,
            },
            cancel,
        )
        .await?;
        let identity = upload_identity(&upload, "ComfyUI upload returned invalid file identity.")?;
        workflow::bind(&mut graph, &c["workflow"], &key, json!(identity), true)?;
    }
    if let Some(image) = input.mask {
        let part = Part::bytes(image.bytes.clone())
            .file_name(format!("{}.png", Uuid::new_v4()))
            .mime_str(&image.mime)
            .map_err(|_| invalid("Invalid mask upload type."))?;
        let upload = api::json(
            c,
            headers,
            api::Operation {
                operation: "upload/image",
                prefix: "",
                method: Method::POST,
                body: Body::Multipart(Form::new().part("image", part)),
                query: None,
            },
            cancel,
        )
        .await?;
        let identity = upload_identity(
            &upload,
            "ComfyUI mask upload returned invalid file identity.",
        )?;
        workflow::bind(&mut graph, &c["workflow"], "mask", json!(identity), true)?;
    }
    let value = api::json(
        c,
        headers,
        api::Operation {
            operation: "prompt",
            prefix: "",
            method: Method::POST,
            body: Body::Json(json!({"prompt":graph,"client_id":Uuid::new_v4().to_string()})),
            query: None,
        },
        cancel,
    )
    .await?;
    object(&value)?;
    let id = value["prompt_id"]
        .as_utf16()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            NumeraError::new(
                "outcome_unknown",
                "ComfyUI submission has no recoverable prompt ID.",
            )
            .stage("submission")
        })?;
    let empty = json!({});
    let node_errors = object(
        value
            .get("node_errors")
            .filter(|v| !v.is_null())
            .unwrap_or(&empty),
    )?;
    let warnings = if node_errors.is_empty() {
        vec![]
    } else {
        let mut units: Vec<u16> =
            "ComfyUI accepted only valid workflow branches; rejected node IDs: "
                .encode_utf16()
                .collect();
        for (index, key) in node_errors.keys().enumerate() {
            if index != 0 {
                units.extend(", ".encode_utf16());
            }
            units.extend(key.as_utf16());
        }
        vec![JsString::from_utf16(units)]
    };
    Ok((
        Job {
            id: JsString::from_utf16(id.to_vec()),
            kind: "comfyui".into(),
        },
        warnings,
    ))
}

fn upload_identity(value: &Value, message: &str) -> Result<String> {
    object(value)?;
    let name = value["name"]
        .as_str()
        .ok_or_else(|| NumeraError::new("invalid_response", message).stage("upload"))?;
    let folder = value["subfolder"]
        .as_str()
        .ok_or_else(|| NumeraError::new("invalid_response", message).stage("upload"))?;
    Ok(if folder.is_empty() {
        name.into()
    } else {
        format!("{folder}/{name}")
    })
}

pub struct OutputLimits {
    pub item: usize,
    pub aggregate: usize,
}

pub async fn status(
    c: &Value,
    headers: &HashMap<String, String>,
    id: &JsString,
    cancel: &CancellationToken,
    limits: OutputLimits,
    mut retain: impl FnMut(u32, &[u8]) -> Result<()>,
    mut completed: impl FnMut() -> Result<()>,
) -> Result<Status> {
    let encoded = super::gemini::encode_component(id.checked_str()?);
    let history = api::json(
        c,
        headers,
        api::Operation {
            operation: &format!("history/{encoded}"),
            prefix: "",
            method: Method::GET,
            body: Body::Empty,
            query: None,
        },
        cancel,
    )
    .await?;
    object(&history)?;
    let Some(entry) = history.get(id).filter(|v| !v.is_null() && **v != false) else {
        return Ok(Status {
            result: None,
            terminal: None,
        });
    };
    object(entry)?;
    object(&entry["status"])?;
    if entry["status"]["status_str"] == "error" {
        return Ok(Status {
            result: None,
            terminal: Some("failed"),
        });
    }
    if entry["status"]["completed"] != true {
        return Ok(Status {
            result: None,
            terminal: None,
        });
    }
    completed()?;
    object(&entry["outputs"])?;
    let mut images = Vec::new();
    let mut total = 0usize;
    for node in array(&c["workflow"]["outputNodes"]) {
        let node = node
            .as_str()
            .ok_or_else(|| invalid("Invalid output node."))?;
        let empty = json!({});
        let output = entry["outputs"]
            .get(node)
            .filter(|v| !v.is_null())
            .unwrap_or(&empty);
        object(output)?;
        for file in array(&output["images"]) {
            object(file)?;
            let Some(filename) = file["filename"].as_str() else {
                continue;
            };
            let Some(kind) = file["type"]
                .as_str()
                .filter(|s| ["output", "temp"].contains(s))
            else {
                continue;
            };
            if images.len() >= 10 {
                return Err(invalid("Provider returned too many final image items."));
            }
            let query = json!({"filename":filename,"subfolder":file["subfolder"].as_str().unwrap_or(""),"type":kind});
            let response = api::request(
                c,
                headers,
                api::Operation {
                    operation: "view",
                    prefix: "",
                    method: Method::GET,
                    body: Body::Empty,
                    query: Some(&query),
                },
                cancel,
                limits.item,
            )
            .await?;
            total = total
                .checked_add(response.bytes.len())
                .ok_or_else(|| invalid("Aggregate output bytes exceeded."))?;
            if total > limits.aggregate {
                return Err(invalid("Aggregate output bytes exceeded."));
            }
            retain(images.len() as u32, &response.bytes)?;
            images.push(ResultItem {
                bytes: Some(response.bytes),
                ..Default::default()
            });
        }
    }
    if images.is_empty() {
        return Err(NumeraError::new(
            "no_image_returned",
            "ComfyUI finished without owned final output images.",
        )
        .stage("response"));
    }
    Ok(Status {
        result: Some(Normalized {
            images,
            upstream_id: Some(id.into()),
            ..Default::default()
        }),
        terminal: None,
    })
}
