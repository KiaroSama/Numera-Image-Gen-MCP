use super::{invalid, js_trim};
use crate::compat::{
    js_schema::Schema,
    value::{Value, json},
};
use crate::error::Result;
use std::{collections::HashSet, path::Path, sync::OnceLock};
use url::Url;

static ADVANCED: OnceLock<std::result::Result<serde_json::Value, crate::error::NumeraError>> =
    OnceLock::new();
static SIMPLE: OnceLock<std::result::Result<serde_json::Value, crate::error::NumeraError>> =
    OnceLock::new();

pub(crate) fn default_and_validate(value: &mut Value, simple: bool) -> Result<()> {
    let cached = if simple { &SIMPLE } else { &ADVANCED };
    let schema = cached
        .get_or_init(|| {
            let text = if simple {
                include_str!("../../schemas/simple-config.schema.json")
            } else {
                include_str!("../../schemas/config.schema.json")
            };
            let schema: serde_json::Value = serde_json::from_str(text)
                .map_err(|_| invalid("Invalid bundled config schema."))?;
            Schema::compile(&schema)?;
            Ok(schema)
        })
        .as_ref()
        .map_err(Clone::clone)?;
    if !simple && value.is_object() && value.get("files").is_none() {
        value["files"] = json!({});
    }
    let mut authoritative = value.to_js();
    let result = Schema::compile(schema)?.default_and_validate(&mut authoritative);
    *value = Value::from_js(&authoritative);
    result.map_err(|_| {
        invalid(if simple {
            "Simple configuration requires api_endpoint, api_key and models with id/name."
        } else {
            "Configuration does not match schemaVersion 1. Check the generated JSON Schema."
        })
    })
}

pub(crate) fn cross_validate(config: &mut Value, timeout: Option<&str>) -> Result<()> {
    let connections = config["connections"]
        .as_object_mut()
        .ok_or_else(|| invalid("Invalid connections."))?;
    for connection in connections.values_mut() {
        let auth = &connection["auth"];
        if auth["apiKey"].as_utf16().is_some_and(|key| {
            super::js_trim_utf16(key).is_empty() || key.iter().any(|u| matches!(*u, 0 | 10 | 13))
        }) {
            return Err(invalid(
                "Inline credential must be nonempty and cannot contain control characters.",
            ));
        }
        let mut ids = HashSet::new();
        if connection["configuredModels"]
            .as_array()
            .is_some_and(|models| {
                models
                    .iter()
                    .any(|m| !ids.insert(m["id"].as_utf16().unwrap_or(&[]).to_vec()))
            })
        {
            return Err(invalid("Configured model IDs must be unique."));
        }
        if let Some(proxy) = connection.get("proxy") {
            let url = Url::parse(&String::from_utf16_lossy(
                proxy["url"]
                    .as_utf16()
                    .ok_or_else(|| invalid("Invalid proxy URL."))?,
            ))
            .map_err(|_| invalid("Invalid proxy URL."))?;
            if !["http", "https"].contains(&url.scheme())
                || !url.username().is_empty()
                || url.password().is_some_and(|p| !p.is_empty())
                || url.query().is_some_and(|q| !q.is_empty())
                || url.fragment().is_some_and(|f| !f.is_empty())
            {
                return Err(invalid(
                    "Proxy URL must not contain credentials, query or fragments.",
                ));
            }
        }
        validate_base(connection)?;
        if let Some(origin) = auth["origin"].as_utf16().filter(|v| !v.is_empty())
            && url_origin(&String::from_utf16_lossy(origin))?
                != url_origin(&String::from_utf16_lossy(
                    connection["baseUrl"]
                        .as_utf16()
                        .ok_or_else(|| invalid("Invalid API base URL."))?,
                ))?
        {
            return Err(invalid("Credential destination origin mismatch."));
        }
        if auth["type"] != "none"
            && ["apiKey", "secretEnv", "secretFile", "secretDpapiFile"]
                .iter()
                .filter(|field| auth[**field].as_utf16().is_some_and(|v| !v.is_empty()))
                .count()
                != 1
        {
            return Err(invalid(
                "Select exactly one secret source per authenticated connection.",
            ));
        }
        if auth["type"] == "header" {
            let header = auth["header"].as_str().unwrap_or("");
            if header.is_empty()
                || !header
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                || ["host", "content-length", "cookie"]
                    .contains(&header.to_ascii_lowercase().as_str())
            {
                return Err(invalid("Invalid authentication header."));
            }
        }
        if let Some(edit) = connection.get("edit") {
            let multipart = edit["mode"] == "multipart";
            let encoding = edit["encoding"].as_str().unwrap_or("");
            if (multipart && !["image", "image[]"].contains(&encoding))
                || (!multipart && encoding == "image[]")
                || (!multipart
                    && encoding == "image"
                    && edit["maxReferences"].as_u64().unwrap_or(0) > 1)
            {
                return Err(invalid(
                    "Edit mode, encoding and reference capacity are incompatible.",
                ));
            }
        }
        if let Some(timeout) = timeout {
            let n = js_number(timeout).ok_or_else(|| invalid("Invalid request timeout."))?;
            if n.fract() != 0.0 || !(1.0..=3600000.0).contains(&n) {
                return Err(invalid("Invalid request timeout."));
            }
            connection["requestTimeoutMs"] = json!(n as u64);
        }
    }
    let mut paths = vec![
        &config["outputDir"],
        &config["stateDir"],
        &config["logging"]["directory"],
    ];
    if let Some(roots) = config["files"]["allowedInputRoots"].as_array() {
        paths.extend(roots);
    }
    if paths
        .iter()
        .any(|v| v.as_str().is_none_or(|path| !absolute(Path::new(path))))
    {
        return Err(invalid("Storage and input roots must be absolute paths."));
    }
    if config["defaultConnection"].is_string() && config["defaultConnection"].as_str().is_none() {
        return Err(invalid("Default connection does not exist."));
    }
    if let Some(name) = config["defaultConnection"]
        .as_str()
        .filter(|s| !s.is_empty())
        && config["connections"].get(name).is_none()
    {
        return Err(invalid("Default connection does not exist."));
    }
    Ok(())
}

fn validate_base(connection: &Value) -> Result<()> {
    let base = String::from_utf16_lossy(
        connection["baseUrl"]
            .as_utf16()
            .ok_or_else(|| invalid("Invalid API base URL."))?,
    );
    let url = Url::parse(&base).map_err(|_| invalid("Invalid API base URL."))?;
    let lower = base.to_ascii_lowercase();
    if !["http", "https"].contains(&url.scheme())
        || !url.username().is_empty()
        || url.password().is_some_and(|p| !p.is_empty())
        || url.query().is_some_and(|q| !q.is_empty())
        || url.fragment().is_some_and(|f| !f.is_empty())
        || ["%2e", "%2f", "%5c"].iter().any(|s| lower.contains(s))
        || base.split('/').any(|part| part == "..")
    {
        return Err(invalid(
            "API base must not contain credentials, query, fragments or traversal.",
        ));
    }
    let path = url.path().strip_suffix('/').unwrap_or(url.path());
    if [
        "generations",
        "edits",
        "generateContent",
        "responses",
        "interactions",
        "images",
        "chat/completions",
    ]
    .iter()
    .any(|endpoint| path.ends_with(&format!("/{endpoint}")))
    {
        return Err(invalid("Supply an API prefix, not a full endpoint URL."));
    }
    if connection["baseUrlMode"] == "origin" && url.path() != "/" {
        return Err(invalid("Origin mode requires an origin without a path."));
    }
    if let Some(query) = connection["query"].as_object() {
        for key in query.keys() {
            let lower: Vec<u16> = key
                .as_utf16()
                .iter()
                .map(|u| if (65..=90).contains(u) { *u + 32 } else { *u })
                .collect();
            if key != "pageToken"
                && ["key", "token", "secret", "auth", "signature"]
                    .iter()
                    .any(|s| {
                        lower
                            .windows(s.len())
                            .any(|w| w.iter().copied().eq(s.encode_utf16()))
                    })
            {
                return Err(invalid("Credentials must not be query parameters."));
            }
        }
    }
    Ok(())
}

pub(crate) fn url_origin(value: &str) -> Result<String> {
    let url = Url::parse(value).map_err(|_| invalid("Invalid destination URL."))?;
    Ok(url.origin().ascii_serialization())
}

pub(crate) fn absolute(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let s = path.as_os_str().to_string_lossy();
        s.starts_with(['/', '\\'])
            || (s.as_bytes().get(1) == Some(&b':')
                && s.as_bytes().get(2).is_some_and(|b| b"/\\".contains(b)))
    }
    #[cfg(not(windows))]
    {
        path.is_absolute()
    }
}

pub(crate) fn js_number(text: &str) -> Option<f64> {
    let text = js_trim(text);
    if text.is_empty() {
        return Some(0.0);
    }
    let radix = if text.starts_with("0x") || text.starts_with("0X") {
        Some(16)
    } else if text.starts_with("0o") || text.starts_with("0O") {
        Some(8)
    } else if text.starts_with("0b") || text.starts_with("0B") {
        Some(2)
    } else {
        None
    };
    if let Some(radix) = radix {
        let digits = &text[2..];
        if digits.is_empty() {
            return None;
        }
        let mut value = 0.0;
        for c in digits.chars() {
            value = value * f64::from(radix) + f64::from(c.to_digit(radix)?);
        }
        return value.is_finite().then_some(value);
    }
    if !text
        .bytes()
        .all(|b| b.is_ascii_digit() || b"+-eE.".contains(&b))
    {
        return None;
    }
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}
