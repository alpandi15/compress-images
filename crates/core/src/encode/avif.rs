use image::DynamicImage;
use ravif::{Encoder, Img};
use rgb::FromSlice;

use super::EncodeParams;
use crate::error::{CoreError, Result};
use crate::transform::has_visible_alpha;

/// ravif cannot embed an ICC profile; Exif is embedded when `keep_metadata` is on (SPEC §9).
pub fn encode(img: &DynamicImage, p: &EncodeParams<'_>) -> Result<Vec<u8>> {
    let q = f32::from(p.quality);
    let mut enc = Encoder::new()
        .with_quality(q)
        .with_alpha_quality(q)
        .with_speed(p.avif_speed);
    if let Some(exif) = p.exif {
        enc = enc.with_exif(exif);
    }
    let (w, h) = (img.width() as usize, img.height() as usize);
    let res = if has_visible_alpha(img) {
        let rgba = img.to_rgba8();
        enc.encode_rgba(Img::new(rgba.as_raw().as_rgba(), w, h))
    } else {
        let rgb = img.to_rgb8();
        enc.encode_rgb(Img::new(rgb.as_raw().as_rgb(), w, h))
    };
    res.map(|e| e.avif_file).map_err(CoreError::encode)
}
