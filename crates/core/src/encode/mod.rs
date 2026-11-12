//! Encoders, one module per output format (SPEC §3.6).

mod avif;
mod jpeg;
mod png;
mod tiff;
mod webp;

use image::DynamicImage;

use crate::detect::ImageKind;
use crate::error::{CoreError, Result};
use crate::options::Options;

/// Everything an encoder needs besides the pixels.
pub struct EncodeParams<'a> {
    pub quality: u8,
    pub input_kind: ImageKind,
    /// ICC profile to embed where supported (always passed when present).
    pub icc: Option<&'a [u8]>,
    /// Exif to embed; `Some` only when `keep_metadata` is on.
    pub exif: Option<&'a [u8]>,
    pub keep_metadata: bool,
    pub avif_speed: u8,
}

impl<'a> EncodeParams<'a> {
    pub fn new(
        opts: &Options,
        input_kind: ImageKind,
        icc: Option<&'a [u8]>,
        exif: Option<&'a [u8]>,
    ) -> Self {
        Self {
            quality: opts.quality.get(),
            input_kind,
            icc,
            exif: if opts.keep_metadata { exif } else { None },
            keep_metadata: opts.keep_metadata,
            avif_speed: opts.avif_speed,
        }
    }
}

pub fn encode(out: ImageKind, img: &DynamicImage, p: &EncodeParams<'_>) -> Result<Vec<u8>> {
    match out {
        ImageKind::Jpeg => jpeg::encode(img, p),
        ImageKind::Png => png::encode(img, p),
        ImageKind::Webp => webp::encode(img, p),
        ImageKind::Avif => avif::encode(img, p),
        ImageKind::Tiff => tiff::encode(img, p),
        ImageKind::Bmp | ImageKind::Heic => Err(CoreError::Encode(format!(
            "{} is not an output format",
            out.extension()
        ))),
    }
}
