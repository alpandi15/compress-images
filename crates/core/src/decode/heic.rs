//! HEIC/HEIF via libheif. libheif applies `irot`/`imir` itself, so Exif orientation must
//! not be applied a second time (SPEC §3.8).

use image::metadata::Orientation;
use image::{DynamicImage, RgbImage, RgbaImage};
use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

use super::Decoded;
use crate::error::{CoreError, Result};

pub fn decode(data: &[u8]) -> Result<Decoded> {
    let lib = LibHeif::new();
    let ctx = HeifContext::read_from_bytes(data).map_err(CoreError::decode)?;
    let handle = ctx.primary_image_handle().map_err(CoreError::decode)?;
    let has_alpha = handle.has_alpha_channel();
    let chroma = if has_alpha {
        RgbChroma::Rgba
    } else {
        RgbChroma::Rgb
    };
    let decoded = lib
        .decode(&handle, ColorSpace::Rgb(chroma), None)
        .map_err(CoreError::decode)?;

    let planes = decoded.planes();
    let plane = planes
        .interleaved
        .ok_or_else(|| CoreError::Decode("libheif returned no interleaved plane".into()))?;
    let channels = if has_alpha { 4 } else { 3 };
    let (w, h) = (plane.width, plane.height);
    let row_len = w as usize * channels;
    let mut pixels = Vec::with_capacity(row_len * h as usize);
    for row in plane.data.chunks(plane.stride).take(h as usize) {
        let row = row
            .get(..row_len)
            .ok_or_else(|| CoreError::Decode("truncated HEIC plane".into()))?;
        pixels.extend_from_slice(row);
    }

    let bad_size = || CoreError::Decode("HEIC plane size mismatch".into());
    let image = if has_alpha {
        DynamicImage::ImageRgba8(RgbaImage::from_raw(w, h, pixels).ok_or_else(bad_size)?)
    } else {
        DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, pixels).ok_or_else(bad_size)?)
    };

    let icc = handle.color_profile_raw().map(|p| p.data);
    let exif = handle
        .all_metadata()
        .into_iter()
        .find(|m| m.item_type.0 == *b"Exif")
        .and_then(|m| exif_tiff_payload(&m.raw_data))
        .map(|mut tiff| {
            let _ = Orientation::remove_from_exif_chunk(&mut tiff);
            tiff
        });

    Ok(Decoded { image, icc, exif })
}

/// HEIF Exif items start with a 4-byte big-endian offset to the TIFF header.
fn exif_tiff_payload(raw: &[u8]) -> Option<Vec<u8>> {
    let off = u32::from_be_bytes(raw.get(0..4)?.try_into().ok()?) as usize;
    raw.get(4usize.checked_add(off)?..).map(<[u8]>::to_vec)
}
