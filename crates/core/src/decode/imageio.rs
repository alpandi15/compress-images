//! HEIC/HEIF and AVIF on macOS via the system ImageIO framework.
//!
//! Using the OS decoder means the macOS app ships no libheif/dav1d of its own (SPEC §6.5).
//! ImageIO reports `irot`/`imir` as an orientation instead of applying it, so it is applied
//! here; Exif comes from the container directly (ImageIO only exposes it parsed).

use std::ffi::c_void;

use image::metadata::Orientation;
use image::{DynamicImage, RgbImage, RgbaImage};
use objc2_core_foundation::{
    CFData, CFNumber, CFRetained, CFString, CFType, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGColorSpaceModel, CGContext, CGImage, CGImageAlphaInfo,
};
use objc2_image_io::{kCGImagePropertyOrientation, CGImageSource};

use super::{heif_exif, Decoded};
use crate::error::{CoreError, Result};

/// Same ceiling as the raster decoders: refuse absurd dimensions instead of allocating them.
const MAX_PIXELS: usize = 512 * 1024 * 1024 / 4;

pub fn decode(data: &[u8]) -> Result<Decoded> {
    let err = |msg: &str| CoreError::Decode(msg.to_string());
    let cf_data = CFData::from_bytes(data);
    // SAFETY: plain CoreFoundation calls on objects we own; ImageIO copies what it keeps.
    let source = unsafe { CGImageSource::with_data(&cf_data, None) }
        .ok_or_else(|| err("ImageIO could not read the file"))?;
    if unsafe { source.count() } == 0 {
        return Err(err("ImageIO found no image in the file"));
    }
    let cg = unsafe { source.image_at_index(0, None) }
        .ok_or_else(|| err("ImageIO could not decode the image (needs macOS 13+ for AVIF)"))?;
    let orientation = orientation(&source).unwrap_or(1);

    let (width, height) = (CGImage::width(Some(&cg)), CGImage::height(Some(&cg)));
    if width == 0 || height == 0 || width.saturating_mul(height) > MAX_PIXELS {
        return Err(err("unsupported image dimensions"));
    }
    let has_alpha = !matches!(
        CGImage::alpha_info(Some(&cg)),
        CGImageAlphaInfo::None | CGImageAlphaInfo::NoneSkipLast | CGImageAlphaInfo::NoneSkipFirst
    );

    // Render in the image's own RGB color space so pixel values aren't converted and its ICC
    // profile still describes them. Anything else (gray, CMYK) is converted to sRGB.
    let own_space = CGImage::color_space(Some(&cg))
        .filter(|s| CGColorSpace::model(Some(s)) == CGColorSpaceModel::RGB);
    let srgb = || {
        // SAFETY: reading an immutable framework constant.
        CGColorSpace::with_name(Some(unsafe { objc2_core_graphics::kCGColorSpaceSRGB }))
    };
    let space = own_space
        .clone()
        .or_else(srgb)
        .ok_or_else(|| err("no RGB color space available"))?;
    // sRGB is what every encoder assumes anyway, so don't embed a profile for it.
    let icc = own_space
        .filter(|s| {
            CGColorSpace::name(Some(s)).is_none_or(|n| n.to_string() != "kCGColorSpaceSRGB")
        })
        .and_then(|s| CGColorSpace::icc_data(Some(&s)))
        .map(|d| d.to_vec());

    let mut rgba = render(&cg, &space, width, height, has_alpha)?;
    let (w, h) = (width as u32, height as u32);
    let bad_size = || err("ImageIO buffer size mismatch");
    let mut image = if has_alpha {
        unpremultiply(&mut rgba);
        DynamicImage::ImageRgba8(RgbaImage::from_raw(w, h, rgba).ok_or_else(bad_size)?)
    } else {
        let rgb: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, rgb).ok_or_else(bad_size)?)
    };
    image.apply_orientation(
        Orientation::from_exif(orientation).unwrap_or(Orientation::NoTransforms),
    );

    let exif = heif_exif::extract(data).map(|mut tiff| {
        let _ = Orientation::remove_from_exif_chunk(&mut tiff);
        tiff
    });

    Ok(Decoded { image, icc, exif })
}

/// `kCGImagePropertyOrientation` (1–8), which ImageIO derives from `irot`/`imir`.
fn orientation(source: &CGImageSource) -> Option<u8> {
    // SAFETY: the properties dictionary maps CFString keys to CF objects.
    let props = unsafe { source.properties_at_index(0, None) }?;
    let props = unsafe { props.cast_unchecked::<CFString, CFType>() };
    let value = props.get(unsafe { kCGImagePropertyOrientation })?;
    let number: CFRetained<CFNumber> = value.downcast().ok()?;
    u8::try_from(number.as_i32()?).ok()
}

/// Draws the image into an 8-bit RGBA (or RGBX) buffer.
fn render(
    cg: &CGImage,
    space: &CGColorSpace,
    width: usize,
    height: usize,
    has_alpha: bool,
) -> Result<Vec<u8>> {
    let stride = width * 4;
    let mut buf = vec![0u8; stride * height];
    // Bitmap contexts only support premultiplied alpha; it is undone afterwards.
    let alpha = if has_alpha {
        CGImageAlphaInfo::PremultipliedLast
    } else {
        CGImageAlphaInfo::NoneSkipLast
    };
    // SAFETY: `buf` outlives the context and has exactly `stride * height` bytes, matching
    // the geometry passed in; the context is dropped before `buf` is returned.
    let ctx: CFRetained<CGContext> = unsafe {
        CGBitmapContextCreate(
            buf.as_mut_ptr().cast::<c_void>(),
            width,
            height,
            8,
            stride,
            Some(space),
            alpha.0,
        )
    }
    .ok_or_else(|| CoreError::Decode("could not create a bitmap context".into()))?;
    let rect = CGRect::new(
        CGPoint::new(0.0, 0.0),
        CGSize::new(width as f64, height as f64),
    );
    CGContext::draw_image(Some(&ctx), rect, Some(cg));
    drop(ctx);
    Ok(buf)
}

fn unpremultiply(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = u32::from(px[3]);
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    fn fixture(name: &str) -> Vec<u8> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        std::fs::read(dir.join(name)).unwrap()
    }

    #[test]
    fn decodes_heic_and_avif() {
        for name in ["photo.heic", "photo.avif"] {
            let d = decode(&fixture(name)).unwrap();
            assert_eq!(d.image.dimensions(), (200, 150), "{name}");
            assert!(d.icc.is_none(), "{name}: sRGB needs no profile");
        }
    }

    #[test]
    fn applies_irot_and_strips_orientation_from_exif() {
        let d = decode(&fixture("rotated.heic")).unwrap();
        assert_eq!(d.image.dimensions(), (150, 200));
        let exif = d.exif.expect("Exif carried over");
        let mut probe = exif.clone();
        assert!(matches!(
            Orientation::remove_from_exif_chunk(&mut probe),
            None | Some(Orientation::NoTransforms)
        ));
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(b"not an image").is_err());
    }

    #[test]
    fn unpremultiply_restores_colors() {
        let mut px = [64, 32, 0, 128, 10, 20, 30, 0, 1, 2, 3, 255];
        unpremultiply(&mut px);
        assert_eq!(px, [128, 64, 0, 128, 10, 20, 30, 0, 1, 2, 3, 255]);
    }
}
