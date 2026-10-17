//! Source-compatible validation for the bundled schema dialect, on authoritative JS values.
//! Schemas are trusted serde metadata; instance strings, keys and unknown records never are.
use super::{
    js_json::{JsValue, from_serde},
    js_value_access::checked_utf16,
};
use crate::error::{NumeraError, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

fn unsupported() -> NumeraError {
    NumeraError::new(
        "unsupported_schema",
        "The bundled JSON schema uses an unsupported or invalid rule.",
    )
}

fn invalid() -> NumeraError {
    NumeraError::new(
        "invalid_json_schema",
        "The JSON value does not match the bundled schema.",
    )
}

pub struct Schema<'a> {
    root: &'a Value,
}

fn reference<'a>(root: &'a Value, schema: &'a Value) -> Result<Option<&'a Value>> {
    let Some(reference) = schema.get("$ref") else {
        return Ok(None);
    };
    let reference = reference.as_str().ok_or_else(unsupported)?;
    let pointer = reference
        .strip_prefix('#')
        .filter(|p| p.starts_with('/'))
        .ok_or_else(unsupported)?;
    root.pointer(pointer).map(Some).ok_or_else(unsupported)
}

fn schema_children<'a>(root: &'a Value, schema: &'a Value) -> Result<Vec<&'a Value>> {
    let mut children = Vec::new();
    if let Some(value) = reference(root, schema)? {
        children.push(value);
    }
    for keyword in ["properties", "$defs"] {
        if let Some(value) = schema.get(keyword) {
            children.extend(value.as_object().ok_or_else(unsupported)?.values());
        }
    }
    for keyword in ["items", "additionalProperties", "propertyNames"] {
        if let Some(value) = schema.get(keyword) {
            children.push(value);
        }
    }
    for keyword in ["anyOf", "oneOf"] {
        if let Some(value) = schema.get(keyword) {
            let values = value
                .as_array()
                .filter(|v| !v.is_empty())
                .ok_or_else(unsupported)?;
            children.extend(values);
        }
    }
    Ok(children)
}

fn supported(schema: &Value) -> Result<()> {
    if schema.is_boolean() {
        return Ok(());
    }
    let object = schema.as_object().ok_or_else(unsupported)?;
    const KEYS: &[&str] = &[
        "$schema",
        "$ref",
        "$defs",
        "type",
        "properties",
        "propertyNames",
        "additionalProperties",
        "required",
        "items",
        "anyOf",
        "oneOf",
        "enum",
        "const",
        "default",
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "minLength",
        "maxLength",
        "minItems",
        "maxItems",
        "format",
        "pattern",
    ];
    if object.keys().any(|key| !KEYS.contains(&key.as_str())) {
        return Err(unsupported());
    }
    if let Some(dialect) = object.get("$schema")
        && dialect.as_str() != Some("https://json-schema.org/draft/2020-12/schema")
    {
        return Err(unsupported());
    }
    // Bundled refs have no assertion/default siblings. Default merging for refs would otherwise
    // require a defined precedence; fail closed on such schema drift rather than drop siblings.
    if object.contains_key("$ref")
        && object
            .keys()
            .any(|k| !["$ref", "$schema", "$defs"].contains(&k.as_str()))
    {
        return Err(unsupported());
    }
    if let Some(kind) = object.get("type")
        && !kind.as_str().is_some_and(|k| {
            [
                "null", "boolean", "number", "integer", "string", "object", "array",
            ]
            .contains(&k)
        })
    {
        return Err(unsupported());
    }
    for key in ["minLength", "maxLength", "minItems", "maxItems"] {
        if let Some(bound) = object.get(key)
            && bound.as_u64().is_none()
        {
            return Err(unsupported());
        }
    }
    for key in ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"] {
        if let Some(bound) = object.get(key)
            && bound.as_f64().is_none_or(|n| !n.is_finite())
        {
            return Err(unsupported());
        }
    }
    if let Some(values) = object.get("required")
        && !values
            .as_array()
            .is_some_and(|a| a.iter().all(Value::is_string))
    {
        return Err(unsupported());
    }
    if let Some(values) = object.get("enum")
        && !values.as_array().is_some_and(|a| !a.is_empty())
    {
        return Err(unsupported());
    }
    if let Some(pattern) = object.get("pattern")
        && !pattern.as_str().is_some_and(|p| {
            [
                "\\S",
                "^\\/.*",
                "^[a-zA-Z0-9_-]{1,64}$",
                "^[a-zA-Z0-9_-]{1,100}$",
            ]
            .contains(&p)
        })
    {
        return Err(unsupported());
    }
    if let Some(format) = object.get("format")
        && !format
            .as_str()
            .is_some_and(|f| ["uri", "uuid", "starts_with"].contains(&f))
    {
        return Err(unsupported());
    }
    if object.get("format").and_then(Value::as_str) == Some("starts_with")
        && object.get("pattern").and_then(Value::as_str) != Some("^\\/.*")
    {
        return Err(unsupported());
    }
    // Property-name rules in these bundles are string constraints, never applicator trees.
    if let Some(names) = object.get("propertyNames")
        && !names.as_object().is_some_and(|o| {
            o.keys().all(|k| {
                [
                    "type",
                    "pattern",
                    "minLength",
                    "maxLength",
                    "format",
                    "enum",
                    "const",
                ]
                .contains(&k.as_str())
            })
        })
    {
        return Err(unsupported());
    }
    Ok(())
}

impl<'a> Schema<'a> {
    /// Checks every schema rule up front, including local refs and unreachable branches.
    /// This is deliberately the bundled dialect, not a general external JSON Schema engine.
    pub fn compile(root: &'a Value) -> Result<Self> {
        enum Task<'a> {
            Enter(&'a Value),
            Exit(usize),
        }
        let mut pending = vec![Task::Enter(root)];
        let mut active = HashSet::new();
        let mut visited = HashSet::new();
        while let Some(task) = pending.pop() {
            match task {
                Task::Enter(schema) => {
                    let id = schema as *const Value as usize;
                    if active.contains(&id) {
                        return Err(unsupported());
                    }
                    if visited.contains(&id) {
                        continue;
                    }
                    supported(schema)?;
                    active.insert(id);
                    pending.push(Task::Exit(id));
                    pending.extend(schema_children(root, schema)?.into_iter().map(Task::Enter));
                }
                Task::Exit(id) => {
                    active.remove(&id);
                    visited.insert(id);
                }
            }
        }
        // Current unions contain assertions only. Reject branch-default drift instead of
        // inventing ambiguous union-default precedence or cloning untrusted instances.
        let mut pending = vec![(root, false)];
        let mut visited = HashSet::new();
        while let Some((schema, union)) = pending.pop() {
            if !visited.insert((schema as *const Value as usize, union)) {
                continue;
            }
            if union && schema.get("default").is_some() {
                return Err(unsupported());
            }
            for child in schema_children(root, schema)? {
                let combinator = ["anyOf", "oneOf"].iter().any(|k| {
                    schema
                        .get(*k)
                        .and_then(Value::as_array)
                        .is_some_and(|a| a.iter().any(|v| std::ptr::eq(v, child)))
                });
                pending.push((child, union || combinator));
            }
        }
        Ok(Self { root })
    }

    pub fn validate(&self, value: &JsValue) -> Result<()> {
        if self.matches(value, self.root) {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    /// Applies only declared property defaults before validation. On failure, defaults remain;
    /// no unknown record content is normalized, converted, rewritten or discarded.
    pub fn default_and_validate(&self, value: &mut JsValue) -> Result<()> {
        let mut pending = vec![(&mut *value, self.root)];
        while let Some((value, schema)) = pending.pop() {
            if let Some(target) = reference(self.root, schema)? {
                pending.push((value, target));
                continue;
            }
            match value {
                JsValue::Object(entries) => {
                    let properties = schema.get("properties").and_then(Value::as_object);
                    if let Some(properties) = properties {
                        for (key, child) in properties {
                            if entries
                                .iter()
                                .any(|(k, _)| k.iter().copied().eq(key.encode_utf16()))
                            {
                                continue;
                            }
                            if let Some(default) = child.get("default") {
                                entries.push((key.encode_utf16().collect(), from_serde(default)?));
                            }
                        }
                    }
                    for (key, child_value) in entries {
                        let child = properties
                            .and_then(|p| {
                                p.iter()
                                    .find(|(k, _)| key.iter().copied().eq(k.encode_utf16()))
                                    .map(|(_, v)| v)
                            })
                            .or_else(|| schema.get("additionalProperties"));
                        if let Some(child) = child {
                            pending.push((child_value, child));
                        }
                    }
                }
                JsValue::Array(array) => {
                    if let Some(child) = schema.get("items") {
                        pending.extend(array.iter_mut().map(|v| (v, child)));
                    }
                }
                _ => {}
            }
        }
        self.validate(value)
    }

    fn matches(&self, value: &JsValue, schema: &Value) -> bool {
        enum Task<'a> {
            Value(&'a JsValue, &'a Value),
            Combine {
                base: bool,
                all: usize,
                any: usize,
                one: usize,
            },
        }
        let mut pending = vec![Task::Value(value, schema)];
        let mut complete = Vec::new();
        while let Some(task) = pending.pop() {
            match task {
                Task::Combine {
                    base,
                    all,
                    any,
                    one,
                } => {
                    let values = complete.split_off(complete.len() - all - any - one);
                    let valid = base
                        && values[..all].iter().all(|b| *b)
                        && (any == 0 || values[all..all + any].iter().any(|b| *b))
                        && (one == 0 || values[all + any..].iter().filter(|b| **b).count() == 1);
                    complete.push(valid);
                }
                Task::Value(value, schema) => {
                    if let Some(allowed) = schema.as_bool() {
                        complete.push(allowed);
                        continue;
                    }
                    let mut children = Vec::new();
                    let base = shallow(value, schema);
                    if let Ok(Some(target)) = reference(self.root, schema) {
                        children.push((value, target));
                    }
                    if let Some(entries) = value.entries() {
                        let effective: HashMap<&[u16], &JsValue> =
                            entries.iter().map(|(k, v)| (k.as_slice(), v)).collect();
                        let properties = schema.get("properties").and_then(Value::as_object);
                        for (key, child_value) in effective {
                            let child = properties
                                .and_then(|p| {
                                    p.iter()
                                        .find(|(k, _)| key.iter().copied().eq(k.encode_utf16()))
                                        .map(|(_, v)| v)
                                })
                                .or_else(|| schema.get("additionalProperties"));
                            if let Some(child) = child {
                                children.push((child_value, child));
                            }
                        }
                    } else if let Some(array) = value.as_array()
                        && let Some(child) = schema.get("items")
                    {
                        children.extend(array.iter().map(|v| (v, child)));
                    }
                    let all = children.len();
                    let any = schema
                        .get("anyOf")
                        .and_then(Value::as_array)
                        .map_or(0, |branches| {
                            children.extend(branches.iter().map(|s| (value, s)));
                            branches.len()
                        });
                    let one = schema
                        .get("oneOf")
                        .and_then(Value::as_array)
                        .map_or(0, |branches| {
                            children.extend(branches.iter().map(|s| (value, s)));
                            branches.len()
                        });
                    pending.push(Task::Combine {
                        base,
                        all,
                        any,
                        one,
                    });
                    pending.extend(children.into_iter().rev().map(|(v, s)| Task::Value(v, s)));
                }
            }
        }
        complete.pop().unwrap_or(false)
    }
}

fn js_whitespace(unit: u16) -> bool {
    matches!(
        unit,
        0x9..=0xd | 0x20 | 0xa0 | 0x1680 | 0x2000..=0x200a | 0x2028 | 0x2029
            | 0x202f | 0x205f | 0x3000 | 0xfeff
    )
}

fn string_matches(units: &[u16], schema: &Value) -> bool {
    if schema
        .get("minLength")
        .and_then(Value::as_u64)
        .is_some_and(|n| (units.len() as u64) < n)
        || schema
            .get("maxLength")
            .and_then(Value::as_u64)
            .is_some_and(|n| (units.len() as u64) > n)
    {
        return false;
    }
    if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
        let valid = match pattern {
            "\\S" => units.iter().any(|u| !js_whitespace(*u)),
            "^\\/.*" => units.first() == Some(&47),
            "^[a-zA-Z0-9_-]{1,64}$" | "^[a-zA-Z0-9_-]{1,100}$" => {
                let max = if pattern.contains("64") { 64 } else { 100 };
                !units.is_empty()
                    && units.len() <= max
                    && units
                        .iter()
                        .all(|u| matches!(*u, 45 | 48..=57 | 65..=90 | 95 | 97..=122))
            }
            _ => false,
        };
        if !valid {
            return false;
        }
    }
    match schema.get("format").and_then(Value::as_str) {
        // WHATWG URL/URL.canParse take USVString. This disposable validation view is never
        // written into the authoritative instance, and never used by hashing or JSON output.
        Some("uri") => {
            let start = units
                .iter()
                .position(|u| !js_whitespace(*u))
                .unwrap_or(units.len());
            let end = units
                .iter()
                .rposition(|u| !js_whitespace(*u))
                .map_or(start, |i| i + 1);
            url::Url::parse(&String::from_utf16_lossy(&units[start..end])).is_ok()
        }
        Some("uuid") => checked_utf16(units).is_ok_and(|s| {
            let bytes = s.as_bytes();
            if bytes.len() != 36 || uuid::Uuid::parse_str(&s).is_err() {
                return false;
            }
            s == "00000000-0000-0000-0000-000000000000"
                || s == "ffffffff-ffff-ffff-ffff-ffffffffffff"
                || (matches!(bytes[14], b'1'..=b'8')
                    && matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B'))
        }),
        _ => true,
    }
}

fn shallow(value: &JsValue, schema: &Value) -> bool {
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        let valid = match kind {
            "null" => value.is_null(),
            "boolean" => value.is_boolean(),
            "number" => value.as_f64().is_some_and(f64::is_finite),
            // Zod4 int requires safe integers even when generated JSON Schema omits bounds.
            "integer" => value.as_f64().is_some_and(|n| {
                n.fract() == 0.0 && (-9007199254740991.0..=9007199254740991.0).contains(&n)
            }),
            "string" => value.is_string(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            _ => false,
        };
        if !valid {
            return false;
        }
    }
    if let Some(expected) = schema.get("const")
        && !from_serde(expected).is_ok_and(|v| value.structurally_equal(&v))
    {
        return false;
    }
    if let Some(expected) = schema.get("enum").and_then(Value::as_array)
        && !expected
            .iter()
            .any(|v| from_serde(v).is_ok_and(|v| value.structurally_equal(&v)))
    {
        return false;
    }
    if let Some(number) = value.as_f64() {
        for (key, exclusive, minimum) in [
            ("minimum", false, true),
            ("maximum", false, false),
            ("exclusiveMinimum", true, true),
            ("exclusiveMaximum", true, false),
        ] {
            if let Some(bound) = schema.get(key).and_then(Value::as_f64) {
                let valid = match (minimum, exclusive) {
                    (true, true) => number > bound,
                    (true, false) => number >= bound,
                    (false, true) => number < bound,
                    (false, false) => number <= bound,
                };
                if !valid {
                    return false;
                }
            }
        }
    }
    if let Some(units) = value.as_utf16()
        && !string_matches(units, schema)
    {
        return false;
    }
    if let Some(array) = value.as_array()
        && (schema
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|n| (array.len() as u64) < n)
            || schema
                .get("maxItems")
                .and_then(Value::as_u64)
                .is_some_and(|n| (array.len() as u64) > n))
    {
        return false;
    }
    if let Some(entries) = value.entries() {
        if schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|required| {
                required
                    .iter()
                    .any(|k| k.as_str().is_none_or(|k| value.get(k).is_none()))
            })
        {
            return false;
        }
        if let Some(names) = schema.get("propertyNames") {
            for (key, _) in entries {
                if !shallow(&JsValue::String(key.clone()), names) {
                    return false;
                }
            }
        }
    }
    true
}
