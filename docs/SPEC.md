# Compress Images — Product & Technical Specification

> **Status:** v0.1 implemented (core + CLI) · **Last updated:** 2026-11-12
> This document is the single source of truth for product behavior. If code and spec disagree, the spec wins until it is deliberately changed. Any behavior change must update this file in the same change.

---

## 1. Overview

**Compress Images** is a cross-platform desktop application (macOS, Windows, Linux/Ubuntu) that compresses image files individually or in bulk (whole folders, including subfolders). Results are written to a dedicated output folder; originals are never modified.

It is distributed as open source on GitHub. The primary user is the author; secondary users are anyone downloading GitHub releases.

### 1.1 Goals

- Compress a single file, multiple files, or an entire folder tree in one action.
- Configurable compression strength (quality 1–100 %) with simple presets.
- Optional format conversion (e.g. everything → WebP).
- Launchable from the OS file manager context menu (right-click on file or folder).
- Fast (parallel), safe (never touches originals), predictable (deterministic output paths).

### 1.2 Non-goals (for now)

- GIF, SVG, ICO, RAW camera formats.
- Animated images (animated WebP / APNG are skipped, not compressed).
- Cloud upload, accounts, telemetry of any kind.
- Image editing (crop, filters, watermarks).
- Paid code signing (may be revisited after v1.0).

---

## 2. Glossary

| Term | Meaning |
|---|---|
| **Input** | A path the user selected: a file or a folder. |
| **Root** | For a folder input: the folder itself. For a file input: the file's parent directory. |
| **Output dir** | `<root>/<out_dir_name>/`, default name `compressed`. |
| **Job** | One invocation (GUI run, CLI run, or context-menu action) over one or more inputs. |
| **Plan** | The fully resolved list of `(source → destination, action)` pairs, computed before any encoding. |
| **Quick mode** | The app launched with paths as arguments (from the context menu): shows a minimal progress window, no full GUI. |

---

## 3. Functional Requirements

### 3.1 Supported formats

Detection is by **file content (magic bytes)**, not by extension. A `.jpg` that is actually a PNG is treated as PNG and its output gets the correct extension.

| Input format | Decode | Default output ("same") | Notes |
|---|---|---|---|
| JPEG (`.jpg`, `.jpeg`, `.jfif`) | ✅ | JPEG | CMYK JPEGs are converted to RGB. |
| PNG | ✅ | PNG | APNG (animated) → skipped. |
| WebP | ✅ | WebP | Animated WebP → skipped. |
| AVIF | ✅ (feature `avif-decode`) | AVIF | macOS: system ImageIO (macOS 13+). Elsewhere: `dav1d`. Without the feature: reported as unsupported. |
| TIFF (`.tif`, `.tiff`) | ✅ | TIFF (lossless deflate) | Multi-page TIFF → skipped. Quality setting does not apply to TIFF→TIFF. |
| BMP | ✅ | **PNG** | BMP has no useful compression; BMP→BMP is never produced. |
| HEIC / HEIF | ✅ (feature `heic`) | **JPEG** | Decode-only; HEIC is never an output format (patent/licensing). macOS: system ImageIO. Elsewhere: `libheif`. |
| GIF, SVG, ICO, others | ❌ | — | Silently skipped with reason `unsupported`. |

### 3.2 Output formats

User-selectable `output_format`: `same` (default) · `jpg` · `png` · `webp` · `avif`.

- When the output format differs from the input, the extension changes: `photo.png` → `compressed/photo.webp`.
- Converting an image with alpha to JPEG flattens it onto a **white** background.

### 3.3 Input handling and scanning

1. A job accepts **1..N inputs**, each a file or a folder. Mixed selections are allowed.
2. **Folder inputs** are scanned **recursively by default** (`recursive = true`; can be disabled).
3. During scanning, the following are **excluded**:
   - Any directory whose name equals `out_dir_name` (case-insensitive), at any depth. This prevents re-compressing previous output.
   - Hidden files and directories (name starts with `.`) and, on Windows, files with the Hidden or System attribute.
   - Symbolic links / junctions (not followed).
   - Zero-byte files.
4. Each remaining file is sniffed (first 4 KB); non-images and unsupported formats are recorded as `Skipped { reason: unsupported }` and do not count as errors. Animated PNG/WebP (and multi-page TIFF when visible in the header) are already marked `Skipped` here, so `--dry-run` reports them; the executor re-checks with the full file.
5. A **file input** that is itself unsupported is reported the same way (not a fatal error).

### 3.4 Output location rules

| Input | Source path | Destination |
|---|---|---|
| File `D/a.jpg` | `D/a.jpg` | `D/compressed/a.jpg` |
| Folder `R` | `R/a.jpg` | `R/compressed/a.jpg` |
| Folder `R` | `R/sub/deep/b.heic` | `R/compressed/sub/deep/b.jpg` |

- The relative directory structure under a folder root is **mirrored** inside its output dir.
- Multiple file inputs from different directories each go to their own `<parent>/compressed/`.
- If a file input is also contained in a selected folder input, it is processed **once**, using the folder's root (deduplicate by canonical path).
- Output directories are created on demand. Failure to create one fails all files under that root (reported per file) but does not abort other roots.

### 3.5 Name collisions and conflicts

**Collisions within a plan** (two sources map to the same destination, e.g. `photo.heic` → `photo.jpg` next to a real `photo.jpg`, or `a.png` and `a.jpg` both converting to WebP):
- Resolved deterministically at plan time. Files whose format does **not** change claim their natural name first; then converted files, in sorted source-path order. The first claimant keeps `a.webp`; later ones become `a-png.webp`, `a-jpg.webp` (suffix is the source extension, lowercased). If that is also taken, append `-2`, `-3`, … (`a-png-2.webp`).
- Example (default settings): `photo.jpg` → `photo.jpg`, `photo.heic` → `photo-heic.jpg`, `photo.png` → `photo.png`, `photo.bmp` → `photo-bmp.png`.
- Collision checks are case-insensitive (macOS and Windows filesystems are case-insensitive by default).

**Conflicts with existing files** on disk, controlled by `on_conflict`:
- `overwrite` (default): replace. Re-running a job regenerates output.
- `skip`: leave the existing file and report `Skipped { reason: exists }`.
- `rename`: write `name-1.ext`, `name-2.ext`, ….

### 3.6 Compression behavior

**Quality** is an integer 1–100. Presets:

| Preset | Quality |
|---|---|
| Light | 85 |
| **Balanced (default)** | **70** |
| Aggressive | 50 |
| Custom | 1–100 slider |

**Per-encoder mapping:**

| Output | Encoder | Mapping |
|---|---|---|
| JPEG | `mozjpeg` | `quality = Q`; progressive; optimized Huffman; chroma subsampling 4:2:0 when `Q < 90`, else 4:4:4. |
| PNG | `imagequant` + `oxipng` | `Q = 100` → lossless optimization only (oxipng). `Q < 100` → quantize with quality range `min = max(0, Q − 20)`, `target = Q`; if quantization cannot reach `min`, fall back to lossless. Always finish with oxipng (preset 2, strip per metadata policy). |
| WebP | `libwebp` | Lossy, `quality = Q`. If `Q = 100` **and** source is lossless (PNG/BMP/TIFF) → lossless WebP. |
| AVIF | `ravif` | `quality = Q`, `alpha_quality = Q`, speed 6 (configurable 1–10). |
| TIFF | `image` | Lossless deflate; `Q` ignored. |

**Never-larger rule:**
- If the output format **equals** the input format, the image was **not resized**, and the encoded result is **≥ the original size**, copy the original bytes instead and report `KeptOriginal`. (A resized image is always kept: the user explicitly asked for smaller dimensions.)
- If the format **changed** (explicit conversion or BMP/HEIC default mapping), keep the converted output even if larger, and report it with a `larger_than_source` warning.

### 3.7 Resize (optional)

- `max_width` and/or `max_height` in pixels; unset = no resize.
- Aspect ratio preserved; **never upscale**.
- Filter: Lanczos3 (`fast_image_resize`).
- Applied after orientation correction, before encoding.

### 3.8 Metadata

- Default `keep_metadata = false`:
  - EXIF orientation is **applied to the pixels first**, then all EXIF/XMP/IPTC is stripped (so images never appear rotated wrongly).
  - The **ICC color profile is preserved** wherever the output encoder supports embedding it (to avoid color shifts). Encoders that cannot embed ICC must be listed in §9 Known limitations.
- `keep_metadata = true`: copy EXIF/ICC/XMP where the output format supports it; orientation tag is reset to 1 if pixels were rotated.
- HEIC: `libheif` already applies `irot`/`imir` transforms on decode. Do **not** apply EXIF orientation a second time.

### 3.9 Bit depth and color

- Lossy encoders receive 8-bit RGB(A). High-bit-depth sources (16-bit PNG/TIFF) are down-converted for lossy outputs.
- TIFF→TIFF and lossless PNG keep the source bit depth where possible.
- Grayscale is preserved where the encoder supports it.

### 3.10 Safety guarantees (hard requirements)

1. **Originals are never modified, moved, renamed, or deleted.**
2. **Atomic writes:** encode to a temporary file in the destination directory (`.<name>.<random>.tmp`), `fsync`, then rename over the destination. A crash never leaves a half-written output under the final name.
3. Temp files are removed on failure and on cancellation.
4. The app never writes outside `<root>/<out_dir_name>/` except its own config file.

### 3.11 Per-file outcomes

Every planned file ends in exactly one state:

| Status | Meaning | Counts as error |
|---|---|---|
| `Compressed { before, after }` | Written successfully. | No |
| `KeptOriginal { size }` | Re-encode was not smaller; original copied. | No |
| `Skipped { reason }` | `unsupported`, `animated`, `multi_page`, `exists`, `cancelled`. | No |
| `Failed { error }` | Decode/encode/IO error. | **Yes** |

A per-file failure **never aborts the job**. The job summary reports totals: file counts per status, bytes before/after, percentage saved, elapsed time.

### 3.12 Cancellation

- Jobs are cancellable from GUI, quick mode, and CLI (Ctrl+C).
- Files already completed remain; in-flight files are discarded (temp files deleted); the rest become `Skipped { reason: cancelled }`.

---

## 4. User Interfaces

### 4.1 CLI (`compress-images`)

```
compress-images [OPTIONS] <PATH>...

Options:
  -q, --quality <1-100>          Compression quality [default: 70]
  -p, --preset <PRESET>          light | balanced | aggressive (overrides --quality)
  -f, --format <FORMAT>          same | jpg | png | webp | avif [default: same]
      --no-recursive             Do not descend into subfolders
      --max-width <PX>           Downscale to at most this width
      --max-height <PX>          Downscale to at most this height
      --keep-metadata            Keep EXIF/XMP metadata
  -o, --out-dir-name <NAME>      Output folder name [default: compressed]
      --on-conflict <POLICY>     overwrite | skip | rename [default: overwrite]
  -j, --jobs <N>                 Worker threads [default: number of CPUs]
      --avif-speed <1-10>        AVIF encoder speed [default: 6]
  -n, --dry-run                  Print the plan without writing anything
      --json                     Machine-readable output (one JSON event per line)
  -v, --verbose                  Verbose logging
  -h, --help / -V, --version
```

**Exit codes:** `0` all files succeeded (no `Failed`) · `1` at least one `Failed`, or some inputs could not be read · `2` usage/argument error · `3` fatal (e.g. none of the inputs exist) · `130` cancelled with Ctrl+C.

Human output lists each processed file with sizes; unsupported (non-image) files are listed only with `--verbose`. `--json` prints one event object per line (`started`, `file_started`, `file_done`, `finished`, plus `input_error`; `planned` in dry-run mode).

### 4.2 Desktop GUI (Tauri)

- Drag & drop files/folders onto the window, or use "Add files" / "Add folder" pickers.
- Settings panel: preset + quality slider, output format, recursive toggle, resize, keep metadata, output folder name, conflict policy.
- Progress: overall bar, per-file list with status and size savings, cancel button.
- Result summary: `N files · 48.3 MB → 12.1 MB (−75 %)` with an **Open output folder** button.
- Settings tab "Integrations": toggles to install/uninstall the context-menu integration for the current OS (see §5). *(v0.3)*
- UI languages: English and Indonesian; "System" (default) picks Indonesian when the OS language starts with `id`.
- Settings are saved automatically (debounced) as they change; invalid values (e.g. an output folder name with a slash) are flagged inline and not saved.
- Only one job runs at a time across all windows of the app.

### 4.3 Quick mode (context-menu launch)

- Triggered when the app binary receives paths as arguments: `<app> --compress <PATH>...`.
- Uses the **last saved settings** from the GUI (no prompts).
- Shows a small always-on-top window (label `quick`): progress bar, current file, cancel button; on finish shows the summary + "Open folder" / "Open app" and auto-closes after 10 s unless the user interacts with it (any click or key press cancels the countdown).
- If the main window is already running a job, the quick window waits and starts as soon as it is free.
- Batches that arrive while a quick job runs are processed right after it, in the same window.
- **Single-instance:** when the file manager launches the app once per selected item (Windows does this), additional instances forward their paths to the first instance, which merges arrivals within a 500 ms debounce window (reset by every arrival) into **one job**.
  - Windows / Linux: `tauri-plugin-single-instance` (named mutex / D-Bus name, both atomic).
  - macOS: own implementation (`app/src-tauri/src/mac_single_instance.rs`). The plugin's connect-then-bind check races when processes start at the same moment (observed: two primaries → two separate jobs). An exclusive file lock (`File::try_lock`) picks the primary atomically; secondaries retry connecting to its Unix socket for up to 5 s. Lock and socket live in the per-user temp dir.

### 4.4 Settings persistence

- Stored as TOML in Tauri's app config dir (`app.path().app_config_dir()`), bundle id `io.github.alpandi15.compress-images`:
  - macOS: `~/Library/Application Support/<bundle-id>/config.toml`
  - Windows: `%APPDATA%\<bundle-id>\config.toml`
  - Linux: `$XDG_CONFIG_HOME/<bundle-id>/config.toml` (usually `~/.config/…`)
- Contents: `[options]` (the core `Options`), `preset` (`light` | `balanced` | `aggressive` | `custom`), `language` (`auto` | `en` | `id`). `custom` is stored explicitly because TOML cannot represent "no preset".
- Unknown keys ignored; missing keys take defaults; each invalid value falls back to its default individually with a logged warning (one bad entry never resets the rest). If a named preset contradicts the stored quality (hand-edited file), the quality wins and the preset becomes `custom`.
- Writes are atomic (temp file + rename).

---

## 5. OS Integration (context menu)

Integrations are installed **per user** (no admin/root needed) and are managed from the GUI ("Right-click menu" card in Settings: status, add/reinstall, remove). Uninstall removes everything install created. The menu label is the product name, **"Compress Images"**, on every platform (no translation needed). Implementation: `app/src-tauri/src/integrations/`.

Each platform module has a pure generator (unit-tested on all platforms) and an installer whose target location is injected, so tests never touch the developer's real registry or profile. Status reports `installed`, and `outdated` when the entry points at a different copy of the app (moved/reinstalled); "Reinstall" fixes that.

The launched program is the running executable, except for an AppImage, where it is `$APPIMAGE` (the executable lives in a temporary mount that changes every run).

### 5.1 Windows

- Registry under `HKCU\Software\Classes` (per user):
  - For each supported extension (`jpg jpeg jfif png webp avif tif tiff bmp heic heif`): `SystemFileAssociations\.<ext>\shell\CompressImages` with `MUIVerb = "Compress Images"`, `Icon = "<exe>",0`, `MultiSelectModel = "Player"` (otherwise Explorer hides the verb when > 15 files are selected), and `command\(Default) = "<exe>" --compress "%1"`.
  - Folders: `Directory\shell\CompressImages` → `"<exe>" --compress "%1"`.
  - Folder background (right-click empty space inside a folder): `Directory\Background\shell\CompressImages` → `"<exe>" --compress "%V"`.
- Explorer starts one process per selected item; single-instance merging (§4.3) turns them into one job.
- **NSIS installer** (`installMode: currentUser`) registers the entries on install and deletes them on uninstall via `app/src-tauri/windows/hooks.nsh`. That file is generated from the same Rust code (`nsis_hooks()`), and a test fails if it drifts. Reinstalling/updating re-adds the menu even if the user removed it.
- The `.msi` package has no such hook: users of the MSI add the menu from the GUI, and should remove it from the GUI before uninstalling. (Decide in v0.4 whether to ship the MSI at all.)
- Windows 11: these entries appear under **"Show more options"**. A top-level Windows 11 entry (`IExplorerCommand` + sparse package) is planned for v1.0.

### 5.2 macOS

- A Finder **Quick Action** `~/Library/Services/Compress Images.workflow` (Automator "Run Shell Script", input passed as arguments, Finder only, for `public.image` and `public.folder`).
- Script: `open -n '<path>.app' --args --compress "$@"`. `open` returns immediately, so there is no spinning gear in the menu bar. The new process forwards to a running instance (§4.3). Unbundled dev builds run the binary directly in the background.
- After install/uninstall the services cache is refreshed with `/System/Library/CoreServices/pbs -flush`.
- Usage: right-click → **Quick Actions** → Compress Images.

### 5.3 Linux

All files go under `$XDG_DATA_HOME` (default `~/.local/share`):

| File manager | Mechanism | Location |
|---|---|---|
| Nautilus (GNOME Files) | Python extension (top-level item), **if** the `nautilus-python` plugin is installed (`…/nautilus/extensions-*/libnautilus-python.so` found); otherwise a Nautilus script (under "Scripts") | `nautilus-python/extensions/compress_images.py` · `nautilus/scripts/Compress Images` |
| Dolphin (KDE) | Service menu (executable) | `kio/servicemenus/compress-images.desktop` |
| Nemo (Cinnamon) | Nemo action | `nemo/actions/compress-images.nemo_action` |

- Dolphin and Nemo files are always written (they do nothing if that file manager isn't installed).
- The Nautilus script reads `NAUTILUS_SCRIPT_SELECTED_FILE_PATHS` (one path per line), falling back to its arguments, then to the current folder.
- The extension supports the Nautilus 3 and 4 APIs. Nautilus must be restarted (`nautilus -q`) to load it.

---

## 6. Architecture

### 6.1 Repository layout

```
compress-images/
├── Cargo.toml                 # workspace
├── crates/
│   ├── core/                  # crate `compress-core`: all logic, no UI, no CLI parsing
│   │   └── src/
│   │       ├── lib.rs         # public API
│   │       ├── options.rs     # Options, presets, validation
│   │       ├── detect.rs      # magic-byte sniffing → ImageKind
│   │       ├── scan.rs        # walk inputs, exclusions
│   │       ├── plan.rs        # PURE: sources → destinations, collisions, dedup
│   │       ├── decode/        # raster (image crate), heic (libheif), imageio (macOS), heif_exif
│   │       ├── encode/        # one module per output format
│   │       ├── transform.rs   # orientation, resize, alpha flatten, bit depth
│   │       ├── execute.rs     # parallel runner, atomic writes, cancellation
│   │       ├── event.rs       # progress events
│   │       └── error.rs
│   ├── cli/                   # crate `compress-images` (binary)
│   └── image-avif-native/     # empty helper crate: enables `image`'s dav1d AVIF decoder off macOS (§6.5)
├── app/                       # Tauri v2 desktop app: Svelte 5 + TS frontend (src/, package.json)
│   └── src-tauri/             # Rust side: commands, settings, job runner, quick mode, single-instance
│       ├── src/integrations/  # context menu per OS (§5)
│       └── windows/hooks.nsh  # generated NSIS hooks (registry entries)
├── tests/fixtures/            # small, license-clean sample images
├── scripts/                   # maintenance scripts (third-party licenses)
├── about.toml, about.hbs      # cargo-about config/template for THIRD_PARTY_LICENSES.html
├── docs/
│   ├── SPEC.md                # this file
│   └── adr/                   # architecture decision records (NNNN-title.md)
├── .github/workflows/         # CI + release
└── .github/scripts/           # CI helpers (vcpkg setup on Windows)
```

### 6.2 Pipeline

```
inputs ──► scan ──► detect ──► plan ──► execute (parallel) ──► summary
                                 │            │
                       pure, unit-tested   emits Event stream
                       (no I/O on images)  (Started, FileStarted, FileDone, Finished)
```

- **`plan` is pure**: given a list of discovered files (path + detected kind + root) and `Options`, it returns the `Plan`. It must be testable without touching the filesystem (existing-file checks for `on_conflict` are injected).
- **`execute`** runs the plan on a `rayon` thread pool sized by `jobs`, checks the cancel flag between files, and reports through a callback.

### 6.3 Core public API (shape, not final signatures)

```rust
pub struct Options {
    pub quality: Quality,              // 1..=100
    pub output_format: OutputFormat,   // Same | Jpeg | Png | Webp | Avif
    pub recursive: bool,
    pub resize: Option<Resize>,        // max_width / max_height
    pub keep_metadata: bool,
    pub out_dir_name: String,
    pub on_conflict: ConflictPolicy,   // Overwrite | Skip | Rename
    pub jobs: Option<usize>,
    pub avif_speed: u8,
}

pub fn scan(inputs: &[PathBuf], opts: &Options) -> Result<Vec<Discovered>, CoreError>;
pub fn plan(found: Vec<Discovered>, opts: &Options, exists: &dyn Fn(&Path) -> bool) -> Plan;
pub fn execute(
    plan: &Plan,
    opts: &Options,
    cancel: &AtomicBool,
    on_event: &(dyn Fn(Event) + Sync),
) -> Summary;
```

The CLI and the Tauri app are thin adapters over this API. **The core must not depend on `clap`, `tauri`, or any UI crate, and must not print to stdout/stderr** (use `tracing` + events).

### 6.4 Key dependencies

| Purpose | Crate | Native dependency |
|---|---|---|
| JPEG encode | `mozjpeg` | bundled (built from source; `nasm` for x86 SIMD) |
| PNG quantize | `imagequant` | — |
| PNG optimize | `oxipng` | — |
| WebP encode/decode | `webp` / `libwebp-sys` | bundled |
| AVIF encode | `ravif` | — (`nasm` for x86 asm) |
| Decode (PNG/JPEG/TIFF/BMP/WebP, AVIF via feature off macOS) | `image` | `dav1d` for `avif-decode` (not on macOS) |
| HEIC decode (Windows, Linux) | `libheif-rs` | `libheif` + `libde265` (feature `heic`) |
| HEIC + AVIF decode (macOS) | `objc2-image-io`, `objc2-core-graphics`, `objc2-core-foundation` | — (system ImageIO framework) |
| Resize | `fast_image_resize` | — |
| Parallelism | `rayon` | — |
| Directory walk | `walkdir` | — |
| Magic bytes | `infer` (or in-house sniffing) | — |
| CLI | `clap` (derive) | — |
| Errors | `thiserror` (core), `anyhow` (bins) | — |
| Logging | `tracing`, `tracing-subscriber` | — |
| Config | `serde`, `toml`, `directories` | — |
| Desktop | `tauri` v2, `tauri-plugin-single-instance`, `tauri-plugin-dialog`, `tauri-plugin-opener` | — |

### 6.5 Cargo features (crate `compress-core`)

| Feature | Default | Enables |
|---|---|---|
| `heic` | on | HEIC/HEIF decoding: ImageIO on macOS, `libheif` elsewhere |
| `avif-decode` | on | AVIF decoding: ImageIO on macOS, `dav1d` (through `image`) elsewhere |
| `heic-embedded` | off | `heic`, with libheif compiled from source and its decoders built in instead of loaded as plugins. Used for Linux release builds; no effect on macOS. |
| `imageio` | (internal) | The macOS ImageIO backend, switched on by `heic`/`avif-decode` |

Builds must succeed with `--no-default-features`; the disabled formats are then reported as `unsupported`. The CLI and app crates forward `heic`, `avif-decode` and `heic-embedded`.

Using the OS decoder on macOS (D-13) means the macOS build links no codec libraries at all, which is what makes an unsigned universal `.dmg` practical. Cargo can't enable a dependency feature for one target only, so `avif-native` is switched on through the empty `image-avif-native` crate, which compress-core depends on only off macOS.

ImageIO hands back pixels without applying `irot`/`imir`; the backend applies the reported orientation itself. Exif is read from the container by `decode/heif_exif.rs`, which the AVIF path on other platforms uses as well.

---

## 7. Build, Distribution & CI

### 7.1 Local prerequisites (macOS)

- Xcode / Command Line Tools, Homebrew
- Rust via **rustup** (not Homebrew), plus target `x86_64-apple-darwin` for universal builds
- `brew install nasm` (HEIC/AVIF use the system ImageIO framework; `libheif` from Homebrew is only needed for `heif-enc` when regenerating fixtures)
- Windows/Linux minimum native versions: `libheif` ≥ 1.17 (API pinned via `libheif-rs` feature `v1_17`), `dav1d` ≥ 1.3.
- MSRV: Rust 1.88 (required by `image` and `oxipng`).
- Node.js LTS + pnpm (frontend), `cargo install tauri-cli --version "^2" --locked`

### 7.2 Artifacts

| OS | Artifacts |
|---|---|
| macOS | `.dmg` (universal: arm64 + x86_64), macOS 10.15+, ad-hoc signed |
| Windows | NSIS `.exe` installer (per-user) + `.msi` |
| Linux | `.deb` (Ubuntu 24.04+) + `.AppImage` |
| All | standalone CLI binary in a `.tar.gz` / `.zip` |

### 7.3 CI (GitHub Actions)

- **`ci.yml`** (every push, PRs): matrix `macos-latest`, `windows-latest`, `ubuntu-24.04` → frontend check/build, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace`, a `--no-default-features` run, and on Linux a `heic-embedded` test run. Ubuntu 24.04 is the oldest release whose `libheif`/`dav1d` packages meet the minimum versions. An MSRV job checks core + CLI on 1.88 and the app on 1.90.
- **`release.yml`** (tag `v*`): checks that the tag matches the workspace version (a `-suffix` marks a pre-release) and that `THIRD_PARTY_LICENSES.html` is current, creates a **draft** GitHub Release, then builds on each OS with `pnpm tauri build` and uploads the artifacts in §7.2 plus CLI archives. Each job fails if a binary depends on a codec library it doesn't ship (Homebrew paths on macOS, codec DLLs on Windows, the system `libheif` on Linux).
- Native codec libraries per platform:
  - **macOS:** none (ImageIO). Only `nasm` is installed.
  - **Windows:** `libheif` and `dav1d` from the runner's `vcpkg`, triplet `x64-windows-static-md`, linked statically (`.github/scripts/vcpkg-windows.ps1`, cached between runs). vcpkg's `libheif` pulls in `libde265` and `x265`.
  - **Linux:** `dav1d` and `libde265` from `apt`, dynamically linked (`.deb` depends on `libdav1d7` and `libde265-0`; the AppImage bundles them). `libheif` is compiled in (`heic-embedded`), because Ubuntu's `libheif` loads its HEVC decoder as a plugin that an AppImage can't carry. Building on 24.04 means the AppImage needs glibc 2.39+.

### 7.4 Unsigned distribution

No paid signing before v1.0. The macOS app is ad-hoc signed (`signingIdentity: "-"`), which Apple Silicon requires to launch it at all. The README must document:
- macOS: `xattr -dr com.apple.quarantine "/Applications/Compress Images.app"` or right-click → Open.
- Windows: SmartScreen → "More info" → "Run anyway".

### 7.5 Licensing

- Project license: **GPL-3.0-only** (decision D-6). Chosen so lossy PNG quantization via `imagequant` (GPL-3.0) can be used.
- Third-party: `imagequant` (GPL-3.0), `libheif` (LGPL-3.0, dynamically linked), `dav1d` (BSD-2), `mozjpeg` (BSD/IJG), `libwebp` (BSD). All are compatible with GPL-3.0.
- `THIRD_PARTY_LICENSES.html` is generated with `cargo-about` (`scripts/third-party-licenses.sh`, config in `about.toml`, which also lists the accepted licenses). It is committed, attached to every release and included in the CLI archives; the release workflow fails if it is stale. Native C libraries are listed by hand in `about.hbs`.
- The Windows build statically links vcpkg's `x265` (GPL-2.0-or-later, compatible with GPL-3.0) as part of `libheif`, although only decoding is used.

---

## 8. Roadmap & Acceptance Criteria

### v0.1 — Core + CLI
- [x] Workspace with `compress-core` and `compress-images` CLI.
- [x] Scan, detect, plan, execute exactly per §3.
- [x] All formats in §3.1 (HEIC/AVIF behind features).
- [x] Unit tests for `plan` (collisions, dedup, exclusions, mirroring); integration tests for CLI on fixtures.
- [ ] CI green on all three OSes. *(Workflow written; verified locally on macOS only. Needs a GitHub push to run.)*

**Done when:** `compress-images -q 70 -f same ./fixtures` produces correctly placed, decodable, smaller-or-kept outputs, and re-running is idempotent.

### v0.2 — Desktop GUI
- [x] Tauri v2 app using the core; GUI per §4.2; settings persistence per §4.4.
- [x] Quick mode + single-instance merging per §4.3. *(End-to-end verified on macOS: simultaneous launches merge into one job, the window auto-closes and the process exits. Windows/Linux rely on the plugin and are unverified until CI and a manual check.)*
- [x] Closing a window while its job runs cancels the job cleanly first (no half-written temp files).

### v0.3 — Context-menu integration
- [x] Windows registry install/uninstall from the app + NSIS hooks; macOS Quick Action; Linux Nautilus/Dolphin/Nemo per §5. *(Verified on macOS: generated plists lint clean and the workflow runs via `automator`, launching quick mode with every selected item as its own argument. The Finder menu was confirmed by hand (right-click → Quick Actions → Compress Images). Windows/Linux file managers still need a manual check.)*

### v0.4 — Packaging & polish
- [x] Release workflow producing all artifacts in §7.2. *(macOS: universal build and Homebrew-free linkage verified locally. Windows/Linux: verified only through the release workflow on GitHub.)*
- [x] macOS build decodes HEIC/AVIF with ImageIO, so it ships no codec libraries (D-13); Windows links libheif/dav1d statically, Linux compiles libheif in (D-14).
- [ ] README screenshots. *(Install and unsigned-app instructions are done.)*
- [x] `THIRD_PARTY_LICENSES.html` (cargo-about).
- [x] Indonesian UI translation. *(Shipped with v0.2.)*

### v1.0
- [ ] Windows 11 top-level context menu (`IExplorerCommand` + sparse package).
- [ ] Auto-update (`tauri-plugin-updater`).
- [ ] Revisit code signing.

---

## 9. Known Limitations

- Animated WebP/APNG, multi-page TIFF: skipped.
- HEIC cannot be an output format.
- macOS: AVIF input needs macOS 13 or newer (ImageIO); on older versions those files fail with a decode error.
- AVIF `irot`/`imir` transforms are applied on macOS (ImageIO) but not by the `image` crate's AVIF decoder on Windows/Linux. HEIC transforms are applied everywhere.
- macOS: HEIC/AVIF images with transparency go through premultiplied alpha, so nearly transparent pixels can lose a little color precision.
- Linux AppImage: needs glibc 2.39+ (built on Ubuntu 24.04).
- Windows 11: context menu under "Show more options" until v1.0.
- `KeptOriginal` copies the source bytes as-is, so its metadata (including GPS) is kept even when `keep_metadata` is off.
- Metadata support per output encoder:

  | Output | ICC profile (always kept) | Exif with `keep_metadata` |
  |---|---|---|
  | JPEG (mozjpeg) | ✅ | ✅ (if ≤ 64 KB) |
  | PNG (imagequant/oxipng) | ✅ | ❌ |
  | TIFF | ✅ | ❌ |
  | WebP (`webp` crate) | ❌ | ❌ |
  | AVIF (ravif) | ❌ | ✅ |

  XMP and IPTC are never carried over yet.

---

## 10. Decision Log

| ID | Decision | Status |
|---|---|---|
| D-1 | Rust core + Tauri v2 desktop shell | Accepted |
| D-2 | Output in `<root>/compressed/`, mirrored structure, recursive by default | Accepted |
| D-3 | GIF/SVG/ICO out of scope | Accepted |
| D-4 | HEIC decode-only, default output JPEG; BMP default output PNG | Accepted |
| D-5 | Distribute unsigned via GitHub Releases until v1.0 | Accepted |
| D-6 | Project licensed GPL-3.0-only (allows `imagequant` for lossy PNG) | Accepted (2026-11-12) |
| D-7 | Frontend: Svelte 5 + TypeScript + Vite | Accepted (2026-11-12) |
| D-9 | Same-format files claim output names before converted files (collision priority) | Accepted (2026-11-12) |
| D-10 | Never-larger rule does not apply to resized images | Accepted (2026-11-12) |
| D-8 | Bundle identifier `io.github.alpandi15.compress-images` | Accepted (2026-11-12) |
| D-11 | Own single-instance implementation on macOS (plugin races on simultaneous launches) | Accepted (2026-11-12) |
| D-12 | NSIS installer registers the Windows context menu automatically; other platforms opt in from the GUI | Accepted (2026-11-12) |
| D-13 | macOS decodes HEIC/AVIF with the system ImageIO framework instead of bundling libheif/dav1d | Accepted (2026-11-17) |
| D-14 | Release codecs: Windows links libheif/dav1d statically (vcpkg); Linux compiles libheif in (`heic-embedded`) and links distro dav1d/libde265 | Accepted (2026-11-17) |

Larger decisions get an ADR in `docs/adr/NNNN-short-title.md` (Context · Decision · Consequences).
