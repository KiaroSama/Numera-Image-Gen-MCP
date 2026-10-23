use numera_image_gen_mcp::compat::js_json::{
    JsValue, MAX_JSON_BYTES, canonical_json, fingerprint, from_serde, parse, stringify, to_serde,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Oracle {
    name: String,
    text: String,
    serialized: String,
    canonical: String,
    sha256: String,
}

#[test]
fn raw_source_json_preserves_binary64_utf16_and_object_identity() {
    let fixture =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/raw-json.json");
    let bytes = std::fs::read(fixture).expect("CI must generate the raw source JSON oracle");
    // Only the outer envelope is decoded by serde: its text fields escape lone surrogates.
    let cases: Vec<Oracle> = serde_json::from_slice(&bytes).expect("valid oracle envelope");
    let expected = [
        "overflow-values",
        "lone-surrogate-value",
        "lone-surrogate-key",
        "paired-surrogate",
        "duplicate-keys",
        "binary64-rounded-integer",
    ];
    assert_eq!(
        cases.len(),
        expected.len(),
        "all raw source cases are required"
    );
    for (case, name) in cases.iter().zip(expected) {
        assert_eq!(case.name, name);
        let value = parse(&case.text).expect("source JSON must parse without domain loss");
        assert_eq!(
            stringify(&value).expect("serialize"),
            case.serialized,
            "{name}"
        );
        assert_eq!(
            canonical_json(&value).expect("canonical"),
            case.canonical,
            "{name}"
        );
        assert_eq!(
            fingerprint(&value).expect("fingerprint"),
            case.sha256,
            "{name}"
        );
    }
}

#[test]
fn strict_json_rejects_malformed_syntax_and_non_json_whitespace() {
    for text in [
        "",
        " ",
        "[",
        "{",
        "[1,]",
        "{\"a\":1,}",
        "{a:1}",
        "{\"a\" 1}",
        "[1 2]",
        "[1}",
        "{\"a\":1]",
        "true false",
        "nullx",
        "NaN",
        "Infinity",
        "undefined",
        "+1",
        "01",
        "-01",
        "-",
        "1.",
        ".1",
        "1e",
        "1e+",
        "1e-",
        "\"\\x00\"",
        "\"\\u123\"",
        "\"\\uZZZZ\"",
        "\"\\\"",
        "\"unterminated",
        "\"\n\"",
        "\"\u{0}\"",
        "\u{feff}null",
        "\u{a0}null",
        "null//comment",
        "/*x*/null",
    ] {
        let error = parse(text).expect_err("strict JSON syntax must fail");
        assert_eq!(error.code, "invalid_json");
        assert_eq!(error.message, "Cannot parse JSON text.");
    }
    assert_eq!(
        stringify(&parse(" \t\r\n[null,true,false,{},[]]\n").expect("valid")).expect("serialize"),
        "[null,true,false,{},[]]"
    );
}

#[test]
fn utf16_strings_escape_only_controls_and_unpaired_surrogates() {
    let input = r#"["\"\\\/\b\f\n\r\t\u0000\u001f","\uD800x\uDC00","\uDC00\uD800","\ud83c\udf0d","🌍é\u2028\u2029"]"#;
    let expected = concat!(
        r#"["\"\\/\b\f\n\r\t\u0000\u001f","\ud800x\udc00","\udc00\ud800","🌍","🌍é"#,
        "\u{2028}\u{2029}",
        "\"]"
    );
    let value = parse(input).expect("all UTF-16 code units are valid JSON string content");
    assert_eq!(stringify(&value).expect("serialize"), expected);
    let lone = parse(r#""\ud800""#).expect("lone surrogate");
    assert!(matches!(&lone, JsValue::String(units) if units == &[0xd800]));
    let pair = parse(r#""\ud83c\udf0d""#).expect("paired surrogate");
    assert!(matches!(&pair, JsValue::String(units) if units == &[0xd83c, 0xdf0d]));
}

#[test]
fn numbers_use_js_binary64_but_nonfinite_values_are_not_lost_during_parse() {
    let input = "[-0,-0.0,-1e-400,1e+400,-1E+400,9007199254740993,1e-7,1e-6,1e20,1e21]";
    let value = parse(input).expect("valid numbers including overflow");
    assert_eq!(
        stringify(&value).expect("serialize"),
        "[0,0,0,null,null,9007199254740992,1e-7,0.000001,100000000000000000000,1e+21]"
    );
    let JsValue::Array(values) = &value else {
        panic!("array expected")
    };
    assert!(matches!(&values[0], JsValue::Number(n) if *n == 0.0 && n.is_sign_negative()));
    assert!(matches!(&values[2], JsValue::Number(n) if *n == 0.0 && n.is_sign_negative()));
    assert!(matches!(&values[3], JsValue::Number(n) if *n == f64::INFINITY));
    assert!(matches!(&values[4], JsValue::Number(n) if *n == f64::NEG_INFINITY));
    assert_eq!(
        stringify(&JsValue::Number(f64::NAN)).expect("serialize NaN"),
        "null"
    );
}

#[test]
fn object_assignment_keeps_first_position_and_js_index_enumeration() {
    let input = r#"{"z":0,"10":10,"2":2,"a":1,"\u0061":3,"01":1,"4294967295":5,"4294967294":4,"0":0,"-0":0}"#;
    let value = parse(input).expect("object");
    assert_eq!(
        stringify(&value).expect("serialize"),
        r#"{"0":0,"2":2,"10":10,"4294967294":4,"z":0,"a":3,"01":1,"4294967295":5,"-0":0}"#
    );
    let JsValue::Object(entries) = &value else {
        panic!("object expected")
    };
    assert_eq!(entries.len(), 9);
    assert_eq!(entries[0].0, vec![u16::from(b'z')]);
    let literal = JsValue::Object(vec![
        (vec![97], JsValue::Number(1.0)),
        (vec![98], JsValue::Number(2.0)),
        (vec![97], JsValue::Number(3.0)),
    ]);
    assert_eq!(
        stringify(&literal).expect("assigned object"),
        r#"{"a":3,"b":2}"#
    );
}

#[test]
fn stable_english_collation_preserves_equal_utf16_keys_and_nested_index_order() {
    let input =
        r#"{"z":{"10":10,"2":2,"b":2,"a":1},"é":1,"e\u0301":2,"a":3,"\udfff":4,"\ud800":5,"�":6}"#;
    let value = parse(input).expect("object");
    let canonical = canonical_json(&value).expect("English collation");
    assert!(canonical.contains(r#""z":{"2":2,"10":10,"a":1,"b":2}"#));
    assert!(
        canonical.find("\"é\":1").expect("composed")
            < canonical.find("\"e\u{301}\":2").expect("decomposed")
    );
    assert!(
        canonical.find(r#""\udfff":4"#).expect("first lone")
            < canonical.find(r#""\ud800":5"#).expect("second lone")
    );
    assert!(
        canonical.find(r#""\ud800":5"#).expect("second lone")
            < canonical.find("\"�\":6").expect("replacement")
    );
    assert_eq!(
        canonical_json(&value.clone()).expect("cloned canonical"),
        canonical
    );
}

#[test]
fn serde_conversion_is_explicit_and_rejects_unrepresentable_values_without_markers() {
    let source = serde_json::json!({"ordinary": [true, null, "🌍", 7.25]});
    let value = from_serde(&source).expect("representable source");
    assert_eq!(to_serde(&value).expect("representable target"), source);
    for text in [r#"{"text":"\ud800"}"#, r#"{"\udfff":1}"#, "[1e400]"] {
        let value = parse(text).expect("full JS domain");
        let error = to_serde(&value).expect_err("no lossy Value conversion");
        assert_eq!(error.code, "unrepresentable_json");
    }
    let source =
        serde_json::json!({"__numera_js_json__": {"string": [55296.0]}, "Infinity": "Infinity"});
    assert_eq!(
        to_serde(&from_serde(&source).expect("ordinary record")).expect("unchanged"),
        source
    );
    let negative_zero = to_serde(&parse("-0").expect("negative zero")).expect("representable");
    assert!(negative_zero.as_f64().expect("number").is_sign_negative());
}

#[test]
fn byte_limit_is_utf8_bytes_and_includes_whitespace() {
    let text = format!("null{}", " ".repeat(MAX_JSON_BYTES - 4));
    assert!(matches!(parse(&text).expect("exact bound"), JsValue::Null));
    assert_eq!(
        parse(&(text + " ")).expect_err("over bound").message,
        "JSON text exceeds 1 MiB."
    );
    let text = format!("\"{}\"", "é".repeat(MAX_JSON_BYTES / 2));
    assert_eq!(
        parse(&text).expect_err("UTF-8 byte bound").message,
        "JSON text exceeds 1 MiB."
    );
}

#[test]
fn deep_values_clone_serialize_convert_and_drop_on_a_small_stack_even_after_failure() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let depth = 8192;
            let text = format!("{}null{}", "[".repeat(depth), "]".repeat(depth));
            let value = parse(&text).expect("deep JSON");
            let cloned = value.clone();
            assert_eq!(stringify(&cloned).expect("deep stringify"), text);
            assert_eq!(canonical_json(&cloned).expect("deep canonical"), text);
            let converted = to_serde(&cloned).expect("deep serde conversion");
            let back = from_serde(&converted).expect("deep serde source");
            assert_eq!(stringify(&back).expect("converted stringify"), text);
            numera_image_gen_mcp::compat::js_json::drop_serde(converted);
            drop(back);
            drop(cloned);
            drop(value);
            assert!(parse(&(text.clone() + "!")).is_err());
            assert!(parse(&format!("{}0", "[".repeat(depth))).is_err());
            assert!(parse(&format!("{{\"a\":{text},\"a\":0,\"bad\":}}")).is_err());
            assert!(parse(&format!("{{\"a\":{text},\"bad\":{}}}", "[".repeat(depth))).is_err());
            let value = parse(&format!("{{\"a\":{text},\"a\":0}}")).expect("deep replacement");
            assert_eq!(stringify(&value).expect("replaced"), r#"{"a":0}"#);
            let value = parse(&format!("[{text},\"\\ud800\"]")).expect("deep then unrepresentable");
            assert!(to_serde(&value).is_err());
        })
        .expect("bounded test thread")
        .join()
        .expect("deep traversal must not overflow stack");
}
