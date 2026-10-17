use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::{logging::redact, runtime_paths::RuntimePaths};
mod common;

#[test]
fn scratch_is_project_owned_and_escape_is_rejected() {
    let workspace = common::Workspace::new();
    let paths = RuntimePaths::new(&workspace.0).expect("local runtime directories");
    assert!(
        paths
            .temp
            .starts_with(std::fs::canonicalize(&workspace.0).unwrap())
    );
    assert!(
        paths
            .cache
            .starts_with(std::fs::canonicalize(&workspace.0).unwrap())
    );
    assert!(paths.verify(&workspace.0.join("../outside")).is_err());
    assert!(paths.verify(&paths.temp).is_ok());
}

#[test]
fn exact_source_nested_signed_query_redaction_preserves_status_without_private_values() {
    let result = redact(
        &json!({
            "status": 401,
            "nested": {
                "authorization": "Bearer private",
                "prompt": "private prompt",
                "url": "https://a.example/img?signature=private"
            }
        }),
        0,
    )
    .to_string();
    assert!(!result.contains("private"));
    assert!(result.contains("401"));
}

#[test]
fn fixture_cleanup_retries_only_documented_windows_transient_errors() {
    for code in [32, 145, 303] {
        assert!(common::cleanup_retryable(
            &std::io::Error::from_raw_os_error(code)
        ));
    }
    for code in [5, 2, 87] {
        assert!(!common::cleanup_retryable(
            &std::io::Error::from_raw_os_error(code)
        ));
    }
    assert!(!common::cleanup_retryable(&std::io::Error::other(
        "unknown fixture error"
    )));
}

#[test]
fn secrets_are_redacted_without_replacing_safe_metadata() {
    let value = redact(
        &json!({
            "prompt": "private", "Authorization": "Bearer synthetic", "count": 1,
            "nested": {"path": "private", "url": "https://example.org/image?secret=value"}
        }),
        0,
    );
    assert_eq!(value["prompt"], "[REDACTED]");
    assert_eq!(value["Authorization"], "[REDACTED]");
    assert_eq!(value["count"], 1);
    assert_eq!(value["nested"]["path"], "[REDACTED]");
    assert_eq!(value["nested"]["url"], "https://example.org/image");
}
