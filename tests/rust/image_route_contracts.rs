#[path = "common/image.rs"]
mod image_fixture;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    adapters::{build_request, types::Input},
    capabilities,
    files::images::inspect,
    http::api::Body,
    jobs::Receipt,
    services::requirements,
};
fn input<'a>(
    connection: &'a Value,
    request: &'a Value,
    references: &'a [numera_image_gen_mcp::files::images::Image],
    mask: Option<&'a numera_image_gen_mcp::files::images::Image>,
) -> Input<'a> {
    Input {
        connection,
        request,
        references,
        mask,
        model: {
            static MODEL: std::sync::OnceLock<numera_image_gen_mcp::compat::value::JsString> =
                std::sync::OnceLock::new();
            MODEL.get_or_init(|| "opaque/image".into())
        },
        operation: "edit",
    }
}

#[test]
fn explicit_native_json_generation_fields_preserve_references_masks_and_reject_unconfigured_routes()
{
    let image = inspect(image_fixture::original(), &json!({}), None).unwrap();
    let refs = vec![image.clone(), image.clone()];
    let request = json!({"prompt":"Preserve 日本語","count":1,"provider_options":{}});
    for mode in ["json", "generation"] {
        let c = json!({"adapter":"openai-images","edit":{"mode":mode,"encoding":"images","maxReferences":2,"masks":true,"maskPolarity":"transparent-edit"}});
        let prepared = build_request(&input(&c, &request, &refs, Some(&image))).unwrap();
        assert_eq!(
            prepared.operation,
            if mode == "json" {
                "images/edits"
            } else {
                "images/generations"
            }
        );
        let Body::Json(body) = prepared.body else {
            panic!("explicit JSON route required")
        };
        assert_eq!(body["images"].as_array().unwrap().len(), 2);
        assert!(
            body["mask"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
    }
    // Builder preserves explicit masks; capability policy rejects unverified forwarding before it.
    let generic = json!({"adapter":"openai-images"});
    assert!(matches!(
        build_request(&input(&generic, &request, &refs[..1], Some(&image)))
            .unwrap()
            .body,
        Body::Multipart(_)
    ));
    for (c, code, reference_count) in [
        (
            json!({"adapter":"openai-images","gateway":"9router"}),
            "unsupported_operation",
            1,
        ),
        (
            json!({"adapter":"openai-images","edit":{"mode":"json","encoding":"image","maxReferences":1}}),
            "unsupported_operation",
            2,
        ),
        (
            json!({"adapter":"openai-images","edit":{"mode":"json","encoding":"image[]","maxReferences":2,"masks":true,"maskPolarity":"transparent-edit"}}),
            "invalid_configuration",
            1,
        ),
    ] {
        assert_eq!(
            build_request(&input(&c, &request, &refs[..reference_count], Some(&image)))
                .err()
                .unwrap()
                .code,
            code
        );
    }
    let c = json!({"adapter":"openai-images","gateway":"9router"});
    let r = json!({"prompt":"fixture","count":1,"quality":"high","provider_options":{}});
    let raw = Input {
        model: &"antigravity/gemini-3.1-flash-image".into(),
        operation: "generate",
        references: &[],
        mask: None,
        connection: &c,
        request: &r,
    };
    assert_eq!(
        build_request(&raw).err().unwrap().code,
        "unsupported_parameter"
    );
}

#[test]
fn all_explicit_json_reference_encodings_preserve_order_scalar_masks_and_gateway_ids() {
    let first = inspect(image_fixture::original(), &json!({}), None).unwrap();
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/images.json");
    let fixtures: Vec<Value> = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    let jpeg = fixtures
        .iter()
        .find(|fixture| fixture["metadata"]["mime"] == "image/jpeg")
        .unwrap();
    let second = inspect(
        numera_image_gen_mcp::files::images::decode_base64(
            jpeg["base64"].as_str().unwrap(),
            1048576,
        )
        .unwrap(),
        &json!({}),
        None,
    )
    .unwrap();
    assert_ne!(first.sha256, second.sha256);
    let references = [first.clone(), second.clone()];
    let encoded = references
        .iter()
        .map(numera_image_gen_mcp::adapters::types::mime_data)
        .collect::<Vec<_>>();
    let request = json!({"prompt":"Preserve 日本語","count":1,"provider_options":{}});
    for mode in ["json", "generation"] {
        for encoding in ["image", "images", "image_urls", "input_references"] {
            let c = json!({"adapter":"openai-images","edit":{"mode":mode,"encoding":encoding,"maxReferences":if encoding=="image"{1}else{2},"masks":true,"maskPolarity":"transparent-edit"}});
            let sources = if encoding == "image" {
                &references[..1]
            } else {
                &references[..]
            };
            let built = build_request(&input(&c, &request, sources, Some(&first))).unwrap();
            assert_eq!(
                built.operation,
                if mode == "json" {
                    "images/edits"
                } else {
                    "images/generations"
                }
            );
            assert_eq!(built.prefix, "/v1");
            let Body::Json(body) = built.body else {
                panic!("configured JSON route required")
            };
            let expected = match encoding {
                "image" => json!(encoded[0]),
                "input_references" => json!(
                    encoded
                        .iter()
                        .map(|url| json!({"type":"image_url","image_url":{"url":url}}))
                        .collect::<Vec<_>>()
                ),
                _ => json!(encoded),
            };
            assert_eq!(body[encoding], expected, "{mode}-{encoding}");
            assert_eq!(body["mask"], encoded[0]);
            assert_eq!(body["model"], "opaque/image");
            assert_eq!(body["prompt"], request["prompt"]);
        }
    }
    let c = json!({"adapter":"openai-images","gateway":"9router","edit":{"mode":"json","encoding":"image","maxReferences":1}});
    let built = build_request(&input(&c, &request, &references[..1], None)).unwrap();
    assert_eq!(built.operation, "images/generations");
    let Body::Json(body) = built.body else {
        panic!("9router embedded JSON required")
    };
    assert_eq!(body["image"], encoded[0]);
    for gateway in ["omniroute", "9router"] {
        let mut connection =
            json!({"adapter":"openai-images","gateway":gateway,"modelOverrides":{}});
        for prefix in ["codex", "cx"] {
            let model = format!("{prefix}/fixture").into();
            let built = build_request(&Input {
                connection: &connection,
                request: &request,
                references: &references[..1],
                mask: None,
                model: &model,
                operation: "edit",
            })
            .unwrap();
            if let Body::Json(body) = built.body {
                assert_eq!(body["model"], model);
            } else {
                assert_eq!(built.operation, "images/edits");
            }
            let cap = capabilities::capabilities(&connection, &model);
            assert_eq!(cap["gateway_forwarding"]["image_to_image"], "supported");
            if gateway == "omniroute" {
                connection["modelOverrides"][&model] =
                    json!({"defaults":{},"capabilities":{},"maxCount":2});
                assert_eq!(
                    capabilities::effective_request(
                        &connection,
                        &json!({"prompt":"fixture","count":2,"provider_options":{}}),
                        &model,
                    )
                    .unwrap_err()
                    .code,
                    "unsupported_parameter"
                );
            }
        }
        let cap = capabilities::capabilities(&connection, &"cxx/fixture".into());
        assert_eq!(cap["gateway_forwarding"]["image_to_image"], "unknown");
    }
    for tier in ["8K", "invalid"] {
        let connection = json!({"adapter":"openai-images","gateway":"omniroute"});
        let request = json!({"prompt":"fixture","count":1,"image_size":tier,"provider_options":{}});
        let result = build_request(&Input {
            connection: &connection,
            request: &request,
            references: &[],
            mask: None,
            model: &"antigravity/opaque".into(),
            operation: "generate",
        });
        assert_eq!(result.err().unwrap().code, "unsupported_parameter");
    }
}

#[test]
fn gemini_inline_references_and_interaction_format_preserve_exact_native_fields() {
    let image = inspect(image_fixture::original(), &json!({}), None).unwrap();
    let sources = [image.clone()];
    for adapter in ["gemini", "gemini-interactions"] {
        let connection = json!({"adapter":adapter});
        let mut request = json!({"prompt":"Preserve café","count":1,"aspect_ratio":"1:1","image_size":"2K","provider_options":{}});
        if adapter == "gemini-interactions" {
            request["output_format"] = json!("jpeg");
        }
        let built = build_request(&input(&connection, &request, &sources, None)).unwrap();
        assert_eq!(built.prefix, "/v1beta");
        let Body::Json(body) = built.body else {
            panic!("native JSON required")
        };
        let bytes = if adapter == "gemini" {
            &body["contents"][0]["parts"][1]["inlineData"]["data"]
        } else {
            &body["input"][1]["data"]
        };
        assert_eq!(
            numera_image_gen_mcp::files::images::decode_base64(bytes.as_str().unwrap(), 1048576)
                .unwrap(),
            image.bytes
        );
        if adapter == "gemini" {
            assert_eq!(
                body["contents"][0]["parts"][1]["inlineData"]["mimeType"],
                image.mime
            );
            assert_eq!(
                body["generationConfig"]["imageConfig"],
                json!({"aspectRatio":"1:1","imageSize":"2K"})
            );
        } else {
            assert_eq!(body["input"][1]["mime_type"], image.mime);
            assert_eq!(body["store"], false);
            assert_eq!(
                body["response_format"],
                json!({"type":"image","aspect_ratio":"1:1","image_size":"2K","mime_type":"image/jpeg"})
            );
        }
        assert_eq!(
            build_request(&input(&connection, &request, &sources, Some(&image)))
                .err()
                .unwrap()
                .code,
            "unsupported_operation"
        );
        for field in ["quality", "background", "size", "negative_prompt"] {
            let mut invalid = request.clone();
            invalid[field] = json!("unsupported");
            assert_eq!(
                build_request(&input(&connection, &invalid, &sources, None))
                    .err()
                    .unwrap()
                    .code,
                "unsupported_parameter"
            );
        }
    }
}

#[test]
fn requested_ratio_tier_format_and_upscale_deviations_use_original_dimensions_and_evidence() {
    let image = inspect(image_fixture::original(), &json!({}), None).unwrap();
    let receipt = Receipt::new(
        json!({"request_id":"fixture","output_requirements":{"output_format":"jpeg","size":"8x6","aspect_ratio":"1:1","image_size":"4K","upscale_source":{"width":7,"height":5},"dimension_evidence":{"pixel_sizes":[{"aspect_ratio":"1:1","resolution":"4K","width":4096,"height":4096}]}}}),
    );
    let deviations = requirements::deviations(&receipt, &image, 0);
    assert_eq!(deviations.len(), 5);
    assert!(
        deviations.contains(&"Output 0 aspect ratio differs from requested aspect_ratio.".into())
    );
    assert!(deviations.contains(
        &"Output 0 dimensions differ from the documented requested resolution tier.".into()
    ));
    for ratio in ["7:5", "not-ratio", "0:1", "1:0"] {
        let receipt = Receipt::new(json!({"output_requirements":{"aspect_ratio":ratio}}));
        assert!(requirements::deviations(&receipt, &image, 0).is_empty());
    }
    let receipt = Receipt::new(
        json!({"output_requirements":{"aspect_ratio":"1:1","image_size":"1K","dimension_evidence":{"pixel_sizes":[{"aspect_ratio":"1:1","resolution":"1K","width":7,"height":5}]}}}),
    );
    assert!(requirements::deviations(&receipt, &image, 0).is_empty());
    assert!(requirements::deviations(&Receipt::new(json!({})), &image, 0).is_empty());
}
