use crate::compat::value::{JsString, Value, json};
use crate::error::{NumeraError, Result};

const SOURCE: &str =
    "https://ai.google.dev/gemini-api/docs/image-generation#aspect-ratios-and-image-size";
const TIERS: &[&str] = &["0.5K", "1K", "2K", "4K"];
// Google AI for Developers, CC BY 4.0. Discrete documented sizes, never inferred pixels.
const FLASH: &[(&str, [&str; 4])] = &[
    ("1:1", ["512x512", "1024x1024", "2048x2048", "4096x4096"]),
    ("1:4", ["256x1024", "512x2048", "1024x4096", "2048x8192"]),
    ("1:8", ["192x1536", "384x3072", "768x6144", "1536x12288"]),
    ("2:3", ["424x632", "848x1264", "1696x2528", "3392x5056"]),
    ("3:2", ["632x424", "1264x848", "2528x1696", "5056x3392"]),
    ("3:4", ["448x600", "896x1200", "1792x2400", "3584x4800"]),
    ("4:1", ["1024x256", "2048x512", "4096x1024", "8192x2048"]),
    ("4:3", ["600x448", "1200x896", "2400x1792", "4800x3584"]),
    ("4:5", ["464x576", "928x1152", "1856x2304", "3712x4608"]),
    ("5:4", ["576x464", "1152x928", "2304x1856", "4608x3712"]),
    ("8:1", ["1536x192", "3072x384", "6144x768", "12288x1536"]),
    ("9:16", ["384x688", "768x1376", "1536x2752", "3072x5504"]),
    ("16:9", ["688x384", "1376x768", "2752x1536", "5504x3072"]),
    ("21:9", ["792x168", "1584x672", "3168x1344", "6336x2688"]),
];

fn known(model: &str) -> Option<bool> {
    if [
        "gemini-3.1-flash-image",
        "models/gemini-3.1-flash-image",
        "antigravity/gemini-3.1-flash-image",
    ]
    .contains(&model)
    {
        Some(false)
    } else if [
        "gemini-3-pro-image",
        "gemini-3-pro-image-preview",
        "models/gemini-3-pro-image",
        "models/gemini-3-pro-image-preview",
        "antigravity/gemini-3-pro-image",
        "antigravity/gemini-3-pro-image-preview",
    ]
    .contains(&model)
    {
        Some(true)
    } else {
        None
    }
}

pub fn image_dimensions(connection: &Value, model: &JsString, max_pixels: u64) -> Value {
    let pro = model.as_str().and_then(known);
    let native =
        ["gemini", "gemini-interactions"].contains(&connection["adapter"].as_str().unwrap_or(""));
    let omni = connection["gateway"] == "omniroute" && model.starts_with("antigravity/");
    let requested = connection["modelOverrides"][model]["supportedParameters"].as_array();
    let accepted: Vec<Value> = if let Some(requested) = requested {
        requested
            .iter()
            .filter(|v| {
                v.as_str()
                    .is_some_and(|s| ["size", "aspect_ratio", "image_size"].contains(&s))
            })
            .cloned()
            .collect()
    } else {
        let values: &[&str] = if connection["adapter"] == "openrouter-images" {
            &["size", "aspect_ratio", "image_size"]
        } else if native || connection["adapter"] == "chat-images" {
            &["aspect_ratio", "image_size"]
        } else if omni {
            &["size", "aspect_ratio", "image_size"]
        } else if connection["adapter"] == "comfyui" {
            &[]
        } else {
            &["size"]
        };
        values.iter().map(|s| json!(s)).collect()
    };
    let start = if pro == Some(true) { 1 } else { 0 };
    let ratios: Vec<&str> = FLASH
        .iter()
        .filter(|(ratio, _)| pro != Some(true) || !["1:4", "1:8", "4:1", "8:1"].contains(ratio))
        .map(|(ratio, _)| *ratio)
        .collect();
    let mut pixels = Vec::new();
    if pro.is_some() {
        for (ratio, sizes) in FLASH.iter().filter(|(ratio, _)| ratios.contains(ratio)) {
            for (i, size) in sizes.iter().enumerate().skip(start) {
                if let Some((w, h)) = size.split_once('x')
                    && let (Ok(width), Ok(height)) = (w.parse::<u64>(), h.parse::<u64>())
                {
                    pixels.push(json!({"aspect_ratio":ratio, "resolution":TIERS[i], "width":width, "height":height}));
                }
            }
        }
    }
    json!({"preset_aspect_ratios":FLASH.iter().map(|(r, _)| *r).collect::<Vec<_>>(), "presets_are_model_support":false,
        "model_support":{"status":if pro.is_some(){"documented"}else{"unknown"}, "aspect_ratios":pro.map(|_|ratios),
            "resolution_tiers":pro.map(|_|&TIERS[start..]), "pixel_sizes":pro.map(|_|pixels),
            "custom_dimensions":{"status":if pro.is_some(){"unsupported"}else{"unknown"}, "explanation":if pro.is_some(){"Select a documented aspect ratio and native tier; arbitrary pixel dimensions are not a native contract."}else{"No verified model-specific custom pixel-size contract is available."}},
            "evidence":if pro.is_some(){json!([{"source":SOURCE,"checked_at":"2026-10-05","kind":"official_documentation"}])}else{json!([])}},
        "route_support":{"accepted_parameters":accepted, "resolution_tiers":if omni {json!(["1K","2K","4K"])} else if pro.is_some() && native {json!(&TIERS[start..])} else {Value::Null},
            "max_pixels":max_pixels,"custom_dimensions":"unknown", "explanation":if omni{"Gateway size maps aspect ratio, not arbitrary pixels. 0.5K is not forwarded as a distinct tier. Use aspect_ratio/image_size."}else{"Accepted request fields do not establish model/account support. Generic Images does not accept undeclared aspect_ratio/image_size fields."}},
        "account_verified":false,"output_dimensions_must_be_verified":true})
}

pub fn evidence(connection: &Value, model: &JsString) -> Value {
    let dimensions = image_dimensions(connection, model, 64000000);
    let pixels = &dimensions["model_support"]["pixel_sizes"];
    let name = model.as_str().unwrap_or("");
    let engine = name
        .strip_prefix("cx/")
        .or_else(|| name.strip_prefix("codex/"))
        .unwrap_or(name);
    let gpt = [
        "gpt-image-2",
        "gpt-image-2.5-sunburst",
        "gpt-image-2.5-flare",
    ]
    .contains(&engine);
    let discrete = pixels.as_array();
    json!({"kind":if discrete.is_some(){"discrete"}else if gpt{"custom"}else{"unknown"},
        "source":if discrete.is_some(){Some(SOURCE)}else if gpt{Some("https://developers.openai.com/api/docs/guides/image-generation#customize-image-output")}else{None},
        "checked_at":if discrete.is_some() || gpt{Some("2026-10-05")}else{None},
        "max_edge":discrete.and_then(|p|p.iter().flat_map(|v|[v["width"].as_u64(),v["height"].as_u64()]).flatten().max()).or(if gpt{Some(3840)}else{None}),
        "max_pixels":discrete.and_then(|p|p.iter().filter_map(|v|Some(v["width"].as_u64()?*v["height"].as_u64()?)).max()).or(if gpt{Some(8294400)}else{None}),
        "min_pixels":if gpt{Some(655360)}else{None},"dimension_multiple":if gpt{Some(16)}else{None},
        "pixel_sizes":pixels,"account_verified":false,"forwarding_guaranteed":false})
}

pub fn validate(connection: &Value, model: &JsString, request: &Value) -> Result<()> {
    let info = evidence(connection, model);
    if let Some(pixels) = info["pixel_sizes"].as_array() {
        for (field, key, message) in [
            (
                "aspect_ratio",
                "aspect_ratio",
                "Requested aspect ratio is outside the documented model presets.",
            ),
            (
                "image_size",
                "resolution",
                "Requested resolution tier is outside the documented model presets.",
            ),
        ] {
            if request[field].as_utf16().is_some_and(|s| !s.is_empty())
                && !pixels.iter().any(|p| p[key] == request[field])
            {
                return Err(NumeraError::new("unsupported_parameter", message));
            }
        }
    }
    if info["kind"] != "custom" {
        return Ok(());
    }
    let Some(size) = request["size"].as_str() else {
        return Ok(());
    };
    let Some((w, h)) = size.split_once('x') else {
        return Ok(());
    };
    if w.is_empty() || h.is_empty() || !w.bytes().chain(h.bytes()).all(|b| b.is_ascii_digit()) {
        return Ok(());
    }
    let invalid = || {
        NumeraError::new(
            "unsupported_parameter",
            "Requested dimensions exceed the documented image-model limits; a gateway cannot guarantee larger native output.",
        )
    };
    let width: u64 = w.parse().map_err(|_| invalid())?;
    let height: u64 = h.parse().map_err(|_| invalid())?;
    let count = width.checked_mul(height).ok_or_else(invalid)?;
    if width > 3840
        || height > 3840
        || !(655360..=8294400).contains(&count)
        || !width.is_multiple_of(16)
        || !height.is_multiple_of(16)
        || width * 3 < height
        || width > height * 3
    {
        return Err(invalid());
    }
    Ok(())
}
