//! Pixel transforms applied between decode and encode: resize, alpha handling.

use fast_image_resize::{ResizeOptions, Resizer};
use image::{DynamicImage, RgbImage};

use crate::error::{CoreError, Result};
use crate::options::Resize;

/// Downscales to fit `bounds` (Lanczos3). Returns `None` if no resize is needed.
pub fn resize(img: &DynamicImage, bounds: &Resize) -> Result<Option<DynamicImage>> {
    let Some((w, h)) = bounds.target_size(img.width(), img.height()) else {
        return Ok(None);
    };
    // fast_image_resize doesn't support every DynamicImage variant; normalise the rare ones.
    let src = match img {
        DynamicImage::ImageLuma8(_)
        | DynamicImage::ImageLumaA8(_)
        | DynamicImage::ImageRgb8(_)
        | DynamicImage::ImageRgba8(_)
        | DynamicImage::ImageLuma16(_)
        | DynamicImage::ImageLumaA16(_)
        | DynamicImage::ImageRgb16(_)
        | DynamicImage::ImageRgba16(_) => std::borrow::Cow::Borrowed(img),
        other if other.color().has_alpha() => {
            std::borrow::Cow::Owned(DynamicImage::ImageRgba16(other.to_rgba16()))
        }
        other => std::borrow::Cow::Owned(DynamicImage::ImageRgb16(other.to_rgb16())),
    };
    let mut dst = DynamicImage::new(w, h, src.color());
    Resizer::new()
        .resize(src.as_ref(), &mut dst, &ResizeOptions::new())
        .map_err(|e| CoreError::Encode(format!("resize failed: {e}")))?;
    Ok(Some(dst))
}

/// True if the image has an alpha channel with at least one non-opaque pixel.
pub fn has_visible_alpha(img: &DynamicImage) -> bool {
    match img {
        DynamicImage::ImageLumaA8(i) => i.pixels().any(|p| p.0[1] != u8::MAX),
        DynamicImage::ImageRgba8(i) => i.pixels().any(|p| p.0[3] != u8::MAX),
        DynamicImage::ImageLumaA16(i) => i.pixels().any(|p| p.0[1] != u16::MAX),
        DynamicImage::ImageRgba16(i) => i.pixels().any(|p| p.0[3] != u16::MAX),
        other if other.color().has_alpha() => other.to_rgba8().pixels().any(|p| p.0[3] != 255),
        _ => false,
    }
}

/// True for single-channel (optionally with alpha) images.
pub fn is_grayscale(img: &DynamicImage) -> bool {
    matches!(
        img,
        DynamicImage::ImageLuma8(_)
            | DynamicImage::ImageLumaA8(_)
            | DynamicImage::ImageLuma16(_)
            | DynamicImage::ImageLumaA16(_)
    )
}

/// 8-bit RGB with any transparency composited onto white (for formats without alpha).
pub fn flatten_to_rgb8(img: &DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut out = Vec::with_capacity(w as usize * h as usize * 3);
    for p in rgba.pixels() {
        let [r, g, b, a] = p.0;
        let a = u16::from(a);
        for c in [r, g, b] {
            let v = (u16::from(c) * a + 255 * (255 - a) + 127) / 255;
            out.push(v as u8);
        }
    }
    // Invariant: `out` has exactly w*h*3 bytes, built from a w×h image.
    RgbImage::from_raw(w, h, out).expect("buffer size matches dimensions")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn flatten_composites_on_white() {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        img.put_pixel(1, 0, Rgba([0, 0, 0, 255]));
        let out = flatten_to_rgb8(&DynamicImage::ImageRgba8(img));
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255]);
        assert_eq!(out.get_pixel(1, 0).0, [0, 0, 0]);
    }

    #[test]
    fn alpha_detection() {
        let opaque = DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])));
        assert!(!has_visible_alpha(&opaque));
        let mut t = RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]));
        t.put_pixel(1, 1, Rgba([0, 0, 0, 10]));
        assert!(has_visible_alpha(&DynamicImage::ImageRgba8(t)));
    }

    #[test]
    fn resize_keeps_aspect() {
        let img = DynamicImage::new_rgb8(400, 200);
        let r = Resize {
            max_width: Some(100),
            max_height: None,
        };
        let out = resize(&img, &r).unwrap().unwrap();
        assert_eq!((out.width(), out.height()), (100, 50));
        assert!(resize(&img, &Resize::default()).unwrap().is_none());
    }
}
