mod common;
#[path = "common/comfy_source.rs"]
mod source;
use common::Workspace;
use numera_image_gen_mcp::{
    adapters::workflow,
    compat::value::{Value, json},
    services::request,
};

fn connection(workspace: &Workspace, configured: Value) -> Value {
    source::config(
        workspace,
        "https://fixture.example",
        json!({"workflow":configured}),
    )
    .select(None)
    .unwrap()
    .1
}

#[test]
fn exact_source_prompt_seed_count_options_bind_on_clone_without_changing_configuration() {
    let workspace = Workspace::new();
    let mut configured = source::workflow();
    configured["bindings"]["strength"] = json!({"node":"sampler","input":"denoise"});
    let connection = connection(&workspace, configured.clone());
    let request = request::parse(json!({
        "prompt":"Draw a café","seed":42,"count":2,"provider_options":{"strength":0.7}
    }))
    .unwrap();
    let mut graph =
        workflow::prepare(&source::input(&connection, &request, &[], None, "generate")).unwrap();
    assert_eq!(graph["text"]["inputs"]["text"], "Draw a café");
    assert_eq!(
        graph["sampler"]["inputs"],
        json!({"seed":42,"batch_size":2,"denoise":0.7})
    );
    assert_eq!(connection["workflow"]["graph"], configured["graph"]);
    workflow::bind(
        &mut graph,
        &connection["workflow"],
        "image",
        json!("uploaded.png"),
        true,
    )
    .unwrap();
    assert_eq!(graph["load"]["inputs"]["image"], "uploaded.png");
    assert_eq!(
        configured["graph"]["load"]["inputs"]["image"],
        "original.png"
    );
    workflow::bind(
        &mut graph,
        &connection["workflow"],
        "optional-unbound",
        json!(1),
        false,
    )
    .unwrap();
}

fn common_field(field: &str) {
    let workspace = Workspace::new();
    let value = if field == "output_format" {
        "png"
    } else {
        "fixture-value"
    };
    let mut raw = json!({"prompt":"Draw a café"});
    raw[field] = json!(value);
    let request = request::parse(raw).unwrap();
    let first = connection(&workspace, source::workflow());
    assert_eq!(
        workflow::prepare(&source::input(&first, &request, &[], None, "generate"))
            .unwrap_err()
            .code,
        "unsupported_parameter"
    );
    let mut configured = source::workflow();
    configured["bindings"][field] = json!({"node":"sampler","input":field});
    let second = connection(&workspace, configured);
    let graph =
        workflow::prepare(&source::input(&second, &request, &[], None, "generate")).unwrap();
    assert_eq!(graph["sampler"]["inputs"][field], value);
}

#[test]
fn exact_source_size_requires_explicit_binding() {
    common_field("size");
}
#[test]
fn exact_source_aspect_ratio_requires_explicit_binding() {
    common_field("aspect_ratio");
}
#[test]
fn exact_source_image_size_requires_explicit_binding() {
    common_field("image_size");
}
#[test]
fn exact_source_quality_requires_explicit_binding() {
    common_field("quality");
}
#[test]
fn exact_source_output_format_requires_explicit_binding() {
    common_field("output_format");
}
#[test]
fn exact_source_background_requires_explicit_binding() {
    common_field("background");
}
#[test]
fn exact_source_negative_prompt_requires_explicit_binding() {
    common_field("negative_prompt");
}

#[test]
fn exact_source_invalid_graphs_and_unbound_inputs_reject_in_original_order() {
    let workspace = Workspace::new();
    let mut absent = connection(&workspace, source::workflow());
    absent.as_object_mut().unwrap().remove("workflow");
    let request = request::parse(json!({"prompt":"Draw a café"})).unwrap();
    assert_eq!(
        workflow::prepare(&source::input(&absent, &request, &[], None, "generate"))
            .unwrap_err()
            .code,
        "invalid_configuration"
    );
    for mode in ["empty", "oversized", "ui", "output", "binding"] {
        let mut configured = source::workflow();
        match mode {
            "empty" => configured["graph"] = json!({}),
            "oversized" => {
                configured["graph"] = Value::Object(
                    (0..301)
                        .map(|index| {
                            (
                                format!("node-{index}"),
                                json!({"class_type":"LoadImage","inputs":{}}),
                            )
                        })
                        .collect(),
                )
            }
            "ui" => configured["graph"]["text"] = json!({"widgets_values":["prompt"]}),
            "output" => configured["outputNodes"] = json!(["absent"]),
            "binding" => configured["bindings"]["prompt"] = json!({"node":"absent","input":"text"}),
            _ => unreachable!(),
        }
        // The source mutates an already parsed profile; its raw UI graph is not reparsed.
        let mut profile = connection(&workspace, source::workflow());
        profile["workflow"] = configured;
        assert_eq!(
            workflow::prepare(&source::input(&profile, &request, &[], None, "generate"))
                .unwrap_err()
                .code,
            "invalid_configuration",
            "{mode}"
        );
    }
    let mut no_prompt = source::workflow();
    no_prompt["bindings"]
        .as_object_mut()
        .unwrap()
        .remove("prompt");
    let profile = connection(&workspace, no_prompt);
    assert_eq!(
        workflow::prepare(&source::input(&profile, &request, &[], None, "generate"))
            .unwrap_err()
            .code,
        "unsupported_parameter"
    );
    let mut no_count = source::workflow();
    no_count["bindings"]
        .as_object_mut()
        .unwrap()
        .remove("count");
    let profile = connection(&workspace, no_count);
    let count = request::parse(json!({"prompt":"Draw a café","count":2})).unwrap();
    assert_eq!(
        workflow::prepare(&source::input(&profile, &count, &[], None, "generate"))
            .unwrap_err()
            .code,
        "unsupported_parameter"
    );
    let profile = connection(&workspace, source::workflow());
    let options =
        request::parse(json!({"prompt":"Draw a café","provider_options":{"unbound":1}})).unwrap();
    assert_eq!(
        workflow::prepare(&source::input(&profile, &options, &[], None, "generate"))
            .unwrap_err()
            .code,
        "unsupported_parameter"
    );
}
