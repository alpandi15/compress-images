//! Content-based format detection (SPEC §3.1). Extensions are never trusted.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::event::SkipReason;

/// Image formats the engine knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageKind {
    Jpeg,
    Png,
    Webp,
    Avif,
    Tiff,
    Bmp,
    Heic,
}

impl ImageKind {
    /// Canonical lowercase file extension used for outputs.
    pub fn extension(self) -> &'static str {
        match self {
            ImageKind::Jpeg => "jpg",
            ImageKind::Png => "png",
            ImageKind::Webp => "webp",
            ImageKind::Avif => "avif",
            ImageKind::Tiff => "tiff",
            ImageKind::Bmp => "bmp",
            ImageKind::Heic => "heic",
        }
    }

    /// Whether this build can decode the format (HEIC and AVIF depend on cargo features).
    pub fn is_decodable(self) -> bool {
        let heic = cfg!(feature = "heic");
        let avif = cfg!(feature = "avif-decode");
        match self {
            ImageKind::Heic => heic,
            ImageKind::Avif => avif,
            _ => true,
        }
    }

    /// Formats whose pixels are stored without loss, so lossless re-encoding is meaningful.
    pub fn is_lossless_source(self) -> bool {
        matches!(self, ImageKind::Png | ImageKind::Bmp | ImageKind::Tiff)
    }
}

/// Number of leading bytes needed by [`sniff`].
pub const SNIFF_LEN: usize = 128;

/// Bytes read during scanning: enough for [`sniff`] and, in practice, for the header-based
/// animation checks, so a dry run can already report animated files.
const SCAN_HEAD_LEN: u64 = 4096;

/// Identifies an image format from the first bytes of a file.
pub fn sniff(header: &[u8]) -> Option<ImageKind> {
    let h = header;
    if h.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(ImageKind::Jpeg);
    }
    if h.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(ImageKind::Png);
    }
    if h.len() >= 12 && &h[0..4] == b"RIFF" && &h[8..12] == b"WEBP" {
        return Some(ImageKind::Webp);
    }
    if h.starts_with(b"II*\0") || h.starts_with(b"MM\0*") {
        return Some(ImageKind::Tiff);
    }
    if h.starts_with(b"BM") && h.len() >= 14 {
        return Some(ImageKind::Bmp);
    }
    sniff_isobmff(h)
}

/// AVIF and HEIC are both ISO-BMFF containers; tell them apart by `ftyp` brands.
fn sniff_isobmff(h: &[u8]) -> Option<ImageKind> {
    if h.len() < 16 || &h[4..8] != b"ftyp" {
        return None;
    }
    let box_len = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as usize;
    let end = box_len.clamp(16, h.len());
    let major = &h[8..12];
    // Compatible brands start after major brand (4) + minor version (4).
    let (compat, _) = h[16..end].as_chunks::<4>();
    let brands: Vec<&[u8]> = std::iter::once(major)
        .chain(compat.iter().map(|b| b.as_slice()))
        .collect();

    const AVIF: [&[u8]; 2] = [b"avif", b"avis"];
    const HEIC: [&[u8]; 8] = [
        b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs",
    ];
    const GENERIC: [&[u8]; 2] = [b"mif1", b"msf1"];

    if AVIF.contains(&major) {
        return Some(ImageKind::Avif);
    }
    if HEIC.contains(&major) {
        return Some(ImageKind::Heic);
    }
    if brands.iter().any(|b| AVIF.contains(b)) {
        return Some(ImageKind::Avif);
    }
    if brands
        .iter()
        .any(|b| HEIC.contains(b) || GENERIC.contains(b))
    {
        return Some(ImageKind::Heic);
    }
    None
}

/// Reads the head of `path`: its format and, if detectable from the header alone, why it
/// can't be processed (animated / multi-page). The executor re-checks with the full file.
pub fn sniff_file(path: &Path) -> io::Result<(Option<ImageKind>, Option<SkipReason>)> {
    let mut buf = Vec::with_capacity(SCAN_HEAD_LEN as usize);
    File::open(path)?
        .take(SCAN_HEAD_LEN)
        .read_to_end(&mut buf)?;
    let kind = sniff(&buf);
    let variant = kind.and_then(|k| unsupported_variant(k, &buf));
    Ok((kind, variant))
}

/// Detects inputs that are valid images but out of scope (animation, multi-page).
pub(crate) fn unsupported_variant(kind: ImageKind, data: &[u8]) -> Option<SkipReason> {
    let hit = match kind {
        ImageKind::Png => is_apng(data),
        ImageKind::Webp => is_animated_webp(data),
        ImageKind::Tiff => is_multi_page_tiff(data),
        _ => false,
    };
    hit.then_some(match kind {
        ImageKind::Tiff => SkipReason::MultiPage,
        _ => SkipReason::Animated,
    })
}

/// APNG files carry an `acTL` chunk before the first `IDAT`.
fn is_apng(data: &[u8]) -> bool {
    let mut pos = 8;
    while pos + 8 <= data.len() {
        let len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let ty = &data[pos + 4..pos + 8];
        if ty == b"acTL" {
            return true;
        }
        if ty == b"IDAT" || ty == b"IEND" {
            return false;
        }
        pos = pos.saturating_add(12).saturating_add(len as usize);
    }
    false
}

/// Extended WebP (`VP8X`) has an animation flag in its first flag byte.
fn is_animated_webp(data: &[u8]) -> bool {
    data.len() >= 21 && &data[12..16] == b"VP8X" && data[20] & 0x02 != 0
}

/// A TIFF is multi-page if its first IFD links to a next IFD.
fn is_multi_page_tiff(data: &[u8]) -> bool {
    let le = match data.get(0..4) {
        Some(b"II*\0") => true,
        Some(b"MM\0*") => false,
        _ => return false, // BigTIFF or garbage: let the decoder decide.
    };
    let u16_at = |p: usize| -> Option<u16> {
        let b = data.get(p..p + 2)?;
        Some(if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32_at = |p: usize| -> Option<u32> {
        let b = data.get(p..p + 4)?;
        let a = [b[0], b[1], b[2], b[3]];
        Some(if le {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        })
    };
    let check = || -> Option<bool> {
        let ifd = u32_at(4)? as usize;
        let count = u16_at(ifd)? as usize;
        let next = u32_at(ifd + 2 + count * 12)?;
        Some(next != 0)
    };
    check().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ftyp(major: &[u8; 4], compat: &[&[u8; 4]]) -> Vec<u8> {
        let len = 16 + 4 * compat.len();
        let mut v = (len as u32).to_be_bytes().to_vec();
        v.extend_from_slice(b"ftyp");
        v.extend_from_slice(major);
        v.extend_from_slice(&[0, 0, 0, 0]);
        for c in compat {
            v.extend_from_slice(*c);
        }
        v
    }

    #[test]
    fn magic_bytes() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(ImageKind::Jpeg));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n...."), Some(ImageKind::Png));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some(ImageKind::Webp));
        assert_eq!(sniff(b"II*\0\x08\0\0\0"), Some(ImageKind::Tiff));
        assert_eq!(sniff(b"MM\0*\0\0\0\x08"), Some(ImageKind::Tiff));
        assert_eq!(sniff(b"BM\0\0\0\0\0\0\0\0\0\0\0\0"), Some(ImageKind::Bmp));
        assert_eq!(sniff(b"GIF89a"), None);
        assert_eq!(sniff(b"hello world"), None);
        assert_eq!(sniff(b""), None);
    }

    #[test]
    fn isobmff_brands() {
        assert_eq!(sniff(&ftyp(b"avif", &[b"mif1"])), Some(ImageKind::Avif));
        assert_eq!(sniff(&ftyp(b"heic", &[b"mif1"])), Some(ImageKind::Heic));
        assert_eq!(sniff(&ftyp(b"mif1", &[b"avif"])), Some(ImageKind::Avif));
        assert_eq!(sniff(&ftyp(b"mif1", &[b"heic"])), Some(ImageKind::Heic));
        assert_eq!(sniff(&ftyp(b"isom", &[b"mp41"])), None, "MP4 video");
    }

    #[test]
    fn animated_webp_flag() {
        let mut d = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0".to_vec();
        d.push(0x02);
        d.extend_from_slice(&[0; 9]);
        assert_eq!(
            unsupported_variant(ImageKind::Webp, &d),
            Some(SkipReason::Animated)
        );
        d[20] = 0x10; // alpha flag only
        assert_eq!(unsupported_variant(ImageKind::Webp, &d), None);
    }

    #[test]
    fn multi_page_tiff() {
        // Little-endian header, IFD at 8 with 0 entries, next IFD offset = 14 / 0.
        let mut d = b"II*\0\x08\0\0\0".to_vec();
        d.extend_from_slice(&[0, 0]);
        d.extend_from_slice(&14u32.to_le_bytes());
        assert!(is_multi_page_tiff(&d));
        d[10..14].copy_from_slice(&0u32.to_le_bytes());
        assert!(!is_multi_page_tiff(&d));
    }
}
