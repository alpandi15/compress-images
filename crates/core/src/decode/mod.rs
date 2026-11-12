//! Decoders. Every decoder returns upright pixels: orientation is already applied.

#[cfg(feature = "heic")]
mod heic;
mod raster;

use image::DynamicImage;

use crate::detect::ImageKind;
use crate::error::Result;

/// Decoded pixels plus the metadata we may carry over.
pub struct Decoded {
    pub image: DynamicImage,
    /// ICC color profile, preserved by default (SPEC §3.8).
    pub icc: Option<Vec<u8>>,
    /// Raw Exif (TIFF structure, no `Exif\0\0` prefix) with orientation reset to 1.
    pub exif: Option<Vec<u8>>,
}

pub fn decode(kind: ImageKind, data: &[u8]) -> Result<Decoded> {
    match kind {
        ImageKind::Heic => decode_heic(data),
        other => raster::decode(other, data),
    }
}

#[cfg(feature = "heic")]
fn decode_heic(data: &[u8]) -> Result<Decoded> {
    heic::decode(data)
}

#[cfg(not(feature = "heic"))]
fn decode_heic(_: &[u8]) -> Result<Decoded> {
    Err(crate::error::CoreError::Decode(
        "HEIC support is not enabled in this build".into(),
    ))
}
