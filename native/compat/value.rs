//! Authoritative JavaScript JSON values, with cached Unicode views only when lossless.
use super::js_json::{self, JsValue};
use crate::error::{NumeraError, Result};
use std::{
    fmt,
    ops::{Index, IndexMut},
};

#[path = "value_object.rs"]
mod object;
#[path = "value_serde.rs"]
mod wire;
pub use object::{Entry, Object, ObjectKey};
pub use wire::ToValue;

#[path = "value_scalar.rs"]
mod scalar;
pub use scalar::{JsString, Number};

macro_rules! number_equality {
    ($($kind:ty),*) => {$(
        impl PartialEq<$kind> for Value {
            fn eq(&self, other: &$kind) -> bool {
                self.as_f64() == Some(*other as f64)
            }
        }
    )*};
}
number_equality!(f64, f32, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

#[derive(Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(Number),
    String(JsString),
    Array(Vec<Value>),
    Object(Object),
}
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Null => "Value::Null",
            Self::Bool(_) => "Value::Bool",
            Self::Number(_) => "Value::Number",
            Self::String(_) => "Value::String",
            Self::Array(_) => "Value::Array",
            Self::Object(_) => "Value::Object",
        })
    }
}
impl Drop for Value {
    fn drop(&mut self) {
        fn drain(value: &mut Value, pending: &mut Vec<Value>) {
            match value {
                Value::Array(values) => pending.append(values),
                Value::Object(object) => pending.extend(object.0.drain(..).map(|(_, v)| v)),
                _ => {}
            }
        }
        let mut pending = Vec::new();
        drain(self, &mut pending);
        while let Some(mut value) = pending.pop() {
            drain(&mut value, &mut pending);
        }
    }
}
impl Clone for Value {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Value(&'a Value),
            Array(usize),
            Object(Vec<JsString>),
        }
        let mut pending = vec![Task::Value(self)];
        let mut complete = Vec::new();
        while let Some(task) = pending.pop() {
            match task {
                Task::Value(Value::Array(values)) => {
                    pending.push(Task::Array(values.len()));
                    pending.extend(values.iter().rev().map(Task::Value));
                }
                Task::Value(Value::Object(object)) => {
                    pending.push(Task::Object(object.keys().cloned().collect()));
                    pending.extend(object.values().rev().map(Task::Value));
                }
                Task::Value(value) => complete.push(match value {
                    Self::Null => Self::Null,
                    Self::Bool(v) => Self::Bool(*v),
                    Self::Number(v) => Self::Number(*v),
                    Self::String(v) => Self::String(v.clone()),
                    _ => unreachable!("containers are assembled separately"),
                }),
                Task::Array(n) => {
                    let values = complete.split_off(complete.len() - n);
                    complete.push(Self::Array(values));
                }
                Task::Object(keys) => {
                    let values = complete.split_off(complete.len() - keys.len());
                    complete.push(Self::Object(Object(keys.into_iter().zip(values).collect())));
                }
            }
        }
        complete.pop().expect("one cloned value")
    }
}

impl Value {
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
        if let Self::Bool(v) = self {
            Some(*v)
        } else {
            None
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        if let Self::Number(v) = self {
            v.as_f64()
        } else {
            None
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        if let Self::Number(v) = self {
            v.as_i64()
        } else {
            None
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        if let Self::Number(v) = self {
            v.as_u64()
        } else {
            None
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(v) = self {
            v.as_str()
        } else {
            None
        }
    }
    pub fn as_js_string(&self) -> Option<&JsString> {
        if let Self::String(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn as_utf16(&self) -> Option<&[u16]> {
        if let Self::String(v) = self {
            Some(v.as_utf16())
        } else {
            None
        }
    }
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        if let Self::Array(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        if let Self::Array(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn as_object(&self) -> Option<&Object> {
        if let Self::Object(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        if let Self::Object(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn get<I: ValueIndex>(&self, key: I) -> Option<&Value> {
        key.get(self)
    }
    pub fn get_mut<I: ValueIndex>(&mut self, key: I) -> Option<&mut Value> {
        key.get_mut(self)
    }
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }
    pub fn checked_index_mut(&mut self, key: &str) -> Result<&mut Value> {
        if self.is_null() {
            *self = Self::Object(Object::new());
        }
        self.as_object_mut()
            .map(|object| object.entry(key).or_default())
            .ok_or_else(|| {
                NumeraError::new(
                    "invalid_json_type",
                    "Object mutation requires an object or null.",
                )
            })
    }
    pub fn checked_array_index_mut(&mut self, index: usize) -> Result<&mut Value> {
        self.as_array_mut()
            .and_then(|values| values.get_mut(index))
            .ok_or_else(|| {
                NumeraError::new(
                    "invalid_json_type",
                    "Array mutation requires an existing array item.",
                )
            })
    }

    pub fn parse(text: &str) -> Result<Self> {
        js_json::parse(text).map(|v| Self::from_js(&v))
    }
    pub fn parse_with_limit(text: &str, limit: usize) -> Result<Self> {
        js_json::parse_bounded(text, limit).map(|v| Self::from_js(&v))
    }
    pub fn stringify(&self) -> Result<String> {
        js_json::stringify(&self.to_js())
    }
    pub fn canonical_json(&self) -> Result<String> {
        js_json::canonical_json(&self.to_js())
    }
    pub fn fingerprint(&self) -> Result<String> {
        js_json::fingerprint(&self.to_js())
    }
    pub fn from_serde(value: &serde_json::Value) -> Result<Self> {
        js_json::from_serde(value).map(|v| Self::from_js(&v))
    }
    pub fn to_serde(&self) -> Result<serde_json::Value> {
        js_json::to_serde(&self.to_js())
    }

    pub fn from_js(value: &JsValue) -> Self {
        enum Task<'a> {
            Value(&'a JsValue),
            Array(usize),
            Object(Vec<JsString>),
        }
        let mut pending = vec![Task::Value(value)];
        let mut complete = Vec::new();
        while let Some(task) = pending.pop() {
            match task {
                Task::Value(JsValue::Array(values)) => {
                    pending.push(Task::Array(values.len()));
                    pending.extend(values.iter().rev().map(Task::Value));
                }
                Task::Value(JsValue::Object(entries)) => {
                    pending.push(Task::Object(
                        entries
                            .iter()
                            .map(|(k, _)| JsString::from_utf16(k.clone()))
                            .collect(),
                    ));
                    pending.extend(entries.iter().rev().map(|(_, v)| Task::Value(v)));
                }
                Task::Value(value) => complete.push(match value {
                    JsValue::Null => Self::Null,
                    JsValue::Bool(v) => Self::Bool(*v),
                    JsValue::Number(v) => Self::Number(Number(*v)),
                    JsValue::String(v) => Self::String(JsString::from_utf16(v.clone())),
                    _ => unreachable!("containers are assembled separately"),
                }),
                Task::Array(n) => {
                    let values = complete.split_off(complete.len() - n);
                    complete.push(Self::Array(values));
                }
                Task::Object(keys) => {
                    let values = complete.split_off(complete.len() - keys.len());
                    complete.push(Self::Object(keys.into_iter().zip(values).collect()));
                }
            }
        }
        complete.pop().expect("one converted value")
    }
    pub fn to_js(&self) -> JsValue {
        enum Task<'a> {
            Value(&'a Value),
            Array(usize),
            Object(Vec<Vec<u16>>),
        }
        let mut pending = vec![Task::Value(self)];
        let mut complete = Vec::new();
        while let Some(task) = pending.pop() {
            match task {
                Task::Value(Self::Array(values)) => {
                    pending.push(Task::Array(values.len()));
                    pending.extend(values.iter().rev().map(Task::Value));
                }
                Task::Value(Self::Object(object)) => {
                    pending.push(Task::Object(
                        object.keys().map(|k| k.as_utf16().to_vec()).collect(),
                    ));
                    pending.extend(object.values().rev().map(Task::Value));
                }
                Task::Value(value) => complete.push(match value {
                    Self::Null => JsValue::Null,
                    Self::Bool(v) => JsValue::Bool(*v),
                    Self::Number(v) => JsValue::Number(v.0),
                    Self::String(v) => JsValue::String(v.as_utf16().to_vec()),
                    _ => unreachable!("containers are assembled separately"),
                }),
                Task::Array(n) => {
                    let values = complete.split_off(complete.len() - n);
                    complete.push(JsValue::Array(values));
                }
                Task::Object(keys) => {
                    let values = complete.split_off(complete.len() - keys.len());
                    complete.push(JsValue::Object(keys.into_iter().zip(values).collect()));
                }
            }
        }
        complete.pop().expect("one converted value")
    }
    pub fn structurally_equal(&self, other: &Self) -> bool {
        let mut pending = vec![(self, other)];
        while let Some((a, b)) = pending.pop() {
            match (a, b) {
                (Self::Null, Self::Null) => {}
                (Self::Bool(a), Self::Bool(b)) if a == b => {}
                (Self::Number(a), Self::Number(b)) if a == b => {}
                (Self::String(a), Self::String(b)) if a == b => {}
                (Self::Array(a), Self::Array(b)) if a.len() == b.len() => {
                    pending.extend(a.iter().zip(b))
                }
                (Self::Object(a), Self::Object(b)) if a.len() == b.len() => {
                    let values: std::collections::HashMap<&[u16], &Value> = b
                        .iter()
                        .map(|(key, value)| (key.as_utf16(), value))
                        .collect();
                    for (key, value) in a {
                        match values.get(key.as_utf16()) {
                            Some(&other) => pending.push((value, other)),
                            None => return false,
                        }
                    }
                }
                _ => return false,
            }
        }
        true
    }
}
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.structurally_equal(other)
    }
}
impl PartialEq<bool> for Value {
    fn eq(&self, other: &bool) -> bool {
        self.as_bool() == Some(*other)
    }
}
impl PartialEq<str> for Value {
    fn eq(&self, other: &str) -> bool {
        self.as_utf16()
            .is_some_and(|v| v.iter().copied().eq(other.encode_utf16()))
    }
}
impl PartialEq<JsString> for Value {
    fn eq(&self, other: &JsString) -> bool {
        self.as_utf16() == Some(other.as_utf16())
    }
}
impl PartialEq<&&str> for Value {
    fn eq(&self, other: &&&str) -> bool {
        self == **other
    }
}
impl PartialEq<String> for Value {
    fn eq(&self, other: &String) -> bool {
        self == other.as_str()
    }
}
impl PartialEq<&JsString> for Value {
    fn eq(&self, other: &&JsString) -> bool {
        self == *other
    }
}
impl PartialEq<&str> for Value {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}
pub trait ValueIndex {
    fn get(self, value: &Value) -> Option<&Value>;
    fn get_mut(self, value: &mut Value) -> Option<&mut Value>;
}
impl ValueIndex for &str {
    fn get(self, value: &Value) -> Option<&Value> {
        value.as_object()?.get(self)
    }
    fn get_mut(self, value: &mut Value) -> Option<&mut Value> {
        value.as_object_mut()?.get_mut(self)
    }
}
impl ValueIndex for usize {
    fn get(self, value: &Value) -> Option<&Value> {
        value.as_array()?.get(self)
    }
    fn get_mut(self, value: &mut Value) -> Option<&mut Value> {
        value.as_array_mut()?.get_mut(self)
    }
}
impl ValueIndex for &JsString {
    fn get(self, value: &Value) -> Option<&Value> {
        value.as_object()?.get_utf16(self.as_utf16())
    }
    fn get_mut(self, value: &mut Value) -> Option<&mut Value> {
        value.as_object_mut()?.get_utf16_mut(self.as_utf16())
    }
}
impl ValueIndex for &&str {
    fn get(self, value: &Value) -> Option<&Value> {
        value.get(*self)
    }
    fn get_mut(self, value: &mut Value) -> Option<&mut Value> {
        value.get_mut(*self)
    }
}
impl Index<&&str> for Value {
    type Output = Value;
    fn index(&self, key: &&str) -> &Self::Output {
        &self[*key]
    }
}
impl IndexMut<&&str> for Value {
    fn index_mut(&mut self, key: &&str) -> &mut Self::Output {
        &mut self[*key]
    }
}
impl ValueIndex for &String {
    fn get(self, value: &Value) -> Option<&Value> {
        value.get(self.as_str())
    }
    fn get_mut(self, value: &mut Value) -> Option<&mut Value> {
        value.get_mut(self.as_str())
    }
}
static NULL: Value = Value::Null;
impl Index<&str> for Value {
    type Output = Value;
    fn index(&self, key: &str) -> &Self::Output {
        self.get(key).unwrap_or(&NULL)
    }
}
impl IndexMut<&str> for Value {
    fn index_mut(&mut self, key: &str) -> &mut Self::Output {
        self.checked_index_mut(key)
            .expect("object index requires object or null")
    }
}
impl Index<usize> for Value {
    type Output = Value;
    fn index(&self, index: usize) -> &Self::Output {
        self.as_array().and_then(|a| a.get(index)).unwrap_or(&NULL)
    }
}
impl IndexMut<usize> for Value {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.checked_array_index_mut(index)
            .expect("array index must exist")
    }
}
impl Index<&JsString> for Value {
    type Output = Value;
    fn index(&self, key: &JsString) -> &Self::Output {
        self.get(key).unwrap_or(&NULL)
    }
}
impl IndexMut<&JsString> for Value {
    fn index_mut(&mut self, key: &JsString) -> &mut Self::Output {
        if self.is_null() {
            *self = Self::Object(Object::new());
        }
        self.as_object_mut()
            .expect("object index requires object or null")
            .entry(key.clone())
            .or_default()
    }
}
impl Index<&String> for Value {
    type Output = Value;
    fn index(&self, key: &String) -> &Self::Output {
        &self[key.as_str()]
    }
}
impl IndexMut<&String> for Value {
    fn index_mut(&mut self, key: &String) -> &mut Self::Output {
        &mut self[key.as_str()]
    }
}
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.stringify().map_err(|_| fmt::Error)?)
    }
}

/// Direct construction: expression leaves implement ToValue, never a serde JSON round trip.
#[macro_export]
macro_rules! native_json {
    (null) => { $crate::compat::value::Value::Null };
    ([]) => { $crate::compat::value::Value::Array(Vec::new()) };
    ({}) => { $crate::compat::value::Value::Object($crate::compat::value::Object::new()) };
    ([$($tokens:tt)*]) => {{
        let mut values = Vec::new();
        $crate::native_json!(@array values; $($tokens)*);
        $crate::compat::value::Value::Array(values)
    }};
    ({$($tokens:tt)*}) => {{
        let mut values = $crate::compat::value::Object::new();
        $crate::native_json!(@object values; $($tokens)*);
        $crate::compat::value::Value::Object(values)
    }};
    (@array $out:ident;) => {};
    (@array $out:ident; null $(,$($rest:tt)*)?) => {
        $out.push($crate::native_json!(null)); $crate::native_json!(@array $out; $($($rest)*)?);
    };
    (@array $out:ident; [$($value:tt)*] $(,$($rest:tt)*)?) => {
        $out.push($crate::native_json!([$($value)*])); $crate::native_json!(@array $out; $($($rest)*)?);
    };
    (@array $out:ident; {$($value:tt)*} $(,$($rest:tt)*)?) => {
        $out.push($crate::native_json!({$($value)*})); $crate::native_json!(@array $out; $($($rest)*)?);
    };
    (@array $out:ident; $value:expr $(,$($rest:tt)*)?) => {
        $out.push($crate::compat::value::ToValue::to_value(&$value));
        $crate::native_json!(@array $out; $($($rest)*)?);
    };
    (@object $out:ident;) => {};
    (@object $out:ident; $key:tt : null $(,$($rest:tt)*)?) => {
        $out.insert(($key).into(), $crate::native_json!(null));
        $crate::native_json!(@object $out; $($($rest)*)?);
    };
    (@object $out:ident; $key:tt : [$($value:tt)*] $(,$($rest:tt)*)?) => {
        $out.insert(($key).into(), $crate::native_json!([$($value)*]));
        $crate::native_json!(@object $out; $($($rest)*)?);
    };
    (@object $out:ident; $key:tt : {$($value:tt)*} $(,$($rest:tt)*)?) => {
        $out.insert(($key).into(), $crate::native_json!({$($value)*}));
        $crate::native_json!(@object $out; $($($rest)*)?);
    };
    (@object $out:ident; $key:tt : $value:expr $(,$($rest:tt)*)?) => {
        $out.insert(($key).into(), $crate::compat::value::ToValue::to_value(&$value));
        $crate::native_json!(@object $out; $($($rest)*)?);
    };
    ($value:expr) => { $crate::compat::value::ToValue::to_value(&$value) };
}
pub use crate::native_json as json;
