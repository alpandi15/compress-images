# AGENTS.md

Operating guide for AI coding agents (and humans) working in this repository.

**Compress Images** is a cross-platform (macOS / Windows / Linux) image compressor written in Rust, with a Tauri v2 desktop app and OS context-menu integration. It compresses single files or whole folder trees into a `compressed/` subfolder, never touching the originals.

> 📘 **Read [docs/SPEC.md](docs/SPEC.md) before changing any behavior.** The spec is the source of truth. If your change alters behavior, update the spec in the same change. If the spec is ambiguous or contradicts itself, stop and ask. Don't guess.

---

## 1. Ground Rules (non-negotiable)

1. **Never modify, move, rename, or delete a user's original files.** The only write location is `<root>/<out_dir_name>/` (plus the app's own config file). This applies to code *and* to anything you run while developing.
2. **Atomic writes only:** temp file in the destination dir → fsync → rename. See SPEC §3.10.
3. **A per-file failure must never abort a job.** Return a `Failed` outcome and continue.
4. **`compress-core` stays UI-agnostic:** no `clap`, `tauri`, or UI crates; no `println!`/`eprintln!`. Report through `Event`s and `tracing`.
5. **`plan` stays pure:** no image decoding and no direct filesystem access. Existence checks are injected. This keeps it fully unit-testable.
6. **Paths are `Path`/`PathBuf`/`OsStr` end to end.** Never `to_str().unwrap()`. Non-UTF-8 paths (Linux) and long paths (Windows > 260 chars) must work.
7. **No telemetry, no network calls** in the app or the CLI.
8. **Development and tests only touch temp directories** (`tempfile::TempDir`). Never point the tool at real user folders (`~/Pictures`, `~/Desktop`, …).
9. **Don't commit personal photos.** Fixtures must be small (< 200 KB each), synthetic or public-domain, and listed in `tests/fixtures/README.md` with their source/license.

---

## 2. Repository Map

```
crates/core/        compress-core — scan, detect, plan, decode, transform, encode, execute
crates/cli/         compress-images — thin clap adapter over compress-core
app/                Desktop app frontend (Svelte 5 + TypeScript + Vite): src/, package.json
app/src-tauri/      Tauri v2 backend: commands, settings, job runner, quick mode, single-instance
integrations/       Context-menu templates per OS (windows/, macos/, linux/)
tests/fixtures/     Sample images for tests
docs/SPEC.md        Product & technical spec (source of truth)
docs/adr/           Architecture Decision Records
.github/workflows/  CI (ci.yml) and releases (release.yml)
```

Some directories don't exist yet. Follow the roadmap in SPEC §8; don't scaffold later milestones early.

---

## 3. Environment Setup (macOS dev machine)

```bash
# Rust must come from rustup, NOT Homebrew
rustup target add x86_64-apple-darwin
brew install nasm libheif dav1d pkg-config cmake
cargo install tauri-cli --version "^2" --locked   # only needed from v0.2
```

Linux (Ubuntu 24.04+; older releases ship a too-old libheif/dav1d): `sudo apt install build-essential nasm pkg-config libheif-dev libdav1d-dev` (+ Tauri's WebKitGTK deps from v0.2).
Windows: MSVC Build Tools, `nasm`, and `vcpkg install libheif dav1d`.

If `libheif`/`dav1d` are unavailable, build with `--no-default-features` (HEIC/AVIF decode disabled).

---

## 4. Commands

| Task | Command |
|---|---|
| Build | `cargo build --workspace` |
| Test (all) | `cargo test --workspace` |
| Test without native codecs | `cargo test --workspace --no-default-features` |
| Single test | `cargo test -p compress-core plan::tests::collision_suffix` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Format | `cargo fmt --all` |
| Run CLI | `cargo run -p compress-images -- -q 70 <path>` |
| Dry run | `cargo run -p compress-images -- --dry-run <path>` |
| Regenerate NSIS hooks | `cargo test -p compress-images-app -- --ignored regenerate_nsis_hooks` (after changing Windows registry entries) |
| Export Quick Action for a manual test | `WORKFLOW_OUT=<dir> WORKFLOW_LAUNCHER=<exe> cargo test -p compress-images-app -- --ignored export_workflow`, then `automator -i <file> "<dir>/Compress Images.workflow"` |
| Regenerate fixtures | `cargo run -p compress-core --example gen_fixtures` (+ `heif-enc`, see `tests/fixtures/README.md`) |
| Frontend deps (once) | `cd app && pnpm install` |
| Frontend type-check | `cd app && pnpm check` |
| Frontend build | `cd app && pnpm build` (**required before `cargo build/test --workspace`**: the Tauri crate embeds `app/dist` at compile time) |
| Desktop dev | `cd app && cargo tauri dev` |
| Desktop bundle | `cd app && cargo tauri build` (`--bundles app` for just the macOS `.app`) |
| Quick mode by hand | `"target/release/bundle/macos/Compress Images.app/Contents/MacOS/compress-images-app" --compress <paths>` |

Always run **fmt → clippy → test** before you declare a task done.

---

## 5. Coding Conventions

### Rust
- Edition 2021, stable toolchain; MSRV is pinned in the workspace `Cargo.toml` (`rust-version`, currently 1.88). Re-check it when bumping dependencies.
- Errors: `thiserror` enums in `compress-core`; `anyhow` only in binaries (`cli`, `app`). No `unwrap()`/`expect()` on fallible runtime paths. They're allowed in tests and for true invariants (with a comment explaining why).
- Decoders live in `decode/` per backend (`raster.rs` = everything the `image` crate decodes, `heic.rs` = libheif); encoders live in `encode/`, one module per output format. Adding a format means: detect → decode → (encode) → plan mapping → tests → SPEC §3.1 update.
- Native codecs (mozjpeg in particular) report errors by panicking. Keep them behind `catch_unwind` (see `encode/jpeg.rs` and `execute.rs`), and never build with `panic = "abort"`.
- Platform-specific code goes behind `#[cfg(target_os = "...")]` and lives in clearly named modules (`integrations/windows.rs`, etc.). Shared logic stays platform-neutral.
- Public items in `compress-core` get doc comments. Explain *why*, not *what*.
- Keep functions small and testable; prefer passing `&Options` over global state.

### Dependencies
- Justify every new crate in the PR/commit description. Prefer pure-Rust crates.
- Any new **native/C dependency** must be added to SPEC §6.4 and §7, and to the setup instructions above.
- Check licenses: the project is **GPL-3.0-only** (SPEC decision D-6). Every dependency must be GPL-3.0-compatible. Flag anything proprietary, GPL-2.0-only, or with an unclear license.

### Frontend (v0.2+)
- Svelte 5 runes + TypeScript strict mode; `pnpm check` must pass with zero warnings.
- All business logic lives in Rust; the UI only calls Tauri commands (`src/lib/api.ts`) and renders events (`src/lib/job.svelte.ts`). Keep the TS types in `api.ts` in sync with the Rust serde output.
- All user-facing strings go through `t()` in `src/lib/i18n.svelte.ts` (English + Indonesian; the `id` dictionary is typed against `en`, so a missing key is a type error).
- npm `@tauri-apps/*` packages must match the Rust crates' major.minor (`cargo tauri build` refuses mismatches). Bump both sides together.
- MSRV: the app crate needs Rust 1.90 (Tauri); core and CLI stay at the workspace MSRV.

---

## 6. Testing Requirements

- **`plan`**: exhaustive unit tests covering output mirroring, `compressed/` exclusion at any depth, hidden/symlink exclusion, dedup of overlapping inputs, conversion collisions (`a.png` + `a.jpg` → WebP), and all `on_conflict` policies.
- **Encoders**: for each format, the output must **decode successfully**, have the expected dimensions/orientation, and follow the never-larger rule (SPEC §3.6).
- **Detection**: wrong-extension fixtures (PNG named `.jpg`) and non-image files.
- **CLI**: integration tests with `assert_cmd` against a `TempDir` copy of the fixtures. Check exit codes (SPEC §4.1) and that **originals are byte-identical** after the run.
- **Idempotency**: running the same job twice gives the same output set and never compresses `compressed/` contents.
- Tests that need `heic`/`avif-decode` are gated with `#[cfg(feature = "...")]`.

---

## 7. Workflow for Agents

1. **Orient:** read this file and the relevant SPEC sections. Check the roadmap (SPEC §8) to see which milestone you're in.
2. **Plan:** for anything bigger than a small fix, state the plan (files touched, approach, tests) before writing code.
3. **Implement** in small, reviewable steps. Keep unrelated refactors out of the change.
4. **Verify:** fmt, clippy, tests. For behavior changes, also run the CLI against a temp copy of the fixtures and inspect the output.
5. **Document:** update SPEC (behavior), README (user-facing), and the decision log / ADR (architectural choices).
6. **Report honestly:** say what was done, what was verified, and what wasn't. Don't claim a platform works if you only tested on macOS. Say "untested on Windows/Linux" and leave it to CI.

### When to stop and ask
- The spec is ambiguous, or the change would contradict it.
- An open decision in SPEC §10 (e.g. D-6 licensing, D-8 bundle id) blocks the work.
- A change would add a native dependency, change the license posture, or touch the safety guarantees in §1.
- Anything involving writing to the registry, `~/Library/Services`, or file-manager config on the **developer's real machine**. Integration code must be testable without installing into the live user profile (inject the target base directory / registry root).

---

## 8. Git & Commits

- **Conventional Commits:** `feat(core): ...`, `fix(cli): ...`, `docs(spec): ...`, `test(plan): ...`, `ci: ...`, `chore: ...`.
- Scopes: `core`, `cli`, `app`, `ui`, `integrations`, `spec`, `ci`, `deps`.
- One logical change per commit; behavior changes include the SPEC update and tests in the same commit.
- Branch from `main`; never force-push `main`. Only commit/push when asked.

---

## 9. Definition of Done

- [ ] Behavior matches docs/SPEC.md (spec updated if behavior changed)
- [ ] `cargo fmt --all` clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo test --workspace` passes (and `--no-default-features` builds)
- [ ] New logic covered by tests; originals verified untouched in integration tests
- [ ] No new `unwrap()` on runtime paths, no `println!` in core
- [ ] New dependencies justified; native deps and licenses documented
- [ ] README / ADR updated where relevant
