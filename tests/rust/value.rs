use numera_image_gen_mcp::compat::value::{JsString, Number, Object, Value};

#[test]
fn authoritative_values_keep_infinity_and_utf16_across_clone_wire_and_conversion() {
    let value = Value::parse(r#"{"n":1e400,"\ud800":"\udfff","pair":"🌍"}"#).expect("parse");
    assert_eq!(value["n"].as_f64(), Some(f64::INFINITY));
    assert_eq!(
        value
            .as_object()
            .expect("object")
            .get_utf16(&[0xd800])
            .and_then(Value::as_utf16),
        Some([0xdfff].as_slice())
    );
    assert!(value.to_serde().is_err());
    assert_eq!(
        value.stringify().expect("wire"),
        r#"{"n":null,"\ud800":"\udfff","pair":"🌍"}"#
    );
    assert!(value.structurally_equal(&value.clone()));
    assert_eq!(value["pair"].as_str(), Some("🌍"));
    assert!(
        value
            .as_object()
            .expect("object")
            .get_utf16(&[0xd800])
            .expect("value")
            .as_str()
            .is_none()
    );
}

#[test]
fn object_mutation_iteration_entry_and_index_keep_exact_first_insertion() {
    let mut object = Object::new();
    object.insert("b".into(), Value::Bool(true));
    object.insert("a".into(), Value::Null);
    object.insert("b".into(), Value::Bool(false));
    object.insert(
        JsString::from_utf16(vec![0xd800]),
        Value::Number(Number::from(7.0)),
    );
    assert_eq!(object.keys().next().expect("first").as_str(), Some("b"));
    assert_eq!(object.len(), 3);
    object.entry("a").or_insert(Value::Bool(true));
    object.entry("c").or_insert(Value::Bool(true));
    assert!(object.get("a").expect("existing").is_null());
    for value in object.values_mut() {
        if value.as_bool() == Some(false) {
            *value = Value::Bool(true);
        }
    }
    assert_eq!(object.remove("b").expect("removed").as_bool(), Some(true));
    let value = Value::Object(object);
    assert!(value["missing"].is_null());
    assert!(value[0].is_null());
    let keys: Vec<_> = value
        .as_object()
        .expect("object")
        .keys()
        .map(JsString::as_utf16)
        .collect();
    assert_eq!(keys[0], [97]);
    assert_eq!(keys[1], [0xd800]);
    assert_eq!(keys[2], [99]);
    let mut value = Value::Null;
    value["created"] = Value::Array(vec![Value::Null]);
    value["created"][0] = Value::Bool(true);
    assert_eq!(value["created"][0].as_bool(), Some(true));
    assert!(Value::Bool(false).checked_index_mut("a").is_err());
    assert!(Value::Array(Vec::new()).checked_array_index_mut(0).is_err());
}

#[test]
fn serde_raw_json_boundaries_preserve_lone_units_but_serialize_nonfinite_as_null() {
    let text = r#"{"text":"\ud800","key":{"\udfff":1},"n":1e400}"#;
    let value: Value = serde_json::from_str(text).expect("raw decode");
    assert_eq!(value["n"].as_f64(), Some(f64::INFINITY));
    assert_eq!(value["text"].as_utf16(), Some([0xd800].as_slice()));
    assert_eq!(
        serde_json::to_string(&value).expect("raw encode"),
        r#"{"text":"\ud800","key":{"\udfff":1},"n":null}"#
    );
    let metadata = serde_json::json!({"valid": [1, "🌍"]});
    let value = Value::from_serde(&metadata).expect("metadata");
    assert_eq!(
        value.to_serde().expect("representable"),
        serde_json::json!({"valid":[1.0,"🌍"]})
    );
}

#[test]
fn cached_strings_never_replace_lone_units_and_numbers_follow_js_equality() {
    let valid = JsString::from("🌍");
    assert_eq!(valid.as_str(), Some("🌍"));
    assert_eq!(valid.as_utf16(), [0xd83c, 0xdf0d]);
    assert_eq!(valid.checked_str().expect("valid"), "🌍");
    let invalid = JsString::from_utf16(vec![0xd800]);
    assert!(invalid.as_str().is_none());
    assert_eq!(
        invalid.checked_str().expect_err("no replacement").code,
        "unrepresentable_json"
    );
    assert!(
        Value::Number(Number::from(-0.0)).structurally_equal(&Value::Number(Number::from(0.0)))
    );
    assert!(
        !Value::Number(Number::from(f64::NAN))
            .structurally_equal(&Value::Number(Number::from(f64::NAN)))
    );
    assert!(Number::from(f64::INFINITY).as_u64().is_none());
    assert_eq!(Number::from(7u32).as_u64(), Some(7));
}

#[test]
fn direct_macro_preserves_nested_js_values_without_a_serde_projection() {
    use numera_image_gen_mcp::compat::value::json;
    let lone = JsString::from_utf16(vec![0xd800]);
    let infinity = f64::INFINITY;
    let original = Value::parse(r#"{"\udfff":"\ud800","n":-1e400}"#).expect("original");
    let value =
        json!({"n": infinity, "text": lone, "items": [null, true, original, {"n": infinity}]});
    assert_eq!(value["n"].as_f64(), Some(f64::INFINITY));
    assert_eq!(value["text"].as_utf16(), Some([0xd800].as_slice()));
    assert_eq!(value["items"][2]["n"].as_f64(), Some(f64::NEG_INFINITY));
    assert_eq!(value["items"][3]["n"].as_f64(), Some(f64::INFINITY));
    let object = json!({(lone.clone()): infinity});
    assert_eq!(
        object
            .as_object()
            .expect("object")
            .get_utf16(&[0xd800])
            .and_then(Value::as_f64),
        Some(f64::INFINITY)
    );
    let optional: Option<&str> = None;
    let values = vec![1u64, 2];
    assert_eq!(
        json!({"none": optional, "values": values})["values"][1].as_u64(),
        Some(2)
    );
}

#[test]
fn deep_facade_lifecycle_and_mutation_are_iterative() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let depth = 8192;
            let text = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
            let value = Value::parse(&text).expect("deep");
            let mut copy = value.clone();
            assert!(value.structurally_equal(&copy));
            let mut leaf = &mut copy;
            for _ in 0..depth {
                leaf = &mut leaf[0];
            }
            *leaf = Value::Bool(true);
            assert!(!value.structurally_equal(&copy));
            assert_eq!(value.stringify().expect("deep wire"), text);
        })
        .expect("owned thread")
        .join()
        .expect("iterative lifecycle");
}
