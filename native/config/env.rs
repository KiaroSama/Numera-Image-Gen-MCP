use super::{invalid, js_trim};
use crate::compat::value::{Object, Value, json};
use crate::error::Result;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
};

pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;

pub fn read_config_text(file: &Path) -> Result<String> {
    let read = || -> std::io::Result<String> {
        let file = File::open(file)?;
        if !file.metadata()?.is_file() {
            return Err(std::io::Error::other("Not a file"));
        }
        let mut bytes = Vec::new();
        file.take((MAX_CONFIG_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(std::io::Error::other("Too large"));
        }
        // TextDecoder's default strips one leading UTF-8 BOM.
        let text = String::from_utf8(bytes).map_err(|_| std::io::Error::other("Invalid UTF-8"))?;
        Ok(text.strip_prefix('\u{FEFF}').unwrap_or(&text).to_owned())
    };
    read().map_err(|_| invalid("Cannot read configuration as a UTF-8 file up to 1 MiB."))
}

fn env_error() -> crate::error::NumeraError {
    invalid("Invalid env configuration. Check keys, values and unique assignments.")
}

fn strict_assignments(text: &str) -> Result<HashSet<String>> {
    let mut rest = text;
    let mut keys = HashSet::new();
    while !rest.is_empty() {
        let trimmed = rest.trim_start_matches(|c: char| js_trim(&c.to_string()).is_empty());
        if trimmed.len() != rest.len() {
            rest = trimmed;
            continue;
        }
        if rest.starts_with('#') {
            let end = rest.find(['\r', '\n']).unwrap_or(rest.len());
            rest = &rest[end..];
            if rest.is_empty() {
                continue;
            }
            rest = rest
                .strip_prefix("\r\n")
                .or_else(|| rest.strip_prefix('\n'))
                .ok_or_else(env_error)?;
            continue;
        }
        if let Some(export) = rest
            .strip_prefix("export")
            .filter(|s| s.starts_with([' ', '\t']))
        {
            rest = export.trim_start_matches([' ', '\t']);
        }
        let length = rest
            .bytes()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
            .count();
        let key = &rest[..length];
        if key.is_empty() || key.as_bytes()[0].is_ascii_digit() || !keys.insert(key.to_owned()) {
            return Err(env_error());
        }
        rest = rest[length..].trim_start_matches([' ', '\t']);
        rest = rest
            .strip_prefix('=')
            .ok_or_else(env_error)?
            .trim_start_matches([' ', '\t']);
        if let Some(quote @ (b'"' | b'\'' | b'`')) = rest.as_bytes().first().copied() {
            let end = rest[1..].find(quote as char).ok_or_else(env_error)? + 1;
            rest = rest[end + 1..].trim_start_matches([' ', '\t']);
            if rest.starts_with('#') {
                let end = rest.find(['\r', '\n']).unwrap_or(rest.len());
                rest = &rest[end..];
            }
            if rest.is_empty() {
                continue;
            }
            rest = rest
                .strip_prefix("\r\n")
                .or_else(|| rest.strip_prefix('\n'))
                .ok_or_else(env_error)?;
        } else {
            rest = rest.find('\n').map_or("", |end| &rest[end + 1..]);
        }
    }
    Ok(keys)
}

// Match util.parseEnv after the source's strict scanner; the native parser strips CR globally
// and expands literal \\n only in double quotes. It does not perform shell interpolation.
fn node_values(text: &str) -> HashMap<String, String> {
    let normalized = text.replace('\r', "");
    let trim = |s: &str| s.trim_matches([' ', '\t', '\n']).to_owned();
    let mut content = trim(&normalized);
    let mut values = HashMap::new();
    while !content.is_empty() {
        if content.starts_with(['\n', '#']) {
            content = content
                .find('\n')
                .map_or(String::new(), |end| content[end + 1..].to_owned());
            continue;
        }
        let Some(end) = content.find(['=', '\n']) else {
            break;
        };
        if content.as_bytes()[end] == b'\n' {
            content = trim(&content[end + 1..]);
            continue;
        }
        let mut key = trim(&content[..end]);
        content = content[end + 1..].to_owned();
        if content.is_empty() || content.starts_with('\n') {
            values.insert(key, String::new());
            continue;
        }
        content = trim(&content);
        if key.is_empty() {
            continue;
        }
        if let Some(export) = key.strip_prefix("export ") {
            key = trim(export);
        }
        if content.is_empty() {
            values.insert(key, String::new());
            break;
        }
        let quote = content.as_bytes()[0];
        if b"\"'`".contains(&quote) {
            let Some(end) = content[1..].find(quote as char).map(|n| n + 1) else {
                break;
            };
            let value = &content[1..end];
            values.insert(
                key,
                if quote == b'"' {
                    value.replace("\\n", "\n")
                } else {
                    value.into()
                },
            );
            content = content[end + 1..]
                .find('\n')
                .map_or(String::new(), |n| content[end + 2 + n..].to_owned());
            continue;
        }
        let end = content.find('\n').unwrap_or(content.len());
        let line = &content[..end];
        let value = line.split('#').next().unwrap_or("");
        values.insert(key, trim(value));
        content = if end == content.len() {
            String::new()
        } else {
            trim(&content[end + 1..])
        };
    }
    values
}

pub fn parse_env_config(text: &str) -> Result<Value> {
    if text.len() > MAX_CONFIG_BYTES || text.contains('\0') {
        return Err(env_error());
    }
    let keys = strict_assignments(text)?;
    let values = node_values(text);
    if keys.len() != values.len() {
        return Err(env_error());
    }
    let top = [
        "API_ENDPOINT",
        "API_KEY",
        "PROFILE",
        "ORCHESTRATION_MODEL",
        "EDIT_MODE",
        "EDIT_ENCODING",
        "EDIT_MASK_POLARITY",
        "EDIT_MAX_REFERENCES",
        "EDIT_MASKS",
    ];
    let mut slots: BTreeMap<u16, Object> = BTreeMap::new();
    for (key, value) in &values {
        if top.contains(&key.as_str()) {
            continue;
        }
        let (slot, field) = key
            .strip_prefix("MODEL_")
            .and_then(|s| s.split_once('_'))
            .ok_or_else(env_error)?;
        if slot.is_empty()
            || slot.len() > 3
            || slot.starts_with('0')
            || !slot.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(env_error());
        }
        let number: u16 = slot.parse().map_err(|_| env_error())?;
        if number == 0
            || number > 100
            || !["ID", "NAME", "ENABLED", "OUTPUT_FORMAT", "SIZE", "QUALITY"].contains(&field)
        {
            return Err(env_error());
        }
        let model = slots.entry(number).or_default();
        if !value.is_empty() {
            model.insert(
                field.to_ascii_lowercase().into(),
                if field == "ENABLED" {
                    json!(boolean(value)?)
                } else {
                    json!(value)
                },
            );
        }
    }
    let mut raw = Object::new();
    for (key, field) in [("API_ENDPOINT", "api_endpoint"), ("API_KEY", "api_key")] {
        if let Some(value) = values.get(key) {
            raw.insert(field.into(), json!(value));
        }
    }
    raw.insert(
        "models".into(),
        Value::Array(
            slots
                .into_values()
                .filter(|model| !model.is_empty())
                .map(Value::Object)
                .collect(),
        ),
    );
    for (key, field) in [
        ("PROFILE", "profile"),
        ("ORCHESTRATION_MODEL", "orchestration_model"),
    ] {
        if let Some(value) = values.get(key).filter(|v| !v.is_empty()) {
            raw.insert(field.into(), json!(value));
        }
    }
    let mut edit = Object::new();
    for (key, field) in [
        ("EDIT_MODE", "mode"),
        ("EDIT_ENCODING", "encoding"),
        ("EDIT_MASK_POLARITY", "maskPolarity"),
    ] {
        if let Some(value) = values.get(key).filter(|v| !v.is_empty()) {
            edit.insert(field.into(), json!(value));
        }
    }
    if let Some(value) = values.get("EDIT_MASKS").filter(|v| !v.is_empty()) {
        edit.insert("masks".into(), json!(boolean(value)?));
    }
    if let Some(value) = values.get("EDIT_MAX_REFERENCES").filter(|v| !v.is_empty()) {
        let n: u8 = value.parse().map_err(|_| env_error())?;
        if n == 0 || n > 32 || n.to_string() != *value {
            return Err(env_error());
        }
        edit.insert("maxReferences".into(), json!(n));
    }
    if values.get("EDIT_MODE").is_some_and(|v| !v.is_empty()) {
        raw.insert("edit".into(), Value::Object(edit));
    } else if !edit.is_empty() {
        return Err(env_error());
    }
    Ok(Value::Object(raw))
}

fn boolean(value: &str) -> Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(env_error()),
    }
}
