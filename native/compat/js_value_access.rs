//! Access and mutation on authoritative JS values; no UTF-16 replacement or numeric coercion.
use super::js_json::JsValue;
use crate::error::{NumeraError, Result};
use std::collections::HashMap;

fn wrong_type() -> NumeraError {
    NumeraError::new("invalid_json_type", "The JSON value has the wrong type.")
}

/// Conversion only at a boundary that requires Unicode scalar values. JSON strings and
/// keys themselves remain UTF-16, including unpaired surrogates.
pub fn checked_utf16(units: &[u16]) -> Result<String> {
    String::from_utf16(units).map_err(|_| {
        NumeraError::new(
            "unrepresentable_json",
            "The UTF-16 string cannot be represented without losing JavaScript semantics.",
        )
    })
}

impl JsValue {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn is_boolean(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Self::Number(_))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    /// Checked integer view, not JavaScript ToInteger or a saturating Rust cast.
    pub fn as_i64(&self) -> Option<i64> {
        self.as_f64()
            .filter(|n| {
                n.fract() == 0.0 && (-9223372036854775808.0..9223372036854775808.0).contains(n)
            })
            .map(|n| n as i64)
    }

    pub fn as_u64(&self) -> Option<u64> {
        self.as_f64()
            .filter(|n| n.fract() == 0.0 && (0.0..18446744073709551616.0).contains(n))
            .map(|n| n as u64)
    }

    pub fn as_utf16(&self) -> Option<&[u16]> {
        match self {
            Self::String(units) => Some(units),
            _ => None,
        }
    }

    pub fn checked_string(&self) -> Result<String> {
        checked_utf16(self.as_utf16().ok_or_else(wrong_type)?)
    }

    pub fn as_array(&self) -> Option<&[JsValue]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<JsValue>> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    /// Borrowed storage entries in first-insertion order. Parsed objects have unique keys;
    /// externally constructed duplicates are resolved last-value-first-position by set/stringify.
    pub fn entries(&self) -> Option<&[(Vec<u16>, JsValue)]> {
        match self {
            Self::Object(entries) => Some(entries),
            _ => None,
        }
    }

    pub fn entries_mut(&mut self) -> Option<&mut Vec<(Vec<u16>, JsValue)>> {
        match self {
            Self::Object(entries) => Some(entries),
            _ => None,
        }
    }

    pub fn at(&self, index: usize) -> Option<&JsValue> {
        self.as_array()?.get(index)
    }

    pub fn at_mut(&mut self, index: usize) -> Option<&mut JsValue> {
        self.as_array_mut()?.get_mut(index)
    }

    pub fn get_utf16(&self, key: &[u16]) -> Option<&JsValue> {
        self.entries()?
            .iter()
            .rev()
            .find(|(candidate, _)| candidate.as_slice() == key)
            .map(|(_, value)| value)
    }

    pub fn get_utf16_mut(&mut self, key: &[u16]) -> Option<&mut JsValue> {
        self.entries_mut()?
            .iter_mut()
            .rev()
            .find(|(candidate, _)| candidate.as_slice() == key)
            .map(|(_, value)| value)
    }

    pub fn get(&self, key: &str) -> Option<&JsValue> {
        self.entries()?
            .iter()
            .rev()
            .find(|(candidate, _)| candidate.iter().copied().eq(key.encode_utf16()))
            .map(|(_, value)| value)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut JsValue> {
        self.entries_mut()?
            .iter_mut()
            .rev()
            .find(|(candidate, _)| candidate.iter().copied().eq(key.encode_utf16()))
            .map(|(_, value)| value)
    }

    pub fn set(&mut self, key: &str, value: JsValue) -> Result<Option<JsValue>> {
        self.set_utf16(key.encode_utf16().collect(), value)
    }

    /// Replaces the effective value at its first insertion position, returning the former
    /// last value. Also repairs duplicate keys allowed by the public enum constructor.
    pub fn set_utf16(&mut self, key: Vec<u16>, value: JsValue) -> Result<Option<JsValue>> {
        let entries = self.entries_mut().ok_or_else(wrong_type)?;
        let Some(first) = entries.iter().position(|(candidate, _)| *candidate == key) else {
            entries.push((key, value));
            return Ok(None);
        };
        let mut previous = std::mem::replace(&mut entries[first].1, value);
        let mut seen = false;
        entries.retain_mut(|(candidate, value)| {
            if *candidate != key {
                return true;
            }
            if !seen {
                seen = true;
                return true;
            }
            previous = std::mem::take(value);
            false
        });
        Ok(Some(previous))
    }

    pub fn remove(&mut self, key: &str) -> Option<JsValue> {
        self.remove_utf16(&key.encode_utf16().collect::<Vec<_>>())
    }

    /// Removes every duplicate and returns the last effective value, not an earlier shadow.
    pub fn remove_utf16(&mut self, key: &[u16]) -> Option<JsValue> {
        let entries = self.entries_mut()?;
        let mut previous = None;
        entries.retain_mut(|(candidate, value)| {
            if candidate.as_slice() != key {
                return true;
            }
            previous = Some(std::mem::take(value));
            false
        });
        previous
    }

    /// Structural comparison, not JS object-reference equality. Objects ignore insertion order
    /// and compare last effective values; arrays retain order and UTF-16 strings compare exactly.
    /// Numbers follow JS strict numeric equality: +0 == -0 and NaN != NaN. Consequently this is
    /// not an equivalence relation for values containing NaN, and cannot implement Rust Eq.
    pub fn structurally_equal(&self, other: &Self) -> bool {
        fn effective(entries: &[(Vec<u16>, JsValue)]) -> HashMap<&[u16], &JsValue> {
            entries
                .iter()
                .map(|(key, value)| (key.as_slice(), value))
                .collect()
        }
        let mut pending = vec![(self, other)];
        while let Some((left, right)) = pending.pop() {
            match (left, right) {
                (Self::Null, Self::Null) => {}
                (Self::Bool(a), Self::Bool(b)) if a == b => {}
                (Self::Number(a), Self::Number(b)) if a == b => {}
                (Self::String(a), Self::String(b)) if a == b => {}
                (Self::Array(a), Self::Array(b)) if a.len() == b.len() => {
                    pending.extend(a.iter().zip(b));
                }
                (Self::Object(a), Self::Object(b)) => {
                    let a = effective(a);
                    let b = effective(b);
                    if a.len() != b.len() {
                        return false;
                    }
                    for (key, left) in a {
                        let Some(&right) = b.get(key) else {
                            return false;
                        };
                        pending.push((left, right));
                    }
                }
                _ => return false,
            }
        }
        true
    }
}
