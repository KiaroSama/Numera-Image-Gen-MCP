//! UTF-16 identity and binary64 scalar semantics behind explicit lossless views.
use super::{JsValue, js_json};
use crate::error::{NumeraError, Result};
use std::fmt;

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct JsString {
    units: Vec<u16>,
    unicode: Option<String>,
}
impl JsString {
    pub fn from_utf16(units: Vec<u16>) -> Self {
        let unicode = String::from_utf16(&units).ok();
        Self { units, unicode }
    }
    pub fn as_utf16(&self) -> &[u16] {
        &self.units
    }
    pub fn as_str(&self) -> Option<&str> {
        self.unicode.as_deref()
    }
    pub fn checked_str(&self) -> Result<&str> {
        self.as_str().ok_or_else(|| {
            NumeraError::new(
                "unrepresentable_json",
                "The UTF-16 string has no lossless Unicode view.",
            )
        })
    }
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }
    pub fn len(&self) -> usize {
        self.units.len()
    }
    pub fn starts_with(&self, prefix: &str) -> bool {
        self.units
            .iter()
            .copied()
            .take(prefix.encode_utf16().count())
            .eq(prefix.encode_utf16())
    }
    pub fn contains_ascii_case_insensitive(&self, needle: &str) -> bool {
        let needle: Vec<u16> = needle.encode_utf16().collect();
        if needle.is_empty() {
            return true;
        }
        self.units.windows(needle.len()).any(|window| {
            window.iter().zip(&needle).all(|(a, b)| {
                u8::try_from(*a).is_ok_and(|a| u16::from(a.to_ascii_lowercase()) == *b)
            })
        })
    }
}
impl From<String> for JsString {
    fn from(value: String) -> Self {
        Self {
            units: value.encode_utf16().collect(),
            unicode: Some(value),
        }
    }
}
impl From<&String> for JsString {
    fn from(value: &String) -> Self {
        value.clone().into()
    }
}
impl From<&&str> for JsString {
    fn from(value: &&str) -> Self {
        (*value).into()
    }
}
impl From<&JsString> for JsString {
    fn from(value: &JsString) -> Self {
        value.clone()
    }
}
impl From<&str> for JsString {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}
impl fmt::Debug for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JsString")
    }
}
impl fmt::Display for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(value) = self.as_str() {
            f.write_str(value)
        } else {
            f.write_str(
                &js_json::stringify(&JsValue::String(self.units.clone()))
                    .map_err(|_| fmt::Error)?,
            )
        }
    }
}
impl PartialEq<String> for JsString {
    fn eq(&self, other: &String) -> bool {
        self == other.as_str()
    }
}
impl PartialEq<str> for JsString {
    fn eq(&self, other: &str) -> bool {
        self.units.iter().copied().eq(other.encode_utf16())
    }
}
impl PartialEq<&str> for JsString {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Number(pub f64);
impl Number {
    pub fn from_f64(value: f64) -> Option<Self> {
        Some(Self(value))
    }
    pub fn as_f64(&self) -> Option<f64> {
        Some(self.0)
    }
    pub fn as_i64(&self) -> Option<i64> {
        (self.0.fract() == 0.0 && (-9223372036854775808.0..9223372036854775808.0).contains(&self.0))
            .then_some(self.0 as i64)
    }
    pub fn as_u64(&self) -> Option<u64> {
        (self.0.fract() == 0.0 && (0.0..18446744073709551616.0).contains(&self.0))
            .then_some(self.0 as u64)
    }
}
macro_rules! numbers {
    ($($kind:ty),*) => {$(
        impl From<$kind> for Number {
            fn from(value: $kind) -> Self {
                Self(value as f64)
            }
        }
    )*};
}
numbers!(
    f64, f32, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, i128, u128
);
