use super::{Config, invalid, read_config_text, validate};
use crate::compat::value::{Value, json};
use crate::error::{NumeraError, Result};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub(crate) fn arguments(args: &[String]) -> Result<HashMap<String, String>> {
    let strings = [
        "config",
        "default-connection",
        "output-dir",
        "state-dir",
        "log-dir",
        "log-level",
        "request-timeout-ms",
        "max-concurrent-requests",
        "return-mode",
    ];
    let mut values = HashMap::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            if index + 1 == args.len() {
                break;
            }
            return Err(NumeraError::new(
                "ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL",
                "Unexpected positional configuration argument.",
            ));
        }
        let Some(flag) = arg.strip_prefix("--").filter(|flag| !flag.is_empty()) else {
            let code = if arg.starts_with('-') && arg.len() > 1 {
                "ERR_PARSE_ARGS_UNKNOWN_OPTION"
            } else {
                "ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL"
            };
            return Err(NumeraError::new(code, "Unknown configuration argument."));
        };
        let (name, inline) = flag
            .split_once('=')
            .map_or((flag, None), |(name, value)| (name, Some(value)));
        if name == "openai-compat" {
            if inline.is_some() {
                return Err(NumeraError::new(
                    "ERR_PARSE_ARGS_INVALID_OPTION_VALUE",
                    "Boolean configuration option does not take a value.",
                ));
            }
            values.insert(name.into(), "true".into());
            index += 1;
            continue;
        }
        if !strings.contains(&name) {
            return Err(NumeraError::new(
                "ERR_PARSE_ARGS_UNKNOWN_OPTION",
                "Unknown configuration option.",
            ));
        }
        let value = if let Some(value) = inline {
            value.to_owned()
        } else {
            index += 1;
            let value = args
                .get(index)
                .filter(|value| !(value.len() > 1 && value.starts_with('-')))
                .ok_or_else(|| {
                    NumeraError::new(
                        "ERR_PARSE_ARGS_INVALID_OPTION_VALUE",
                        "Configuration option requires an unambiguous string value.",
                    )
                })?;
            value.clone()
        };
        values.insert(name.into(), value);
        index += 1;
    }
    Ok(values)
}

pub(crate) fn get<'a>(
    flags: &'a HashMap<String, String>,
    env: &'a HashMap<String, String>,
    flag: &str,
    key: &str,
) -> Option<&'a str> {
    flags.get(flag).or_else(|| env.get(key)).map(String::as_str)
}

pub fn user_root(env: &HashMap<String, String>) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let home = env
            .get("USERPROFILE")
            .cloned()
            .or_else(|| std::env::var("USERPROFILE").ok());
        let root = env
            .get("LOCALAPPDATA")
            .cloned()
            .or(home)
            .ok_or_else(|| invalid("Cannot resolve the current user's configuration root."))?;
        Ok(PathBuf::from(root).join("Numera").join("ImageGen"))
    }
    #[cfg(not(windows))]
    {
        let base = if let Some(root) = env.get("XDG_CONFIG_HOME") {
            PathBuf::from(root)
        } else {
            let home = env
                .get("HOME")
                .cloned()
                .or_else(|| std::env::var("HOME").ok())
                .ok_or_else(|| invalid("Cannot resolve the current user's configuration root."))?;
            PathBuf::from(home).join(".config")
        };
        Ok(base.join("numera-image-gen"))
    }
}

/// Adjacent legacy files are relative to the installed executable/project, never the caller CWD.
pub fn config_file(
    args: &[String],
    env: &HashMap<String, String>,
    root: &Path,
) -> Result<Option<PathBuf>> {
    let flags = arguments(args)?;
    if flags.contains_key("openai-compat") {
        return Ok(None);
    }
    let file =
        if let Some(file) = get(&flags, env, "config", "NUMERA_CONFIG").filter(|s| !s.is_empty()) {
            PathBuf::from(file)
        } else if root.join(".env").try_exists().unwrap_or(false) {
            root.join(".env")
        } else if root.join("config.local.json").try_exists().unwrap_or(false) {
            root.join("config.local.json")
        } else {
            user_root(env)?.join("config.json")
        };
    if !validate::absolute(&file) {
        return Err(invalid("NUMERA_CONFIG must be absolute."));
    }
    Ok(Some(file))
}

pub fn load_config(args: &[String], env: &HashMap<String, String>, root: &Path) -> Result<Config> {
    let file = config_file(args, env, root)?;
    if let Some(file) = file {
        Config::parse(&read_config_text(&file)?, &file, args, env)
    } else {
        Config::parse("", &root.join("config.json"), args, env)
    }
}

pub(crate) fn compat_config(env: &HashMap<String, String>) -> Result<Value> {
    let root = user_root(env)?;
    let mut connection = json!({"adapter":"openai-images", "baseUrl":env.get("OPENAI_BASE_URL").map_or("https://api.openai.com/v1", String::as_str),
        "auth":{"type":"bearer","secretEnv":"OPENAI_API_KEY"}});
    if let Some(model) = env.get("OPENAI_MODEL") {
        connection["defaultModel"] = json!(model);
    }
    Ok(
        json!({"schemaVersion":1,"defaultConnection":"openai-compat", "outputDir":root.join("outputs").to_str().ok_or_else(|| invalid("Output path is not Unicode."))?,
        "stateDir":root.join("state").to_str().ok_or_else(|| invalid("State path is not Unicode."))?, "logging":{"directory":root.join("logs").to_str().ok_or_else(|| invalid("Log path is not Unicode."))?}, "connections":{"openai-compat":connection}}),
    )
}

pub(crate) fn overrides(
    raw: &mut Value,
    flags: &HashMap<String, String>,
    env: &HashMap<String, String>,
) -> Result<()> {
    for (field, flag, key) in [
        (
            "defaultConnection",
            "default-connection",
            "NUMERA_DEFAULT_CONNECTION",
        ),
        ("outputDir", "output-dir", "NUMERA_OUTPUT_DIR"),
        ("stateDir", "state-dir", "NUMERA_STATE_DIR"),
        ("returnMode", "return-mode", "NUMERA_RETURN_MODE"),
    ] {
        if let Some(value) = get(flags, env, flag, key) {
            replace_field(raw, field, json!(value));
        }
    }
    if raw.get("logging").is_none_or(Value::is_null) {
        raw["logging"] = json!({});
    }
    for (field, flag, key) in [
        ("directory", "log-dir", "NUMERA_LOG_DIR"),
        ("level", "log-level", "NUMERA_LOG_LEVEL"),
    ] {
        if let Some(value) = get(flags, env, flag, key) {
            if !raw["logging"].is_object() {
                return Err(invalid("Invalid logging configuration."));
            }
            replace_field(&mut raw["logging"], field, json!(value));
        }
    }
    if let Some(value) = get(
        flags,
        env,
        "max-concurrent-requests",
        "NUMERA_MAX_CONCURRENT_REQUESTS",
    ) {
        replace_field(
            raw,
            "maxConcurrentRequests",
            validate::js_number(value).map_or(Value::Null, |n| json!(n)),
        );
    }
    Ok(())
}

fn replace_field(object: &mut Value, field: &str, value: Value) {
    if let Some(object) = object.as_object_mut()
        && let Some(previous) = object.insert(field.into(), value)
    {
        super::drop_json(previous);
    }
}
