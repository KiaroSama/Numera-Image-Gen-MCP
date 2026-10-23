use crate::compat::value::{JsString, Value, json};
use crate::{
    dimensions::validate,
    error::{NumeraError, Result},
};

pub const COMMON_FIELDS: &[&str] = &[
    "size",
    "aspect_ratio",
    "image_size",
    "quality",
    "output_format",
    "background",
    "seed",
    "negative_prompt",
];
pub fn codex(model: &JsString) -> bool {
    model.starts_with("codex/") || model.starts_with("cx/")
}

pub fn safe_options(value: &Value, allowed: Option<&Value>, depth: usize) -> Result<Value> {
    let object = value
        .as_object()
        .ok_or_else(|| NumeraError::new("invalid_input", "Provider options must be an object."))?;
    if depth > 6 || object.len() > 50 {
        return Err(NumeraError::new(
            "invalid_input",
            "Provider options exceed structure limits.",
        ));
    }
    const RESERVED: &[&str] = &[
        "model",
        "prompt",
        "n",
        "count",
        "image",
        "images",
        "image_url",
        "image_urls",
        "input_references",
        "contents",
        "input",
        "messages",
        "tools",
        "mask",
        "url",
        "baseUrl",
        "headers",
        "auth",
        "authorization",
        "request_id",
        "__proto__",
        "prototype",
        "constructor",
    ];
    for (key, child) in object {
        let sensitive = [
            "token", "secret", "password", "upload", "endpoint", "apikey",
        ]
        .iter()
        .any(|part| key.contains_ascii_case_insensitive(part))
            || key.as_utf16().windows(7).any(|units| {
                let part = JsString::from_utf16(units.to_vec());
                JsString::from_utf16(part.as_utf16()[..3].to_vec())
                    .contains_ascii_case_insensitive("api")
                    && JsString::from_utf16(units[4..].to_vec())
                        .contains_ascii_case_insensitive("key")
            });
        if key.as_str().is_some_and(|key| RESERVED.contains(&key))
            || sensitive
            || allowed
                .and_then(Value::as_array)
                .is_some_and(|keys| !keys.iter().any(|v| v == key))
        {
            return Err(NumeraError::new(
                "unsupported_parameter",
                "An explicit provider option is not allowed.",
            ));
        }
        if let Some(array) = child.as_array() {
            if array.len() > 32 || array.iter().any(|v| v.is_object() || v.is_array()) {
                return Err(NumeraError::new(
                    "invalid_input",
                    "Nested option arrays are unsupported.",
                ));
            }
        } else if child.is_object() {
            safe_options(child, None, depth + 1)?;
        }
    }
    Ok(value.clone())
}

pub fn capabilities(connection: &Value, model: &JsString) -> Value {
    let adapter = connection["adapter"].as_str().unwrap_or("");
    let gateway = connection["gateway"].as_str();
    let native = [
        "gemini",
        "gemini-interactions",
        "openai-responses",
        "openrouter-images",
        "chat-images",
    ]
    .contains(&adapter);
    let edit = connection.get("edit");
    let mut refs = if native || edit.is_some() {
        "supported"
    } else {
        "unknown"
    };
    let mut masks = if connection["edit"]["masks"] == true {
        "supported"
    } else {
        "unsupported"
    };
    let mut limitations = Vec::new();
    if gateway.is_some() && model.starts_with("antigravity/") {
        refs = "unsupported";
        limitations.push("Examined gateway Antigravity route forwards text only; reference editing is disabled until explicitly verified by a corrected gateway version.");
    }
    if gateway.is_some_and(|g| ["omniroute", "9router"].contains(&g)) {
        masks = "unsupported";
        if codex(model) {
            refs = "supported";
        }
    }
    if native {
        masks = "unsupported";
    }
    if adapter == "comfyui" {
        refs = if connection["workflow"]["bindings"]
            .get("image")
            .is_some_and(|v| !v.is_null())
        {
            "supported"
        } else {
            "unsupported"
        };
        masks = if connection["workflow"]["bindings"]
            .get("mask")
            .is_some_and(|v| !v.is_null())
            && connection["edit"]["maskPolarity"]
                .as_str()
                .is_some_and(|v| !v.is_empty())
        {
            "supported"
        } else {
            "unsupported"
        };
    }
    let overrides = connection["modelOverrides"].get(model);
    let mut forwarding = json!({"text_to_image":"supported", "image_to_image":refs, "masks":masks});
    if let Some(values) = overrides.and_then(|v| v["capabilities"].as_object())
        && let Some(object) = forwarding.as_object_mut()
    {
        object.extend(values.clone());
    }
    let max_refs = overrides
        .and_then(|v| v["maxReferences"].as_u64())
        .unwrap_or_else(|| {
            if gateway == Some("omniroute") && codex(model) {
                8
            } else if gateway == Some("9router") {
                1
            } else if native {
                14
            } else {
                connection["edit"]["maxReferences"].as_u64().unwrap_or(1)
            }
        });
    json!({"model":{"text_to_image":"unknown","image_to_image":"unknown"}, "gateway_forwarding":forwarding,
        "account_availability":"unknown", "max_references":max_refs,
        "max_count":overrides.and_then(|v| v["maxCount"].as_u64()).unwrap_or(1),
        "parameters":overrides.and_then(|v| v.get("supportedParameters")).cloned().unwrap_or(Value::Null),
        "evidence":[{"kind":if overrides.is_some(){"configured"}else{"documented"}, "gateway_version":connection.get("gatewayVersion"),
            "source":match gateway {Some("9router")=>Some("https://github.com/decolua/9router"),Some("omniroute")=>Some("https://github.com/diegosouzapw/OmniRoute"),_=>None}}],
        "limitations":limitations})
}

pub fn edit_contract(edit: Option<&Value>, count: usize) -> Result<()> {
    let Some(edit) = edit else {
        return Ok(());
    };
    let encoding = edit["encoding"].as_str().unwrap_or("");
    if edit["mode"] == "multipart" {
        if !["image", "image[]"].contains(&encoding) {
            return Err(NumeraError::new(
                "invalid_configuration",
                "Multipart editing requires image or image[] encoding.",
            ));
        }
    } else if encoding == "image[]" {
        return Err(NumeraError::new(
            "invalid_configuration",
            "JSON editing cannot use multipart image[] encoding.",
        ));
    } else if encoding == "image" && (edit["maxReferences"].as_u64().unwrap_or(1) > 1 || count > 1)
    {
        return Err(NumeraError::new(
            "unsupported_operation",
            "Scalar image encoding supports exactly one reference; use a verified array encoding.",
        ));
    }
    Ok(())
}

pub fn effective_request(connection: &Value, request: &Value, model: &JsString) -> Result<Value> {
    let mut merged = connection["defaults"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let overrides = &connection["modelOverrides"][model];
    if let Some(values) = overrides["defaults"].as_object() {
        merged.extend(values.clone());
    }
    merged.extend(
        request
            .as_object()
            .ok_or_else(|| NumeraError::new("invalid_input", "Expected an image request object."))?
            .clone(),
    );
    for key in COMMON_FIELDS {
        if request.get(*key).is_none()
            && let Some(value) = overrides["defaults"]
                .get(*key)
                .filter(|v| !v.is_null())
                .or_else(|| connection["defaults"].get(*key))
        {
            merged.insert((*key).into(), value.clone());
        }
    }
    let mut options = connection["extensions"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    if let Some(values) = request["provider_options"].as_object() {
        options.extend(values.clone());
    }
    let options = safe_options(
        &Value::Object(options),
        connection.get("providerOptionKeys"),
        0,
    )?;
    for key in COMMON_FIELDS {
        if merged.contains_key(*key) && options.get(*key).is_some() {
            return Err(NumeraError::new(
                "invalid_input",
                &format!("Conflicting common/provider parameter {key}."),
            ));
        }
    }
    merged.insert("provider_options".into(), options);
    let merged = Value::Object(merged);
    if merged["background"] == "transparent" && merged["output_format"] == "jpeg" {
        return Err(NumeraError::new(
            "unsupported_parameter",
            "Transparent background cannot use JPEG.",
        ));
    }
    if connection["allowedModels"]
        .as_array()
        .is_some_and(|a| !a.iter().any(|v| v == model))
        || connection["deniedModels"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == model))
    {
        return Err(NumeraError::new(
            "model_unavailable",
            "Model is denied by connection policy.",
        ));
    }
    let refs = request["reference_images"].as_array().map_or(0, Vec::len);
    edit_contract(
        connection.get("edit"),
        merged["reference_images"].as_array().map_or(0, Vec::len),
    )?;
    validate(connection, model, &merged)?;
    let cap = capabilities(connection, model);
    let count = request["count"].as_u64().unwrap_or(1);
    let adapter = connection["adapter"].as_str().unwrap_or("");
    if count > 1
        && [
            "gemini",
            "gemini-interactions",
            "openai-responses",
            "chat-images",
        ]
        .contains(&adapter)
    {
        return Err(NumeraError::new(
            "unsupported_parameter",
            "This native adapter has no documented batch field; count must be 1.",
        ));
    }
    if count > cap["max_count"].as_u64().unwrap_or(1) {
        return Err(NumeraError::new(
            "unsupported_parameter",
            "Native batch count is not verified. Configure documented maxCount; multiple submissions are never automatic.",
        ));
    }
    if refs as u64 > cap["max_references"].as_u64().unwrap_or(1) {
        return Err(NumeraError::new(
            "unsupported_operation",
            "Too many references for selected route.",
        ));
    }
    if refs > 0 && cap["gateway_forwarding"]["image_to_image"] != "supported" {
        return Err(NumeraError::new(
            "unsupported_operation",
            "Reference input forwarding is not verified for this route.",
        ));
    }
    let mask = !request["mask"].is_null() || !request["edit_region"].is_null();
    if mask && cap["gateway_forwarding"]["masks"] != "supported" {
        return Err(NumeraError::new(
            "unsupported_operation",
            "Mask forwarding is unsupported for this route.",
        ));
    }
    if mask
        && connection["edit"]["maskPolarity"]
            .as_str()
            .is_none_or(|v| v.is_empty())
        && adapter != "comfyui"
    {
        return Err(NumeraError::new(
            "unsupported_operation",
            "Configure documented mask polarity before submitting.",
        ));
    }
    if connection["gateway"] == "omniroute" && codex(model) && count > 1 {
        return Err(NumeraError::new(
            "unsupported_parameter",
            "OmniRoute Codex count fans out paid requests; count must be 1.",
        ));
    }
    Ok(merged)
}
