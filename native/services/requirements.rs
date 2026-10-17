use crate::compat::value::{JsString, Value, json};
use crate::{dimensions, files::images::Image, jobs::Receipt};

pub fn snapshot(connection: &Value, model: &JsString, request: &Value) -> Value {
    let mut result = json!({"dimension_evidence":dimensions::evidence(connection,model)});
    for key in ["aspect_ratio", "image_size"] {
        if let Some(value) = request.get(key) {
            result[key] = value.clone();
        }
    }
    result
}

pub fn deviations(receipt: &Receipt, image: &Image, index: u32) -> Vec<String> {
    let Some(expected) = receipt.value.get("output_requirements") else {
        return vec![];
    };
    let mut result = Vec::new();
    if let Some(format) = expected["output_format"].as_str().filter(|s| !s.is_empty())
        && image.mime != format!("image/{format}")
    {
        result.push(format!(
            "Output {index} format is {}, not requested {format}.",
            image.mime
        ));
    }
    if let Some(size) = expected["size"].as_str()
        && size.split_once('x').is_some_and(|(w, h)| {
            !w.is_empty() && !h.is_empty() && w.bytes().chain(h.bytes()).all(|b| b.is_ascii_digit())
        })
        && size != format!("{}x{}", image.width, image.height)
    {
        result.push(format!(
            "Output {index} dimensions differ from requested size."
        ));
    }
    let pixels = crate::error::array(&expected["dimension_evidence"]["pixel_sizes"]);
    if let Some(ratio) = expected["aspect_ratio"].as_str().filter(|s| !s.is_empty()) {
        let matches_native = pixels.iter().any(|p| {
            p["aspect_ratio"] == ratio && p["width"] == image.width && p["height"] == image.height
        });
        if let Some((w, h)) = ratio
            .split_once(':')
            .and_then(|(w, h)| Some((w.parse::<f64>().ok()?, h.parse::<f64>().ok()?)))
            && w != 0.0
            && h != 0.0
            && !matches_native
            && f64::from(image.width) * h != f64::from(image.height) * w
        {
            result.push(format!(
                "Output {index} aspect ratio differs from requested aspect_ratio."
            ));
        }
    }
    if expected["image_size"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
        && expected["aspect_ratio"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        && let Some(target) = pixels.iter().find(|p| {
            p["aspect_ratio"] == expected["aspect_ratio"]
                && p["resolution"] == expected["image_size"]
        })
        && (target["width"] != image.width || target["height"] != image.height)
    {
        result.push(format!(
            "Output {index} dimensions differ from the documented requested resolution tier."
        ));
    }
    if let Some(source) = expected.get("upscale_source") {
        let w = source["width"].as_u64().unwrap_or(0);
        let h = source["height"].as_u64().unwrap_or(0);
        if u64::from(image.width) < w
            || u64::from(image.height) < h
            || (u64::from(image.width) == w && u64::from(image.height) == h)
        {
            result.push(format!(
                "Output {index} was not upscaled beyond the reference dimensions."
            ));
        }
    }
    if expected["background"] == "transparent" && !image.alpha {
        result.push(format!("Output {index} has no alpha channel."));
    }
    result
}

pub fn warnings(receipt: &Receipt) -> Vec<String> {
    let r = &receipt.value["output_requirements"];
    let target = crate::error::array(&r["dimension_evidence"]["pixel_sizes"])
        .iter()
        .any(|size| {
            size["aspect_ratio"] == r["aspect_ratio"] && size["resolution"] == r["image_size"]
        });
    if r["image_size"].as_str().is_some_and(|s| !s.is_empty()) && !target {
        vec![
            "Requested resolution tier could not be verified: exact model/tier/aspect evidence is unavailable.".into(),
        ]
    } else {
        vec![]
    }
}
