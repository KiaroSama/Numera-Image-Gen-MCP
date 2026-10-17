use numera_image_gen_mcp::compat::canonical_json::{canonical_json, fingerprint};
use numera_image_gen_mcp::compat::value::Value;

#[test]
fn sha256_matches_published_known_answer() {
    assert_eq!(
        numera_image_gen_mcp::compat::hash::sha256(b"hello world"),
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
}

#[test]
fn generated_source_canonicalization_cases_are_nonempty_and_exact() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = root.join(".ci-work/parity/canonical.json");
    let bytes = std::fs::read(fixture).expect("CI must generate source oracle fixtures first");
    let cases: Vec<Value> = serde_json::from_slice(&bytes).expect("valid generated oracle");
    assert!(!cases.is_empty(), "an empty oracle must never pass parity");
    for case in cases {
        let name = case["name"].as_str().expect("named oracle case");
        let input = &case["input"];
        assert_eq!(
            canonical_json(input).expect("canonical input"),
            case["canonical"].as_str().expect("canonical oracle string"),
            "{name}"
        );
        assert_eq!(
            fingerprint(input).expect("source-compatible hash"),
            case["fingerprint"]
                .as_str()
                .expect("fingerprint oracle string"),
            "{name}"
        );
    }
}
