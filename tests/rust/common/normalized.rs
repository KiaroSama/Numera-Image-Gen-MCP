use numera_image_gen_mcp::adapters::types::Normalized;
use numera_image_gen_mcp::compat::value::{Value, json};

pub fn normalized(value: Normalized) -> Value {
    let mut result = json!({"images":value.images.into_iter().map(|image| {
        let mut item = json!({});
        if let Some(value) = image.base64 { item["base64"] = json!(value); }
        if let Some(value) = image.url { item["url"] = json!(value); }
        if let Some(value) = image.error { item["error"] = json!(value); }
        if let Some(value) = image.bytes { item["bytes"] = json!({"type":"Buffer","data":value}); }
        item
    }).collect::<Vec<_>>(), "upstreamModel":value.upstream_model,"warnings":value.warnings});
    if let Some(value) = value.terminal {
        result["terminal"] = json!(value);
    }
    if let Some(value) = value.upstream_id {
        result["upstreamId"] = json!(value);
    }
    if let Some(value) = value.job {
        result["job"] = json!({"id":value.id,"kind":value.kind});
    }
    if let Some(value) = value.usage {
        result["usage"] = value;
    }
    if let Some(value) = value.continuation {
        result["continuation"] = value;
    }
    result
}
