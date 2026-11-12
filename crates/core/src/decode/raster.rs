//! JPEG, PNG, WebP, TIFF, BMP and (with `avif-decode`) AVIF via the `image` crate.

use std::io::Cursor;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

use super::Decoded;
use crate::detect::ImageKind;
use crate::error::{CoreError, Result};

/// Upper bound on decoder allocations; generous enough for very large photos/scans.
const MAX_ALLOC: u64 = 2 * 1024 * 1024 * 1024;

pub fn decode(kind: ImageKind, data: &[u8]) -> Result<Decoded> {
    let format = match kind {
        ImageKind::Jpeg => ImageFormat::Jpeg,
        ImageKind::Png => ImageFormat::Png,
        ImageKind::Webp => ImageFormat::WebP,
        ImageKind::Tiff => ImageFormat::Tiff,
        ImageKind::Bmp => ImageFormat::Bmp,
        ImageKind::Avif => ImageFormat::Avif,
        ImageKind::Heic => return Err(CoreError::Decode("HEIC is not a raster format".into())),
    };

    let mut reader = ImageReader::new(Cursor::new(data));
    reader.set_format(format);
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);

    let mut decoder = reader.into_decoder().map_err(CoreError::decode)?;
    // Metadata is best-effort: a broken Exif block must not prevent compressing the pixels.
    let icc = decoder.icc_profile().ok().flatten();
    let mut exif = decoder.exif_metadata().ok().flatten();
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);

    let mut image = DynamicImage::from_decoder(decoder).map_err(CoreError::decode)?;
    image.apply_orientation(orientation);
    if let Some(e) = exif.as_mut() {
        let _ = Orientation::remove_from_exif_chunk(e);
    }

    Ok(Decoded { image, icc, exif })
}
