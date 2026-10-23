use numera_image_gen_mcp::compat::js_json::{JsValue, parse, stringify};
use numera_image_gen_mcp::compat::js_value_access::checked_utf16;

#[test]
fn exact_keys_support_surrogates_without_unicode_normalization() {
    let mut value = parse(r#"{"é":1,"é":2,"\ud800":3,"�":4,"🌍":5}"#).expect("object");
    assert_eq!(value.get("é").and_then(JsValue::as_f64), Some(1.0));
    assert_eq!(value.get("e\u{301}").and_then(JsValue::as_f64), Some(2.0));
    assert_eq!(
        value.get_utf16(&[0xd800]).and_then(JsValue::as_f64),
        Some(3.0)
    );
    assert_eq!(value.get("�").and_then(JsValue::as_f64), Some(4.0));
    assert_eq!(
        value.get_utf16(&[0xd83c, 0xdf0d]).and_then(JsValue::as_f64),
        Some(5.0)
    );
    *value.get_utf16_mut(&[0xd800]).expect("exact lone key") = JsValue::Bool(true);
    *value.get_mut("🌍").expect("astral key") = JsValue::Null;
    assert_eq!(
        value.get_utf16(&[0xd800]).and_then(JsValue::as_bool),
        Some(true)
    );
    assert!(value.get("🌍").expect("key retained").is_null());
    assert!(value.get("missing").is_none());
    assert!(value.get_mut("missing").is_none());
    assert!(value.get_utf16(&[0xd801]).is_none());
    assert!(value.get_utf16_mut(&[0xd801]).is_none());
}

#[test]
fn assignment_retains_first_position_and_removal_returns_last_effective_value() {
    let mut value = JsValue::Object(vec![
        (vec![97], JsValue::Number(1.0)),
        (vec![98], JsValue::Number(2.0)),
        (vec![97], JsValue::Number(3.0)),
    ]);
    assert_eq!(value.get("a").and_then(JsValue::as_f64), Some(3.0));
    assert_eq!(
        value
            .set("a", JsValue::Number(4.0))
            .expect("object")
            .expect("old value")
            .as_f64(),
        Some(3.0)
    );
    assert_eq!(stringify(&value).expect("assigned"), r#"{"a":4,"b":2}"#);
    assert_eq!(value.entries().expect("entries").len(), 2);
    assert!(
        value
            .set_utf16(vec![0xd800], JsValue::Bool(true))
            .expect("new key")
            .is_none()
    );
    assert_eq!(value.remove("a").expect("removed").as_f64(), Some(4.0));
    assert_eq!(
        value
            .remove_utf16(&[0xd800])
            .expect("removed lone")
            .as_bool(),
        Some(true)
    );
    assert!(value.remove("missing").is_none());
    assert_eq!(stringify(&value).expect("remaining"), r#"{"b":2}"#);
    let mut duplicate = JsValue::Object(vec![
        (vec![97], JsValue::Number(1.0)),
        (vec![98], JsValue::Null),
        (vec![97], JsValue::Number(9.0)),
    ]);
    assert_eq!(
        duplicate.remove("a").expect("last effective").as_f64(),
        Some(9.0)
    );
    assert_eq!(
        stringify(&duplicate).expect("all matches removed"),
        r#"{"b":null}"#
    );
    let mut scalar = JsValue::Number(1.0);
    assert!(scalar.get("a").is_none());
    assert!(scalar.get_mut("a").is_none());
    assert!(scalar.remove("a").is_none());
    assert_eq!(
        scalar.set("a", JsValue::Null).expect_err("not object").code,
        "invalid_json_type"
    );
}

#[test]
fn checked_strings_fail_explicitly_but_units_remain_authoritative() {
    assert_eq!(checked_utf16(&[0xd83c, 0xdf0d]).expect("pair"), "🌍");
    assert_eq!(
        JsValue::String(vec![0xd83c, 0xdf0d])
            .checked_string()
            .expect("string"),
        "🌍"
    );
    for units in [vec![0xd800], vec![0xdc00], vec![0xd800, 97]] {
        let value = JsValue::String(units.clone());
        assert_eq!(value.as_utf16(), Some(units.as_slice()));
        let error = value.checked_string().expect_err("no replacement");
        assert_eq!(error.code, "unrepresentable_json");
        assert!(!error.message.contains('�'));
    }
    assert_eq!(
        JsValue::Bool(true)
            .checked_string()
            .expect_err("not string")
            .code,
        "invalid_json_type"
    );
    assert!(JsValue::Null.as_utf16().is_none());
}

#[test]
fn scalar_access_never_coerces_and_integer_access_checks_bounds() {
    assert!(JsValue::Null.is_null());
    assert!(JsValue::Bool(false).is_boolean());
    assert!(JsValue::Number(f64::INFINITY).is_number());
    assert!(JsValue::String(vec![0xd800]).is_string());
    assert!(!JsValue::Null.is_boolean());
    assert!(!JsValue::Null.is_number());
    assert!(!JsValue::Null.is_string());
    assert_eq!(JsValue::Bool(false).as_bool(), Some(false));
    assert!(JsValue::Number(1.0).as_bool().is_none());
    assert!(JsValue::String(vec![49]).as_f64().is_none());
    assert_eq!(JsValue::Number(f64::INFINITY).as_f64(), Some(f64::INFINITY));
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.5] {
        assert!(JsValue::Number(number).as_i64().is_none());
        assert!(JsValue::Number(number).as_u64().is_none());
    }
    assert_eq!(JsValue::Number(-0.0).as_i64(), Some(0));
    assert_eq!(JsValue::Number(-0.0).as_u64(), Some(0));
    assert_eq!(JsValue::Number(-1.0).as_i64(), Some(-1));
    assert!(JsValue::Number(-1.0).as_u64().is_none());
    assert_eq!(
        JsValue::Number(-9223372036854775808.0).as_i64(),
        Some(i64::MIN)
    );
    assert!(JsValue::Number(9223372036854775808.0).as_i64().is_none());
    assert!(JsValue::Number(18446744073709551616.0).as_u64().is_none());
    assert_eq!(
        JsValue::Number(9007199254740992.0).as_u64(),
        Some(9007199254740992)
    );
}

#[test]
fn arrays_and_objects_expose_borrowed_values_without_hidden_conversion() {
    let mut value = parse(r#"[{"a":"\ud800"},2]"#).expect("array");
    assert!(value.is_array());
    assert_eq!(value.as_array().expect("array").len(), 2);
    assert!(value.at(0).expect("item").is_object());
    assert!(value.at(2).is_none());
    *value.at_mut(1).expect("mutable item") = JsValue::Bool(false);
    value
        .as_array_mut()
        .expect("mutable array")
        .push(JsValue::Null);
    let object = value.at_mut(0).expect("object");
    let entries = object.entries_mut().expect("mutable entries");
    entries[0].1 = JsValue::Number(7.0);
    assert_eq!(
        stringify(&value).expect("mutated"),
        r#"[{"a":7},false,null]"#
    );
    assert!(JsValue::Null.as_array().is_none());
    assert!(JsValue::Null.as_array_mut().is_none());
    assert!(JsValue::Null.at(0).is_none());
    assert!(JsValue::Null.at_mut(0).is_none());
    assert!(JsValue::Null.entries().is_none());
    assert!(JsValue::Null.entries_mut().is_none());
}

#[test]
fn structural_equality_ignores_object_order_but_preserves_types_units_and_array_order() {
    let left = parse(r#"{"a":1,"b":["\ud800",-0]}"#).expect("left");
    let right = parse(r#"{"b":["\ud800",0],"a":1}"#).expect("right");
    assert!(left.structurally_equal(&right));
    assert!(!left.structurally_equal(&parse(r#"{"a":1,"b":["�",0]}"#).expect("different units")));
    assert!(
        !parse("[1,2]")
            .expect("array")
            .structurally_equal(&parse("[2,1]").expect("array"))
    );
    assert!(!JsValue::Bool(true).structurally_equal(&JsValue::Number(1.0)));
    assert!(!JsValue::Number(f64::NAN).structurally_equal(&JsValue::Number(f64::NAN)));
    assert!(JsValue::Number(f64::INFINITY).structurally_equal(&JsValue::Number(f64::INFINITY)));
    assert!(
        !JsValue::Number(f64::INFINITY).structurally_equal(&JsValue::Number(f64::NEG_INFINITY))
    );
    let duplicate = JsValue::Object(vec![
        (vec![97], JsValue::Number(0.0)),
        (vec![97], JsValue::Number(1.0)),
    ]);
    assert!(duplicate.structurally_equal(&parse(r#"{"a":1}"#).expect("last effective")));
    assert!(!duplicate.structurally_equal(&parse(r#"{"a":1,"b":0}"#).expect("extra key")));
}

#[test]
fn deep_comparison_and_mutation_drop_are_iterative_and_owned() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let depth = 8192;
            let text = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
            let mut left = parse(&text).expect("deep left");
            let right = left.clone();
            assert!(left.structurally_equal(&right));
            let mut leaf = &mut left;
            for _ in 0..depth {
                leaf = leaf.at_mut(0).expect("nested item");
            }
            *leaf = JsValue::Number(1.0);
            assert!(!left.structurally_equal(&right));
            let mut object = JsValue::Object(vec![(vec![97], left), (vec![97], right)]);
            let replaced = object.set("a", JsValue::Null).expect("deep replacement");
            drop(replaced);
            assert!(object.get("a").expect("replacement").is_null());
        })
        .expect("owned bounded thread")
        .join()
        .expect("no stack overflow");
}
