mod common;
#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use common::Workspace;
use numera_image_gen_mcp::compat::value::json;
use numera_image_gen_mcp::{
    files::{images::inspect, output},
    jobs::{Receipt, Store},
    services::requirements,
};

fn receipt(id: &str) -> Receipt {
    Receipt::new(
        json!({"request_id":id,"connection":"test","status":"prepared","outputs":[],"errors":[],"warnings":[],"deviations":[],
        "generation_outcome":"not_submitted","created_at":"2026-10-11T00:00:00.000Z","updated_at":"2026-10-11T00:00:00.000Z"}),
    )
}

#[test]
fn writer_fences_terminal_accounting_and_original_journal_survive_reopen() {
    let workspace = Workspace::new();
    let config = workspace.config("https://invalid.example/v1", "openai-images");
    let root = workspace.0.join("state");
    let mut one = Store::open(&root).unwrap();
    let mut two = Store::open(&root).unwrap();
    let (_, mut first) = one
        .prepare(receipt("same"), "effective", "account")
        .unwrap();
    assert!(one.admit("same", "test", 1, 1).unwrap());
    first.value["status"] = json!("running");
    first.value["upstream_job"] = json!({"id":"original","kind":"responses"});
    one.update(&mut first).unwrap();
    assert!(!two.claim("same", None, false).unwrap());
    let closed = two
        .terminate(
            "same",
            "failed",
            &json!({"code":"provider_rejection"}),
            None,
            Some(&json!({"id":"original","kind":"responses"})),
        )
        .unwrap();
    assert_eq!(closed.value["upstream_terminal"], "failed");
    assert!(!one.owns("same", Some(&first)).unwrap());
    assert!(one.update(&mut first).is_err());
    let (_, second) = two.prepare(receipt("next"), "other", "account").unwrap();
    assert!(two.admit("next", "test", 1, 1).unwrap());
    two.release("next", Some(&second)).unwrap();
    let (_, mut original) = two.prepare(receipt("output"), "image", "account").unwrap();
    assert!(two.admit("output", "test", 32, 32).unwrap());
    original.value["status"] = json!("submitting");
    two.update(&mut original).unwrap();
    assert!(two.claim("output", Some(&original), false).unwrap());
    original = two.get("output").unwrap();
    two.reserve("output", 1048576, 2097152).unwrap();
    let bytes = image_fixture::original();
    let image = inspect(bytes.clone(), &config, None).unwrap();
    two.retain("output", 0, &bytes, Some(&original)).unwrap();
    let pending = two.pending("output").unwrap();
    assert_eq!(pending.len(), 1);
    let stable = pending[0].output_id.clone();
    let output = output::save(
        &image,
        &config,
        &mut two,
        output::Publication {
            id: "output",
            index: 0,
            subdir: Some("saved"),
            prefix: None,
            deviations: &[],
            expected: Some(&original),
            stable: true,
        },
    )
    .unwrap();
    assert_eq!(output["output_id"], stable);
    assert!(!two.has_pending("output").unwrap());
    assert_eq!(
        two.get("output").unwrap().value["outputs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    two.release("output", Some(&original)).unwrap();
    drop(one);
    drop(two);
    let reopened = Store::open(&root).unwrap();
    let (_, saved) = output::owned(&config, &reopened, &stable).unwrap();
    assert_eq!(saved.bytes, bytes);
    assert_eq!(
        reopened.get("same").unwrap().value["upstream_terminal"],
        "failed"
    );
}

#[test]
fn documented_resolution_and_original_format_deviations_are_reported_not_resized() {
    let workspace = Workspace::new();
    let config = workspace.config("https://invalid.example/v1", "openai-images");
    let image = inspect(image_fixture::original(), &config, None).unwrap();
    let mut r = receipt("requirements");
    r.value["output_requirements"] = json!({"size":"1024x1024","output_format":"jpeg","background":"transparent","aspect_ratio":"1:1","image_size":"4K","upscale_source":{"width":8,"height":6},
        "dimension_evidence":{"pixel_sizes":[{"aspect_ratio":"1:1","resolution":"4K","width":4096,"height":4096}]}});
    let d = requirements::deviations(&r, &image, 0);
    assert!(d.iter().any(|s| s.contains("format")));
    assert!(d.iter().any(|s| s.contains("dimensions")));
    assert!(d.iter().any(|s| s.contains("not upscaled")));
    assert!(requirements::warnings(&r).is_empty());
}
