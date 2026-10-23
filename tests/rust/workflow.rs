#[path = "common/config.rs"]
mod config_fixture;
#[path = "common/image.rs"]
mod image_fixture;
use numera_image_gen_mcp::compat::value::{Value, json};
use numera_image_gen_mcp::{
    adapters::{types::Input, workflow},
    files::images::{self, Image},
};
mod common;

// Source contracts: tests/contract/{comfyui,reference-preflight}.test.ts and
// tests/unit/workflow-validation.test.ts. These pure seams do not prove uploads or GPU access.
fn configured() -> Value {
    json!({"workflow":{
        "graph":{
            "text":{"class_type":"CLIPTextEncode","inputs":{"text":"original"}},
            "sampler":{"class_type":"KSampler","inputs":{"seed":0,"batch_size":1}},
            "load":{"class_type":"LoadImage","inputs":{"image":"original.png","mask":"mask.png"}},
            "second":{"class_type":"LoadImage","inputs":{"image":"second.png"}},
            "save":{"class_type":"SaveImage","inputs":{"images":["sampler",0]}}
        },
        "bindings":{
            "prompt":{"node":"text","input":"text"},
            "seed":{"node":"sampler","input":"seed"},
            "count":{"node":"sampler","input":"batch_size"},
            "image":{"node":"load","input":"image"},
            "image_2":{"node":"second","input":"image"},
            "mask":{"node":"load","input":"mask"}
        },"outputNodes":["save"],"requireGpu":true
    }})
}
fn request() -> Value {
    json!({"prompt":"Draw a café","count":1,"provider_options":{}})
}
fn input<'a>(
    connection: &'a Value,
    request: &'a Value,
    references: &'a [Image],
    mask: Option<&'a Image>,
) -> Input<'a> {
    Input {
        connection,
        request,
        references,
        mask,
        model: {
            static MODEL: std::sync::OnceLock<numera_image_gen_mcp::compat::value::JsString> =
                std::sync::OnceLock::new();
            MODEL.get_or_init(|| "workflow".into())
        },
        operation: "generate",
    }
}
fn code<T>(result: numera_image_gen_mcp::error::Result<T>) -> String {
    result.err().expect("invalid fixture must reject").code
}

#[test]
fn explicit_bindings_preserve_unicode_parameters_and_shared_graph() {
    let mut connection = configured();
    connection["workflow"]["bindings"]["strength"] = json!({"node":"sampler","input":"denoise"});
    let before = connection.clone();
    let request =
        json!({"prompt":"Draw a café","seed":42,"count":2,"provider_options":{"strength":0.7}});
    let mut graph = workflow::prepare(&input(&connection, &request, &[], None)).unwrap();
    assert_eq!(graph["text"]["inputs"]["text"], "Draw a café");
    assert_eq!(
        graph["sampler"]["inputs"],
        json!({"seed":42,"batch_size":2,"denoise":0.7})
    );
    workflow::bind(
        &mut graph,
        &connection["workflow"],
        "image",
        json!("uploaded.png"),
        true,
    )
    .unwrap();
    workflow::bind(
        &mut graph,
        &connection["workflow"],
        "optional-unbound",
        json!(1),
        false,
    )
    .unwrap();
    assert_eq!(graph["load"]["inputs"]["image"], "uploaded.png");
    assert_eq!(
        connection, before,
        "one operation must not mutate the configured workflow"
    );
}

#[test]
fn common_settings_and_provider_options_are_never_silently_dropped() {
    for field in [
        "size",
        "aspect_ratio",
        "image_size",
        "quality",
        "output_format",
        "background",
        "negative_prompt",
    ] {
        let mut request = request();
        request[field] = json!(if field == "output_format" {
            "png"
        } else {
            "fixture-value"
        });
        let mut connection = configured();
        assert_eq!(
            code(workflow::prepare(&input(&connection, &request, &[], None))),
            "unsupported_parameter",
            "{field}"
        );
        connection["workflow"]["bindings"][field] = json!({"node":"sampler","input":field});
        let graph = workflow::prepare(&input(&connection, &request, &[], None)).unwrap();
        assert_eq!(graph["sampler"]["inputs"][field], request[field], "{field}");
    }
    for mode in ["count", "provider"] {
        let mut connection = configured();
        let mut request = request();
        if mode == "count" {
            connection["workflow"]["bindings"]
                .as_object_mut()
                .unwrap()
                .remove("count");
            request["count"] = json!(2);
        } else {
            request["provider_options"] = json!({"unbound":1});
        }
        assert_eq!(
            code(workflow::prepare(&input(&connection, &request, &[], None))),
            "unsupported_parameter",
            "{mode}"
        );
    }
}

#[test]
fn absent_empty_oversized_ui_graphs_and_dangling_bindings_fail_closed() {
    let request = request();
    assert_eq!(
        code(workflow::prepare(&input(&json!({}), &request, &[], None))),
        "invalid_configuration"
    );
    for mode in ["empty", "oversized", "ui", "output", "binding", "prompt"] {
        let mut connection = configured();
        match mode {
            "empty" => connection["workflow"]["graph"] = json!({}),
            "oversized" => {
                connection["workflow"]["graph"] = Value::Object(
                    (0..301)
                        .map(|i| {
                            (
                                format!("node-{i}"),
                                json!({"class_type":"LoadImage","inputs":{}}),
                            )
                        })
                        .collect(),
                )
            }
            "ui" => connection["workflow"]["graph"]["text"] = json!({"widgets_values":["prompt"]}),
            "output" => connection["workflow"]["outputNodes"] = json!(["absent"]),
            "binding" => {
                connection["workflow"]["bindings"]["prompt"] =
                    json!({"node":"absent","input":"text"})
            }
            "prompt" => {
                connection["workflow"]["bindings"]
                    .as_object_mut()
                    .unwrap()
                    .remove("prompt");
            }
            _ => unreachable!(),
        }
        assert_eq!(
            code(workflow::prepare(&input(&connection, &request, &[], None))),
            if mode == "prompt" {
                "unsupported_parameter"
            } else {
                "invalid_configuration"
            },
            "{mode}"
        );
    }
}

#[test]
fn every_reference_and_mask_requires_an_existing_distinct_input_target() {
    let workspace = common::Workspace::new();
    let config = workspace.config("https://fixture.example", "comfyui");
    let image = images::inspect(image_fixture::original(), &config, None).unwrap();
    let references = vec![image.clone(), image.clone()];
    let request = request();
    for mode in [
        "second",
        "mask",
        "node",
        "slot",
        "collision",
        "changed-after-preflight",
    ] {
        let mut connection = configured();
        if mode == "changed-after-preflight" {
            workflow::prepare(&input(&connection, &request, &references, None)).unwrap();
        }
        match mode {
            "second" | "changed-after-preflight" => {
                connection["workflow"]["bindings"]
                    .as_object_mut()
                    .unwrap()
                    .remove("image_2");
            }
            "mask" => {
                connection["workflow"]["bindings"]
                    .as_object_mut()
                    .unwrap()
                    .remove("mask");
            }
            "node" => {
                connection["workflow"]["bindings"]["image_2"] =
                    json!({"node":"missing","input":"image"})
            }
            "slot" => {
                connection["workflow"]["bindings"]["image_2"] =
                    json!({"node":"second","input":"imagge"})
            }
            "collision" => {
                connection["workflow"]["bindings"]["image_2"] =
                    connection["workflow"]["bindings"]["image"].clone()
            }
            _ => unreachable!(),
        }
        assert_eq!(
            code(workflow::references(&input(
                &connection,
                &request,
                &references,
                if mode == "mask" { Some(&image) } else { None }
            ))),
            if matches!(mode, "second" | "mask" | "changed-after-preflight") {
                "unsupported_operation"
            } else {
                "invalid_configuration"
            },
            "{mode}"
        );
    }
    let connection = configured();
    workflow::references(&input(&connection, &request, &references, Some(&image))).unwrap();
}

#[test]
fn installed_descriptors_reject_enum_scalar_range_cycle_and_invalid_links() {
    let workflow = json!({"graph":{
        "a":{"class_type":"Source","inputs":{"count":1,"mode":"fast"}},
        "b":{"class_type":"Save","inputs":{"image":["a",0]}}
    },"bindings":{"prompt":{"node":"a","input":"mode"}},"outputNodes":["b"]});
    let info = json!({"Source":{"input":{"required":{"count":["INT",{"min":1,"max":2}],"mode":[["fast","slow"]]}},"output":["IMAGE"]},
        "Save":{"input":{"required":{"image":["IMAGE"]}},"output_node":true}});
    workflow::validate(&workflow["graph"], &workflow, &info).unwrap();
    for mode in [
        "enum",
        "cycle",
        "range-low",
        "range-high",
        "fractional",
        "scalar",
        "required",
        "dangling",
        "index",
        "index-negative",
        "index-fractional",
        "link-shape",
        "missing-node",
        "not-output",
        "string-scalar",
        "boolean-scalar",
        "descriptor-shape",
        "inputs-shape",
        "bounds-shape",
    ] {
        let mut graph = workflow["graph"].clone();
        let mut installed = info.clone();
        match mode {
            "enum" => graph["a"]["inputs"]["mode"] = json!("unsupported"),
            "cycle" => graph["a"]["inputs"]["count"] = json!(["b", 0]),
            "range-low" => graph["a"]["inputs"]["count"] = json!(0),
            "range-high" => graph["a"]["inputs"]["count"] = json!(3),
            "fractional" => graph["a"]["inputs"]["count"] = json!(1.5),
            "scalar" => graph["a"]["inputs"]["count"] = json!("1"),
            "required" => {
                graph["a"]["inputs"]
                    .as_object_mut()
                    .unwrap()
                    .remove("count");
            }
            "dangling" => graph["b"]["inputs"]["image"] = json!(["absent", 0]),
            "index" => graph["b"]["inputs"]["image"] = json!(["a", 1]),
            "index-negative" => graph["b"]["inputs"]["image"] = json!(["a", -1]),
            "index-fractional" => graph["b"]["inputs"]["image"] = json!(["a", 0.5]),
            "link-shape" => graph["b"]["inputs"]["image"] = json!(["a", 0, "extra"]),
            "missing-node" => {
                installed.as_object_mut().unwrap().remove("Source");
            }
            "not-output" => installed["Save"]["output_node"] = json!(false),
            "string-scalar" => {
                installed["Source"]["input"]["required"]["mode"] = json!(["STRING"]);
                graph["a"]["inputs"]["mode"] = json!(true);
            }
            "boolean-scalar" => {
                installed["Source"]["input"]["required"]["mode"] = json!(["BOOLEAN"]);
                graph["a"]["inputs"]["mode"] = json!("true");
            }
            "descriptor-shape" => installed["Source"]["input"] = json!(2),
            "inputs-shape" => graph["a"]["inputs"] = json!([]),
            "bounds-shape" => {
                installed["Source"]["input"]["required"]["count"] = json!(["INT", []])
            }
            _ => unreachable!(),
        }
        assert_eq!(
            code(workflow::validate(&graph, &workflow, &installed)),
            if mode == "missing-node" {
                "unsupported_operation"
            } else if matches!(mode, "descriptor-shape" | "inputs-shape" | "bounds-shape") {
                "invalid_response"
            } else {
                "invalid_configuration"
            },
            "{mode}"
        );
    }
}

#[test]
fn exact_source_installed_graph_preserves_enum_and_cycle_error_messages() {
    let workspace = common::Workspace::new();
    let raw = json!({
        "schemaVersion":1,"defaultConnection":"local",
        "outputDir":workspace.0.join("outputs").to_str().unwrap(),
        "stateDir":workspace.0.join("state").to_str().unwrap(),
        "logging":{"directory":workspace.0.join("logs").to_str().unwrap()},
        "files":{"allowedInputRoots":[workspace.0.join("inputs").to_str().unwrap()]},
        "connections":{"local":{
            "adapter":"comfyui","baseUrl":"https://api.example/v1","auth":{"type":"none"},
            "workflow":{
                "graph":{
                    "a":{"class_type":"Source","inputs":{"count":1,"mode":"fast"}},
                    "b":{"class_type":"Save","inputs":{"image":["a",0]}}
                },
                "bindings":{"prompt":{"node":"a","input":"mode"}},"outputNodes":["b"]
            }
        }}
    });
    let connection = numera_image_gen_mcp::config::Config::parse(
        &raw.to_string(),
        &workspace.0.join("config.json"),
        &[],
        &std::collections::HashMap::new(),
    )
    .unwrap();
    let configured = &connection.value["connections"]["local"]["workflow"];
    let info = json!({
        "Source":{"input":{"required":{"count":["INT",{"min":1,"max":2}],"mode":[["fast","slow"]]}},"output":["IMAGE"]},
        "Save":{"input":{"required":{"image":["IMAGE"]}},"output_node":true}
    });
    workflow::validate(&configured["graph"], configured, &info).unwrap();
    let mut bad = configured["graph"].clone();
    bad["a"]["inputs"]["mode"] = json!("unsupported");
    let error = workflow::validate(&bad, configured, &info).unwrap_err();
    assert!(error.message.contains("enum"));
    let mut cycle = configured["graph"].clone();
    cycle["a"]["inputs"]["count"] = json!(["b", 0]);
    let error = workflow::validate(&cycle, configured, &info).unwrap_err();
    assert!(error.message.contains("cycle"));
}
