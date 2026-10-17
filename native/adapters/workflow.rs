use super::types::Input;
use crate::compat::value::{JsString, Value};
use crate::error::{NumeraError, Result, object};
use std::collections::HashSet;

fn invalid(message: &str) -> NumeraError {
    NumeraError::new("invalid_configuration", message)
}

pub fn references(input: &Input<'_>) -> Result<()> {
    let workflow = input
        .connection
        .get("workflow")
        .ok_or_else(|| invalid("Configure an explicit ComfyUI workflow."))?;
    let mut required = (0..input.references.len())
        .map(|i| {
            if i == 0 {
                "image".into()
            } else {
                format!("image_{}", i + 1)
            }
        })
        .collect::<Vec<_>>();
    if input.mask.is_some() {
        required.push("mask".into());
    }
    for key in required {
        let target = workflow["bindings"].get(&key).ok_or_else(|| {
            NumeraError::new(
                "unsupported_operation",
                &format!("Workflow lacks {key} reference binding."),
            )
        })?;
        let node_id = target["node"]
            .as_js_string()
            .ok_or_else(|| invalid("Invalid reference binding node."))?;
        let slot = target["input"]
            .as_js_string()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                invalid(&format!(
                    "Workflow binding {key} has no valid input target."
                ))
            })?;
        let node = workflow["graph"].get(node_id).ok_or_else(|| {
            invalid(&format!(
                "Workflow binding {key} has no valid input target."
            ))
        })?;
        let inputs = object(node)?.get("inputs").ok_or_else(|| {
            invalid(&format!(
                "Workflow binding {key} has no valid input target."
            ))
        })?;
        if !object(inputs)?.contains_key(slot) {
            return Err(invalid(&format!(
                "Workflow binding {key} references an absent input slot."
            )));
        }
        if object(&workflow["bindings"])?.iter().any(|(other, v)| {
            other != &key && v["node"] == target["node"] && v["input"] == target["input"]
        }) {
            return Err(invalid(
                "Reference bindings must have distinct input targets.",
            ));
        }
    }
    Ok(())
}

pub fn bind(
    graph: &mut Value,
    workflow: &Value,
    key: impl Into<JsString>,
    value: Value,
    required: bool,
) -> Result<()> {
    let key = key.into();
    let Some(target) = workflow["bindings"].get(&key) else {
        return if required {
            Err(NumeraError::new(
                "unsupported_parameter",
                &format!("ComfyUI workflow has no {key} binding."),
            ))
        } else {
            Ok(())
        };
    };
    let id = target["node"]
        .as_js_string()
        .ok_or_else(|| invalid("Invalid workflow node binding."))?;
    let slot = target["input"]
        .as_js_string()
        .ok_or_else(|| invalid("Invalid workflow input binding."))?;
    let node = graph.get_mut(id).ok_or_else(|| {
        invalid(&format!(
            "Workflow binding {key} references an absent node."
        ))
    })?;
    node["inputs"]
        .as_object_mut()
        .ok_or_else(|| {
            NumeraError::new("invalid_response", "Expected an object.").stage("response")
        })?
        .insert(slot.into(), value);
    Ok(())
}

pub fn prepare(input: &Input<'_>) -> Result<Value> {
    let workflow = input.connection.get("workflow").ok_or_else(|| {
        invalid("Configure an API-format ComfyUI workflow and explicit bindings.")
    })?;
    references(input)?;
    let mut graph = crate::config::clone_json(&workflow["graph"]);
    if object(&graph)?.is_empty() || object(&graph)?.len() > 300 {
        return Err(invalid("Invalid bounded ComfyUI graph."));
    }
    for (id, node) in object(&graph)? {
        if !object(node)?
            .get("class_type")
            .is_some_and(Value::is_string)
            || node.get("inputs").is_none()
        {
            return Err(invalid(&format!("Node {id} is not API format.")));
        }
        object(&node["inputs"])?;
    }
    bind(
        &mut graph,
        workflow,
        "prompt",
        input.request["prompt"].clone(),
        true,
    )?;
    for key in crate::capabilities::COMMON_FIELDS {
        if let Some(value) = input.request.get(*key) {
            bind(&mut graph, workflow, key, value.clone(), true)?;
        }
    }
    if input.request["count"].as_u64().unwrap_or(1) > 1 {
        bind(
            &mut graph,
            workflow,
            "count",
            input.request["count"].clone(),
            true,
        )?;
    }
    for (key, value) in object(&input.request["provider_options"])? {
        bind(&mut graph, workflow, key, value.clone(), true)?;
    }
    for id in crate::error::array(&workflow["outputNodes"]) {
        if graph
            .get(
                id.as_js_string()
                    .ok_or_else(|| invalid("Invalid output node ID."))?,
            )
            .is_none()
        {
            return Err(invalid("Output binding references an absent node."));
        }
    }
    Ok(graph)
}

pub fn validate(graph: &Value, workflow: &Value, info: &Value) -> Result<()> {
    fn visit<'a>(
        id: &'a JsString,
        graph: &'a Value,
        info: &Value,
        visiting: &mut HashSet<&'a JsString>,
        visited: &mut HashSet<&'a JsString>,
    ) -> Result<()> {
        if visiting.contains(id) {
            return Err(invalid("ComfyUI workflow contains a cycle."));
        }
        if visited.contains(id) {
            return Ok(());
        }
        let node = graph
            .get(id)
            .ok_or_else(|| invalid("Workflow link does not reference a valid node output."))?;
        object(node)?;
        visiting.insert(id);
        let kind = node["class_type"]
            .as_js_string()
            .ok_or_else(|| invalid("Invalid workflow class type."))?;
        let descriptor = info.get(kind).ok_or_else(|| {
            NumeraError::new(
                "unsupported_operation",
                "Workflow requires a node not installed on this backend.",
            )
        })?;
        object(descriptor)?;
        let empty = crate::compat::value::json!({});
        let input_info = descriptor.get("input").unwrap_or(&empty);
        object(input_info)?;
        let required = input_info.get("required").unwrap_or(&empty);
        let optional = input_info.get("optional").unwrap_or(&empty);
        let inputs = object(&node["inputs"])?;
        for key in object(required)?.keys() {
            if !inputs.contains_key(key) {
                return Err(invalid(&format!(
                    "Workflow node {id} is missing required input {key}."
                )));
            }
        }
        object(optional)?;
        for (key, value) in inputs {
            let Some(raw) = required
                .get(key)
                .filter(|v| !v.is_null())
                .or_else(|| optional.get(key))
            else {
                continue;
            };
            if let Some(link) = value.as_array() {
                let upstream = link
                    .first()
                    .and_then(Value::as_js_string)
                    .filter(|s| graph.get(*s).is_some());
                let number = link
                    .get(1)
                    .and_then(Value::as_f64)
                    .filter(|n| n.is_finite() && n.fract() == 0.0 && *n >= 0.0);
                if link.len() != 2 || upstream.is_none() || number.is_none() {
                    return Err(invalid(
                        "Workflow link does not reference a valid node output.",
                    ));
                }
                let upstream = upstream.ok_or_else(|| invalid("Invalid workflow link."))?;
                visit(upstream, graph, info, visiting, visited)?;
                let kind = graph[upstream]["class_type"]
                    .as_js_string()
                    .ok_or_else(|| invalid("Invalid workflow class type."))?;
                object(&info[kind])?;
                if info[kind]["output"]
                    .as_array()
                    .is_some_and(|a| number.is_some_and(|n| n >= a.len() as f64))
                {
                    return Err(invalid("Workflow link output index is out of range."));
                }
                continue;
            }
            let Some(raw) = raw.as_array().filter(|a| !a.is_empty()) else {
                continue;
            };
            let kind = &raw[0];
            if let Some(choices) = kind.as_array() {
                if !choices.contains(value) {
                    return Err(invalid(&format!(
                        "Workflow input {key} is not an installed enum value."
                    )));
                }
            } else if kind == "INT" || kind == "FLOAT" {
                let number = value
                    .as_f64()
                    .filter(|n| n.is_finite() && (kind != "INT" || n.fract() == 0.0))
                    .ok_or_else(|| {
                        invalid(&format!(
                            "Workflow input {key} must be {}.",
                            kind.as_str().unwrap_or("number")
                        ))
                    })?;
                let bounds = raw
                    .get(1)
                    .filter(|v| v.is_object() || v.is_array())
                    .unwrap_or(&empty);
                object(bounds)?;
                if bounds["min"].as_f64().is_some_and(|n| number < n)
                    || bounds["max"].as_f64().is_some_and(|n| number > n)
                {
                    return Err(invalid(&format!(
                        "Workflow input {key} exceeds node bounds."
                    )));
                }
            } else if (kind == "STRING" && !value.is_string())
                || (kind == "BOOLEAN" && !value.is_boolean())
            {
                return Err(invalid(&format!(
                    "Workflow input {key} has the wrong scalar type."
                )));
            }
        }
        visiting.remove(id);
        visited.insert(id);
        Ok(())
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for id in object(graph)?.keys() {
        visit(id, graph, info, &mut visiting, &mut visited)?;
    }
    for id in crate::error::array(&workflow["outputNodes"]) {
        let id = id
            .as_js_string()
            .ok_or_else(|| invalid("Invalid output node ID."))?;
        object(&graph[id])?;
        let kind = graph[id]["class_type"]
            .as_js_string()
            .ok_or_else(|| invalid("Invalid workflow class type."))?;
        object(&info[kind])?;
        if info[kind]["output_node"] == false {
            return Err(invalid("Configured output node is not an output node."));
        }
    }
    Ok(())
}
