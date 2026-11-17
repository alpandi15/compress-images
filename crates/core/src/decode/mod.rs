//! Decoders. Every decoder returns upright pixels: orientation is already applied.
//!
//! HEIC and AVIF go through the system ImageIO framework on macOS and through libheif /
//! dav1d (via `image`) elsewhere (SPEC §6.5).

#[cfg(all(feature = "heic", not(target_os = "macos")))]
mod heic;
mod heif_exif;
#[cfg(all(feature = "imageio", target_os = "macos"))]
mod imageio;
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
    if !kind.is_decodable() {
        return Err(crate::error::CoreError::Decode(format!(
            "{kind:?} support is not enabled in this build"
        )));
    }
    match kind {
        ImageKind::Heic => decode_heic(data),
        ImageKind::Avif => decode_avif(data),
        other => raster::decode(other, data),
    }
}

#[cfg(all(feature = "heic", target_os = "macos"))]
fn decode_heic(data: &[u8]) -> Result<Decoded> {
    imageio::decode(data)
}

#[cfg(all(feature = "heic", not(target_os = "macos")))]
fn decode_heic(data: &[u8]) -> Result<Decoded> {
    heic::decode(data)
}

#[cfg(not(feature = "heic"))]
fn decode_heic(_: &[u8]) -> Result<Decoded> {
    unreachable!("guarded by is_decodable")
}

#[cfg(all(feature = "avif-decode", target_os = "macos"))]
fn decode_avif(data: &[u8]) -> Result<Decoded> {
    imageio::decode(data)
}

#[cfg(not(all(feature = "avif-decode", target_os = "macos")))]
fn decode_avif(data: &[u8]) -> Result<Decoded> {
    raster::decode(ImageKind::Avif, data)
}
