use crate::error::{NumeraError, Result};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

/// Resolves one owned value without retaining a source lock across credential I/O.
pub trait EnvironmentSource: Send + Sync {
    fn value(&self, name: &str) -> Result<Option<String>>;
}

impl EnvironmentSource for HashMap<String, String> {
    fn value(&self, name: &str) -> Result<Option<String>> {
        Ok(self.get(name).cloned())
    }
}

/// Instance-owned state, not process-global OS environment mutation.
#[derive(Clone, Default)]
pub struct Environment {
    values: Arc<RwLock<HashMap<String, String>>>,
}

impl From<HashMap<String, String>> for Environment {
    fn from(values: HashMap<String, String>) -> Self {
        Self {
            values: Arc::new(RwLock::new(values)),
        }
    }
}

impl EnvironmentSource for Environment {
    fn value(&self, name: &str) -> Result<Option<String>> {
        self.values
            .read()
            .map(|values| values.get(name).cloned())
            .map_err(|_| unavailable())
    }
}

impl Environment {
    pub fn set(&self, name: &str, value: &str) -> Result<()> {
        validate_name(name)?;
        if value.contains('\0') {
            return Err(invalid());
        }
        self.values
            .write()
            .map_err(|_| unavailable())?
            .insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        validate_name(name)?;
        self.values.write().map_err(|_| unavailable())?.remove(name);
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.contains(['=', '\0']) {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> NumeraError {
    NumeraError::new(
        "invalid_configuration",
        "Invalid runtime environment assignment.",
    )
}

fn unavailable() -> NumeraError {
    NumeraError::new(
        "missing_credentials",
        "Runtime environment is unavailable; no credentials were selected.",
    )
}

#[cfg(test)]
mod tests {
    use super::{Environment, EnvironmentSource};
    use crate::error::Result;
    use std::{collections::HashMap, panic::AssertUnwindSafe};

    #[test]
    fn selected_values_are_owned_while_clones_share_updates_and_removal() -> Result<()> {
        let source = HashMap::from([("SELECTED".to_owned(), "synthetic-a".to_owned())]);
        assert_eq!(source.value("SELECTED")?.as_deref(), Some("synthetic-a"));
        let env = Environment::from(source);
        let shared = env.clone();
        let pinned = env.value("SELECTED")?;
        shared.set("SELECTED", "synthetic-b")?;
        assert_eq!(pinned.as_deref(), Some("synthetic-a"));
        assert_eq!(env.value("SELECTED")?.as_deref(), Some("synthetic-b"));
        assert!(env.value("selected")?.is_none());
        assert!(Environment::default().value("SELECTED")?.is_none());
        shared.remove("SELECTED")?;
        assert!(env.value("SELECTED")?.is_none());
        shared.remove("SELECTED")?;
        Ok(())
    }

    #[test]
    fn invalid_assignments_and_poison_refuse_without_exposing_values() {
        let env = Environment::default();
        for name in ["", "bad=name", "bad\0name"] {
            assert_eq!(
                env.set(name, "synthetic").unwrap_err().code,
                "invalid_configuration"
            );
            assert_eq!(env.remove(name).unwrap_err().code, "invalid_configuration");
        }
        assert_eq!(
            env.set("SELECTED", "bad\0value").unwrap_err().code,
            "invalid_configuration"
        );
        // Header validation remains the credential resolver's responsibility.
        env.set("SELECTED", "").unwrap();
        assert_eq!(env.value("SELECTED").unwrap().as_deref(), Some(""));
        env.set("SELECTED", "synthetic-private").unwrap();
        let poisoned = env.clone();
        assert!(
            std::panic::catch_unwind(AssertUnwindSafe(|| {
                let _guard = poisoned.values.write().unwrap();
                panic!("Controlled runtime environment poison");
            }))
            .is_err()
        );
        let errors = [
            env.value("SELECTED").unwrap_err(),
            env.set("SELECTED", "replacement").unwrap_err(),
            env.remove("SELECTED").unwrap_err(),
        ];
        for error in errors {
            assert_eq!(error.code, "missing_credentials");
            assert_eq!(
                error.message,
                "Runtime environment is unavailable; no credentials were selected."
            );
            assert!(!error.message.contains("synthetic-private"));
        }
    }
}
