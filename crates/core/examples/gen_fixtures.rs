//! Regenerates the synthetic test fixtures in `tests/fixtures/`.
//!
//! Run from the workspace root: `cargo run -p compress-core --example gen_fixtures`
//! The HEIC fixture is produced separately with `heif-enc` (see tests/fixtures/README.md).

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GrayImage, ImageFormat, Luma, Rgb, RgbImage, Rgba, RgbaImage};
use mozjpeg::{ColorSpace, Compress, Marker};

type Res<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn main() -> Res {
    let out: PathBuf = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures"));
    fs::create_dir_all(out.join("sub/deep"))?;

    let photo = photo_like(200, 150, 1);

    // High-quality JPEG: re-encoding at the default quality must shrink it.
    fs::write(out.join("photo.jpg"), jpeg(&photo, 95)?)?;
    save(
        DynamicImage::ImageRgb8(photo.clone()),
        &out.join("photo.png"),
        ImageFormat::Png,
    )?;
    save(
        DynamicImage::ImageRgb8(photo.clone()),
        &out.join("photo.tiff"),
        ImageFormat::Tiff,
    )?;
    save(
        DynamicImage::ImageRgb8(photo_like(160, 120, 2)),
        &out.join("photo.bmp"),
        ImageFormat::Bmp,
    )?;

    let webp = webp::Encoder::from_rgb(photo.as_raw(), photo.width(), photo.height()).encode(95.0);
    fs::write(out.join("photo.webp"), &*webp)?;

    let avif = ravif::Encoder::new()
        .with_quality(95.0)
        .with_speed(8)
        .encode_rgb(ravif::Img::new(
            rgb::FromSlice::as_rgb(photo.as_raw().as_slice()),
            photo.width() as usize,
            photo.height() as usize,
        ))?;
    fs::write(out.join("photo.avif"), &avif.avif_file)?;

    save(
        DynamicImage::ImageRgba8(with_alpha(160, 120)),
        &out.join("alpha.png"),
        ImageFormat::Png,
    )?;
    save(
        DynamicImage::ImageLuma8(gray(120, 90)),
        &out.join("gray.png"),
        ImageFormat::Png,
    )?;

    // 200×100 landscape pixels tagged "rotate 90° CW": must come out 100×200.
    fs::write(
        out.join("rotated.jpg"),
        jpeg_with_orientation(&photo_like(200, 100, 3), 6),
    )?;

    // PNG content behind a .jpg extension: detection must go by content.
    let mut fake = Vec::new();
    photo_like(64, 64, 4).write_to(&mut Cursor::new(&mut fake), ImageFormat::Png)?;
    fs::write(out.join("fake-png.jpg"), &fake)?;

    // Animated variants are detected from headers and skipped before decoding.
    fs::write(out.join("animated.png"), apng_from(&fake))?;
    fs::write(out.join("animated.webp"), animated_webp_header())?;

    fs::write(
        out.join("sub/deep/nested.jpg"),
        jpeg(&photo_like(120, 90, 5), 95)?,
    )?;

    fs::write(
        out.join("notes.txt"),
        "Not an image. Must be skipped as unsupported.\n",
    )?;

    println!("fixtures written to {}", out.display());
    Ok(())
}

fn save(img: DynamicImage, path: &Path, fmt: ImageFormat) -> Res {
    img.save_with_format(path, fmt)?;
    Ok(())
}

fn jpeg(img: &RgbImage, q: u8) -> Res<Vec<u8>> {
    let mut buf = Vec::new();
    JpegEncoder::new_with_quality(&mut buf, q).encode_image(img)?;
    Ok(buf)
}

/// Deterministic pseudo-random generator so fixtures are reproducible byte-for-byte.
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u8 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 24) as u8
    }
}

/// Smooth gradients + soft shapes + light noise: compresses like a real photo.
fn photo_like(w: u32, h: u32, seed: u32) -> RgbImage {
    let mut rng = Lcg(seed);
    RgbImage::from_fn(w, h, |x, y| {
        let fx = x as f32 / w as f32;
        let fy = y as f32 / h as f32;
        let dx = fx - 0.6;
        let dy = fy - 0.4;
        let blob = (-(dx * dx + dy * dy) * 12.0).exp();
        let n = f32::from(rng.next()) / 255.0 * 18.0 - 9.0;
        let r = 40.0 + 180.0 * fx + 30.0 * blob + n;
        let g = 60.0 + 120.0 * fy + 80.0 * blob + n;
        let b = 150.0 - 90.0 * fx + 60.0 * (fy * 6.0).sin() + n;
        Rgb([clamp(r), clamp(g), clamp(b)])
    })
}

fn with_alpha(w: u32, h: u32) -> RgbaImage {
    let base = photo_like(w, h, 7);
    RgbaImage::from_fn(w, h, |x, y| {
        let [r, g, b] = base.get_pixel(x, y).0;
        // Fully transparent top-left corner; opaque disc in the middle; soft edge between.
        let dx = x as f32 - w as f32 / 2.0;
        let dy = y as f32 - h as f32 / 2.0;
        let d = (dx * dx + dy * dy).sqrt();
        let a = if x < 10 && y < 10 {
            0.0
        } else {
            (255.0 - (d - 40.0).max(0.0) * 6.0).clamp(0.0, 255.0)
        };
        Rgba([r, g, b, a as u8])
    })
}

fn gray(w: u32, h: u32) -> GrayImage {
    let mut rng = Lcg(9);
    GrayImage::from_fn(w, h, |x, y| {
        let v = 30.0 + 200.0 * (x + y) as f32 / (w + h) as f32 + f32::from(rng.next() % 12);
        Luma([clamp(v)])
    })
}

fn clamp(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

fn jpeg_with_orientation(img: &RgbImage, orientation: u16) -> Vec<u8> {
    // Minimal little-endian Exif: IFD0 with a single Orientation (0x0112) SHORT entry.
    let mut exif = b"Exif\0\0II*\0\x08\0\0\0".to_vec();
    exif.extend_from_slice(&1u16.to_le_bytes());
    exif.extend_from_slice(&0x0112u16.to_le_bytes());
    exif.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    exif.extend_from_slice(&1u32.to_le_bytes()); // count
    exif.extend_from_slice(&orientation.to_le_bytes());
    exif.extend_from_slice(&[0, 0]); // value padding
    exif.extend_from_slice(&0u32.to_le_bytes()); // no next IFD

    let mut c = Compress::new(ColorSpace::JCS_RGB);
    c.set_size(img.width() as usize, img.height() as usize);
    c.set_quality(95.0);
    let mut s = c.start_compress(Vec::new()).expect("start jpeg");
    s.write_marker(Marker::APP(1), &exif);
    s.write_scanlines(img.as_raw()).expect("write scanlines");
    s.finish().expect("finish jpeg")
}

/// Inserts an `acTL` chunk right after IHDR, which is what marks a PNG as animated.
fn apng_from(png: &[u8]) -> Vec<u8> {
    let ihdr_end = 8 + 4 + 4 + 13 + 4;
    let mut data = Vec::new();
    data.extend_from_slice(&8u32.to_be_bytes());
    let mut body = b"acTL".to_vec();
    body.extend_from_slice(&1u32.to_be_bytes()); // num_frames
    body.extend_from_slice(&0u32.to_be_bytes()); // num_plays
    data.extend_from_slice(&body);
    data.extend_from_slice(&crc32(&body).to_be_bytes());

    let mut out = png[..ihdr_end].to_vec();
    out.extend_from_slice(&data);
    out.extend_from_slice(&png[ihdr_end..]);
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// RIFF/WEBP header with a VP8X chunk whose animation flag is set.
fn animated_webp_header() -> Vec<u8> {
    let mut d = b"RIFF".to_vec();
    d.extend_from_slice(&22u32.to_le_bytes());
    d.extend_from_slice(b"WEBPVP8X");
    d.extend_from_slice(&10u32.to_le_bytes());
    d.push(0x02); // animation flag
    d.extend_from_slice(&[0; 3]);
    d.extend_from_slice(&[0x3F, 0, 0, 0x3F, 0, 0]); // 64×64 canvas (minus-one encoding)
    d
}
