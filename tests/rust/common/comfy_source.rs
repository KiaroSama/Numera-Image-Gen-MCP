use super::common::Workspace;
use numera_image_gen_mcp::{
    adapters::types::Input,
    compat::value::{JsString, Value, json},
    config::Config,
    files::images::Image,
};
use std::collections::HashMap;

pub fn workflow() -> Value {
    json!({
        "graph":{
            "text":{"class_type":"CLIPTextEncode","inputs":{"text":"original"}},
            "sampler":{"class_type":"KSampler","inputs":{"seed":0,"batch_size":1}},
            "load":{"class_type":"LoadImage","inputs":{"image":"original.png","mask":"mask.png"}},
            "second":{"class_type":"LoadImage","inputs":{"image":"second.png"}},
            "save":{"class_type":"SaveImage","inputs":{"images":["sampler",0]}}
        },
        "bindings":{
            "prompt":{"node":"text","input":"text"},"seed":{"node":"sampler","input":"seed"},
            "count":{"node":"sampler","input":"batch_size"},"image":{"node":"load","input":"image"},
            "image_2":{"node":"second","input":"image"},"mask":{"node":"load","input":"mask"}
        },"outputNodes":["save"],"requireGpu":true
    })
}

pub fn config(workspace: &Workspace, origin: &str, overrides: Value) -> Config {
    let mut profile = json!({
        "adapter":"comfyui","baseUrl":origin,"auth":{"type":"none"},
        "workflow":workflow(),"discoveryTimeoutMs":2000,"requestTimeoutMs":2000
    });
    profile
        .as_object_mut()
        .unwrap()
        .extend(overrides.as_object().unwrap().clone());
    Config::parse(
        &json!({
            "schemaVersion":1,"defaultConnection":"local",
            "outputDir":workspace.0.join("outputs").to_str().unwrap(),
            "stateDir":workspace.0.join("state").to_str().unwrap(),
            "logging":{"directory":workspace.0.join("logs").to_str().unwrap()},
            "files":{"allowedInputRoots":[workspace.0.join("inputs").to_str().unwrap()]},
            "connections":{"local":profile}
        })
        .to_string(),
        &workspace.0.join("config.json"),
        &[],
        &HashMap::new(),
    )
    .unwrap()
}

pub fn input<'a>(
    connection: &'a Value,
    request: &'a Value,
    references: &'a [Image],
    mask: Option<&'a Image>,
    operation: &'a str,
) -> Input<'a> {
    static MODEL: std::sync::OnceLock<JsString> = std::sync::OnceLock::new();
    Input {
        connection,
        request,
        references,
        mask,
        operation,
        model: MODEL.get_or_init(|| "workflow".into()),
    }
}
