use crate::compat::value::{Value, json};
use crate::error::{NumeraError, Result};

#[derive(Clone, Debug)]
pub struct Event {
    pub kind: String,
    pub data: Value,
}

fn error(code: &str, message: &str) -> NumeraError {
    NumeraError::new(code, message).stage("response")
}

/// Retain final frames only; total incoming bytes remain bounded including discarded progress.
pub struct Reader {
    pending: Vec<u8>,
    total: usize,
    limit: usize,
    finals: Vec<Event>,
}
impl Reader {
    pub fn new(limit: usize) -> Self {
        Self {
            pending: Vec::new(),
            total: 0,
            limit,
            finals: Vec::new(),
        }
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<()> {
        self.total = self
            .total
            .checked_add(bytes.len())
            .ok_or_else(|| error("invalid_response", "Stream exceeds aggregate byte limit."))?;
        if self.total > self.limit {
            return Err(error(
                "invalid_response",
                "Stream exceeds aggregate byte limit.",
            ));
        }
        self.pending.extend_from_slice(bytes);
        let mut consumed = 0;
        while let Some((start, length)) = boundary(&self.pending[consumed..]) {
            let end = consumed + start;
            let text = std::str::from_utf8(&self.pending[consumed..end])
                .map_err(|_| error("invalid_response", "Invalid stream UTF-8."))?;
            if let Some(event) = frame(text, true)? {
                self.finals.push(event);
                if self.finals.len() > 1000 {
                    return Err(error("invalid_response", "Stream event count exceeded."));
                }
            }
            consumed = end + length;
        }
        if consumed != 0 {
            self.pending.drain(..consumed);
        }
        // A partial UTF-8 character is allowed only at the end of the current chunk.
        if std::str::from_utf8(&self.pending).is_err_and(|e| e.error_len().is_some()) {
            return Err(error("invalid_response", "Invalid stream UTF-8."));
        }
        Ok(())
    }
    pub fn finish(self) -> Result<Vec<u8>> {
        let text = std::str::from_utf8(&self.pending)
            .map_err(|_| error("invalid_response", "Invalid stream UTF-8."))?;
        if !crate::config::js_trim(text).is_empty() {
            return Err(error(
                "outcome_unknown",
                "Stream ended without a complete terminal frame.",
            ));
        }
        let mut bytes = Vec::new();
        for event in self.finals {
            if event.kind == "done" && event.data == json!({}) {
                bytes.extend_from_slice(b"data: [DONE]\n\n");
            } else {
                if !event.kind.is_empty() {
                    bytes.extend_from_slice(format!("event: {}\n", event.kind).as_bytes());
                }
                bytes.extend_from_slice(format!("data: {}\n\n", event.data).as_bytes());
            }
        }
        Ok(bytes)
    }
}

fn boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    for (i, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        if bytes.get(i + 1) == Some(&b'\n') {
            return Some((
                if i > 0 && bytes[i - 1] == b'\r' {
                    i - 1
                } else {
                    i
                },
                if i > 0 && bytes[i - 1] == b'\r' { 3 } else { 2 },
            ));
        }
        if bytes.get(i + 1..i + 3) == Some(b"\r\n".as_slice()) {
            return Some((
                if i > 0 && bytes[i - 1] == b'\r' {
                    i - 1
                } else {
                    i
                },
                if i > 0 && bytes[i - 1] == b'\r' { 4 } else { 3 },
            ));
        }
    }
    None
}

fn frame(text: &str, filtering: bool) -> Result<Option<Event>> {
    let lines: Vec<_> = text
        .split('\n')
        .map(|s| s.strip_suffix('\r').unwrap_or(s))
        .collect();
    let data = lines
        .iter()
        .filter_map(|s| {
            s.strip_prefix("data:").map(|s| {
                s.trim_start_matches(|c: char| crate::config::js_trim(&c.to_string()).is_empty())
            })
        })
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return Ok(None);
    }
    if data == "[DONE]" {
        return Ok(Some(Event {
            kind: "done".into(),
            data: json!({}),
        }));
    }
    let value = Value::parse_with_limit(&data, 128 * 1024 * 1024)
        .map_err(|_| error("invalid_response", "Invalid streaming event JSON."))?;
    if !value.is_object() {
        return Err(error("invalid_response", "Invalid streaming event JSON."));
    }
    let kind = lines
        .iter()
        .find_map(|line| line.strip_prefix("event:").map(crate::config::js_trim))
        .map(str::to_owned)
        .unwrap_or_else(|| {
            value["type"]
                .as_str()
                .or_else(|| {
                    if filtering {
                        value["event_type"].as_str()
                    } else {
                        None
                    }
                })
                .unwrap_or("")
                .to_owned()
        });
    if filtering && kind == "error" {
        return Err(error(
            "outcome_unknown",
            "Image stream ended with an error; no resubmission made.",
        ));
    }
    if filtering
        && matches!(
            kind.as_str(),
            "progress"
                | "partial_image"
                | "image_generation.partial_image"
                | "response.image_generation_call.partial_image"
        )
    {
        return Ok(None);
    }
    Ok(Some(Event { kind, data: value }))
}

pub fn events(bytes: &[u8], limit: usize) -> Result<Vec<Event>> {
    if bytes.len() > limit {
        return Err(error(
            "invalid_response",
            "Stream exceeds aggregate byte limit.",
        ));
    }
    let text = String::from_utf8_lossy(bytes);
    if !text.ends_with("\n\n") && !text.ends_with("\r\n\r\n") {
        return Err(error(
            "outcome_unknown",
            "Stream ended without a complete terminal frame.",
        ));
    }
    let mut result = Vec::new();
    let mut start = 0;
    while let Some((offset, length)) = boundary(&text.as_bytes()[start..]) {
        if let Some(event) = frame(&text[start..start + offset], false)? {
            result.push(event);
        }
        start += offset + length;
    }
    Ok(result)
}
