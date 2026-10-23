use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    capabilities::{capabilities, effective_request, safe_options},
    dimensions::{evidence, image_dimensions, validate},
    error::Result,
};

fn outcome(result: Result<Value>) -> Value {
    match result {
        Ok(value) => json!({"value":value}),
        Err(error) => json!({"error":error.code}),
    }
}

#[test]
fn source_oracle_policy_dimensions_and_effective_request_match() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".ci-work/parity/policy.json");
    let data = std::fs::read(path).expect("source-generated policy fixtures required");
    let oracle: Value = serde_json::from_slice(&data).expect("valid source oracle");
    for group in ["policy", "effective", "options", "dimensionValidation"] {
        assert!(!oracle[group].as_array().expect("required group").is_empty());
    }
    for case in oracle["policy"].as_array().unwrap() {
        let c = &case["connection"];
        let model = case["model"].as_js_string().unwrap();
        assert_eq!(
            capabilities(c, model),
            case["capabilities"],
            "{}",
            case["name"]
        );
        assert_eq!(
            image_dimensions(c, model, 64000000),
            case["dimensions"],
            "{}",
            case["name"]
        );
        assert_eq!(evidence(c, model), case["evidence"], "{}", case["name"]);
    }
    for case in oracle["effective"].as_array().unwrap() {
        assert_eq!(
            outcome(effective_request(
                &case["connection"],
                &case["request"],
                case["model"].as_js_string().unwrap()
            )),
            case["expected"],
            "{}",
            case["name"]
        );
    }
    for case in oracle["options"].as_array().unwrap() {
        assert_eq!(
            outcome(safe_options(&case["input"], None, 0)),
            case["expected"]
        );
    }
    for case in oracle["dimensionValidation"].as_array().unwrap() {
        assert_eq!(
            outcome(
                validate(
                    &case["connection"],
                    case["model"].as_js_string().unwrap(),
                    &case["request"]
                )
                .map(|()| json!(true))
            ),
            case["expected"]
        );
    }
}
