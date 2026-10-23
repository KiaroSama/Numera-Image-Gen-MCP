mod common;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use image::GenericImageView;
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::files::{editing, images, paths};

#[test]
fn request_seed_matches_source_safe_integer_domain_before_submission() {
    use numera_image_gen_mcp::services::request;
    for seed in [-9007199254740991.0, -0.0, 9007199254740991.0] {
        assert!(request::parse(json!({"prompt":"draw","seed":seed})).is_ok());
    }
    for seed in [-9007199254740992.0, 1.5, 9007199254740992.0] {
        assert_eq!(
            request::parse(json!({"prompt":"draw","seed":seed}))
                .unwrap_err()
                .code,
            "invalid_input"
        );
    }
}

#[test]
fn region_masks_preserve_polarity_pixels_and_reject_outside_bounds() {
    let config = json!({"files":{"maxPixels":64000000}});
    let source = images::inspect(image_fixture::original(), &config, None).unwrap();
    let request = json!({"edit_region":{"x":1,"y":1,"width":2,"height":2}});
    for (polarity, outside, inside) in [
        ("transparent-edit", [255, 255, 255, 255], [255, 255, 255, 0]),
        ("white-edit", [0, 0, 0, 255], [255, 255, 255, 255]),
        ("black-edit", [255, 255, 255, 255], [0, 0, 0, 255]),
    ] {
        let mask = editing::region_mask(
            &request,
            &source,
            &json!({"edit":{"maskPolarity":polarity}}),
            &config,
        )
        .unwrap();
        let decoded = image::load_from_memory(&mask.bytes).unwrap();
        assert_eq!(decoded.get_pixel(0, 0).0, outside);
        assert_eq!(decoded.get_pixel(1, 1).0, inside);
        images::validate_mask(&mask, &source).unwrap();
    }
    assert_eq!(
        editing::region_mask(&request, &source, &json!({}), &config)
            .err()
            .unwrap()
            .code,
        "unsupported_operation"
    );
    assert_eq!(
        editing::region_mask(
            &json!({"edit_region":{"x":source.width,"y":0,"width":1,"height":1}}),
            &source,
            &json!({"edit":{"maskPolarity":"transparent-edit"}}),
            &config
        )
        .err()
        .unwrap()
        .code,
        "invalid_input"
    );
    assert_eq!(
        editing::region_mask(
            &request,
            &source,
            &json!({"edit":{"maskPolarity":"transparent-edit"}}),
            &json!({"files":{"maxAggregateBytes":1}})
        )
        .err()
        .unwrap()
        .code,
        "invalid_input"
    );
}

#[test]
fn exact_source_absolute_unicode_file_urls_limits_and_invalid_inputs_are_enforced() {
    let workspace = Workspace::new();
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".ci-work/parity/image-acceptance.json");
    let oracle = numera_image_gen_mcp::compat::value::Value::parse_with_limit(
        &std::fs::read_to_string(file).expect("exact source image oracle"),
        1048576,
    )
    .unwrap();
    let original = oracle
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "png")
        .unwrap();
    let bytes = images::decode_base64(original["base64"].as_str().unwrap(), 1048576).unwrap();
    let inputs = workspace.0.join("inputs");
    std::fs::create_dir(&inputs).unwrap();
    let path = inputs.join("café image.png");
    std::fs::write(&path, &bytes).unwrap();
    let roots = std::slice::from_ref(&inputs);
    assert_eq!(
        paths::input(path.to_str().unwrap(), roots, bytes.len()).unwrap(),
        bytes
    );
    assert_eq!(
        paths::input(
            url::Url::from_file_path(&path).unwrap().as_str(),
            roots,
            bytes.len()
        )
        .unwrap(),
        bytes
    );
    assert_eq!(
        paths::input(
            path.to_str().unwrap(),
            &[workspace.0.join("not-created")],
            bytes.len()
        )
        .unwrap_err()
        .code,
        "permission_denied"
    );
    assert_eq!(
        paths::input(path.to_str().unwrap(), roots, bytes.len() - 1)
            .unwrap_err()
            .code,
        "input_file_error"
    );
    assert_eq!(
        paths::input(inputs.to_str().unwrap(), roots, bytes.len())
            .unwrap_err()
            .code,
        "input_file_error"
    );
    for invalid in [
        "relative.png".to_owned(),
        "file:///bad%ZZ".to_owned(),
        inputs.join("absent.png").to_str().unwrap().to_owned(),
        format!("{}:stream", path.to_str().unwrap()),
    ] {
        assert_eq!(
            paths::input(&invalid, roots, bytes.len()).unwrap_err().code,
            "input_file_error"
        );
    }
}

#[tokio::test]
async fn exact_source_input_links_output_junctions_non_directories_and_nested_unicode_are_checked()
{
    let scenario = async {
        let workspace = Workspace::new();
        let inputs = workspace.0.join("inputs");
        let outside = workspace.0.join("outside");
        let outputs = workspace.0.join("outputs");
        for directory in [&inputs, &outside, &outputs] {
            std::fs::create_dir(directory).unwrap();
        }
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".ci-work/parity/image-acceptance.json");
        let cases = numera_image_gen_mcp::compat::value::Value::parse_with_limit(
            &std::fs::read_to_string(file).unwrap(),
            1048576,
        )
        .unwrap();
        let case = cases
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == "png")
            .unwrap();
        let bytes = images::decode_base64(case["base64"].as_str().unwrap(), 1048576).unwrap();
        std::fs::write(outside.join("image.png"), bytes).unwrap();
        for parent in ["inputs", "outputs"] {
            let target = workspace.0.join(parent).join("link");
            #[cfg(unix)]
            std::os::unix::fs::symlink(&outside, &target).unwrap();
            #[cfg(windows)]
            {
                let mut child = tokio::process::Command::new("cmd.exe")
                    .args(["/D", "/C", "mklink", "/J"])
                    .arg(std::path::Path::new(parent).join("link"))
                    .arg("outside")
                    .current_dir(std::fs::canonicalize(&workspace.0).unwrap())
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .creation_flags(0x08000000)
                    .kill_on_drop(true)
                    .spawn()
                    .unwrap();
                match tokio::time::timeout(std::time::Duration::from_secs(2), child.wait()).await {
                    Ok(status) => {
                        assert!(status.unwrap().success(), "owned junction creation failed")
                    }
                    Err(_) => {
                        tokio::time::timeout(std::time::Duration::from_secs(2), child.kill())
                            .await
                            .expect("owned junction child must be reaped")
                            .unwrap();
                        panic!("owned junction creation exceeded 2 seconds");
                    }
                }
            }
            assert!(std::fs::symlink_metadata(&target).unwrap().is_symlink());
        }
        assert_eq!(
            paths::input(
                inputs.join("link/image.png").to_str().unwrap(),
                std::slice::from_ref(&inputs),
                1000
            )
            .unwrap_err()
            .code,
            "permission_denied"
        );
        assert_eq!(
            paths::output_directory(&outputs, Some("link/subdir"))
                .unwrap_err()
                .code,
            "output_file_error"
        );
        std::fs::write(outputs.join("not-directory"), b"fixture").unwrap();
        assert_eq!(
            paths::output_directory(&outputs, Some("not-directory/subdir"))
                .unwrap_err()
                .code,
            "output_file_error"
        );
        assert_eq!(
            paths::output_directory(&outputs, Some("nested/café")).unwrap(),
            std::fs::canonicalize(outputs.join("nested/café")).unwrap()
        );
        assert_eq!(
            paths::output_directory(&outputs, Some("../escape"))
                .unwrap_err()
                .code,
            "invalid_input"
        );
    };
    tokio::time::timeout(std::time::Duration::from_secs(8), scenario)
        .await
        .expect("exact source link scenario exceeded 8 seconds");
}

#[test]
fn exact_source_nested_unicode_and_sibling_containment_use_whole_path_components() {
    assert_eq!(
        paths::safe_relative("batch/café-output").unwrap(),
        "batch/café-output"
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/allowed");
    assert!(root.starts_with(&root));
    assert!(root.join("a.png").starts_with(&root));
    let sibling = root.parent().unwrap().join("allowed-other/a.png");
    assert!(!sibling.starts_with(&root));
}

#[test]
fn exact_source_backslash_traversal_and_nul_relative_paths_are_rejected() {
    for path in ["a\\..\\b", "a\0b"] {
        assert_eq!(
            paths::safe_relative(path).unwrap_err().code,
            "invalid_input"
        );
    }
}

#[test]
fn editing_contracts_and_contained_unicode_files_preserve_original_bytes() {
    let workspace = Workspace::new();
    let bytes = image_fixture::original();
    let root = workspace.0.join("inputs");
    std::fs::create_dir(&root).unwrap();
    let file = root.join("café-日本語.png");
    std::fs::write(&file, &bytes).unwrap();
    assert_eq!(
        paths::input(
            file.to_str().unwrap(),
            std::slice::from_ref(&root),
            bytes.len()
        )
        .unwrap(),
        bytes
    );
    let url = url::Url::from_file_path(&file).unwrap();
    assert_eq!(
        paths::input(url.as_str(), std::slice::from_ref(&root), bytes.len()).unwrap(),
        bytes
    );
    assert_eq!(
        paths::input(file.to_str().unwrap(), &[], bytes.len())
            .unwrap_err()
            .code,
        "permission_denied"
    );
    assert_eq!(
        paths::input(file.to_str().unwrap(), std::slice::from_ref(&root), 1)
            .unwrap_err()
            .code,
        "input_file_error"
    );
    for path in [
        "",
        "../escape",
        "a/../b",
        "./a",
        "/absolute",
        "a//b",
        "CON.png",
        "lpt1",
        "a.",
        "a ",
        "a:stream",
        "a?b",
    ] {
        assert_eq!(
            paths::safe_relative(path).unwrap_err().code,
            "invalid_input"
        );
    }
    assert_eq!(
        paths::safe_relative("batch/café-output").unwrap(),
        "batch/café-output"
    );
    let outputs = workspace.0.join("outputs");
    let directory = paths::output_directory(&outputs, Some("batch/café-output")).unwrap();
    assert!(directory.starts_with(std::fs::canonicalize(outputs).unwrap()));
    for size in ["0x10", "01x10", "10x0", "10x01", "10x", "10X10", "abc"] {
        assert!(editing::dimensions(size).is_none());
    }
    assert_eq!(editing::dimensions("16x32"), Some((16, 32)));
    let translated = editing::request(
        json!({"prompt":"Preserve drawing","reference_images":[{}],"target_language":"Japanese"}),
        "edit",
    )
    .unwrap();
    assert!(
        translated["prompt"]
            .as_str()
            .unwrap()
            .contains("Replace the original text inside the image")
    );
    let upscale=editing::request(json!({"prompt":"Preserve drawing","reference_images":[{}],"upscale":true,"count":1,"size":"32x32"}),"edit").unwrap();
    assert!(
        upscale["prompt"]
            .as_str()
            .unwrap()
            .contains("Do not add or remove objects")
    );
    for request in [
        json!({"target_language":"Japanese"}),
        json!({"reference_images":[{}],"mask":{},"edit_region":{}}),
        json!({"reference_images":[{},{}],"upscale":true,"count":1,"size":"32x32"}),
    ] {
        assert_eq!(
            editing::request(request, "edit").unwrap_err().code,
            "invalid_input"
        );
    }
}
