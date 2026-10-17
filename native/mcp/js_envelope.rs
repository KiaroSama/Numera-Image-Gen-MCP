use crate::{
    compat::js_json::{self, JsValue},
    error::{NumeraError, Result},
};
use std::sync::{Arc, Mutex};

const STRING_ID: &str = "numera-utf16:";

#[derive(Clone, Debug)]
pub struct OriginalArguments {
    pub arguments: Arc<JsValue>,
    pub response: Arc<Mutex<Option<crate::compat::value::Value>>>,
    pub expected_id: Option<serde_json::Value>,
}
impl OriginalArguments {
    pub fn new(arguments: JsValue) -> Self {
        Self {
            arguments: Arc::new(arguments),
            response: Arc::new(Mutex::new(None)),
            expected_id: None,
        }
    }
}
fn invalid() -> NumeraError {
    NumeraError::new("invalid_request", "Invalid JSON-RPC request identifier.")
}

/// All client string IDs are encoded, so a client prefix cannot collide with a surrogate ID.
/// Numeric IDs keep the released source SDK's safe-integer domain.
pub fn encode_id(value: &JsValue) -> Result<serde_json::Value> {
    match value {
        JsValue::String(units) => {
            let mut id = String::with_capacity(STRING_ID.len() + units.len() * 4);
            id.push_str(STRING_ID);
            use std::fmt::Write;
            for unit in units {
                write!(&mut id, "{unit:04x}").map_err(|_| invalid())?;
            }
            Ok(serde_json::Value::String(id))
        }
        JsValue::Number(number)
            if number.fract() == 0.0
                && (-9007199254740991.0..=9007199254740991.0).contains(number) =>
        {
            Ok(serde_json::json!(*number as i64))
        }
        _ => Err(invalid()),
    }
}
pub fn decode_id(value: &serde_json::Value) -> Result<JsValue> {
    if let Some(number) = value.as_f64()
        && number.fract() == 0.0
        && (-9007199254740991.0..=9007199254740991.0).contains(&number)
    {
        return Ok(JsValue::Number(number));
    }
    let encoded = value
        .as_str()
        .and_then(|s| s.strip_prefix(STRING_ID))
        .ok_or_else(invalid)?;
    if !encoded.len().is_multiple_of(4) || !encoded.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let units = encoded
        .as_bytes()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| {
            std::str::from_utf8(chunk)
                .ok()
                .and_then(|s| u16::from_str_radix(s, 16).ok())
                .ok_or_else(invalid)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(JsValue::String(units))
}

/// Only protocol metadata enters serde. Original arguments travel in a typed in-process request
/// extension, consumed before application validation and hashing, never in a JSON marker.
pub struct Decoded {
    pub metadata: serde_json::Value,
    pub arguments: Option<JsValue>,
}
pub fn decode(text: &str, limit: usize) -> Result<Decoded> {
    let mut raw = js_json::parse_bounded(text, limit)?;
    if !raw.is_object() {
        return Err(invalid());
    }
    if let Some(id) = raw.get("id")
        && (!id.is_null() || raw.get("method").is_some())
    {
        let encoded = js_json::from_serde(&encode_id(id)?)?;
        raw.set("id", encoded)?;
    }
    if raw
        .get("method")
        .and_then(JsValue::as_utf16)
        .is_some_and(|s| {
            s.iter()
                .copied()
                .eq("notifications/cancelled".encode_utf16())
        })
        && let Some(params) = raw.get_mut("params")
        && let Some(id) = params.get("requestId")
    {
        let encoded = js_json::from_serde(&encode_id(id)?)?;
        params.set("requestId", encoded)?;
    }
    let call = raw.get("id").is_some()
        && raw
            .get("method")
            .and_then(JsValue::as_utf16)
            .is_some_and(|s| s.iter().copied().eq("tools/call".encode_utf16()));
    let arguments = if call {
        Some(
            raw.get_mut("params")
                .and_then(|p| p.remove("arguments"))
                .unwrap_or_else(|| JsValue::Object(Vec::new())),
        )
    } else {
        None
    };
    if arguments.as_ref().is_some_and(|args| !args.is_object()) {
        return Err(NumeraError::new(
            "invalid_request",
            "Tool arguments must be a JSON object.",
        ));
    }
    // JS wire spelling keeps integral SDK IDs as integers rather than serde's distinct 1.0.
    js_json::drop_serde(js_json::to_serde(&raw)?);
    let metadata = serde_json::from_str(&js_json::stringify(&raw)?).map_err(|_| invalid())?;
    Ok(Decoded {
        metadata,
        arguments,
    })
}
pub fn restore_result(message: &mut JsValue, original: &OriginalArguments) -> Result<()> {
    let Some(id) = message.get("id") else {
        return Ok(());
    };
    if let Some(expected) = &original.expected_id
        && !id.structurally_equal(&js_json::from_serde(expected)?)
    {
        return Ok(());
    }
    if let Some(result) = message.get_mut("result") {
        let value = original
            .response
            .lock()
            .map_err(|_| NumeraError::runtime())?
            .take();
        if let Some(value) = value {
            result.set("structuredContent", value.to_js())?;
        }
    }
    Ok(())
}
pub fn restore_id(message: &mut JsValue) -> Result<()> {
    if let Some(id) = message.get("id")
        && !id.is_null()
    {
        let id = decode_id(&js_json::to_serde(id)?)?;
        message.set("id", id)?;
    }
    Ok(())
}
