use std::io::Cursor;

use image::codecs::png::PngEncoder;
use image::{DynamicImage, ImageEncoder};
use oxipng::StripChunks;
use rgb::FromSlice;

use super::EncodeParams;
use crate::error::{CoreError, Result};

/// `Q = 100` → lossless only. Otherwise quantize to a palette with quality range
/// `[Q-20, Q]`, falling back to lossless if that range can't be met. Always finish with oxipng.
pub fn encode(img: &DynamicImage, p: &EncodeParams<'_>) -> Result<Vec<u8>> {
    let raw = if p.quality < 100 {
        match quantize(img, p)? {
            Some(png) => png,
            None => lossless(img, p)?,
        }
    } else {
        lossless(img, p)?
    };
    optimize(&raw, p)
}

fn lossless(img: &DynamicImage, p: &EncodeParams<'_>) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut enc = PngEncoder::new(Cursor::new(&mut buf));
    if let Some(icc) = p.icc {
        let _ = enc.set_icc_profile(icc.to_vec());
    }
    // PNG has no float variant; convert 32F images to 16-bit.
    let img = match img {
        DynamicImage::ImageRgb32F(_) => DynamicImage::ImageRgb16(img.to_rgb16()),
        DynamicImage::ImageRgba32F(_) => DynamicImage::ImageRgba16(img.to_rgba16()),
        _ => img.clone(),
    };
    enc.write_image(
        img.as_bytes(),
        img.width(),
        img.height(),
        img.color().into(),
    )
    .map_err(CoreError::encode)?;
    Ok(buf)
}

/// Returns `Ok(None)` when quantization can't reach the minimum quality.
fn quantize(img: &DynamicImage, p: &EncodeParams<'_>) -> Result<Option<Vec<u8>>> {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let min = p.quality.saturating_sub(20);

    let mut liq = imagequant::new();
    liq.set_quality(min, p.quality).map_err(CoreError::encode)?;
    let pixels = rgba.as_raw().as_rgba().to_vec();
    let mut image = liq
        .new_image(pixels, w as usize, h as usize, 0.0)
        .map_err(CoreError::encode)?;
    let mut res = match liq.quantize(&mut image) {
        Ok(r) => r,
        Err(imagequant::Error::QualityTooLow) => return Ok(None),
        Err(e) => return Err(CoreError::encode(e)),
    };
    res.set_dithering_level(1.0).map_err(CoreError::encode)?;
    let (palette, indices) = res.remapped(&mut image).map_err(CoreError::encode)?;

    let rgb_palette: Vec<u8> = palette.iter().flat_map(|c| [c.r, c.g, c.b]).collect();
    let mut trns: Vec<u8> = palette.iter().map(|c| c.a).collect();
    while trns.last() == Some(&255) {
        trns.pop();
    }

    let mut info = png::Info::with_size(w, h);
    info.color_type = png::ColorType::Indexed;
    info.bit_depth = png::BitDepth::Eight;
    info.palette = Some(rgb_palette.into());
    if !trns.is_empty() {
        info.trns = Some(trns.into());
    }
    if let Some(icc) = p.icc {
        info.icc_profile = Some(icc.to_vec().into());
    }

    let mut buf = Vec::new();
    let mut writer = png::Encoder::with_info(&mut buf, info)
        .and_then(|e| e.write_header())
        .map_err(CoreError::encode)?;
    writer
        .write_image_data(&indices)
        .map_err(CoreError::encode)?;
    writer.finish().map_err(CoreError::encode)?;
    Ok(Some(buf))
}

fn optimize(data: &[u8], p: &EncodeParams<'_>) -> Result<Vec<u8>> {
    let mut opts = oxipng::Options::from_preset(2);
    // `Safe` keeps rendering-relevant chunks (iCCP, sRGB, cICP, pHYs) and drops the rest.
    opts.strip = if p.keep_metadata {
        StripChunks::None
    } else {
        StripChunks::Safe
    };
    oxipng::optimize_from_memory(data, &opts).map_err(CoreError::encode)
}
