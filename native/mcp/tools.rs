use crate::error::{NumeraError, Result};
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Value, json};
use std::sync::Arc;

pub const INSTRUCTIONS: &str = "Discover connection/model capabilities and documented maximum dimensions before images. Never treat route safety pixel limits as model support or promise native 4K/8K without evidence. Verify original returned dimensions. Reuse one request_id per logical operation. Never resubmit unknown outcomes. References require verified forwarding; files are reliable outputs and local paths may not be accessible to remote hosts. Do not replace the host chat model.";

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn string() -> Value {
    json!({"type":"string"})
}
fn pagination() -> Value {
    json!({"limit":{"type":"integer","minimum":1,"maximum":100,"default":25},"offset":{"type":"integer","minimum":0,"maximum":100000,"default":0}})
}

pub fn definitions() -> Result<Vec<Tool>> {
    let image: Value = serde_json::from_str(include_str!(
        "../../schemas/image-request.schema.json"
    ))
    .map_err(|_| {
        NumeraError::new(
            "invalid_configuration",
            "Invalid bundled image request schema.",
        )
    })?;
    let mut models = pagination();
    models["connection"] = string();
    models["refresh"] = json!({"type":"boolean","default":false});
    let entries = vec![
        (
            "health_check",
            "Check local runtime/configuration/storage; optional read-only connection probe. Never verifies generation without an actual generation.",
            object(json!({"probe":{"type":"boolean","default":false}}), &[]),
        ),
        (
            "list_connections",
            "List redacted configured connections and credential readiness; no provider generation.",
            object(json!({}), &[]),
        ),
        (
            "list_models",
            "List configured model IDs/names without network when present; refresh additionally discovers bounded provider catalogs with evidence. A failed catalog does not prove explicit models unavailable.",
            object(models, &[]),
        ),
        (
            "get_model_capabilities",
            "List ratio presets, documented model resolution tiers/pixel sizes and custom-dimension status; separate selected-route forwarding, account availability and verification evidence for one model.",
            object(
                json!({"connection":string(),"model":{"type":"string","minLength":1}}),
                &["model"],
            ),
        ),
        (
            "generate_image",
            "Submit one authorized image generation, saving validated original files. Reuse request_id for duplicates; never retry an unknown outcome.",
            image.clone(),
        ),
        (
            "edit_image",
            "Image-to-image, comic editing, in-image translation and authorized watermark removal. Provide reference_images and an explicit prompt; edit_region={x,y,width,height} selects a pixel rectangle on the first reference and requires native mask support. target_language requests in-image translation. upscale=true requests a larger native size through the same model, with exactly one reference and numeric size; no local resizing. Unsupported references/masks fail before submission; no paid fallback.",
            image,
        ),
        (
            "get_job",
            "Inspect an owned persistent receipt, optionally refresh an existing upstream job; never resubmit.",
            object(
                json!({"request_id":string(),"refresh":{"type":"boolean","default":false}}),
                &["request_id"],
            ),
        ),
        (
            "cancel_job",
            "Stop local waiting for an owned request; request targeted upstream cancellation only when supported. Does not prove a refund.",
            object(json!({"request_id":string()}), &["request_id"]),
        ),
        (
            "list_outputs",
            "List only Numera-owned output records with bounded pagination.",
            object(pagination(), &[]),
        ),
        (
            "get_output_info",
            "Verify owned image bytes/hash/metadata and optionally return a bounded derivative preview.",
            object(
                json!({"output_id":{"type":"string","format":"uuid"},"preview":{"type":"boolean","default":false}}),
                &["output_id"],
            ),
        ),
    ];
    entries
        .into_iter()
        .map(|(name, description, schema)| {
            let schema = schema
                .as_object()
                .cloned()
                .ok_or_else(NumeraError::runtime)?;
            let read = !matches!(name, "generate_image" | "edit_image" | "cancel_job");
            let annotations = ToolAnnotations::new()
                .read_only(read)
                .destructive(false)
                .idempotent(read || name == "cancel_job")
                .open_world(!matches!(name, "list_outputs" | "get_output_info"));
            Ok(Tool::new(name, description, Arc::new(schema))
                .with_annotations(annotations)
                .with_raw_output_schema(Arc::new(
                    json!({"type":"object","additionalProperties":{}})
                        .as_object()
                        .cloned()
                        .ok_or_else(NumeraError::runtime)?,
                )))
        })
        .collect()
}
