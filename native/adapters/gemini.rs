use super::types::{Input, Prepared};
use crate::compat::value::json;
use crate::{
    capabilities::COMMON_FIELDS,
    error::{NumeraError, Result},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

const COMPONENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

pub fn encode_component(value: &str) -> String {
    utf8_percent_encode(value, COMPONENT).to_string()
}

pub fn request(input: &Input<'_>) -> Result<Prepared> {
    let Input {
        connection: c,
        request: r,
        model,
        references,
        mask,
        ..
    } = input;
    if mask.is_some() {
        return Err(NumeraError::new(
            "unsupported_operation",
            "Native Gemini mask polarity is not implemented by this protocol.",
        ));
    }
    for key in COMMON_FIELDS {
        if r.get(*key).is_some() && !["aspect_ratio", "image_size", "output_format"].contains(key) {
            return Err(NumeraError::new(
                "unsupported_parameter",
                &format!("Native Gemini does not accept common {key}."),
            ));
        }
    }
    let present = |key: &str| r[key].as_utf16().is_some_and(|s| !s.is_empty());
    if c["adapter"] == "gemini" {
        if present("output_format") {
            return Err(NumeraError::new(
                "unsupported_parameter",
                "generateContent does not guarantee requested output format.",
            ));
        }
        let mut parts = vec![json!({"text":r["prompt"]})];
        parts.extend(references.iter().map(|image| json!({"inlineData":{"mimeType":image.mime,"data":STANDARD.encode(&image.bytes)}})));
        let mut generation = json!({"responseModalities":["TEXT","IMAGE"]});
        if present("aspect_ratio") || present("image_size") {
            let mut image_config = json!({});
            if present("aspect_ratio") {
                image_config["aspectRatio"] = r["aspect_ratio"].clone();
            }
            if present("image_size") {
                image_config["imageSize"] = r["image_size"].clone();
            }
            generation["imageConfig"] = image_config;
        }
        if let (Some(object), Some(options)) = (
            generation.as_object_mut(),
            r["provider_options"].as_object(),
        ) {
            object.extend(options.clone());
        }
        let model_text = model.checked_str()?;
        let engine = model_text.strip_prefix("models/").unwrap_or(model_text);
        return Ok(Prepared::json(
            &format!(
                "models/{}:generateContent",
                utf8_percent_encode(engine, COMPONENT)
            ),
            "/v1beta",
            json!({"contents":[{"role":"user","parts":parts}], "generationConfig":generation}),
        ));
    }
    let mut inputs = vec![json!({"type":"text","text":r["prompt"]})];
    inputs.extend(references.iter().map(
        |image| json!({"type":"image","mime_type":image.mime,"data":STANDARD.encode(&image.bytes)}),
    ));
    let mut format = json!({"type":"image"});
    for key in ["aspect_ratio", "image_size"] {
        if present(key) {
            format[key] = r[key].clone();
        }
    }
    if let Some(value) = r["output_format"].as_str().filter(|s| !s.is_empty()) {
        format["mime_type"] = json!(format!("image/{value}"));
    }
    let mut body = json!({"model":model,"store":false,"input":inputs,"response_format":format});
    if let (Some(object), Some(options)) = (body.as_object_mut(), r["provider_options"].as_object())
    {
        object.extend(options.clone());
    }
    Ok(Prepared::json("interactions", "/v1beta", body))
}
