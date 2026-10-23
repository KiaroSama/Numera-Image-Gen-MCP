#[path = "common/normalized.rs"]
mod normalized_fixture;
use normalized_fixture::normalized;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    adapters::normalize::{normalize, response},
    error::Result,
    files::images::decode_base64,
    http::sse::{self, Reader},
};

fn outcome(result: Result<Value>) -> Value {
    match result {
        Ok(value) => json!({"value":value}),
        Err(error) => json!({"error":error.code}),
    }
}
fn oracle() -> Value {
    let file =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/streams.json");
    serde_json::from_slice(&std::fs::read(file).expect("hosted source stream oracle required"))
        .unwrap()
}

#[test]
fn final_stream_and_binary_results_match_frozen_source_without_promoting_previews() {
    let oracle = oracle();
    let cases = oracle["cases"].as_array().unwrap();
    assert!(cases.len() > 20, "source result contracts required");
    for case in cases {
        let bytes = if let Some(encoded) = case["base64"].as_str() {
            decode_base64(encoded, 1048576).unwrap()
        } else {
            case["text"].as_str().unwrap().as_bytes().to_vec()
        };
        let actual =
            response(&case["connection"], bytes, case["mime"].as_str().unwrap()).map(normalized);
        assert_eq!(outcome(actual), case["expected"], "{}", case["name"]);
    }
}

#[test]
fn malformed_nested_results_refusals_and_fallbacks_match_source_error_and_partial_contracts() {
    let oracle = oracle();
    let cases = oracle["normalization"].as_array().unwrap();
    assert!(cases.len() >= 70);
    let mut failures = Vec::new();
    for case in cases {
        let actual = outcome(normalize(&case["connection"], &case["raw"]).map(normalized));
        if actual != case["expected"] {
            failures.push(format!(
                "{} / {}: actual={actual}, expected={}",
                case["connection"]["adapter"], case["raw"], case["expected"]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn complete_frame_parsing_matches_source_crlf_multiline_unicode_and_error_contracts() {
    let oracle = oracle();
    let frames = oracle["frames"].as_array().unwrap();
    assert!(frames.len() >= 8);
    for case in frames {
        let actual = sse::events(case["text"].as_str().unwrap().as_bytes(), 4096).map(|events| {
            json!(
                events
                    .into_iter()
                    .map(|event| json!({"type":event.kind,"data":event.data}))
                    .collect::<Vec<_>>()
            )
        });
        assert_eq!(outcome(actual), case["expected"], "{}", case["name"]);
    }
}

#[test]
fn exact_source_seven_byte_preview_completed_done_chunks_preserve_only_terminal_images() {
    let text = "data: {\"type\":\"image_generation.partial_image\",\"b64_json\":\"preview\"}\r\n\r\ndata: {\"type\":\"image_generation.completed\",\"b64_json\":\"ZmluYWw=\"}\r\n\r\ndata: [DONE]\r\n\r\n";
    let mut reader = Reader::new(10000);
    for chunk in text.as_bytes().chunks(7) {
        reader.push(chunk).unwrap();
    }
    let bytes = reader.finish().unwrap();
    assert!(
        !String::from_utf8(bytes.clone())
            .unwrap()
            .contains("preview")
    );
    let result = response(
        &json!({"adapter":"openrouter-images"}),
        bytes,
        "text/event-stream",
    )
    .unwrap();
    assert_eq!(normalized(result)["images"], json!([{"base64":"ZmluYWw="}]));
}

#[test]
fn exact_source_truncated_completed_frame_and_one_byte_limit_never_return_success() {
    let chunk = b"data: {\"type\":\"image_generation.completed\"}";
    let mut reader = Reader::new(10000);
    reader.push(chunk).unwrap();
    assert_eq!(reader.finish().unwrap_err().code, "outcome_unknown");
    let mut reader = Reader::new(1);
    assert_eq!(reader.push(chunk).unwrap_err().code, "invalid_response");
}

#[test]
fn incremental_reader_bounds_discarded_progress_and_rejects_invalid_or_unfinished_input() {
    for (bytes, code) in [
        (b"data: {broken}\n\n".as_slice(), "invalid_response"),
        (b"data: null\n\n".as_slice(), "invalid_response"),
        (b"event: error\ndata: {}\n\n".as_slice(), "outcome_unknown"),
        (
            b"data: {\"text\":\"\xff\"}\n\n".as_slice(),
            "invalid_response",
        ),
        (b"data: {\"text\":\"\xff".as_slice(), "invalid_response"),
    ] {
        let mut reader = Reader::new(1024);
        assert_eq!(reader.push(bytes).unwrap_err().code, code);
    }
    for bytes in [b"data: {}\n".as_slice(), b"\xe6".as_slice()] {
        let mut reader = Reader::new(1024);
        reader.push(bytes).unwrap();
        let expected = if bytes == b"\xe6" {
            "invalid_response"
        } else {
            "outcome_unknown"
        };
        assert_eq!(reader.finish().unwrap_err().code, expected);
    }
    let progress = b"event: progress\ndata: {}\n\n";
    let mut bounded = Reader::new(progress.len());
    bounded.push(progress).unwrap();
    assert_eq!(bounded.push(progress).unwrap_err().code, "invalid_response");
    let mut reader = Reader::new(65536);
    for _ in 0..1000 {
        reader.push(b"data: {}\n\n").unwrap();
    }
    assert_eq!(
        reader.push(b"data: {}\n\n").unwrap_err().code,
        "invalid_response"
    );
    assert_eq!(
        sse::events(b"data: {}\n\n", 1).unwrap_err().code,
        "invalid_response"
    );
    for chunk_size in [1, 2, 7, 64] {
        let frames = ": ignored\r\n\r\nevent: progress\ndata: {}\n\ndata: {\"event_type\":\"partial_image\"}\n\nevent: done\ndata: {\"text\":\"日本語 🌍\"}\n\ndata: [DONE]\n\n";
        let mut reader = Reader::new(4096);
        for chunk in frames.as_bytes().chunks(chunk_size) {
            reader.push(chunk).unwrap();
        }
        let events = sse::events(&reader.finish().unwrap(), 4096).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, "done");
        assert_eq!(events[0].data, json!({"text":"日本語 🌍"}));
        assert_eq!(events[1].data, json!({}));
    }
}
