use image::DynamicImage;

use super::EncodeParams;
use crate::error::{CoreError, Result};
use crate::transform::has_visible_alpha;

/// Lossy at quality Q; lossless when Q = 100 and the source itself was lossless.
/// The `webp` crate cannot embed ICC/Exif (SPEC §9).
pub fn encode(img: &DynamicImage, p: &EncodeParams<'_>) -> Result<Vec<u8>> {
    let lossless = p.quality == 100 && p.input_kind.is_lossless_source();
    let (w, h) = (img.width(), img.height());
    let mem = if has_visible_alpha(img) {
        let rgba = img.to_rgba8();
        webp::Encoder::from_rgba(rgba.as_raw(), w, h).encode_simple(lossless, f32::from(p.quality))
    } else {
        let rgb = img.to_rgb8();
        webp::Encoder::from_rgb(rgb.as_raw(), w, h).encode_simple(lossless, f32::from(p.quality))
    }
    .map_err(|e| CoreError::Encode(format!("WebP: {e:?}")))?;
    Ok(mem.to_vec())
}
