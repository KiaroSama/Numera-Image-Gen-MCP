use super::{Config, read_config_text};
use crate::compat::value::{Value, json};
use crate::{compat::canonical_json::fingerprint, error::Result, logging::Logger};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReloadEvent {
    Unchanged,
    Accepted,
    Invalid,
    RestartRequired,
    SnapshotLimit,
}

pub struct ReloadResult {
    pub config: Arc<Config>,
    pub event: ReloadEvent,
}

/// The caller serializes refreshes with exclusive access; Arc snapshots bind in-flight operations.
/// Retain every accepted generation, including initial config, for exact existing-handle recovery.
pub struct ConfigReload {
    current: Arc<Config>,
    snapshots: Vec<Arc<Config>>,
    file: PathBuf,
    args: Vec<String>,
    env: HashMap<String, String>,
    last_seen: Option<String>,
}

impl ConfigReload {
    pub fn new(
        current: Config,
        file: PathBuf,
        args: Vec<String>,
        env: HashMap<String, String>,
    ) -> Self {
        let current = Arc::new(current);
        Self {
            snapshots: vec![Arc::clone(&current)],
            current,
            file,
            args,
            env,
            last_seen: None,
        }
    }

    pub fn current(&self) -> Arc<Config> {
        Arc::clone(&self.current)
    }
    pub fn snapshots(&self) -> &[Arc<Config>] {
        &self.snapshots
    }

    pub fn refresh(
        &mut self,
        logger: &Logger,
        accept: impl FnOnce(&Config) -> bool,
    ) -> Result<Arc<Config>> {
        let text = read_config_text(&self.file).ok();
        let outcome = self.refresh_text(text.as_deref(), accept)?;
        let message = match outcome.event {
            ReloadEvent::Unchanged => None,
            ReloadEvent::Accepted => Some(("INFO", "Saved configuration accepted.")),
            ReloadEvent::Invalid => Some((
                "WARNING",
                "Saved configuration rejected; retaining last valid configuration.",
            )),
            ReloadEvent::RestartRequired => Some((
                "WARNING",
                "Saved configuration rejected; storage, logging, file limits and global policy changes require restart.",
            )),
            ReloadEvent::SnapshotLimit => Some((
                "WARNING",
                "Saved configuration rejected; retained snapshot limit requires restart.",
            )),
        };
        if let Some((level, message)) = message {
            logger.log(level, "config", message, json!({}));
        }
        Ok(outcome.config)
    }

    /// A supplied text snapshot is the same bytes hashed and parsed; no second file read/TOCTOU.
    pub fn refresh_text(
        &mut self,
        text: Option<&str>,
        accept: impl FnOnce(&Config) -> bool,
    ) -> Result<ReloadResult> {
        let hash = match text {
            Some(text) => fingerprint(&json!(text))?,
            None => "unreadable".into(),
        };
        if self.last_seen.as_ref() == Some(&hash) {
            return Ok(self.result(ReloadEvent::Unchanged));
        }
        self.last_seen = Some(hash);
        let Some(text) = text else {
            return Ok(self.result(ReloadEvent::Invalid));
        };
        let candidate = match Config::parse(text, &self.file, &self.args, &self.env) {
            Ok(candidate) => candidate,
            Err(_) => return Ok(self.result(ReloadEvent::Invalid)),
        };
        if fixed(&candidate)? != fixed(&self.current)? {
            return Ok(self.result(ReloadEvent::RestartRequired));
        }
        if fingerprint(&candidate.value)? == fingerprint(&self.current.value)? {
            return Ok(self.result(ReloadEvent::Unchanged));
        }
        if self.snapshots.len() >= 64 || !accept(&candidate) {
            return Ok(self.result(ReloadEvent::SnapshotLimit));
        }
        self.current = Arc::new(candidate);
        self.snapshots.push(Arc::clone(&self.current));
        Ok(self.result(ReloadEvent::Accepted))
    }

    fn result(&self, event: ReloadEvent) -> ReloadResult {
        ReloadResult {
            config: Arc::clone(&self.current),
            event,
        }
    }
}

fn fixed(config: &Config) -> Result<String> {
    let object = config
        .value
        .as_object()
        .map(|object| {
            object
                .iter()
                .filter(|(key, _)| {
                    key.as_str().is_none_or(|key| {
                        !["connections", "defaultConnection", "returnMode"].contains(&key)
                    })
                })
                .map(|(key, value)| (key.clone(), super::clone_json(value)))
                .collect::<crate::compat::value::Object>()
        })
        .unwrap_or_default();
    let object = super::json::OwnedJson(Value::Object(object));
    fingerprint(&object)
}
