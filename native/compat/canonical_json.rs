//! Source-runtime compatibility for existing persisted request identities.
use super::value::Value;
use crate::error::Result;

pub fn fingerprint(value: &Value) -> Result<String> {
    value.fingerprint()
}

pub fn canonical_json(value: &Value) -> Result<String> {
    value.canonical_json()
}
