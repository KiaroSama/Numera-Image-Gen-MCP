//! Direct builders preserve in-memory JS semantics; serde operates only at actual JSON boundaries.
use super::{JsString, Number, Object, Value};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, HashMap};

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let text = self.stringify().map_err(serde::ser::Error::custom)?;
        let raw =
            serde_json::value::RawValue::from_string(text).map_err(serde::ser::Error::custom)?;
        raw.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(deserializer)?;
        Self::parse_with_limit(raw.get(), 128 * 1024 * 1024).map_err(serde::de::Error::custom)
    }
}

pub trait ToValue {
    fn to_value(&self) -> Value;
}
impl ToValue for Value {
    fn to_value(&self) -> Value {
        self.clone()
    }
}
impl ToValue for crate::compat::js_json::JsValue {
    fn to_value(&self) -> Value {
        Value::from_js(self)
    }
}
impl ToValue for JsString {
    fn to_value(&self) -> Value {
        Value::String(self.clone())
    }
}
impl ToValue for str {
    fn to_value(&self) -> Value {
        Value::String(self.into())
    }
}
impl ToValue for String {
    fn to_value(&self) -> Value {
        self.as_str().to_value()
    }
}
impl ToValue for bool {
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
}
impl ToValue for Number {
    fn to_value(&self) -> Value {
        Value::Number(*self)
    }
}
impl ToValue for () {
    fn to_value(&self) -> Value {
        Value::Null
    }
}
impl<T: ToValue + ?Sized> ToValue for &T {
    fn to_value(&self) -> Value {
        (*self).to_value()
    }
}
impl<T: ToValue> ToValue for Option<T> {
    fn to_value(&self) -> Value {
        self.as_ref().map_or(Value::Null, ToValue::to_value)
    }
}
impl<T: ToValue> ToValue for [T] {
    fn to_value(&self) -> Value {
        Value::Array(self.iter().map(ToValue::to_value).collect())
    }
}
impl<T: ToValue, const N: usize> ToValue for [T; N] {
    fn to_value(&self) -> Value {
        self.as_slice().to_value()
    }
}
impl<T: ToValue> ToValue for Vec<T> {
    fn to_value(&self) -> Value {
        self.as_slice().to_value()
    }
}
impl ToValue for Object {
    fn to_value(&self) -> Value {
        Value::Object(self.clone())
    }
}
impl<K: Clone + Into<JsString>, T: ToValue, S> ToValue for HashMap<K, T, S> {
    fn to_value(&self) -> Value {
        Value::Object(
            self.iter()
                .map(|(k, v)| (k.clone().into(), v.to_value()))
                .collect(),
        )
    }
}
impl<K: Clone + Into<JsString>, T: ToValue> ToValue for BTreeMap<K, T> {
    fn to_value(&self) -> Value {
        Value::Object(
            self.iter()
                .map(|(k, v)| (k.clone().into(), v.to_value()))
                .collect(),
        )
    }
}
macro_rules! numbers {
    ($($kind:ty),*) => {$(
        impl ToValue for $kind {
            fn to_value(&self) -> Value {
                Value::Number(Number::from(*self))
            }
        }
        impl From<$kind> for Value {
            fn from(value: $kind) -> Self {
                value.to_value()
            }
        }
    )*};
}
numbers!(
    f64, f32, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, i128, u128
);
impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(value.into())
    }
}
impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.into())
    }
}
impl From<JsString> for Value {
    fn from(value: JsString) -> Self {
        Self::String(value)
    }
}
impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl FromIterator<Value> for Value {
    fn from_iter<T: IntoIterator<Item = Value>>(iter: T) -> Self {
        Self::Array(iter.into_iter().collect())
    }
}
