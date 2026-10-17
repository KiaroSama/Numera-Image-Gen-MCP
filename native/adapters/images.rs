use super::types::{Input, Prepared, mime_data};
use crate::compat::value::json;
use crate::{
    capabilities::{COMMON_FIELDS, codex, edit_contract},
    error::{NumeraError, Result},
    http::api::Body,
};
use reqwest::multipart::{Form, Part};

pub fn request(input: &Input<'_>) -> Result<Prepared> {
    let Input {
        connection: c,
        request: r,
        model,
        operation,
        references,
        mask,
    } = input;
    let mut fields = json!({"model":model, "prompt":r["prompt"], "n":r["count"]});
    if let (Some(object), Some(options)) =
        (fields.as_object_mut(), r["provider_options"].as_object())
    {
        object.extend(options.clone());
    }
    let allowed = c["modelOverrides"][*model]["supportedParameters"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| {
            let keys: &[&str] = if c["gateway"] == "omniroute" && model.starts_with("antigravity/")
            {
                &["aspect_ratio", "image_size", "size"]
            } else if c["gateway"] == "9router" && model.starts_with("antigravity/") {
                &["size"]
            } else if c["gateway"] == "omniroute" {
                &["size", "quality"]
            } else {
                &["size", "quality", "output_format", "background"]
            };
            keys.iter().map(|s| json!(s)).collect()
        });
    for key in COMMON_FIELDS {
        if let Some(value) = r.get(*key) {
            if !allowed.iter().any(|v| v == key) {
                return Err(NumeraError::new(
                    "unsupported_parameter",
                    &format!("Parameter {key} is unsupported by this route."),
                ));
            }
            fields[*key] = value.clone();
        }
    }
    if c["gateway"] == "omniroute"
        && model.starts_with("antigravity/")
        && r["image_size"]
            .as_js_string()
            .is_some_and(|s| !s.is_empty() && !["1K", "2K", "4K"].iter().any(|tier| s == *tier))
    {
        return Err(NumeraError::new(
            "unsupported_parameter",
            "OmniRoute Antigravity tier must be 1K, 2K or 4K; gateway clamping is not accepted.",
        ));
    }
    if references.is_empty() && *operation == "generate" {
        return Ok(Prepared::json("images/generations", "/v1", fields));
    }
    if references.is_empty() {
        return Err(NumeraError::new(
            "invalid_input",
            "Editing requires source images.",
        ));
    }
    if c["gateway"] == "9router" {
        if mask.is_some() {
            return Err(NumeraError::new(
                "unsupported_operation",
                "9router has no verified mask forwarding contract.",
            ));
        }
        if !codex(model) && c.get("edit").is_none() {
            return Err(NumeraError::new(
                "unsupported_operation",
                "9router reference contract is not verified for this model.",
            ));
        }
        if references.len() == 1 {
            fields["image"] = json!(mime_data(&references[0]));
        } else {
            fields["images"] = json!(references.iter().map(mime_data).collect::<Vec<_>>());
        }
        return Ok(Prepared::json("images/generations", "/v1", fields));
    }
    let fallback = json!({"mode":"multipart", "encoding":"image", "maxReferences":if c["gateway"] == "omniroute" && codex(model){8}else{1}, "masks":false});
    let edit = c.get("edit").unwrap_or(&fallback);
    edit_contract(Some(edit), references.len())?;
    let encoding = edit["encoding"].as_str().ok_or_else(NumeraError::runtime)?;
    if edit["mode"] == "multipart" {
        let mut form = Form::new();
        for (key, value) in fields.as_object().ok_or_else(NumeraError::runtime)? {
            // FormData converts strings to USVString only at this multipart boundary.
            let text = value
                .as_utf16()
                .map(String::from_utf16_lossy)
                .unwrap_or_else(|| value.to_string());
            form = form.text(String::from_utf16_lossy(key.as_utf16()), text);
        }
        for (i, image) in references.iter().enumerate() {
            let part = Part::bytes(image.bytes.clone())
                .file_name(format!("reference-{i}.{}", image.extension))
                .mime_str(&image.mime)
                .map_err(|_| NumeraError::runtime())?;
            form = form.part(
                if encoding == "image[]" {
                    "image[]"
                } else {
                    "image"
                },
                part,
            );
        }
        if let Some(image) = mask {
            form = form.part(
                "mask",
                Part::bytes(image.bytes.clone())
                    .file_name("mask.png")
                    .mime_str(&image.mime)
                    .map_err(|_| NumeraError::runtime())?,
            );
        }
        return Ok(Prepared {
            operation: "images/edits".into(),
            prefix: "/v1",
            body: Body::Multipart(form),
        });
    }
    let encoded: Vec<_> = references.iter().map(mime_data).collect();
    fields[encoding] = if encoding == "image" {
        json!(encoded.first())
    } else if encoding == "input_references" {
        json!(
            encoded
                .iter()
                .map(|url| json!({"type":"image_url","image_url":{"url":url}}))
                .collect::<Vec<_>>()
        )
    } else {
        json!(encoded)
    };
    if let Some(mask) = mask {
        fields["mask"] = json!(mime_data(mask));
    }
    Ok(Prepared::json(
        if edit["mode"] == "generation" {
            "images/generations"
        } else {
            "images/edits"
        },
        "/v1",
        fields,
    ))
}
