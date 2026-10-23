use super::types::{Input, Prepared, mime_data};
use crate::compat::value::json;
use crate::{
    capabilities::COMMON_FIELDS,
    error::{NumeraError, Result},
};

pub fn request(input: &Input<'_>) -> Result<Prepared> {
    let Input {
        connection: c,
        request: r,
        model,
        references,
        mask,
        operation,
    } = input;
    if mask.is_some() {
        return Err(NumeraError::new(
            "unsupported_operation",
            "Use a documented multipart mask route.",
        ));
    }
    if c["adapter"] == "chat-images" {
        if c["chatImageOutput"] != true {
            return Err(NumeraError::new(
                "unsupported_operation",
                "Chat image output requires explicit opt-in.",
            ));
        }
        for key in COMMON_FIELDS {
            if r.get(*key).is_some() && !["aspect_ratio", "image_size"].contains(key) {
                return Err(NumeraError::new(
                    "unsupported_parameter",
                    &format!("Chat image adapter does not map {key}."),
                ));
            }
        }
        let mut content = vec![json!({"type":"text","text":r["prompt"]})];
        content.extend(
            references
                .iter()
                .map(|image| json!({"type":"image_url","image_url":{"url":mime_data(image)}})),
        );
        let mut config = json!({});
        for key in ["aspect_ratio", "image_size"] {
            if r[key].as_utf16().is_some_and(|s| !s.is_empty()) {
                config[key] = r[key].clone();
            }
        }
        let mut body = json!({"model":model,"messages":[{"role":"user","content":content}],"modalities":["image","text"],"image_config":config});
        if let (Some(object), Some(options)) =
            (body.as_object_mut(), r["provider_options"].as_object())
        {
            object.extend(options.clone());
        }
        return Ok(Prepared::json("chat/completions", "/v1", body));
    }
    let orchestration = c["orchestrationModel"]
        .as_js_string()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            NumeraError::new(
                "invalid_configuration",
                "Responses requires a separate orchestrationModel.",
            )
        })?;
    let mut tool = json!({"type":"image_generation","model":model,"action":if *operation=="edit"{"edit"}else{"generate"}});
    for key in COMMON_FIELDS {
        if let Some(value) = r.get(*key) {
            if !["size", "quality", "output_format", "background"].contains(key) {
                return Err(NumeraError::new(
                    "unsupported_parameter",
                    &format!("Responses image tool does not accept {key}."),
                ));
            }
            tool[*key] = value.clone();
        }
    }
    let mut content = vec![json!({"type":"input_text","text":r["prompt"]})];
    content.extend(
        references
            .iter()
            .map(|image| json!({"type":"input_image","image_url":mime_data(image)})),
    );
    let mut body = json!({"model":orchestration,"store":false,"input":[{"role":"user","content":content}],"tools":[tool],"tool_choice":{"type":"image_generation"}});
    if let (Some(object), Some(options)) = (body.as_object_mut(), r["provider_options"].as_object())
    {
        object.extend(options.clone());
    }
    Ok(Prepared::json("responses", "/v1", body))
}
