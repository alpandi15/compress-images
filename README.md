# Compress Images

Compress images one by one or whole folders at a time, on macOS, Windows and Linux.
Results go into a `compressed/` folder. **Your originals are never touched.**

> **Status:** v0.3: command-line tool, desktop app and right-click menu (macOS, Windows,
> Linux). Prebuilt downloads are next (see the [roadmap](docs/SPEC.md#8-roadmap--acceptance-criteria)).

## Desktop app

Drop files or folders onto the window (or use *Add files* / *Add folder*), pick a preset, and
press **Compress**. Settings are remembered. English and Indonesian UI.

## Right-click menu

Open **Settings → Right-click menu → Add to menu** (the Windows installer adds it for you).

| OS | Where it appears |
|---|---|
| Windows | Right-click an image, a folder, or inside a folder → **Compress Images** (Windows 11: under *Show more options*) |
| macOS | Finder: right-click → **Quick Actions → Compress Images** (if it's missing, enable it under *Quick Actions → Customize…*) |
| Linux | Nautilus/Files, Dolphin and Nemo: right-click → **Compress Images** (restart the file manager once, e.g. `nautilus -q`) |

The menu opens a small progress window that closes by itself when done. Selecting several items,
or clicking the menu again within half a second, merges everything into one job. If you move the
app, the Settings card shows **Reinstall** to point the menu at the new location.

## Features

- **Files or folders**, including all subfolders, with the folder structure mirrored inside `compressed/`.
- **Formats:** JPEG, PNG, WebP, AVIF, TIFF, BMP and HEIC/HEIF (iPhone photos). Detection goes by file content, not extension.
- **Quality 1–100** or presets: `light` (85), `balanced` (70, default), `aggressive` (50).
- **Convert** everything to JPEG, PNG, WebP or AVIF.
- **Resize** to a maximum width/height (never upscales).
- **Safe:** atomic writes, and an output is never larger than its source when the format stays the same.
- Strips Exif/GPS by default, while keeping the color profile and correct rotation.

## Usage

```bash
compress-images ~/Pictures/Holiday                # whole folder (recursive)
compress-images photo.jpg scan.png                # individual files
compress-images -p aggressive -f webp ./shots     # preset + convert to WebP
compress-images --max-width 1920 ./camera         # downscale large photos
compress-images --dry-run ./folder                # preview, write nothing
compress-images --help                            # all options
```

Result:

```
Holiday/
├── a.jpg
├── Bali/b.heic
└── compressed/
    ├── a.jpg
    └── Bali/b.jpg
```

## Building from source

Requirements: Rust 1.88+ (via [rustup](https://rustup.rs)), `nasm`, `pkg-config`, and for HEIC/AVIF
input `libheif` ≥ 1.17 and `dav1d` ≥ 1.3.

```bash
# macOS
brew install nasm pkg-config libheif dav1d
# Ubuntu 24.04+
sudo apt install build-essential nasm pkg-config libheif-dev libdav1d-dev

cargo build --release -p compress-images   # CLI: target/release/compress-images
cargo build --release -p compress-images --no-default-features   # CLI without HEIC/AVIF input
```

Desktop app (additionally needs Node.js 22+ and pnpm; on Linux also
`libwebkit2gtk-4.1-dev libxdo-dev libssl-dev librsvg2-dev`):

```bash
cd app
pnpm install
pnpm build                  # the Rust crate embeds app/dist, so build it before `cargo test --workspace`
cargo tauri dev             # run with hot reload
cargo tauri build           # installers in target/release/bundle/
```

## Contributing

Read [AGENTS.md](AGENTS.md) (workflow, rules, commands) and [docs/SPEC.md](docs/SPEC.md)
(behavior spec, source of truth). Before opening a PR, run `cargo fmt --all`,
`cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.

## License

[GPL-3.0-only](LICENSE). Uses [libimagequant](https://pngquant.org/lib/) (GPL-3.0),
mozjpeg, libwebp, rav1e, dav1d and libheif (LGPL-3.0, dynamically linked).
