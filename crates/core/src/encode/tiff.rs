use std::io::Cursor;

use image::DynamicImage;
use tiff::encoder::colortype::{self, ColorType};
use tiff::encoder::compression::DeflateLevel;
use tiff::encoder::{Compression, TiffEncoder, TiffValue};
use tiff::tags::{Predictor, Tag};

use super::EncodeParams;
use crate::error::{CoreError, Result};

/// TIFF tag holding an embedded ICC profile.
const ICC_PROFILE_TAG: u16 = 34675;

/// Lossless deflate with horizontal predictor; quality does not apply (SPEC §3.6).
pub fn encode(img: &DynamicImage, p: &EncodeParams<'_>) -> Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut enc = TiffEncoder::new(&mut buf)
            .map_err(CoreError::encode)?
            .with_compression(Compression::Deflate(DeflateLevel::Balanced))
            .with_predictor(Predictor::Horizontal);
        let (w, h) = (img.width(), img.height());
        let icc = p.icc;
        match img {
            DynamicImage::ImageLuma8(i) => write::<colortype::Gray8>(&mut enc, w, h, i, icc),
            DynamicImage::ImageLuma16(i) => write::<colortype::Gray16>(&mut enc, w, h, i, icc),
            DynamicImage::ImageRgb8(i) => write::<colortype::RGB8>(&mut enc, w, h, i, icc),
            DynamicImage::ImageRgba8(i) => write::<colortype::RGBA8>(&mut enc, w, h, i, icc),
            DynamicImage::ImageRgb16(i) => write::<colortype::RGB16>(&mut enc, w, h, i, icc),
            DynamicImage::ImageRgba16(i) => write::<colortype::RGBA16>(&mut enc, w, h, i, icc),
            // Gray+alpha and float images have no direct TIFF colortype here.
            other if other.color().has_alpha() => {
                write::<colortype::RGBA16>(&mut enc, w, h, &other.to_rgba16(), icc)
            }
            other => write::<colortype::RGB16>(&mut enc, w, h, &other.to_rgb16(), icc),
        }?;
    }
    Ok(buf.into_inner())
}

fn write<C: ColorType>(
    enc: &mut TiffEncoder<&mut Cursor<Vec<u8>>>,
    w: u32,
    h: u32,
    data: &[C::Inner],
    icc: Option<&[u8]>,
) -> Result<()>
where
    [C::Inner]: TiffValue,
{
    let mut image = enc.new_image::<C>(w, h).map_err(CoreError::encode)?;
    if let Some(icc) = icc {
        image
            .encoder()
            .write_tag(Tag::Unknown(ICC_PROFILE_TAG), icc)
            .map_err(CoreError::encode)?;
    }
    image.write_data(data).map_err(CoreError::encode)
}
