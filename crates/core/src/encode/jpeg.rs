use std::panic::{catch_unwind, AssertUnwindSafe};

use image::DynamicImage;
use mozjpeg::{ColorSpace, Compress, Marker};

use super::EncodeParams;
use crate::error::{CoreError, Result};
use crate::transform::{flatten_to_rgb8, is_grayscale};

/// Largest payload a single JPEG APP marker can hold.
const MAX_MARKER_LEN: usize = 65533;

pub fn encode(img: &DynamicImage, p: &EncodeParams<'_>) -> Result<Vec<u8>> {
    let rgb = flatten_to_rgb8(img);
    let gray = is_grayscale(img);
    let (w, h) = rgb.dimensions();
    let pixels: Vec<u8> = if gray {
        rgb.pixels().map(|px| px.0[0]).collect()
    } else {
        rgb.into_raw()
    };

    // libjpeg reports errors by unwinding; contain them so one bad file can't abort a job.
    catch_unwind(AssertUnwindSafe(|| -> std::io::Result<Vec<u8>> {
        let mut c = Compress::new(if gray {
            ColorSpace::JCS_GRAYSCALE
        } else {
            ColorSpace::JCS_RGB
        });
        c.set_size(w as usize, h as usize);
        c.set_quality(f32::from(p.quality));
        c.set_progressive_mode();
        c.set_optimize_coding(true);
        if !gray {
            if p.quality < 90 {
                c.set_chroma_sampling_pixel_sizes((2, 2), (2, 2));
            } else {
                c.set_chroma_sampling_pixel_sizes((1, 1), (1, 1));
            }
        }
        let mut s = c.start_compress(Vec::new())?;
        if let Some(exif) = p.exif {
            let mut app1 = b"Exif\0\0".to_vec();
            app1.extend_from_slice(exif);
            if app1.len() <= MAX_MARKER_LEN {
                s.write_marker(Marker::APP(1), &app1);
            }
        }
        if let Some(icc) = p.icc {
            s.write_icc_profile(icc);
        }
        s.write_scanlines(&pixels)?;
        s.finish()
    }))
    .map_err(|_| CoreError::Encode("JPEG encoder failed".into()))?
    .map_err(CoreError::encode)
}
