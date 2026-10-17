use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use numera_image_gen_mcp::{
    compat::{
        js_json::{self, JsValue},
        value::Value,
    },
    mcp::{
        js_envelope::{OriginalArguments, encode_id},
        js_http_body::JsonBody,
    },
};
use std::time::Duration;

#[tokio::test]
async fn json_and_sse_restore_this_request_exact_id_and_structured_result() {
    tokio::time::timeout(Duration::from_secs(1), async {
        let id = js_json::parse(r#""\ud800""#).expect("ID");
        let encoded = encode_id(&id).expect("SDK ID");
        for sse in [false, true] {
            let mut original = OriginalArguments::new(JsValue::Object(Vec::new()));
            original.expected_id = Some(encoded.clone());
            *original.response.lock().expect("response") =
                Some(Value::parse(r#"{"text":"\udfff","usage":{"n":1e400}}"#).expect("result"));
            let text = format!(r#"{{"jsonrpc":"2.0","id":{encoded},"result":{{"content":[]}}}}"#);
            let text = if sse {
                format!("id: primer\nretry: 3000\ndata:\n\nevent: message\ndata:{text}\n\n")
            } else {
                text
            };
            let body = JsonBody::new(
                Full::new(Bytes::from(text)).boxed(),
                sse,
                4096,
                Some(original),
            );
            let output = body.collect().await.expect("body").to_bytes();
            let text = std::str::from_utf8(&output).expect("UTF-8");
            let json = if sse {
                assert!(text.starts_with("id: primer\nretry: 3000\ndata:\n\n"));
                assert!(!text.contains("Cannot encode the protocol response."));
                text.lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .expect("event")
            } else {
                text
            };
            let value = js_json::parse(json).expect("wire JSON");
            assert!(value.get("id").expect("restored").structurally_equal(&id));
            let result = value
                .get("result")
                .expect("result")
                .get("structuredContent")
                .expect("structured");
            assert_eq!(
                result.get("text").and_then(JsValue::as_utf16),
                Some([0xdfff].as_slice())
            );
            assert!(
                result
                    .get("usage")
                    .expect("usage")
                    .get("n")
                    .expect("nonfinite wire")
                    .is_null()
            );
        }
    })
    .await
    .expect("bounded codec");
}

#[tokio::test]
async fn bounded_codec_failures_produce_protocol_errors_not_silent_success() {
    tokio::time::timeout(Duration::from_secs(1), async {
        for (sse, text, limit) in [
            (false, "invalid", 1024),
            (false, "xxxxxxxx", 4),
            (true, "data: truncated", 1024),
        ] {
            let body = JsonBody::new(Full::new(Bytes::from(text)).boxed(), sse, limit, None);
            let bytes = body.collect().await.expect("body").to_bytes();
            let text = std::str::from_utf8(&bytes).expect("UTF-8");
            assert!(text.contains("Cannot encode the protocol response."));
            if sse {
                assert!(text.starts_with("data: "));
            }
        }
    })
    .await
    .expect("bounded failure codec");
}
