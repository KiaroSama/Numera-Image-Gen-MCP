use super::{MAX_CONFIG_BYTES, invalid};
use crate::{compat::value::Value, error::Result};
use std::ops::{Deref, DerefMut};

pub(super) fn parse(text: &str) -> Result<OwnedJson> {
    if text.len() > MAX_CONFIG_BYTES {
        return Err(invalid("Configuration exceeds 1 MiB."));
    }
    Value::parse_with_limit(text, MAX_CONFIG_BYTES)
        .map(OwnedJson)
        .map_err(|_| invalid("Cannot parse JSON configuration."))
}

pub(super) struct OwnedJson(pub Value);
impl OwnedJson {
    pub fn take(&mut self) -> Value {
        std::mem::take(&mut self.0)
    }
}
impl Deref for OwnedJson {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.0
    }
}
impl DerefMut for OwnedJson {
    fn deref_mut(&mut self) -> &mut Value {
        &mut self.0
    }
}

pub fn drop_json(value: Value) {
    drop(value);
}

pub fn clone_json(value: &Value) -> Value {
    value.clone()
}
