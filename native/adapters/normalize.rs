use super::types::{Job, Normalized, ResultItem};
use crate::compat::value::{JsString, Value, json};
use crate::{
    error::{NumeraError, Result},
    http::sse,
};

fn error(code: &str, message: &str) -> NumeraError {
    NumeraError::new(code, message).stage("response")
}
fn malformed(images: &mut Vec<ResultItem>, value: &Value) -> bool {
    if value.is_object() {
        true
    } else {
        images.push(ResultItem {
            error: Some("Malformed image result item.".into()),
            ..Default::default()
        });
        false
    }
}
fn array(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}
fn string(value: &Value) -> Option<JsString> {
    if let Value::String(value) = value {
        Some(value.clone())
    } else {
        None
    }
}
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::String(v) => !v.is_empty(),
        Value::Number(v) => v.as_f64().is_some_and(|v| v != 0.0 && !v.is_nan()),
        _ => true,
    }
}
fn js_string(value: &Value) -> JsString {
    enum Token<'a> {
        Value(&'a Value, bool),
        Comma,
    }
    let mut units = Vec::new();
    let mut pending = vec![Token::Value(value, false)];
    while let Some(token) = pending.pop() {
        match token {
            Token::Comma => units.push(44),
            Token::Value(Value::String(s), _) => units.extend(s.as_utf16()),
            Token::Value(Value::Null, false) => units.extend("null".encode_utf16()),
            Token::Value(Value::Null, true) => {}
            Token::Value(Value::Object(_), _) => units.extend("[object Object]".encode_utf16()),
            Token::Value(Value::Array(array), _) => {
                for (index, value) in array.iter().enumerate().rev() {
                    pending.push(Token::Value(value, true));
                    if index != 0 {
                        pending.push(Token::Comma);
                    }
                }
            }
            Token::Value(Value::Number(number), _) => {
                units.extend(ryu_js::Buffer::new().format(number.0).encode_utf16())
            }
            Token::Value(Value::Bool(b), _) => {
                units.extend(if *b { "true" } else { "false" }.encode_utf16())
            }
        }
    }
    JsString::from_utf16(units)
}
fn push(images: &mut Vec<ResultItem>, value: &Value) {
    if !malformed(images, value) {
        return;
    }
    if let Some(encoded) = value["b64_json"].as_js_string().filter(|s| !s.is_empty()) {
        images.push(ResultItem {
            base64: Some(encoded.clone()),
            ..Default::default()
        });
    } else if let Some(url) = string(&value["url"]).filter(|s| !s.is_empty()) {
        images.push(ResultItem {
            url: Some(url),
            ..Default::default()
        });
    } else {
        images.push(ResultItem {
            error: Some("Image item contains no final bytes or URL.".into()),
            ..Default::default()
        });
    }
}

pub fn normalize(connection: &Value, value: &Value) -> Result<Normalized> {
    if !value.is_object() {
        return Err(error("invalid_response", "Expected an object."));
    }
    let mut result = Normalized::default();
    let mut signatures = Vec::new();
    let adapter = connection["adapter"].as_str().unwrap_or("");
    if adapter == "gemini" {
        if truthy(&value["promptFeedback"]) {
            if !value["promptFeedback"].is_object() {
                return Err(error("invalid_response", "Expected an object."));
            }
            if truthy(&value["promptFeedback"]["blockReason"]) {
                return Err(error(
                    "provider_rejection",
                    "Provider rejected image generation.",
                ));
            }
        }
        for candidate in array(&value["candidates"]) {
            if !malformed(&mut result.images, candidate) {
                continue;
            }
            if truthy(&candidate["finishReason"]) && candidate["finishReason"] != "STOP" {
                let mut units: Vec<u16> = "Candidate ended with ".encode_utf16().collect();
                units.extend(js_string(&candidate["finishReason"]).as_utf16());
                units.push(46);
                result.warnings.push(JsString::from_utf16(units));
            }
            let content = &candidate["content"];
            if truthy(content) && !malformed(&mut result.images, content) {
                continue;
            }
            for (i, part) in array(&content["parts"]).iter().enumerate() {
                if !malformed(&mut result.images, part) {
                    continue;
                }
                if part["thoughtSignature"].is_string() {
                    signatures.push(json!({"part_index":i,"signature":part["thoughtSignature"]}));
                }
                if part["thought"] == true {
                    continue;
                }
                if truthy(&part["inlineData"]) && malformed(&mut result.images, &part["inlineData"])
                {
                    push(
                        &mut result.images,
                        &json!({"b64_json":part["inlineData"]["data"]}),
                    );
                }
            }
        }
    } else if ["gemini-interactions", "openai-responses"].contains(&adapter) {
        let status = value["status"].as_str().unwrap_or("");
        if ["in_progress", "queued"].contains(&status) {
            let id = string(&value["id"])
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    error(
                        "outcome_unknown",
                        "Async response has no recoverable job ID.",
                    )
                })?;
            result.job = Some(Job {
                id,
                kind: if adapter == "gemini-interactions" {
                    "interactions"
                } else {
                    "responses"
                }
                .into(),
            });
            if adapter == "gemini-interactions" {
                result.upstream_model = string(&value["model"]);
            }
            return Ok(result);
        }
        if adapter == "gemini-interactions" && status == "budget_exceeded" {
            result.terminal = Some("failed".into());
        } else if ["failed", "cancelled", "incomplete"].contains(&status) {
            result.terminal = Some(status.into());
        } else if truthy(&value["status"]) && status != "completed" {
            return Err(error(
                "provider_rejection",
                &format!(
                    "{} ended with {}.",
                    if adapter == "gemini-interactions" {
                        "Interaction"
                    } else {
                        "Response"
                    },
                    js_string(&value["status"])
                ),
            ));
        }
        if adapter == "gemini-interactions" {
            for step in array(&value["steps"]) {
                if !malformed(&mut result.images, step) {
                    continue;
                }
                if step["type"] == "thought" && step["signature"].is_string() {
                    signatures.push(json!({"step_type":"thought","signature":step["signature"]}));
                }
                if step["type"] != "model_output" {
                    continue;
                }
                for part in array(&step["content"]) {
                    if malformed(&mut result.images, part) && part["type"] == "image" {
                        push(
                            &mut result.images,
                            &json!({"b64_json":part["data"],"url":part["uri"]}),
                        );
                    }
                }
            }
            if !result
                .images
                .iter()
                .any(|i| i.base64.is_some() || i.url.is_some())
                && truthy(&value["output_image"])
                && malformed(&mut result.images, &value["output_image"])
            {
                push(
                    &mut result.images,
                    &json!({"b64_json":value["output_image"]["data"],"url":value["output_image"]["uri"]}),
                );
            }
        } else {
            for item in array(&value["output"]) {
                if malformed(&mut result.images, item)
                    && item["type"] == "image_generation_call"
                    && item["status"] == "completed"
                {
                    push(&mut result.images, &json!({"b64_json":item["result"]}));
                }
            }
        }
    } else if adapter == "chat-images" {
        for choice in array(&value["choices"]) {
            if !malformed(&mut result.images, choice) {
                continue;
            }
            let message = &choice["message"];
            if !malformed(&mut result.images, message) {
                continue;
            }
            if truthy(&message["refusal"]) {
                return Err(error(
                    "provider_rejection",
                    "Provider refused image output.",
                ));
            }
            for image in array(&message["images"]) {
                if malformed(&mut result.images, image)
                    && malformed(&mut result.images, &image["image_url"])
                {
                    push(
                        &mut result.images,
                        &json!({"url":image["image_url"]["url"]}),
                    );
                }
            }
        }
    } else {
        for item in array(&value["data"]) {
            push(&mut result.images, item);
        }
    }
    if result.terminal.is_none()
        && !result
            .images
            .iter()
            .any(|i| i.base64.is_some() || i.url.is_some() || i.bytes.is_some())
    {
        return Err(error(
            "no_image_returned",
            "No final image was returned; text, empty bodies and partial previews are not final images.",
        ));
    }
    result.upstream_model = string(&value["model"]);
    result.upstream_id = string(&value["id"]);
    result.usage = value.get("usage").cloned();
    if !signatures.is_empty() {
        result.continuation = Some(json!({"signatures":signatures}));
    }
    Ok(result)
}

pub fn response(connection: &Value, bytes: Vec<u8>, mime: &str) -> Result<Normalized> {
    let image_mime = mime.split(';').next().unwrap_or("");
    if ["image/png", "image/jpeg", "image/webp"].contains(&image_mime) {
        return Ok(Normalized {
            images: vec![ResultItem {
                bytes: Some(bytes),
                ..Default::default()
            }],
            ..Default::default()
        });
    }
    if !mime.contains("text/event-stream") {
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| error("invalid_response", "Invalid response UTF-8."))?;
        let value = Value::parse_with_limit(text, 128 * 1024 * 1024)
            .map_err(|_| error("invalid_response", "Image response is not valid JSON."))?;
        return normalize(connection, &value);
    }
    let events = sse::events(&bytes, 16 * 1024 * 1024)?;
    if connection["adapter"] == "openai-responses"
        && let Some(event) = events
            .iter()
            .find(|e| ["response.failed", "response.incomplete"].contains(&e.kind.as_str()))
    {
        let result = &event.data["response"];
        if truthy(result) {
            if !result.is_object() {
                return Err(error("invalid_response", "Expected an object."));
            }
            if result["id"].as_str().is_some_and(|s| !s.trim().is_empty())
                && ["failed", "incomplete"].contains(&result["status"].as_str().unwrap_or(""))
            {
                return normalize(connection, result);
            }
        }
    }
    if events
        .iter()
        .any(|e| ["error", "response.failed", "response.incomplete"].contains(&e.kind.as_str()))
    {
        return Err(error(
            "outcome_unknown",
            "Image stream ended with an error; no resubmission made.",
        ));
    }
    if connection["gateway"] == "9router" {
        let done = events
            .iter()
            .find(|e| e.kind == "done" && truthy(&e.data["data"]))
            .ok_or_else(|| {
                error(
                    "outcome_unknown",
                    "9router stream has no terminal done result.",
                )
            })?;
        return normalize(connection, &done.data);
    }
    match connection["adapter"].as_str().unwrap_or("") {
        "openrouter-images" => {
            let finals: Vec<_> = events
                .iter()
                .filter(|e| e.kind == "image_generation.completed")
                .collect();
            if !events.iter().any(|e| e.kind == "done") || finals.is_empty() {
                return Err(error(
                    "outcome_unknown",
                    "Image stream has no final completion.",
                ));
            }
            let mut value = json!({"data":finals.iter().map(|e|&e.data).collect::<Vec<_>>()});
            if let Some(usage) = finals.last().and_then(|event| event.data.get("usage")) {
                value["usage"] = usage.clone();
            }
            normalize(connection, &value)
        }
        "openai-responses" => {
            let done = events
                .iter()
                .find(|e| e.kind == "response.completed")
                .filter(|e| truthy(&e.data["response"]))
                .ok_or_else(|| {
                    error(
                        "outcome_unknown",
                        "Responses stream has no completed response.",
                    )
                })?;
            normalize(connection, &done.data["response"])
        }
        "gemini" => normalize(
            connection,
            &json!({"candidates":events.iter().filter(|e|truthy(&e.data["candidates"])).flat_map(|e|array(&e.data["candidates"]).iter()).collect::<Vec<_>>()}),
        ),
        _ => Err(error(
            "unsupported_operation",
            "Streaming contract is not implemented for this adapter.",
        )),
    }
}
