use crate::compat::value::Value;
use crate::{
    error::{NumeraError, Result},
    files::images::{Image, inspect},
};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use std::io::Cursor;

pub fn request(mut request: Value, operation: &str) -> Result<Value> {
    let refs = request["reference_images"].as_array().map_or(0, Vec::len);
    if (!request["edit_region"].is_null() || !request["target_language"].is_null())
        && (operation != "edit" || refs == 0)
    {
        return Err(NumeraError::new(
            "invalid_input",
            "Region editing and in-image translation require edit_image with a reference image.",
        ));
    }
    if !request["edit_region"].is_null() && !request["mask"].is_null() {
        return Err(NumeraError::new(
            "invalid_input",
            "Choose edit_region or an explicit mask, not both.",
        ));
    }
    if request["upscale"] == true {
        let size = request["size"].as_str().and_then(dimensions);
        if operation != "edit"
            || refs != 1
            || !request["mask"].is_null()
            || !request["edit_region"].is_null()
            || !request["target_language"].is_null()
            || request["count"] != 1
            || size.is_none()
        {
            return Err(NumeraError::new(
                "invalid_input",
                "Upscale requires edit_image, exactly one reference/output, a numeric size and no mask, region or translation.",
            ));
        }
        let mut units = request["prompt"].as_utf16().unwrap_or(&[]).to_vec();
        units.extend("\n\nUpscale the reference image to ".encode_utf16());
        units.extend(request["size"].as_utf16().unwrap_or(&[]));
        units.extend(". Enhance fine details while preserving composition, objects, colors and text. Do not add or remove objects.".encode_utf16());
        request["prompt"] = Value::String(crate::compat::value::JsString::from_utf16(units));
    } else if let Some(language) = request["target_language"].as_utf16() {
        let mut units = request["prompt"].as_utf16().unwrap_or(&[]).to_vec();
        units.extend("\n\nTranslate the text ".encode_utf16());
        units.extend(
            if request["edit_region"].is_null() {
                "in the reference image"
            } else {
                "inside the selected region"
            }
            .encode_utf16(),
        );
        units.extend(" into ".encode_utf16());
        units.extend(language);
        units.extend(". Replace the original text inside the image; preserve panel layout, artwork, speech bubbles and text styling. Return the edited image, not a separate text translation.".encode_utf16());
        request["prompt"] = Value::String(crate::compat::value::JsString::from_utf16(units));
    }
    Ok(request)
}

pub fn dimensions(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once('x')?;
    let valid = |s: &str| {
        !s.is_empty()
            && s.len() <= 8
            && !s.starts_with('0')
            && s.bytes().all(|b| b.is_ascii_digit())
    };
    if !valid(width) || !valid(height) {
        return None;
    }
    Some((width.parse().ok()?, height.parse().ok()?))
}

pub fn region_mask(
    request: &Value,
    source: &Image,
    connection: &Value,
    config: &Value,
) -> Result<Image> {
    let polarity = connection["edit"]["maskPolarity"].as_str().ok_or_else(|| {
        NumeraError::new(
            "unsupported_operation",
            "Configure documented mask polarity before region editing.",
        )
    })?;
    let region = &request["edit_region"];
    let (x, y, width, height) = (
        region["x"].as_u64().unwrap_or(u64::MAX),
        region["y"].as_u64().unwrap_or(u64::MAX),
        region["width"].as_u64().unwrap_or(0),
        region["height"].as_u64().unwrap_or(0),
    );
    if x.checked_add(width).is_none_or(|v| v > source.width as u64)
        || y.checked_add(height)
            .is_none_or(|v| v > source.height as u64)
    {
        return Err(NumeraError::new(
            "invalid_input",
            "Edit region must fit within the first reference image dimensions.",
        ));
    }
    if u64::from(source.width) * u64::from(source.height) * 4
        > config["files"]["maxAggregateBytes"]
            .as_u64()
            .unwrap_or(100 * 1024 * 1024)
    {
        return Err(NumeraError::new(
            "invalid_input",
            "Region mask exceeds the working image byte limit.",
        ));
    }
    let outside = if polarity == "white-edit" { 0 } else { 255 };
    let inside = if polarity == "black-edit" { 0 } else { 255 };
    let mut image = ImageBuffer::from_pixel(
        source.width,
        source.height,
        Rgba([outside, outside, outside, 255]),
    );
    for yy in y..y + height {
        for xx in x..x + width {
            image.put_pixel(
                xx as u32,
                yy as u32,
                Rgba([
                    inside,
                    inside,
                    inside,
                    if polarity == "transparent-edit" {
                        0
                    } else {
                        255
                    },
                ]),
            );
        }
    }
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(|_| NumeraError::new("invalid_input", "Cannot encode region mask."))?;
    inspect(
        bytes.into_inner(),
        config,
        Some(
            config["files"]["maxInputBytes"]
                .as_u64()
                .unwrap_or(20 * 1024 * 1024) as usize,
        ),
    )
}
