use numera_image_gen_mcp::compat::{
    js_json::{JsValue, canonical_json, parse, stringify},
    js_schema::Schema,
};
use serde_json::Value;

fn bundled(text: &str) -> Value {
    serde_json::from_str(text).expect("bundled schema")
}

#[derive(serde::Deserialize)]
struct Oracle {
    name: String,
    schema: String,
    text: String,
    valid: bool,
    serialized: Option<String>,
}

#[test]
fn raw_hosted_schema_oracle_matches_source_validity_and_preserved_effective_values() {
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".ci-work/parity/raw-js-schema.json");
    let cases: Vec<Oracle> = serde_json::from_slice(
        &std::fs::read(fixture).expect("hosted source schema oracle is required"),
    )
    .expect("outer oracle envelope");
    assert_eq!(cases.len(), 8, "all source schema cases are required");
    let config = bundled(include_str!("../../schemas/config.schema.json"));
    let request = bundled(include_str!("../../schemas/image-request.schema.json"));
    let config = Schema::compile(&config).expect("config schema");
    let request = Schema::compile(&request).expect("request schema");
    let mut names = std::collections::HashSet::new();
    for case in cases {
        assert!(names.insert(case.name.clone()), "duplicate oracle case");
        let mut value = parse(&case.text).expect("raw source JSON domain");
        let schema = match case.schema.as_str() {
            "config" => {
                // Source files.prefault({}) is not encoded in the generated JSON schema.
                if value.get("files").is_none() {
                    value
                        .set("files", JsValue::Object(Vec::new()))
                        .expect("config object");
                }
                &config
            }
            "image-request" => &request,
            _ => panic!("unknown oracle schema"),
        };
        assert_eq!(
            schema.default_and_validate(&mut value).is_ok(),
            case.valid,
            "{}",
            case.name
        );
        if case.valid {
            let expected = parse(case.serialized.as_deref().expect("valid source output"))
                .expect("raw serialized source output");
            assert_eq!(
                canonical_json(&value).expect("canonical target"),
                canonical_json(&expected).expect("canonical source"),
                "{}",
                case.name
            );
        }
    }
}

#[test]
fn all_actual_bundled_schemas_compile_and_defaults_preserve_unknown_js_values() {
    let config = bundled(include_str!("../../schemas/config.schema.json"));
    let simple = bundled(include_str!("../../schemas/simple-config.schema.json"));
    let request = bundled(include_str!("../../schemas/image-request.schema.json"));
    let schema = Schema::compile(&config).expect("advanced dialect");
    Schema::compile(&simple).expect("simple dialect");
    Schema::compile(&request).expect("request refs and oneOf");
    let mut value = parse(r#"{"schemaVersion":1,"outputDir":"/out","stateDir":"/state","logging":{"directory":"/logs"},"files":{},"connections":{"main":{"adapter":"openai-images","baseUrl":"https://example.com/v1","auth":{"type":"none"},"defaults":{"value":1e400},"extensions":{"\ud800":"\udfff"},"modelOverrides":{"m":{"capabilities":{"value":-1e400}}},"workflow":{"graph":{"\ud800":{"text":"\ud800","n":1e400}},"bindings":{},"outputNodes":["\ud800"]}}}}"#)
        .expect("advanced JSON");
    schema
        .default_and_validate(&mut value)
        .expect("defaulted valid config");
    let connection = value
        .get("connections")
        .expect("connections")
        .get("main")
        .expect("connection");
    assert_eq!(
        connection.get("requestTimeoutMs").and_then(JsValue::as_f64),
        Some(300000.0)
    );
    assert_eq!(
        connection
            .get("defaults")
            .expect("defaults")
            .get("value")
            .and_then(JsValue::as_f64),
        Some(f64::INFINITY)
    );
    assert_eq!(
        connection
            .get("extensions")
            .expect("extensions")
            .get_utf16(&[0xd800])
            .and_then(JsValue::as_utf16),
        Some([0xdfff].as_slice())
    );
    assert_eq!(
        connection
            .get("workflow")
            .expect("workflow")
            .get("graph")
            .expect("graph")
            .get_utf16(&[0xd800])
            .expect("node")
            .get("text")
            .and_then(JsValue::as_utf16),
        Some([0xd800].as_slice())
    );
    assert_eq!(
        value
            .get("returnMode")
            .expect("mode")
            .checked_string()
            .expect("string"),
        "files"
    );
    assert!(schema.validate(&value).is_ok());
}

#[test]
fn simple_union_strings_count_utf16_and_use_js_whitespace_without_replacement() {
    let source = bundled(include_str!("../../schemas/simple-config.schema.json"));
    let schema = Schema::compile(&source).expect("schema");
    let mut value = parse(r#"{"api_endpoint":"https://example.com/v1/\ud800","api_key":"\ud800","models":[{"id":"\ud800","name":"🌍","quality":null,"size":"\udfff"}]}"#)
        .expect("simple");
    schema
        .default_and_validate(&mut value)
        .expect("plain strings allow lone UTF16");
    assert_eq!(
        value
            .get("profile")
            .expect("default")
            .checked_string()
            .expect("string"),
        "openai-images"
    );
    assert_eq!(
        value
            .get("models")
            .expect("models")
            .at(0)
            .expect("model")
            .get("enabled")
            .and_then(JsValue::as_bool),
        Some(true)
    );
    assert!(
        value
            .get("api_endpoint")
            .expect("URL retained")
            .as_utf16()
            .expect("units")
            .contains(&0xd800)
    );
    for text in [
        r#"{"api_endpoint":"bad","api_key":"x","models":[{"id":"m","name":"n"}]}"#,
        r#"{"api_endpoint":"https://example.com","api_key":"x","models":[]}"#,
        r#"{"api_endpoint":"https://example.com","api_key":"x","models":[{"id":"m","name":"n","quality":"﻿ "}]}"#,
        r#"{"api_endpoint":"https://example.com","api_key":"x","models":[{"id":"m","name":"n","output_format":"gif"}]}"#,
    ] {
        assert!(
            schema
                .default_and_validate(&mut parse(text).expect("JSON"))
                .is_err()
        );
    }
    let name = "🌍".repeat(51);
    let mut value = parse(&format!(r#"{{"api_endpoint":"https://example.com","api_key":"x","models":[{{"id":"m","name":"{name}"}}]}}"#))
        .expect("long name");
    assert!(schema.default_and_validate(&mut value).is_err());
}

#[test]
fn request_refs_select_exactly_one_source_and_preserve_provider_options() {
    let source = bundled(include_str!("../../schemas/image-request.schema.json"));
    let schema = Schema::compile(&source).expect("schema");
    let mut value = parse(r#"{"prompt":"\ud800","provider_options":{"n":1e400,"\udfff":"\ud800"},"reference_images":[{"type":"path","path":"\udfff"},{"type":"url","url":"https://example.com/a"},{"type":"output_id","output_id":"00000000-0000-4000-8000-000000000000"},{"type":"data_url","data_url":"x"}]}"#)
        .expect("request");
    schema.default_and_validate(&mut value).expect("request");
    assert_eq!(value.get("count").and_then(JsValue::as_f64), Some(1.0));
    assert_eq!(value.get("wait").and_then(JsValue::as_bool), Some(true));
    assert_eq!(
        value
            .get("provider_options")
            .expect("opaque")
            .get("n")
            .and_then(JsValue::as_f64),
        Some(f64::INFINITY)
    );
    for text in [
        r#"{"prompt":"x","reference_images":[{"type":"path","path":"x","url":"https://example.com"}]}"#,
        r#"{"prompt":"x","mask":{"type":"output_id","output_id":"not-uuid"}}"#,
        r#"{"prompt":"x","seed":1e400}"#,
        r#"{"prompt":"x","seed":9007199254740992}"#,
        r#"{"prompt":"x","count":1.5}"#,
        r#"{"prompt":"x","edit_region":{"x":0,"y":0,"width":0,"height":1}}"#,
        r#"{"prompt":"x","target_language":" ﻿"}"#,
        r#"{"prompt":"x","unknown":1}"#,
    ] {
        assert!(
            schema
                .default_and_validate(&mut parse(text).expect("JSON"))
                .is_err()
        );
    }
}

#[test]
fn schema_drift_is_explicit_not_silently_accepted() {
    for source in [
        serde_json::json!({"type":"object","unevaluatedProperties":false}),
        serde_json::json!({"type":"string","pattern":"unsupported.*"}),
        serde_json::json!({"type":"string","format":"email"}),
        serde_json::json!({"$ref":"https://example.com/schema"}),
        serde_json::json!({"$defs":{"loop":{"$ref":"#/$defs/loop"}},"$ref":"#/$defs/loop"}),
        serde_json::json!({"type":"unknown"}),
        serde_json::json!({"anyOf":[]}),
        serde_json::json!({"type":"string","minLength":-1}),
    ] {
        assert_eq!(
            Schema::compile(&source)
                .err()
                .expect("unsupported schema")
                .code,
            "unsupported_schema"
        );
    }
}

#[test]
fn local_boolean_schema_and_combinators_apply_exact_matching() {
    let source = serde_json::json!({"oneOf":[{"type":"number"},{"type":"integer"}]});
    let schema = Schema::compile(&source).expect("supported");
    assert!(schema.validate(&JsValue::Number(1.5)).is_ok());
    assert!(schema.validate(&JsValue::Number(1.0)).is_err());
    let source = serde_json::json!({"type":"object","properties":{"a":true,"b":false}});
    let schema = Schema::compile(&source).expect("boolean schemas");
    assert!(
        schema
            .validate(&parse(r#"{"a":1e400}"#).expect("JSON"))
            .is_ok()
    );
    assert!(
        schema
            .validate(&parse(r#"{"b":null}"#).expect("JSON"))
            .is_err()
    );
}

#[test]
fn exact_spaced_connection_key_is_rejected_by_the_bundled_schema_itself() {
    let source = bundled(include_str!("../../schemas/config.schema.json"));
    let schema = Schema::compile(&source).expect("advanced dialect");
    let empty = r#"{"schemaVersion":1,"defaultConnection":"local","outputDir":"/out","stateDir":"/state","logging":{"directory":"/logs"},"files":{},"connections":{"bad name":{}}}"#;
    assert!(
        schema
            .default_and_validate(&mut parse(empty).unwrap())
            .is_err()
    );
    let complete = r#"{"schemaVersion":1,"outputDir":"/out","stateDir":"/state","logging":{"directory":"/logs"},"files":{},"connections":{"bad name":{"adapter":"openai-images","baseUrl":"https://api.example/v1","auth":{"type":"none"}}}}"#;
    assert!(
        schema
            .default_and_validate(&mut parse(complete).unwrap())
            .is_err()
    );
    let valid = complete.replace("\"bad name\"", "\"valid_name\"");
    assert!(
        schema
            .default_and_validate(&mut parse(&valid).unwrap())
            .is_ok()
    );
}

#[test]
fn deep_opaque_extensions_are_not_walked_or_converted() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let source = bundled(include_str!("../../schemas/image-request.schema.json"));
            let schema = Schema::compile(&source).expect("schema");
            let depth = 8192;
            let opaque = format!("{}1e400{}", "[".repeat(depth), "]".repeat(depth));
            let mut value = parse(&format!(
                r#"{{"prompt":"x","provider_options":{{"\ud800":{opaque}}}}}"#
            ))
            .expect("deep request");
            schema
                .default_and_validate(&mut value)
                .expect("deep opaque");
            let retained = value
                .get("provider_options")
                .expect("options")
                .get_utf16(&[0xd800])
                .expect("key");
            assert_eq!(
                stringify(retained).expect("serialized"),
                format!("{}null{}", "[".repeat(depth), "]".repeat(depth))
            );
        })
        .expect("owned thread")
        .join()
        .expect("bounded traversal");
}
