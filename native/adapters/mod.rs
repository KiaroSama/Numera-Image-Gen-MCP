pub mod comfy;
mod gemini;
mod images;
pub mod normalize;
mod openai;
mod openrouter;
pub mod types;
pub mod workflow;

use crate::error::{NumeraError, Result};
pub use gemini::encode_component;
use types::{Input, Prepared};

pub fn build_request(input: &Input<'_>) -> Result<Prepared> {
    match input.connection["adapter"].as_str().unwrap_or("") {
        "openai-images" => images::request(input),
        "gemini" | "gemini-interactions" => gemini::request(input),
        "openai-responses" | "chat-images" => openai::request(input),
        "openrouter-images" => openrouter::request(input),
        "comfyui" => Err(NumeraError::new(
            "invalid_configuration",
            "ComfyUI requires workflow execution, not a prompt-only request.",
        )),
        _ => Err(NumeraError::new(
            "invalid_configuration",
            "Unknown configured adapter.",
        )),
    }
}
