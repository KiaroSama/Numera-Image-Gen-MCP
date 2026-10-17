use crate::compat::value::{Object, Value, json};
use crate::runtime_paths::private_directory;
use chrono::Utc;
use regex::Regex;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

fn patterns() -> &'static (Regex, Regex, Regex) {
    static PATTERNS: OnceLock<(Regex, Regex, Regex)> = OnceLock::new();
    PATTERNS.get_or_init(|| (
        Regex::new("(?i)authorization|cookie|token|key|secret|password|signature|prompt|base64|b64_json|inlineData|data_url|path")
            .expect("static sensitive field pattern"),
        Regex::new(r#"https?://[^\s\"<>]+"#).expect("static URL pattern"),
        Regex::new(r"(?i)Bearer\s+\S+").expect("static bearer pattern"),
    ))
}

pub fn redact(value: &Value, depth: usize) -> Value {
    if depth > 8 {
        return Value::String("[REDACTED]".into());
    }
    let (sensitive, urls, bearer) = patterns();
    match value {
        Value::String(text) => {
            let Some(text) = text.as_str() else {
                return json!("[REDACTED]");
            };
            let text = urls.replace_all(text, |capture: &regex::Captures<'_>| {
                url::Url::parse(&capture[0]).map_or_else(
                    |_| "[REDACTED]".into(),
                    |mut url| {
                        url.set_query(None);
                        let _ = url.set_username("");
                        let _ = url.set_password(None);
                        url.to_string()
                    },
                )
            });
            Value::String(
                bearer
                    .replace_all(&text, "Bearer [REDACTED]")
                    .into_owned()
                    .into(),
            )
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(30)
                .map(|v| redact(v, depth + 1))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        if k.as_str().is_none_or(|key| sensitive.is_match(key)) {
                            json!("[REDACTED]")
                        } else {
                            redact(v, depth + 1)
                        },
                    )
                })
                .collect(),
        ),
        value => value.clone(),
    }
}

pub struct Logger {
    file: Mutex<Option<File>>,
    level: usize,
    run_id: String,
}

impl Logger {
    pub fn new(directory: &Path, level: &str) -> Self {
        Self::with_retention(directory, level, 14)
    }

    pub fn with_retention(directory: &Path, level: &str, retention_days: u64) -> Self {
        let run_id = Uuid::new_v4().to_string();
        let file = (|| {
            private_directory(directory).ok()?;
            let name = format!(
                "numera-image-gen-mcp_{}_UTC-{}-{}.log",
                Utc::now().format("%Y-%m-%d_%H-%M-%S"),
                std::process::id(),
                run_id
            );
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(directory.join(name)).ok()
        })();
        if file.is_none() {
            eprintln!("WARNING: File logging unavailable; using stderr.");
        } else if let Ok(entries) = std::fs::read_dir(directory) {
            let age = std::time::Duration::from_secs(retention_days.saturating_mul(86400));
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("numera-image-gen-mcp_")
                    && name.ends_with(".log")
                    && let Ok(info) = std::fs::symlink_metadata(entry.path())
                    && info.is_file()
                    && info
                        .modified()
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|elapsed| elapsed > age)
                    && std::fs::remove_file(entry.path()).is_err()
                {
                    eprintln!("WARNING: Log retention cleanup failed.");
                }
            }
        }
        Self {
            file: Mutex::new(file),
            level: level_index(level),
            run_id,
        }
    }

    pub fn log(&self, level: &str, component: &str, message: &str, context: Value) {
        if level_index(level) < self.level {
            return;
        }
        let mut data = redact(&context, 0)
            .as_object()
            .cloned()
            .unwrap_or_else(Object::new);
        data.insert("run_id".into(), json!(self.run_id));
        data.insert(
            "timestamp".into(),
            json!(Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        );
        data.insert("level".into(), json!(level));
        data.insert("component".into(), json!(component));
        data.insert("message".into(), redact(&json!(message), 0));
        let Ok(mut line) = serde_json::to_vec(&Value::Object(data)) else {
            return;
        };
        line.push(b'\n');
        if let Ok(mut guard) = self.file.lock()
            && let Some(file) = guard.as_mut()
            && file.write_all(&line).and_then(|_| file.flush()).is_err()
        {
            *guard = None;
            eprintln!("WARNING: File logging failed.");
        }
        let _ = std::io::stderr().write_all(&line);
    }
}

fn level_index(level: &str) -> usize {
    match level {
        "DEBUG" => 0,
        "INFO" => 1,
        "WARNING" => 2,
        _ => 3,
    }
}
