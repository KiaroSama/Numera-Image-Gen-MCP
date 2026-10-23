use numera_image_gen_mcp::{
    compat::js_json::{JsValue, parse, stringify},
    mcp::js_envelope::{decode, decode_id, encode_id, restore_id},
};

#[test]
fn protocol_string_ids_are_bijective_including_surrogates_and_client_prefixes() {
    for text in [
        r#""ordinary""#,
        r#""\ud800""#,
        r#""numera-utf16:d800""#,
        r#""🌍""#,
        r#""""#,
    ] {
        let id = parse(text).expect("JS string");
        let encoded = encode_id(&id).expect("protocol ID");
        assert!(id.structurally_equal(&decode_id(&encoded).expect("exact restored ID")));
        let mut envelope = parse(&format!(
            r#"{{"jsonrpc":"2.0","id":{encoded},"result":{{}}}}"#
        ))
        .expect("envelope");
        restore_id(&mut envelope).expect("restore");
        assert!(envelope.get("id").expect("id").structurally_equal(&id));
    }
    let ordinary = encode_id(&parse(r#""numera-utf16:d800""#).expect("prefix")).expect("ID");
    let lone = encode_id(&parse(r#""\ud800""#).expect("lone")).expect("ID");
    assert_ne!(ordinary, lone);
}

#[test]
fn accepted_numeric_ids_match_released_source_safe_integer_domain() {
    for number in [-9007199254740991.0, -0.0, 9007199254740991.0] {
        let encoded = encode_id(&JsValue::Number(number)).expect("safe integer");
        assert!(
            decode_id(&encoded)
                .expect("numeric ID")
                .structurally_equal(&JsValue::Number(number))
        );
    }
    for number in [1.5, 9007199254740992.0, f64::INFINITY, f64::NAN] {
        assert_eq!(
            encode_id(&JsValue::Number(number))
                .expect_err("source rejects ID")
                .code,
            "invalid_request"
        );
    }
    assert!(encode_id(&JsValue::Null).is_err());
    assert!(decode_id(&serde_json::json!("numera-utf16:xxx")).is_err());
}

#[test]
fn original_tool_arguments_do_not_enter_the_sdk_serde_metadata_projection() {
    let input = r#"{"jsonrpc":"2.0","id":"\ud800","method":"tools/call","params":{"name":"generate_image","arguments":{"prompt":"\ud800","provider_options":{"\udfff":1e400}}}}"#;
    let decoded = decode(input, input.len()).expect("raw request");
    assert!(decoded.metadata["params"].get("arguments").is_none());
    let args = decoded.arguments.expect("authoritative original arguments");
    assert_eq!(
        args.get("prompt").and_then(JsValue::as_utf16),
        Some([0xd800].as_slice())
    );
    assert_eq!(
        args.get("provider_options")
            .expect("options")
            .get_utf16(&[0xdfff])
            .and_then(JsValue::as_f64),
        Some(f64::INFINITY)
    );
    assert!(decode(input, input.len() - 1).is_err());
    let cancellation = decode(
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"\ud800"}}"#,
        1024,
    )
    .expect("cancel");
    assert_eq!(
        decoded.metadata["id"],
        cancellation.metadata["params"]["requestId"]
    );
    assert_eq!(
        stringify(&args).expect("wire JSON"),
        r#"{"prompt":"\ud800","provider_options":{"\udfff":null}}"#
    );
    assert!(decode(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"health_check","arguments":null}}"#, 1024).is_err());
    let empty = decode(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"health_check"}}"#,
        1024,
    )
    .expect("missing arguments");
    assert!(empty.arguments.expect("effective default").is_object());
    assert!(
        decode(
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"invalid"}}"#,
            1024
        )
        .is_ok()
    );
}
