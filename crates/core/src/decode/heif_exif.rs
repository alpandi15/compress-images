//! Finds the Exif item in a HEIF-family container (HEIC, AVIF) without decoding anything.
//!
//! The macOS ImageIO backend only exposes Exif as parsed dictionaries, and the `image` crate's
//! AVIF decoder doesn't expose it at all, so the raw TIFF block (needed to carry metadata over
//! when `keep_metadata` is on) is read straight from the ISOBMFF boxes: `meta` → `iinf`
//! (which item is Exif) → `iloc` (where its bytes are).

/// Upper bound on the Exif block we copy; real ones are a few KB, a corrupt size must not
/// make us allocate gigabytes.
const MAX_EXIF: usize = 4 * 1024 * 1024;

/// Raw Exif (TIFF structure, no prefix) from a HEIF/AVIF file, or `None` if absent/malformed.
pub fn extract(file: &[u8]) -> Option<Vec<u8>> {
    let meta = boxes(file).find(|b| &b.kind == b"meta")?.body.get(4..)?; // full box
    let id = boxes(meta)
        .find(|b| &b.kind == b"iinf")
        .and_then(|b| exif_item_id(b.body))?;
    let iloc = boxes(meta).find(|b| &b.kind == b"iloc")?;
    let idat = boxes(meta).find(|b| &b.kind == b"idat").map(|b| b.body);
    let raw = item_data(iloc.body, id, file, idat)?;
    tiff_payload(&raw)
}

/// HEIF Exif items start with a 4-byte big-endian offset to the TIFF header.
pub fn tiff_payload(raw: &[u8]) -> Option<Vec<u8>> {
    let off = be(raw.get(0..4)?) as usize;
    let tiff = raw.get(4usize.checked_add(off)?..)?;
    (tiff.starts_with(b"II*\0") || tiff.starts_with(b"MM\0*")).then(|| tiff.to_vec())
}

struct IsoBox<'a> {
    kind: [u8; 4],
    body: &'a [u8],
}

/// Iterates the boxes laid out back to back in `data`; stops at the first malformed one.
fn boxes(mut data: &[u8]) -> impl Iterator<Item = IsoBox<'_>> {
    std::iter::from_fn(move || {
        let size = be(data.get(0..4)?);
        let kind: [u8; 4] = data.get(4..8)?.try_into().ok()?;
        let (header, total) = match size {
            0 => (8, data.len() as u64),     // box extends to the end
            1 => (16, be(data.get(8..16)?)), // 64-bit size follows
            n => (8, n),
        };
        let total = usize::try_from(total).ok()?;
        if total < header || total > data.len() {
            return None;
        }
        let body = &data[header..total];
        data = &data[total..];
        Some(IsoBox { kind, body })
    })
}

/// Item id of the first `infe` entry whose type is `Exif`.
fn exif_item_id(iinf: &[u8]) -> Option<u32> {
    let version = *iinf.first()?;
    let entries = iinf.get(if version == 0 { 6 } else { 8 }..)?; // full box + entry count
    boxes(entries).filter(|b| &b.kind == b"infe").find_map(|b| {
        let v = *b.body.first()?;
        // Versions 0/1 carry no item type; only 2 (16-bit id) and 3 (32-bit id) do.
        let (id, rest) = match v {
            2 => (be(b.body.get(4..6)?) as u32, b.body.get(6..)?),
            3 => (be(b.body.get(4..8)?) as u32, b.body.get(8..)?),
            _ => return None,
        };
        (rest.get(2..6)? == b"Exif").then_some(id) // skip protection index
    })
}

/// Concatenated extents of item `id`, read from the file or from `idat`.
fn item_data(iloc: &[u8], id: u32, file: &[u8], idat: Option<&[u8]>) -> Option<Vec<u8>> {
    let mut r = Reader { data: iloc, pos: 0 };
    let version = r.take(1)?;
    r.take(3)?; // flags
    let sizes = r.take(2)?;
    let (offset_size, length_size) = ((sizes >> 12) & 0xF, (sizes >> 8) & 0xF);
    let base_offset_size = (sizes >> 4) & 0xF;
    let index_size = if version >= 1 { sizes & 0xF } else { 0 };
    let count = if version < 2 { r.take(2)? } else { r.take(4)? };

    for _ in 0..count {
        let item = if version < 2 { r.take(2)? } else { r.take(4)? };
        let method = if version >= 1 { r.take(2)? & 0xF } else { 0 };
        r.take(2)?; // data reference index
        let base = r.take(base_offset_size as usize)?;
        let extents = r.take(2)?;
        let mut out = Vec::new();
        for _ in 0..extents {
            r.take(index_size as usize)?;
            let offset = base.checked_add(r.take(offset_size as usize)?)?;
            let length = r.take(length_size as usize)?;
            if item != u64::from(id) {
                continue;
            }
            let source = match method {
                0 => file,
                1 => idat?,
                _ => return None, // item-relative offsets: not used for Exif in practice
            };
            let start = usize::try_from(offset).ok()?;
            let end = if length == 0 {
                source.len() // zero length = to the end of the source
            } else {
                start.checked_add(usize::try_from(length).ok()?)?
            };
            out.extend_from_slice(source.get(start..end)?);
            if out.len() > MAX_EXIF {
                return None;
            }
        }
        if item == u64::from(id) {
            return Some(out);
        }
    }
    None
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    /// Reads an `n`-byte big-endian unsigned integer (`n` may be 0, 1, 2, 4 or 8).
    fn take(&mut self, n: usize) -> Option<u64> {
        if n > 8 {
            return None;
        }
        let bytes = self.data.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(be(bytes))
    }
}

fn be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |acc, &b| (acc << 8) | u64::from(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIFF: &[u8] = b"MM\0*\0\0\0\x08\0\0";

    fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(body);
        v
    }

    fn full(kind: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![version, 0, 0, 0];
        v.extend_from_slice(body);
        bx(kind, &v)
    }

    fn infe(id: u16, kind: &[u8; 4]) -> Vec<u8> {
        let mut b = id.to_be_bytes().to_vec();
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(kind);
        b.push(0); // empty item name
        full(b"infe", 2, &b)
    }

    /// A file with an `hvc1` item (id 1) and an Exif item (id 2), Exif stored at the end of
    /// the file (method 0) or inside `idat` (method 1).
    fn heif(method: u8) -> Vec<u8> {
        let mut exif = vec![0, 0, 0, 2, b'x', b'x'];
        exif.extend_from_slice(TIFF);
        let iinf = {
            let mut b = 2u16.to_be_bytes().to_vec();
            b.extend(infe(1, b"hvc1"));
            b.extend(infe(2, b"Exif"));
            full(b"iinf", 0, &b)
        };
        let iloc = |exif_offset: u32| {
            // version 1, offset/length size 4, base offset size 0, index size 0, 2 items
            let mut b = vec![0x44, 0x00, 0, 2];
            for (id, off, len) in [(1u16, 0u32, 1u32), (2, exif_offset, exif.len() as u32)] {
                b.extend(id.to_be_bytes());
                b.extend([0, if id == 2 { method } else { 0 }]);
                b.extend([0, 0, 0, 1]); // data ref index, 1 extent
                b.extend(off.to_be_bytes());
                b.extend(len.to_be_bytes());
            }
            full(b"iloc", 1, &b)
        };
        let ftyp = bx(b"ftyp", b"heic\0\0\0\0mif1heic");
        let idat = if method == 1 {
            bx(b"idat", &exif)
        } else {
            vec![]
        };
        let build = |off: u32| {
            let mut meta_body = full(b"hdlr", 0, b"\0\0\0\0pict\0\0\0\0\0\0\0\0\0\0\0\0\0");
            meta_body.extend(iinf.clone());
            meta_body.extend(iloc(off));
            meta_body.extend(idat.clone());
            let mut f = ftyp.clone();
            f.extend(full(b"meta", 0, &meta_body));
            f
        };
        if method == 1 {
            return build(0);
        }
        let len = build(0).len() + 8;
        let mut f = build(len as u32);
        f.extend(bx(b"mdat", &exif));
        f
    }

    #[test]
    fn finds_exif_stored_in_the_file_or_idat() {
        assert_eq!(extract(&heif(0)).as_deref(), Some(TIFF));
        assert_eq!(extract(&heif(1)).as_deref(), Some(TIFF));
    }

    #[test]
    fn malformed_or_missing_returns_none() {
        assert_eq!(extract(b""), None);
        assert_eq!(extract(b"\0\0\0\x08ftyp"), None);
        let f = heif(0);
        for cut in [10, 40, 120, f.len() - 3] {
            assert_eq!(extract(&f[..cut]), None, "cut at {cut}");
        }
        // Bogus box size larger than the data.
        let mut bad = f.clone();
        bad[0..4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(extract(&bad), None);
    }

    #[test]
    fn payload_needs_a_tiff_header() {
        assert_eq!(
            tiff_payload(b"\0\0\0\0II*\0rest").as_deref(),
            Some(&b"II*\0rest"[..])
        );
        assert_eq!(tiff_payload(b"\0\0\0\0garbage"), None);
        assert_eq!(tiff_payload(b"\xff\xff\xff\xffII*\0"), None);
    }

    #[test]
    fn real_files() {
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        let rotated = std::fs::read(fixtures.join("rotated.heic")).unwrap();
        let exif = extract(&rotated).expect("rotated.heic carries Exif");
        assert!(exif.starts_with(b"MM\0*") || exif.starts_with(b"II*\0"));
        let plain = std::fs::read(fixtures.join("photo.heic")).unwrap();
        assert_eq!(extract(&plain), None);
    }
}
