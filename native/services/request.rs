use crate::compat::value::{Value, json};
use crate::error::{NumeraError, Result};

/// The same public request contract as schemas/image-request.schema.json, with UTF-16 lengths.
pub fn parse(mut value: Value) -> Result<Value> {
    static SCHEMA: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let schema = SCHEMA.get_or_init(|| {
        serde_json::from_str(include_str!("../../schemas/image-request.schema.json"))
            .expect("bundled request schema")
    });
    let mut authoritative = value.to_js();
    crate::compat::js_schema::Schema::compile(schema)?
        .default_and_validate(&mut authoritative)
        .map_err(|_| {
            NumeraError::new(
                "invalid_input",
                "Image request does not match the public request schema.",
            )
        })?;
    value = Value::from_js(&authoritative);
    let invalid = || {
        NumeraError::new(
            "invalid_input",
            "Image request does not match the public request schema.",
        )
    };
    let object = value.as_object_mut().ok_or_else(invalid)?;
    const ALLOWED: &[&str] = &[
        "prompt",
        "connection",
        "model",
        "request_id",
        "count",
        "size",
        "aspect_ratio",
        "image_size",
        "quality",
        "output_format",
        "background",
        "seed",
        "negative_prompt",
        "provider_options",
        "reference_images",
        "mask",
        "upscale",
        "edit_region",
        "target_language",
        "output_subdirectory",
        "filename_prefix",
        "return_mode",
        "wait",
    ];
    if object
        .keys()
        .any(|key| key.as_str().is_none_or(|key| !ALLOWED.contains(&key)))
    {
        return Err(invalid());
    }
    for (key, default) in [
        ("count", json!(1)),
        ("provider_options", json!({})),
        ("reference_images", json!([])),
        ("wait", json!(true)),
    ] {
        object.entry(key).or_insert(default);
    }
    let prompt = value["prompt"].as_utf16().ok_or_else(invalid)?;
    if prompt.len() > 100000 || crate::config::js_trim_utf16(prompt).is_empty() {
        return Err(invalid());
    }
    for key in [
        "connection",
        "model",
        "request_id",
        "size",
        "aspect_ratio",
        "image_size",
        "quality",
        "background",
        "negative_prompt",
        "target_language",
        "output_subdirectory",
        "filename_prefix",
        "output_format",
        "return_mode",
    ] {
        if value.get(key).is_some_and(|v| !v.is_string()) {
            return Err(invalid());
        }
    }
    if value["model"]
        .as_utf16()
        .is_some_and(|s| s.is_empty() || s.len() > 300)
    {
        return Err(invalid());
    }
    if value["request_id"].as_str().is_some_and(|s| {
        s.is_empty()
            || s.len() > 100
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    }) {
        return Err(invalid());
    }
    if value["output_format"]
        .as_str()
        .is_some_and(|s| !["png", "jpeg", "webp"].contains(&s))
        || value["return_mode"]
            .as_str()
            .is_some_and(|s| !["files", "files_and_preview"].contains(&s))
    {
        return Err(invalid());
    }
    for key in ["wait", "upscale"] {
        if value.get(key).is_some_and(|v| !v.is_boolean()) {
            return Err(invalid());
        }
    }
    let count = value["count"].as_f64().ok_or_else(invalid)?;
    if count.fract() != 0.0 || !(1.0..=10.0).contains(&count) {
        return Err(invalid());
    }
    value["count"] = json!(count as u64);
    if value.get("seed").is_some_and(|v| {
        v.as_f64().is_none_or(|n| {
            n.fract() != 0.0 || !(-9007199254740991.0..=9007199254740991.0).contains(&n)
        })
    }) {
        return Err(invalid());
    }
    if !value["provider_options"].is_object() {
        return Err(invalid());
    }
    let refs = value["reference_images"].as_array().ok_or_else(invalid)?;
    if refs.len() > 32 {
        return Err(invalid());
    }
    for source in refs.iter().chain(value.get("mask")) {
        validate_source(source)?;
    }
    if let Some(region) = value.get("edit_region") {
        let object = region.as_object().ok_or_else(invalid)?;
        if object.len() != 4
            || object.keys().any(|k| {
                k.as_str()
                    .is_none_or(|k| !["x", "y", "width", "height"].contains(&k))
            })
        {
            return Err(invalid());
        }
        for key in ["x", "y", "width", "height"] {
            let n = region[key].as_f64().ok_or_else(invalid)?;
            let min = if ["width", "height"].contains(&key) {
                1.0
            } else {
                0.0
            };
            if n.fract() != 0.0 || !(min..=64000000.0).contains(&n) {
                return Err(invalid());
            }
        }
    }
    if let Some(region) = value.get_mut("edit_region") {
        for key in ["x", "y", "width", "height"] {
            if let Some(number) = region[key].as_f64() {
                region[key] = json!(number as u64);
            }
        }
    }
    if value["target_language"]
        .as_utf16()
        .is_some_and(|s| crate::config::js_trim_utf16(s).is_empty() || s.len() > 100)
    {
        return Err(invalid());
    }
    Ok(value)
}

pub fn validate_source(value: &Value) -> Result<()> {
    let invalid = || NumeraError::new("invalid_input", "Invalid typed reference image source.");
    let source = value.as_object().ok_or_else(invalid)?;
    if source.len() != 2 {
        return Err(invalid());
    }
    let kind = value["type"].as_str().ok_or_else(invalid)?;
    let field = match kind {
        "output_id" => "output_id",
        "path" => "path",
        "url" => "url",
        "data_url" => "data_url",
        _ => return Err(invalid()),
    };
    let text = value[field].as_utf16().ok_or_else(invalid)?;
    match kind {
        "output_id" => {
            if text.len() != 36
                || String::from_utf16(text).ok().is_none_or(|s| {
                    let bytes = s.as_bytes();
                    uuid::Uuid::parse_str(&s).is_err()
                        || !(s == "00000000-0000-0000-0000-000000000000"
                            || s == "ffffffff-ffff-ffff-ffff-ffffffffffff"
                            || (matches!(bytes[14], b'1'..=b'8')
                                && matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B')))
                })
            {
                return Err(invalid());
            }
        }
        "path" => {
            if text.is_empty() {
                return Err(invalid());
            }
        }
        "url" => {
            if url::Url::parse(&String::from_utf16_lossy(text)).is_err() {
                return Err(invalid());
            }
        }
        "data_url" => {
            if text.len() > 30000000 {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    Ok(())
}
