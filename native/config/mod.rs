mod credentials;
#[cfg(windows)]
mod dpapi;
mod env;
mod json;
mod load;
mod reload;
mod runtime_environment;
mod validate;

use crate::compat::value::{Object, Value, json};
use crate::error::{NumeraError, Result};
pub use credentials::{PinnedConnection, credentials, snapshot_credentials};
pub use env::{MAX_CONFIG_BYTES, parse_env_config, read_config_text};
pub use json::{clone_json, drop_json};
pub use load::{config_file, load_config, user_root};
pub use reload::{ConfigReload, ReloadEvent, ReloadResult};
pub use runtime_environment::{Environment, EnvironmentSource};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub struct Config {
    pub value: Value,
    pub file: PathBuf,
    pub root: PathBuf,
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Self {
            value: clone_json(&self.value),
            file: self.file.clone(),
            root: self.root.clone(),
        }
    }
}
impl Drop for Config {
    fn drop(&mut self) {
        drop_json(std::mem::take(&mut self.value));
    }
}

impl Config {
    pub fn into_value(mut self) -> Value {
        std::mem::take(&mut self.value)
    }

    pub fn parse(
        text: &str,
        file: &Path,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self> {
        let flags = load::arguments(args)?;
        let selected = flags
            .get("config")
            .or_else(|| env.get("NUMERA_CONFIG"))
            .filter(|s| !s.is_empty());
        let file = selected.map_or_else(|| file.to_path_buf(), PathBuf::from);
        if !flags.contains_key("openai-compat") && !validate::absolute(&file) {
            return Err(invalid("NUMERA_CONFIG must be absolute."));
        }
        let raw = if flags.contains_key("openai-compat") {
            load::compat_config(env)?
        } else {
            if text.len() > MAX_CONFIG_BYTES {
                return Err(invalid("Configuration exceeds 1 MiB."));
            }
            let filename = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if filename.ends_with(".env")
                || filename
                    .split_once(".env.")
                    .is_some_and(|(_, tail)| !tail.is_empty())
            {
                parse_env_config(text)?
            } else {
                json::parse(text)?.take()
            }
        };
        let mut raw = json::OwnedJson(raw);
        if !raw.is_object() {
            return Err(invalid("Configuration must be a JSON object."));
        }
        if raw.get("api_endpoint").is_some() || raw.get("api_key").is_some() {
            let expanded = expand_simple(&mut raw, &file)?;
            drop_json(std::mem::replace(&mut raw.0, expanded));
        }
        load::overrides(&mut raw, &flags, env)?;
        validate::default_and_validate(&mut raw, false)?;
        validate::cross_validate(
            &mut raw,
            load::get(
                &flags,
                env,
                "request-timeout-ms",
                "NUMERA_REQUEST_TIMEOUT_MS",
            ),
        )?;
        let root = file.parent().unwrap_or(Path::new("")).to_path_buf();
        Ok(Self {
            value: raw.take(),
            file,
            root,
        })
    }

    pub fn select(&self, name: Option<&str>) -> Result<(String, Value)> {
        if name.is_none()
            && self.value["defaultConnection"].is_string()
            && self.value["defaultConnection"].as_str().is_none()
        {
            return Err(NumeraError::new(
                "invalid_input",
                "Select an enabled configured connection.",
            ));
        }
        let id = name.or_else(|| self.value["defaultConnection"].as_str());
        if let Some(id) = id.filter(|id| !id.is_empty())
            && let Some(connection) = self.value["connections"].get(id)
            && connection["enabled"] == true
        {
            return Ok((id.to_owned(), clone_json(connection)));
        }
        Err(NumeraError::new(
            "invalid_input",
            "Select an enabled configured connection.",
        ))
    }
}

pub(crate) fn invalid(message: &str) -> NumeraError {
    NumeraError::new("invalid_configuration", message)
}

pub(crate) fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| matches!(c, '\u{0009}'..='\u{000D}' | '\u{0020}' | '\u{00A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'))
}

pub(crate) fn js_trim_utf16(units: &[u16]) -> &[u16] {
    let whitespace = |unit: &u16| matches!(*unit, 0x9..=0xd | 0x20 | 0xa0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff);
    let start = units
        .iter()
        .position(|u| !whitespace(u))
        .unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|u| !whitespace(u))
        .map_or(start, |i| i + 1);
    &units[start..end]
}

fn expand_simple(raw: &mut Value, file: &Path) -> Result<Value> {
    validate::default_and_validate(raw, true)?;
    let key = raw["api_key"]
        .as_utf16()
        .ok_or_else(|| invalid("Invalid key."))?;
    let models = raw["models"]
        .as_array()
        .ok_or_else(|| invalid("Invalid models."))?;
    let mut ids = std::collections::HashSet::new();
    if js_trim_utf16(key).is_empty()
        || key.iter().any(|u| matches!(*u, 0 | 10 | 13))
        || models.iter().any(|m| {
            m["id"]
                .as_utf16()
                .is_none_or(|v| js_trim_utf16(v).is_empty())
                || m["name"]
                    .as_utf16()
                    .is_none_or(|v| js_trim_utf16(v).is_empty())
                || !ids.insert(m["id"].as_utf16().unwrap_or(&[]))
        })
    {
        return Err(invalid(
            "Key and model IDs/names must be nonempty; model IDs must be unique.",
        ));
    }
    let profile = raw["profile"].as_str().unwrap_or("openai-images");
    if (profile == "openai-responses") != raw.get("orchestration_model").is_some() {
        return Err(invalid(
            "orchestration_model is required only for the openai-responses profile.",
        ));
    }
    let enabled: Vec<&Value> = models.iter().filter(|m| m["enabled"] == true).collect();
    if enabled.is_empty() {
        return Err(invalid("Enable at least one configured model."));
    }
    let root = file.parent().unwrap_or(Path::new(""));
    let origin = validate::url_origin(&String::from_utf16_lossy(
        raw["api_endpoint"]
            .as_utf16()
            .ok_or_else(|| invalid("Invalid API endpoint."))?,
    ))?;
    let mut auth = json!({"type": if profile.starts_with("gemini") { "header" } else { "bearer" }, "apiKey":raw["api_key"], "origin":origin});
    if profile.starts_with("gemini") {
        auth["header"] = json!("x-goog-api-key");
    }
    let mut overrides = Object::new();
    for model in &enabled {
        let mut defaults = Object::new();
        for field in ["output_format", "size", "quality"] {
            if let Some(value) = model.get(field).filter(|v| !v.is_null()) {
                defaults.insert(field.into(), value.clone());
            }
        }
        overrides.insert(
            model["id"]
                .as_utf16()
                .map(|v| crate::compat::value::JsString::from_utf16(v.to_vec()))
                .ok_or_else(|| invalid("Invalid model ID."))?,
            json!({"defaults":defaults}),
        );
    }
    let mut connection = json!({
        "adapter": if ["omniroute", "9router"].contains(&profile) { "openai-images" } else { profile },
        "baseUrl":raw["api_endpoint"], "auth":auth, "defaultModel":enabled[0]["id"],
        "deniedModels":models.iter().filter(|m| m["enabled"] == false).map(|m| m["id"].clone()).collect::<Vec<_>>(),
        "configuredModels":enabled.iter().map(|m| json!({"id":m["id"],"name":m["name"]})).collect::<Vec<_>>(),
        "modelOverrides":overrides
    });
    if ["omniroute", "9router"].contains(&profile) {
        connection["gateway"] = json!(profile);
    }
    for (source, target) in [
        ("edit", "edit"),
        ("orchestration_model", "orchestrationModel"),
    ] {
        if let Some(value) = raw.get(source) {
            connection[target] = value.clone();
        }
    }
    Ok(
        json!({"schemaVersion":1,"defaultConnection":"default", "outputDir":root.join("outputs").to_str().ok_or_else(|| invalid("Output path is not Unicode."))?,
        "stateDir":root.join("state").to_str().ok_or_else(|| invalid("State path is not Unicode."))?, "logging":{"directory":root.join("logs").to_str().ok_or_else(|| invalid("Log path is not Unicode."))?},
        "files":{"allowedInputRoots":[root.join("inputs").to_str().ok_or_else(|| invalid("Input path is not Unicode."))?]}, "connections":{"default":connection}}),
    )
}
