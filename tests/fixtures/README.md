# Test fixtures

All images are **synthetic** (generated from math + a seeded PRNG), so they contain no
personal data and are free of third-party copyright. They are dedicated to the public domain
(CC0) and may be regenerated at any time.

Regenerate from the workspace root:

```bash
cargo run -p compress-core --example gen_fixtures
heif-enc -q 90 -o tests/fixtures/photo.heic tests/fixtures/photo.png   # from Homebrew libheif
```

| File | What it tests |
|---|---|
| `photo.jpg` | JPEG at quality 95. Re-encoding at the default quality must shrink it. |
| `photo.png` | 24-bit PNG. Lossy quantization path. |
| `photo.tiff` | Uncompressed TIFF. Lossless deflate output. |
| `photo.bmp` | BMP. Default output is PNG (`photo-bmp.png`, because `photo.png` claims the name). |
| `photo.webp` | Lossy WebP. |
| `photo.avif` | AVIF (needs the `avif-decode` feature). |
| `photo.heic` | HEIC (needs the `heic` feature). Default output is JPEG (`photo-heic.jpg`). |
| `alpha.png` | RGBA with a fully transparent corner. Alpha handling and flattening to white for JPEG. |
| `gray.png` | 8-bit grayscale. |
| `rotated.jpg` | 200×100 pixels with Exif orientation 6. Output must be 100×200. |
| `fake-png.jpg` | PNG content behind a `.jpg` extension. Content-based detection. |
| `animated.png` | PNG with an `acTL` chunk (APNG). Must be skipped as animated. |
| `animated.webp` | WebP `VP8X` header with the animation flag. Must be skipped as animated. |
| `sub/deep/nested.jpg` | Recursion and mirrored output structure. |
| `notes.txt` | Non-image. Must be skipped as unsupported. |

Rules (see AGENTS.md): keep each file under 200 KB, never add real photos, and list every new
fixture here.
