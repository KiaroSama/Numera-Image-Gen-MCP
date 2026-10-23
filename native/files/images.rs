use crate::compat::hash::sha256;
use crate::compat::value::{Value, json};
use crate::error::{NumeraError, Result};
use base64::{
    Engine, alphabet,
    engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, STANDARD},
};
use image::{ImageDecoder, ImageFormat, ImageReader};
use std::io::Cursor;

#[derive(Clone)]
pub struct Image {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub alpha: bool,
    pub sha256: String,
    pub extension: String,
}
impl Image {
    pub fn metadata(&self) -> Value {
        json!({"mime":self.mime,"width":self.width,"height":self.height,
        "alpha":self.alpha,"sha256":self.sha256,"extension":self.extension})
    }
}

pub fn decode_base64(value: &str, limit: usize) -> Result<Vec<u8>> {
    let invalid = || {
        NumeraError::new("invalid_response", "Invalid or excessive Base64 image.").stage("response")
    };
    if value.is_empty()
        || value.len() > limit.div_ceil(3).saturating_mul(4).saturating_add(4)
        || !value.len().is_multiple_of(4)
    {
        return Err(invalid());
    }
    let padding = value.bytes().rev().take_while(|b| *b == b'=').count();
    if padding > 2
        || value[..value.len() - padding]
            .bytes()
            .any(|b| !b.is_ascii_alphanumeric() && b != b'+' && b != b'/')
    {
        return Err(invalid());
    }
    // Node's Buffer decoder ignores unused trailing bits after the source syntax guard.
    let decoder = GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
    );
    let bytes = decoder.decode(value).map_err(|_| invalid())?;
    if bytes.is_empty() || bytes.len() > limit {
        return Err(
            NumeraError::new("invalid_response", "Empty or excessive image bytes.")
                .stage("response"),
        );
    }
    Ok(bytes)
}

pub fn data_url(value: &str, limit: usize) -> Result<Vec<u8>> {
    let Some((prefix, encoded)) = value.split_once(',') else {
        return Err(data_error());
    };
    if !matches!(
        prefix,
        "data:image/png;base64" | "data:image/jpeg;base64" | "data:image/webp;base64"
    ) {
        return Err(data_error());
    }
    decode_base64(encoded, limit)
}
fn data_error() -> NumeraError {
    NumeraError::new("invalid_input", "Expected a PNG/JPEG/WebP Base64 data URL.")
}

pub fn inspect(bytes: Vec<u8>, config: &Value, limit: Option<usize>) -> Result<Image> {
    let limit = limit.unwrap_or(
        config["files"]["maxOutputBytes"]
            .as_u64()
            .unwrap_or(50 * 1024 * 1024) as usize,
    );
    if bytes.is_empty() || bytes.len() > limit {
        return Err(NumeraError::new(
            "invalid_input",
            "Image byte limit exceeded or empty image.",
        ));
    }
    let (format, extension) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        (ImageFormat::Png, "png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        (ImageFormat::Jpeg, "jpg")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) {
        (ImageFormat::WebP, "webp")
    } else {
        return Err(NumeraError::new(
            "invalid_input",
            "Only valid PNG, JPEG and WebP bytes are supported.",
        ));
    };
    let max_pixels = config["files"]["maxPixels"].as_u64().unwrap_or(64000000);
    let invalid = || NumeraError::new("invalid_input", "Image cannot be decoded safely.");
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(max_pixels.saturating_mul(16));
    limits.max_image_width = Some(max_pixels.min(u32::MAX as u64) as u32);
    limits.max_image_height = Some(max_pixels.min(u32::MAX as u64) as u32);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|_| invalid())?;
    let (width, height) = decoder.dimensions();
    let alpha = decoder.color_type().has_alpha();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > max_pixels {
        return Err(invalid());
    }
    reject_animation(&bytes, format)?;
    let decoded = image::DynamicImage::from_decoder(decoder).map_err(|_| invalid())?;
    if decoded.width() != width || decoded.height() != height {
        return Err(invalid());
    }
    let sha256 = sha256(&bytes);
    Ok(Image {
        bytes,
        mime: format!(
            "image/{}",
            if format == ImageFormat::Jpeg {
                "jpeg"
            } else {
                extension
            }
        ),
        width,
        height,
        alpha,
        sha256,
        extension: extension.into(),
    })
}

fn reject_animation(bytes: &[u8], format: ImageFormat) -> Result<()> {
    let invalid = || NumeraError::new("invalid_input", "Image cannot be decoded safely.");
    let (mut offset, header, end) = match format {
        ImageFormat::Png => (8usize, 8usize, bytes.len()),
        ImageFormat::WebP => {
            let len = u32::from_le_bytes(
                bytes
                    .get(4..8)
                    .ok_or_else(invalid)?
                    .try_into()
                    .map_err(|_| invalid())?,
            ) as usize;
            (12usize, 8usize, len.checked_add(8).ok_or_else(invalid)?)
        }
        _ => return Ok(()),
    };
    if end > bytes.len() {
        return Err(invalid());
    }
    while offset + header <= end {
        let (tag, size) = if format == ImageFormat::Png {
            (
                &bytes[offset + 4..offset + 8],
                u32::from_be_bytes(
                    bytes[offset..offset + 4]
                        .try_into()
                        .map_err(|_| invalid())?,
                ) as usize,
            )
        } else {
            (
                &bytes[offset..offset + 4],
                u32::from_le_bytes(
                    bytes[offset + 4..offset + 8]
                        .try_into()
                        .map_err(|_| invalid())?,
                ) as usize,
            )
        };
        if tag == b"acTL" || tag == b"ANIM" || tag == b"ANMF" {
            return Err(invalid());
        }
        if tag == b"VP8X" && size >= 1 && bytes.get(offset + 8).is_some_and(|b| b & 0x02 != 0) {
            return Err(invalid());
        }
        let advance = if format == ImageFormat::Png {
            size.checked_add(12)
        } else {
            size.checked_add(8 + (size % 2))
        }
        .ok_or_else(invalid)?;
        offset = offset.checked_add(advance).ok_or_else(invalid)?;
        if offset > end {
            return Err(invalid());
        }
        if tag == b"IEND" {
            break;
        }
    }
    Ok(())
}

pub fn preview(image: &Image, config: &Value) -> Result<Value> {
    let dimension = config["files"]["previewMaxDimension"]
        .as_u64()
        .unwrap_or(512) as u32;
    let max_bytes = config["files"]["previewMaxBytes"]
        .as_u64()
        .unwrap_or(262144) as usize;
    let decoded = image::load_from_memory(&image.bytes).map_err(|_| {
        NumeraError::new("output_file_error", "Preview cannot be decoded.").stage("preview")
    })?;
    for d in [dimension, (dimension / 2).max(1), dimension.min(64)] {
        let small = if image.width <= d && image.height <= d {
            decoded.clone()
        } else {
            decoded.resize(d, d, image::imageops::FilterType::Lanczos3)
        };
        let mut out = Cursor::new(Vec::new());
        small.write_to(&mut out, ImageFormat::Png).map_err(|_| {
            NumeraError::new("output_file_error", "Preview cannot be encoded.").stage("preview")
        })?;
        if out.get_ref().len() <= max_bytes {
            return Ok(
                json!({"data":STANDARD.encode(out.get_ref()),"mimeType":"image/png","width":small.width(),"height":small.height()}),
            );
        }
    }
    Err(NumeraError::new(
        "output_file_error",
        "Preview cannot fit configured byte limit.",
    )
    .stage("preview"))
}

pub fn validate_mask(mask: &Image, source: &Image) -> Result<()> {
    if mask.mime != "image/png"
        || !mask.alpha
        || mask.width != source.width
        || mask.height != source.height
    {
        return Err(NumeraError::new(
            "invalid_input",
            "Mask must be a PNG with alpha and the same dimensions as the first reference.",
        ));
    }
    Ok(())
}
