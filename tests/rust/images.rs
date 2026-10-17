use base64::{Engine, engine::general_purpose::STANDARD};
use numera_image_gen_mcp::compat::hash::sha256;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::files::images::{
    data_url, decode_base64, inspect, preview, validate_mask,
};

fn exact_source_images() -> Value {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".ci-work/parity/image-acceptance.json");
    Value::parse_with_limit(
        &std::fs::read_to_string(file).expect("exact source image fixtures required"),
        1048576,
    )
    .unwrap()
}

#[test]
fn exact_source_four_mebibyte_base64_preserves_all_bytes_and_strict_boundaries() {
    let bytes = vec![42u8; 4 * 1024 * 1024];
    let encoded = STANDARD.encode(&bytes);
    assert_eq!(decode_base64(&encoded, bytes.len()).unwrap(), bytes);
    let malformed = format!("{}!===", &encoded[..encoded.len() - 4]);
    for invalid in ["AAA", "AAA\n", "AA=A", "A===", "AAAA====", &malformed] {
        assert!(decode_base64(invalid, bytes.len()).is_err());
    }
    assert!(decode_base64(&encoded, bytes.len() - 1).is_err());
    for invalid in ["", "%%%%", "YQ", "YQ===", "Y Q==", "YQ==\n"] {
        assert_eq!(
            decode_base64(invalid, 20).unwrap_err().code,
            "invalid_response"
        );
    }
    for invalid in [
        "data:image/svg+xml;base64,YQ==",
        "data:image/png,YQ==",
        "data:text/plain;base64,YQ==",
        "https://example.com/image.png",
    ] {
        assert_eq!(data_url(invalid, 20).unwrap_err().code, "invalid_input");
    }
}

#[test]
fn exact_source_eight_by_six_png_jpeg_webp_metadata_hash_and_base64_round_trips_match() {
    let cases = exact_source_images();
    assert_eq!(cases.as_array().unwrap().len(), 11);
    for format in ["png", "jpeg", "webp"] {
        let matches: Vec<_> = cases
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["name"] == format)
            .collect();
        assert_eq!(matches.len(), 1);
        let case = matches[0];
        let bytes = decode_base64(case["base64"].as_str().unwrap(), 1048576).unwrap();
        let image = inspect(bytes.clone(), &json!({}), None).unwrap();
        assert_eq!(image.metadata(), case["metadata"]);
        assert_eq!(image.mime, format!("image/{format}"));
        assert_eq!((image.width, image.height), (8, 6));
        assert_eq!(image.alpha, format != "jpeg");
        assert_eq!(
            image.extension,
            if format == "jpeg" { "jpg" } else { format }
        );
        assert_eq!(image.sha256, sha256(&bytes));
        assert_eq!(image.bytes, bytes);
        assert_eq!(
            decode_base64(case["base64"].as_str().unwrap(), bytes.len()).unwrap(),
            bytes
        );
        assert_eq!(
            data_url(
                &format!(
                    "data:image/{format};base64,{}",
                    case["base64"].as_str().unwrap()
                ),
                bytes.len()
            )
            .unwrap(),
            bytes
        );
        assert_eq!(
            decode_base64(case["base64"].as_str().unwrap(), bytes.len() - 1)
                .unwrap_err()
                .code,
            "invalid_response"
        );
    }
}

#[test]
fn exact_source_png_byte_pixel_truncation_and_real_mask_dimension_inputs_reject() {
    let cases = exact_source_images();
    let decode = |name: &str| {
        let case = cases
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == name)
            .unwrap();
        inspect(
            decode_base64(case["base64"].as_str().unwrap(), 1048576).unwrap(),
            &json!({}),
            None,
        )
        .unwrap()
    };
    let source = decode("png");
    assert_eq!(
        inspect(
            source.bytes.clone(),
            &json!({}),
            Some(source.bytes.len() - 1)
        )
        .err()
        .unwrap()
        .code,
        "invalid_input"
    );
    assert_eq!(
        inspect(
            source.bytes.clone(),
            &json!({"files": {"maxPixels": 47}}),
            None
        )
        .err()
        .unwrap()
        .code,
        "invalid_input"
    );
    for invalid in [vec![], b"<svg/>".to_vec(), source.bytes[..20].to_vec()] {
        assert_eq!(
            inspect(invalid, &json!({}), None).err().unwrap().code,
            "invalid_input"
        );
    }
    validate_mask(&source, &source).unwrap();
    for name in ["jpeg", "opaque-png", "width-mask", "height-mask"] {
        assert_eq!(
            validate_mask(&decode(name), &source).unwrap_err().code,
            "invalid_input",
            "{name}"
        );
    }
}

#[test]
fn exact_source_preview_geometry_no_enlargement_and_error_stage_are_preserved() {
    let cases = exact_source_images();
    let decode = |name: &str| {
        let case = cases
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == name)
            .unwrap();
        inspect(
            decode_base64(case["base64"].as_str().unwrap(), 1048576).unwrap(),
            &json!({}),
            None,
        )
        .unwrap()
    };
    let source = decode("webp");
    let config = json!({"files": {"previewMaxDimension": 4, "previewMaxBytes": 262144}});
    let result = preview(&source, &config).unwrap();
    assert_eq!(result["mimeType"], "image/png");
    assert_eq!(result["width"], 4);
    assert_eq!(result["height"], 3);
    let bytes = decode_base64(result["data"].as_str().unwrap(), 262144).unwrap();
    assert!(bytes.len() <= config["files"]["previewMaxBytes"].as_u64().unwrap() as usize);
    let decoded = inspect(bytes, &json!({}), None).unwrap();
    assert_eq!((decoded.width, decoded.height), (4, 3));
    assert_eq!(decoded.mime, "image/png");
    let original = decode("png");
    for (dimension, width, height) in [(100, 8, 6), (1, 1, 1)] {
        let result = preview(
            &original,
            &json!({"files": {"previewMaxDimension": dimension}}),
        )
        .unwrap();
        assert_eq!(result["width"], width);
        assert_eq!(result["height"], height);
    }
    let error = preview(
        &original,
        &json!({"files": {"previewMaxDimension": 1, "previewMaxBytes": 1}}),
    )
    .unwrap_err();
    assert_eq!(error.code, "output_file_error");
    assert_eq!(error.stage, "preview");
}

#[test]
fn generated_original_image_fixtures_are_preserved_and_validated() {
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/images.json");
    let bytes = std::fs::read(path).expect("source image fixture generation required");
    let cases: Vec<numera_image_gen_mcp::compat::value::Value> =
        serde_json::from_slice(&bytes).expect("source fixtures");
    assert!(!cases.is_empty());
    for case in cases {
        let raw = decode_base64(case["base64"].as_str().expect("fixture bytes"), 1024 * 1024)
            .expect("base64");
        let image = inspect(
            raw.clone(),
            &json!({"files":{"maxOutputBytes":1048576,"maxPixels":64000000}}),
            None,
        )
        .expect("real image decode");
        assert_eq!(image.bytes, raw);
        assert_eq!(image.metadata(), case["metadata"], "{}", case["name"]);
        let config = json!({"files":{"maxPixels":64000000,"previewMaxDimension":3,"previewMaxBytes":262144}});
        let derivative = preview(&image, &config).unwrap();
        let derived = inspect(
            decode_base64(derivative["data"].as_str().unwrap(), 262144).unwrap(),
            &config,
            None,
        )
        .unwrap();
        assert!(derived.width <= 3 && derived.height <= 3);
        assert_eq!(derived.mime, "image/png");
        assert_eq!(image.bytes, raw);
        assert_eq!(
            data_url(
                &format!(
                    "data:{};base64,{}",
                    image.mime,
                    case["base64"].as_str().unwrap()
                ),
                1048576
            )
            .unwrap(),
            raw
        );
        if image.mime == "image/png" && image.alpha {
            validate_mask(&image, &image).unwrap();
            let mut wrong = image.clone();
            wrong.width += 1;
            assert_eq!(
                validate_mask(&wrong, &image).unwrap_err().code,
                "invalid_input"
            );
            wrong.width = image.width;
            wrong.alpha = false;
            assert_eq!(
                validate_mask(&wrong, &image).unwrap_err().code,
                "invalid_input"
            );
        } else {
            assert_eq!(
                validate_mask(&image, &image).unwrap_err().code,
                "invalid_input"
            );
        }
        assert_eq!(
            inspect(raw.clone(), &config, Some(raw.len() - 1))
                .err()
                .unwrap()
                .code,
            "invalid_input"
        );
        assert_eq!(
            inspect(raw.clone(), &json!({"files":{"maxPixels":1}}), None)
                .err()
                .unwrap()
                .code,
            "invalid_input"
        );
        assert_eq!(
            preview(
                &image,
                &json!({"files":{"previewMaxDimension":1,"previewMaxBytes":1}})
            )
            .unwrap_err()
            .code,
            "output_file_error"
        );
    }
    assert!(decode_base64("AAAA\n", 1024).is_err());
    assert!(decode_base64("", 1024).is_err());
    for invalid in ["AAA", "AAA\n", "AA=A", "A===", "AAAA====", "%%%%", "Y Q=="] {
        assert_eq!(
            decode_base64(invalid, 1024).unwrap_err().code,
            "invalid_response"
        );
    }
    for invalid in [
        "data:image/svg+xml;base64,YQ==",
        "data:image/png,YQ==",
        "data:text/plain;base64,YQ==",
        "https://example.com/image.png",
    ] {
        assert_eq!(data_url(invalid, 1024).unwrap_err().code, "invalid_input");
    }
    for invalid in [
        vec![],
        b"<svg/>".to_vec(),
        b"RIFFbrokenWEBP".to_vec(),
        b"\x89PNG\r\n\x1a\n".to_vec(),
    ] {
        assert_eq!(
            inspect(invalid, &json!({}), None).err().unwrap().code,
            "invalid_input"
        );
    }
}
