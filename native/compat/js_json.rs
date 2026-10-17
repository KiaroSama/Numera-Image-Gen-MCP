//! JSON.parse / JSON.stringify semantics without losing binary64 or UTF-16 values.
//! serde_json::Value cannot represent infinity or unpaired UTF-16 surrogates.
use super::hash::sha256;
use crate::error::{NumeraError, Result};
use icu_collator::{Collator, CollatorBorrowed, options::CollatorOptions};
use icu_locale::locale;
use std::{cmp::Ordering, collections::HashMap};

pub const MAX_JSON_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub enum JsValue {
    #[default]
    Null,
    Bool(bool),
    Number(f64),
    String(Vec<u16>),
    Array(Vec<JsValue>),
    /// Unique keys retain first insertion position and the last assigned value.
    Object(Vec<(Vec<u16>, JsValue)>),
}

impl std::fmt::Debug for JsValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Debug output must neither recurse on untrusted depth nor disclose payloads.
        f.write_str(match self {
            Self::Null => "JsValue::Null",
            Self::Bool(_) => "JsValue::Bool",
            Self::Number(_) => "JsValue::Number",
            Self::String(_) => "JsValue::String",
            Self::Array(_) => "JsValue::Array",
            Self::Object(_) => "JsValue::Object",
        })
    }
}

impl Drop for JsValue {
    fn drop(&mut self) {
        fn children(value: &mut JsValue, pending: &mut Vec<JsValue>) {
            match value {
                JsValue::Array(array) => pending.append(array),
                JsValue::Object(object) => pending.extend(object.drain(..).map(|(_, v)| v)),
                _ => {}
            }
        }
        // Leaves and drained containers return without allocating or recursively visiting children.
        let mut pending = Vec::new();
        children(self, &mut pending);
        while let Some(mut value) = pending.pop() {
            children(&mut value, &mut pending);
        }
    }
}

enum Build<'a> {
    Value(&'a JsValue),
    Array(usize),
    Object(Vec<Vec<u16>>),
}

impl Clone for JsValue {
    fn clone(&self) -> Self {
        let mut tasks = vec![Build::Value(self)];
        let mut complete = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Build::Value(Self::Array(array)) => {
                    tasks.push(Build::Array(array.len()));
                    tasks.extend(array.iter().rev().map(Build::Value));
                }
                Build::Value(Self::Object(object)) => {
                    tasks.push(Build::Object(
                        object.iter().map(|(k, _)| k.clone()).collect(),
                    ));
                    tasks.extend(object.iter().rev().map(|(_, v)| Build::Value(v)));
                }
                Build::Value(value) => complete.push(match value {
                    Self::Null => Self::Null,
                    Self::Bool(b) => Self::Bool(*b),
                    Self::Number(n) => Self::Number(*n),
                    Self::String(s) => Self::String(s.clone()),
                    _ => unreachable!("containers are assembled separately"),
                }),
                Build::Array(length) => {
                    let values = complete.split_off(complete.len() - length);
                    complete.push(Self::Array(values));
                }
                Build::Object(keys) => {
                    let values = complete.split_off(complete.len() - keys.len());
                    complete.push(Self::Object(keys.into_iter().zip(values).collect()));
                }
            }
        }
        complete.pop().expect("one completed clone")
    }
}

fn invalid() -> NumeraError {
    NumeraError::new("invalid_json", "Cannot parse JSON text.")
}

#[derive(Clone, Copy)]
enum State {
    Initial,
    Key,
    Value,
    After,
}

struct Frame {
    value: JsValue,
    state: State,
    key: Vec<u16>,
    positions: HashMap<Vec<u16>, usize>,
}

impl Frame {
    fn new(object: bool) -> Self {
        Self {
            value: if object {
                JsValue::Object(Vec::new())
            } else {
                JsValue::Array(Vec::new())
            },
            state: State::Initial,
            key: Vec::new(),
            positions: HashMap::new(),
        }
    }

    fn insert(&mut self, value: JsValue) {
        match &mut self.value {
            JsValue::Array(array) => array.push(value),
            JsValue::Object(object) => {
                let key = std::mem::take(&mut self.key);
                if let Some(&position) = self.positions.get(&key) {
                    object[position].1 = value;
                } else {
                    self.positions.insert(key.clone(), object.len());
                    object.push((key, value));
                }
            }
            _ => unreachable!("parse frames are containers"),
        }
        self.state = State::After;
    }
}

struct Parser<'a> {
    text: &'a str,
    position: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.position).copied()
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.position += 1;
        }
    }

    fn consume(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn string(&mut self) -> Result<Vec<u16>> {
        if !self.consume(b'"') {
            return Err(invalid());
        }
        let mut units = Vec::new();
        loop {
            let start = self.position;
            while self
                .peek()
                .is_some_and(|b| b >= 0x20 && b != b'"' && b != b'\\')
            {
                self.position += 1;
            }
            // ASCII delimiters cannot split the UTF-8 input's scalar sequences.
            units.extend(self.text[start..self.position].encode_utf16());
            if self.consume(b'"') {
                return Ok(units);
            }
            if !self.consume(b'\\') {
                return Err(invalid());
            }
            let escape = self.peek().ok_or_else(invalid)?;
            self.position += 1;
            units.push(match escape {
                b'"' => 0x22,
                b'\\' => 0x5c,
                b'/' => 0x2f,
                b'b' => 8,
                b'f' => 12,
                b'n' => 10,
                b'r' => 13,
                b't' => 9,
                b'u' => {
                    let mut unit = 0u16;
                    for _ in 0..4 {
                        let digit = match self.peek().ok_or_else(invalid)? {
                            b'0'..=b'9' => self.text.as_bytes()[self.position] - b'0',
                            b'a'..=b'f' => self.text.as_bytes()[self.position] - b'a' + 10,
                            b'A'..=b'F' => self.text.as_bytes()[self.position] - b'A' + 10,
                            _ => return Err(invalid()),
                        };
                        unit = (unit << 4) | u16::from(digit);
                        self.position += 1;
                    }
                    unit
                }
                _ => return Err(invalid()),
            });
        }
    }

    fn digits(&mut self) -> bool {
        let start = self.position;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.position += 1;
        }
        self.position != start
    }

    fn number(&mut self) -> Result<JsValue> {
        let start = self.position;
        self.consume(b'-');
        if !self.consume(b'0') {
            if !matches!(self.peek(), Some(b'1'..=b'9')) {
                return Err(invalid());
            }
            self.digits();
        }
        if self.consume(b'.') && !self.digits() {
            return Err(invalid());
        }
        if self.consume(b'e') || self.consume(b'E') {
            if !self.consume(b'+') {
                self.consume(b'-');
            }
            if !self.digits() {
                return Err(invalid());
            }
        }
        let number = self.text[start..self.position]
            .parse::<f64>()
            .map_err(|_| invalid())?;
        Ok(JsValue::Number(number))
    }

    fn atom(&mut self) -> Result<JsValue> {
        match self.peek() {
            Some(b'"') => self.string().map(JsValue::String),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b't' | b'f' | b'n') => {
                let (literal, value) = match self.peek() {
                    Some(b't') => ("true", JsValue::Bool(true)),
                    Some(b'f') => ("false", JsValue::Bool(false)),
                    _ => ("null", JsValue::Null),
                };
                if !self.text[self.position..].starts_with(literal) {
                    return Err(invalid());
                }
                self.position += literal.len();
                Ok(value)
            }
            _ => Err(invalid()),
        }
    }
}

/// Strict JSON syntax, bounded before parsing. No call-stack growth on success or failure.
pub fn parse(text: &str) -> Result<JsValue> {
    if text.len() > MAX_JSON_BYTES {
        return Err(NumeraError::new("invalid_json", "JSON text exceeds 1 MiB."));
    }
    parse_bounded(text, MAX_JSON_BYTES)
}

/// Different trust boundaries have different byte ceilings (config versus image-bearing MCP).
pub fn parse_bounded(text: &str, limit: usize) -> Result<JsValue> {
    if text.len() > limit {
        return Err(NumeraError::new(
            "invalid_json",
            "JSON text exceeds its byte limit.",
        ));
    }
    let mut parser = Parser { text, position: 0 };
    let mut frames: Vec<Frame> = Vec::new();
    let mut complete = None;
    loop {
        parser.whitespace();
        if let Some(value) = complete.take() {
            if let Some(frame) = frames.last_mut() {
                frame.insert(value);
            } else {
                return if parser.position == text.len() {
                    Ok(value)
                } else {
                    Err(invalid())
                };
            }
            continue;
        }
        if let Some(frame) = frames.last_mut() {
            let object = matches!(&frame.value, JsValue::Object(_));
            let close = if object { b'}' } else { b']' };
            match frame.state {
                State::Initial if parser.consume(close) => {
                    complete = frames.pop().map(|f| f.value);
                    continue;
                }
                State::Initial | State::Key if object => {
                    frame.key = parser.string()?;
                    parser.whitespace();
                    if !parser.consume(b':') {
                        return Err(invalid());
                    }
                    frame.state = State::Value;
                    continue;
                }
                State::After => {
                    if parser.consume(close) {
                        complete = frames.pop().map(|f| f.value);
                    } else if parser.consume(b',') {
                        frame.state = if object { State::Key } else { State::Value };
                    } else {
                        return Err(invalid());
                    }
                    continue;
                }
                _ => frame.state = State::Value,
            }
        }
        match parser.peek() {
            Some(b'[' | b'{') => {
                let object = parser.consume(b'{');
                if !object {
                    parser.position += 1;
                }
                frames.push(Frame::new(object));
            }
            _ => complete = Some(parser.atom()?),
        }
    }
}

fn array_index(key: &[u16]) -> Option<u32> {
    if key.is_empty() || (key.len() > 1 && key[0] == u16::from(b'0')) {
        return None;
    }
    let mut index = 0u32;
    for &unit in key {
        if !(u16::from(b'0')..=u16::from(b'9')).contains(&unit) {
            return None;
        }
        index = index
            .checked_mul(10)?
            .checked_add(u32::from(unit - u16::from(b'0')))?;
    }
    (index != u32::MAX).then_some(index)
}

fn index_order(a: &[u16], b: &[u16]) -> Ordering {
    match (array_index(a), array_index(b)) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn write_string(units: &[u16], output: &mut String) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    fn escaped(unit: u16, output: &mut String) {
        output.push_str("\\u");
        for shift in [12, 8, 4, 0] {
            output.push(char::from(HEX[usize::from((unit >> shift) & 15)]));
        }
    }
    output.push('"');
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok('"') => output.push_str("\\\""),
            Ok('\\') => output.push_str("\\\\"),
            Ok('\u{8}') => output.push_str("\\b"),
            Ok('\u{c}') => output.push_str("\\f"),
            Ok('\n') => output.push_str("\\n"),
            Ok('\r') => output.push_str("\\r"),
            Ok('\t') => output.push_str("\\t"),
            Ok(c) if c < '\u{20}' => escaped(c as u16, output),
            Ok(c) => output.push(c),
            Err(e) => escaped(e.unpaired_surrogate(), output),
        }
    }
    output.push('"');
}

fn serialize(value: &JsValue, collator: Option<&CollatorBorrowed<'_>>) -> String {
    enum Token<'a> {
        Value(&'a JsValue),
        String(&'a [u16]),
        Character(char),
    }
    let mut output = String::new();
    let mut pending = vec![Token::Value(value)];
    while let Some(token) = pending.pop() {
        match token {
            Token::Character(c) => output.push(c),
            Token::String(s) => write_string(s, &mut output),
            Token::Value(JsValue::Null) => output.push_str("null"),
            Token::Value(JsValue::Bool(b)) => output.push_str(if *b { "true" } else { "false" }),
            Token::Value(JsValue::Number(n)) => {
                if !n.is_finite() {
                    output.push_str("null");
                } else if *n == 0.0 {
                    output.push('0');
                } else {
                    output.push_str(ryu_js::Buffer::new().format_finite(*n));
                }
            }
            Token::Value(JsValue::String(s)) => write_string(s, &mut output),
            Token::Value(JsValue::Array(array)) => {
                output.push('[');
                pending.push(Token::Character(']'));
                for (i, value) in array.iter().enumerate().rev() {
                    pending.push(Token::Value(value));
                    if i != 0 {
                        pending.push(Token::Character(','));
                    }
                }
            }
            Token::Value(JsValue::Object(object)) => {
                let mut entries: Vec<(&[u16], &JsValue)> = Vec::with_capacity(object.len());
                let mut positions: HashMap<&[u16], usize> = HashMap::new();
                for (key, value) in object {
                    if let Some(&position) = positions.get(key.as_slice()) {
                        entries[position] = (key, value);
                    } else {
                        positions.insert(key.as_slice(), entries.len());
                        entries.push((key, value));
                    }
                }
                // Object.entries enumerates index keys first before the stable English sort.
                entries.sort_by(|(a, _), (b, _)| index_order(a, b));
                if let Some(collator) = collator {
                    entries.sort_by(|(a, _), (b, _)| collator.compare_utf16(a, b));
                    // Object.fromEntries / JSON.stringify enumerate indices first again.
                    entries.sort_by(|(a, _), (b, _)| index_order(a, b));
                }
                output.push('{');
                pending.push(Token::Character('}'));
                for (i, (key, value)) in entries.into_iter().enumerate().rev() {
                    pending.push(Token::Value(value));
                    pending.push(Token::Character(':'));
                    pending.push(Token::String(key));
                    if i != 0 {
                        pending.push(Token::Character(','));
                    }
                }
            }
        }
    }
    output
}

pub fn stringify(value: &JsValue) -> Result<String> {
    Ok(serialize(value, None))
}

pub fn canonical_json(value: &JsValue) -> Result<String> {
    let collator = Collator::try_new(locale!("en").into(), CollatorOptions::default())
        .map_err(|_| NumeraError::runtime())?;
    Ok(serialize(value, Some(&collator)))
}

pub fn fingerprint(value: &JsValue) -> Result<String> {
    Ok(sha256(canonical_json(value)?.as_bytes()))
}

fn unrepresentable() -> NumeraError {
    NumeraError::new(
        "unrepresentable_json",
        "The JSON value cannot be represented without losing JavaScript semantics.",
    )
}

/// Converts serde's already-decoded subset; integer values are rounded to JS binary64.
pub fn from_serde(value: &serde_json::Value) -> Result<JsValue> {
    enum Task<'a> {
        Value(&'a serde_json::Value),
        Array(usize),
        Object(Vec<Vec<u16>>),
    }
    use serde_json::Value;
    let mut tasks = vec![Task::Value(value)];
    let mut complete = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Value(Value::Array(array)) => {
                tasks.push(Task::Array(array.len()));
                tasks.extend(array.iter().rev().map(Task::Value));
            }
            Task::Value(Value::Object(object)) => {
                tasks.push(Task::Object(
                    object.keys().map(|k| k.encode_utf16().collect()).collect(),
                ));
                tasks.extend(object.values().rev().map(Task::Value));
            }
            Task::Value(value) => complete.push(match value {
                Value::Null => JsValue::Null,
                Value::Bool(b) => JsValue::Bool(*b),
                Value::Number(n) => JsValue::Number(n.as_f64().ok_or_else(unrepresentable)?),
                Value::String(s) => JsValue::String(s.encode_utf16().collect()),
                _ => unreachable!("containers are assembled separately"),
            }),
            Task::Array(length) => {
                let values = complete.split_off(complete.len() - length);
                complete.push(JsValue::Array(values));
            }
            Task::Object(keys) => {
                let values = complete.split_off(complete.len() - keys.len());
                complete.push(JsValue::Object(keys.into_iter().zip(values).collect()));
            }
        }
    }
    complete.pop().ok_or_else(unrepresentable)
}

pub fn drop_serde(value: serde_json::Value) {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            serde_json::Value::Array(array) => pending.extend(array),
            serde_json::Value::Object(object) => {
                pending.extend(object.into_iter().map(|(_, value)| value))
            }
            _ => {}
        }
    }
}

/// Explicit fallible boundary, never a sentinel or a lossy replacement. The caller owns
/// serde's result and must use iterative clone/drop for deeply nested returned values.
pub fn to_serde(value: &JsValue) -> Result<serde_json::Value> {
    enum Task<'a> {
        Value(&'a JsValue),
        Array(usize),
        Object(Vec<String>),
    }
    use serde_json::Value;
    let mut tasks = vec![Task::Value(value)];
    let mut complete = Vec::new();
    let result = (|| {
        while let Some(task) = tasks.pop() {
            match task {
                Task::Value(JsValue::Array(array)) => {
                    tasks.push(Task::Array(array.len()));
                    tasks.extend(array.iter().rev().map(Task::Value));
                }
                Task::Value(JsValue::Object(object)) => {
                    let keys = object
                        .iter()
                        .map(|(k, _)| String::from_utf16(k))
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .map_err(|_| unrepresentable())?;
                    tasks.push(Task::Object(keys));
                    tasks.extend(object.iter().rev().map(|(_, v)| Task::Value(v)));
                }
                Task::Value(value) => complete.push(match value {
                    JsValue::Null => Value::Null,
                    JsValue::Bool(b) => Value::Bool(*b),
                    JsValue::Number(n) => {
                        Value::Number(serde_json::Number::from_f64(*n).ok_or_else(unrepresentable)?)
                    }
                    JsValue::String(s) => {
                        Value::String(String::from_utf16(s).map_err(|_| unrepresentable())?)
                    }
                    _ => unreachable!("containers are assembled separately"),
                }),
                Task::Array(length) => {
                    let values = complete.split_off(complete.len() - length);
                    complete.push(Value::Array(values));
                }
                Task::Object(keys) => {
                    let values = complete.split_off(complete.len() - keys.len());
                    let mut object = serde_json::Map::new();
                    for (key, value) in keys.into_iter().zip(values) {
                        if let Some(previous) = object.insert(key, value) {
                            drop_serde(previous);
                        }
                    }
                    complete.push(Value::Object(object));
                }
            }
        }
        complete.pop().ok_or_else(unrepresentable)
    })();
    // An error after a completed deep sibling must not recurse through serde's destructor.
    for value in complete {
        drop_serde(value);
    }
    result
}
