use super::types::{Input, Prepared, mime_data};
use crate::compat::value::json;
use crate::{
    capabilities::COMMON_FIELDS,
    error::{NumeraError, Result},
};

pub fn request(input: &Input<'_>) -> Result<Prepared> {
    let Input {
        request: r,
        model,
        references,
        mask,
        ..
    } = input;
    if mask.is_some() {
        return Err(NumeraError::new(
            "unsupported_operation",
            "Dedicated OpenRouter image API does not define a universal mask contract.",
        ));
    }
    let mut fields = json!({"model":model,"prompt":r["prompt"],"n":r["count"],
        "input_references":references.iter().map(|image|json!({"type":"image_url","image_url":{"url":mime_data(image)}})).collect::<Vec<_>>(),
        "provider":{"allow_fallbacks":false}});
    for key in COMMON_FIELDS {
        if let Some(value) = r.get(*key) {
            if *key == "negative_prompt" {
                return Err(NumeraError::new(
                    "unsupported_parameter",
                    "Use explicitly documented provider passthrough for negative prompt.",
                ));
            }
            fields[if *key == "image_size" {
                "resolution"
            } else {
                key
            }] = value.clone();
        }
    }
    if let (Some(object), Some(options)) =
        (fields.as_object_mut(), r["provider_options"].as_object())
    {
        object.extend(options.clone());
    }
    if let Some(provider) = fields["provider"].as_object_mut() {
        provider.insert("allow_fallbacks".into(), json!(false));
    }
    Ok(Prepared::json("images", "/api/v1", fields))
}
