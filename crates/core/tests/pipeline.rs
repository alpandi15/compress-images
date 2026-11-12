//! End-to-end tests: scan → plan → execute on a temp copy of `tests/fixtures`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use compress_core::{
    execute, plan, scan, Event, Options, Outcome, OutputFormat, Resize, SkipReason, Summary,
};
use image::GenericImageView;
use tempfile::TempDir;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dst = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dst);
        } else {
            fs::copy(e.path(), dst).unwrap();
        }
    }
}

fn workspace() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixtures(), dir.path());
    dir
}

/// Every file under `root` (relative path → bytes), optionally skipping the output folder.
fn snapshot(root: &Path, include_output: bool) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    for e in walkdir::WalkDir::new(root) {
        let e = e.unwrap();
        let rel = e.path().strip_prefix(root).unwrap().to_path_buf();
        if !include_output && rel.starts_with("compressed") {
            continue;
        }
        if e.file_type().is_file() {
            out.insert(rel, fs::read(e.path()).unwrap());
        }
    }
    out
}

struct Run {
    summary: Summary,
    /// source file name (relative to root) → (destination relative to root, outcome)
    done: BTreeMap<String, (Option<String>, Outcome)>,
}

fn run(root: &Path, inputs: &[PathBuf], opts: &Options) -> Run {
    run_with_cancel(root, inputs, opts, false)
}

fn run_with_cancel(root: &Path, inputs: &[PathBuf], opts: &Options, cancelled: bool) -> Run {
    opts.validate().unwrap();
    let report = scan(inputs, opts);
    assert!(report.errors.is_empty(), "scan errors: {:?}", report.errors);
    let plan = plan(report.files, opts, &|p| p.exists());
    let root = dunce::canonicalize(root).unwrap();
    let rel = |p: &Path| {
        p.strip_prefix(&root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let done = Mutex::new(BTreeMap::new());
    let summary = execute(&plan, opts, &AtomicBool::new(cancelled), &|e| {
        if let Event::FileDone {
            source,
            destination,
            outcome,
            ..
        } = e
        {
            done.lock()
                .unwrap()
                .insert(rel(&source), (destination.as_deref().map(rel), outcome));
        }
    });
    Run {
        summary,
        done: done.into_inner().unwrap(),
    }
}

fn dest_of(r: &Run, src: &str) -> String {
    match &r.done[src] {
        (Some(d), _) => d.clone(),
        (None, o) => panic!("{src} has no destination: {o:?}"),
    }
}

fn assert_produced(r: &Run, src: &str) {
    match &r.done[src].1 {
        Outcome::Compressed { .. } | Outcome::KeptOriginal { .. } => {}
        other => panic!("{src}: expected output, got {other:?}"),
    }
}

#[test]
fn compresses_whole_folder_and_never_touches_originals() {
    let ws = workspace();
    let root = ws.path();
    let before = snapshot(root, false);

    let r = run(root, &[root.to_path_buf()], &Options::default());

    assert_eq!(
        snapshot(root, false),
        before,
        "originals must be byte-identical"
    );
    assert_eq!(r.summary.failed, 0, "{:#?}", r.done);

    let expected = [
        ("alpha.png", "compressed/alpha.png"),
        ("fake-png.jpg", "compressed/fake-png.png"),
        ("gray.png", "compressed/gray.png"),
        ("photo.bmp", "compressed/photo-bmp.png"),
        ("photo.jpg", "compressed/photo.jpg"),
        ("photo.png", "compressed/photo.png"),
        ("photo.tiff", "compressed/photo.tiff"),
        ("photo.webp", "compressed/photo.webp"),
        ("rotated.jpg", "compressed/rotated.jpg"),
        ("sub/deep/nested.jpg", "compressed/sub/deep/nested.jpg"),
    ];
    for (src, dst) in expected {
        assert_produced(&r, src);
        assert_eq!(dest_of(&r, src), dst, "{src}");
        let img = image::open(root.join(dst)).unwrap_or_else(|e| panic!("{dst}: {e}"));
        assert!(img.width() > 0);
    }
    #[cfg(feature = "heic")]
    assert_eq!(dest_of(&r, "photo.heic"), "compressed/photo-heic.jpg");
    #[cfg(feature = "avif-decode")]
    {
        assert_produced(&r, "photo.avif");
        image::open(root.join("compressed/photo.avif")).unwrap();
    }

    let skipped = |src: &str| match &r.done[src].1 {
        Outcome::Skipped { reason } => *reason,
        o => panic!("{src}: expected skip, got {o:?}"),
    };
    assert_eq!(skipped("notes.txt"), SkipReason::Unsupported);
    assert_eq!(skipped("animated.png"), SkipReason::Animated);
    assert_eq!(skipped("animated.webp"), SkipReason::Animated);

    // No temp files left behind anywhere.
    let leftovers: Vec<_> = snapshot(root, true)
        .into_keys()
        .filter(|p| p.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn outputs_are_smaller_for_high_quality_sources() {
    let ws = workspace();
    let root = ws.path();
    let r = run(root, &[root.to_path_buf()], &Options::default());
    for src in [
        "photo.jpg",
        "photo.png",
        "photo.tiff",
        "photo.bmp",
        "alpha.png",
    ] {
        match &r.done[src].1 {
            Outcome::Compressed { before, after, .. } => {
                assert!(after < before, "{src}: {after} >= {before}")
            }
            o => panic!("{src}: {o:?}"),
        }
    }
    assert!(r.summary.saved_percent() > 0.0);
}

#[test]
fn rerun_is_idempotent_and_ignores_output_folder() {
    let ws = workspace();
    let root = ws.path();
    let opts = Options::default();
    let first = run(root, &[root.to_path_buf()], &opts);
    let outputs_1: Vec<_> = snapshot(root, true).into_keys().collect();
    let second = run(root, &[root.to_path_buf()], &opts);
    let outputs_2: Vec<_> = snapshot(root, true).into_keys().collect();

    assert_eq!(outputs_1, outputs_2);
    assert_eq!(first.done.len(), second.done.len());
    assert!(!root.join("compressed/compressed").exists());
    assert!(second.done.keys().all(|k| !k.starts_with("compressed/")));
}

#[test]
fn exif_orientation_is_applied() {
    let ws = workspace();
    let root = ws.path();
    run(root, &[root.join("rotated.jpg")], &Options::default());
    let img = image::open(root.join("compressed/rotated.jpg")).unwrap();
    assert_eq!(img.dimensions(), (100, 200));
}

#[test]
fn keep_metadata_resets_orientation_tag() {
    let ws = workspace();
    let root = ws.path();
    let opts = Options {
        keep_metadata: true,
        ..Options::default()
    };
    run(root, &[root.join("rotated.jpg")], &opts);
    let out = root.join("compressed/rotated.jpg");
    let data = fs::read(&out).unwrap();
    assert!(
        data.windows(6).any(|w| w == b"Exif\0\0"),
        "Exif should be kept"
    );
    // Pixels are already upright and the tag says so: viewers must not rotate again.
    let mut dec = image::ImageReader::open(&out)
        .unwrap()
        .into_decoder()
        .unwrap();
    use image::ImageDecoder;
    assert_eq!(
        dec.orientation().unwrap(),
        image::metadata::Orientation::NoTransforms
    );
    assert_eq!(image::open(&out).unwrap().dimensions(), (100, 200));
}

#[test]
fn metadata_stripped_by_default() {
    let ws = workspace();
    let root = ws.path();
    run(root, &[root.join("rotated.jpg")], &Options::default());
    let data = fs::read(root.join("compressed/rotated.jpg")).unwrap();
    assert!(!data.windows(6).any(|w| w == b"Exif\0\0"));
}

#[test]
fn alpha_flattened_to_white_for_jpeg() {
    let ws = workspace();
    let root = ws.path();
    let opts = Options {
        output_format: OutputFormat::Jpeg,
        ..Options::default()
    };
    let r = run(root, &[root.join("alpha.png")], &opts);
    assert_eq!(dest_of(&r, "alpha.png"), "compressed/alpha.jpg");
    let img = image::open(root.join("compressed/alpha.jpg"))
        .unwrap()
        .to_rgb8();
    let [r, g, b] = img.get_pixel(1, 1).0;
    assert!(r > 240 && g > 240 && b > 240, "corner was {r},{g},{b}");
}

#[test]
fn converts_to_every_output_format() {
    for (fmt, ext) in [
        (OutputFormat::Jpeg, "jpg"),
        (OutputFormat::Png, "png"),
        (OutputFormat::Webp, "webp"),
        (OutputFormat::Avif, "avif"),
    ] {
        let ws = workspace();
        let root = ws.path();
        let opts = Options {
            output_format: fmt,
            avif_speed: 10,
            ..Options::default()
        };
        let r = run(root, &[root.join("photo.tiff")], &opts);
        assert_produced(&r, "photo.tiff");
        let dst = format!("compressed/photo.{ext}");
        assert_eq!(dest_of(&r, "photo.tiff"), dst);
        let data = fs::read(root.join(&dst)).unwrap();
        assert_eq!(
            compress_core::detect::sniff(&data),
            Some(match fmt {
                OutputFormat::Jpeg => compress_core::ImageKind::Jpeg,
                OutputFormat::Png => compress_core::ImageKind::Png,
                OutputFormat::Webp => compress_core::ImageKind::Webp,
                _ => compress_core::ImageKind::Avif,
            })
        );
    }
}

#[test]
fn resize_downscales_and_keeps_aspect() {
    let ws = workspace();
    let root = ws.path();
    let opts = Options {
        resize: Resize {
            max_width: Some(100),
            max_height: None,
        },
        ..Options::default()
    };
    run(root, &[root.join("photo.jpg")], &opts);
    let img = image::open(root.join("compressed/photo.jpg")).unwrap();
    assert_eq!(img.dimensions(), (100, 75));
}

#[test]
fn single_file_input_goes_next_to_file() {
    let ws = workspace();
    let root = ws.path();
    let r = run(
        root,
        &[root.join("sub/deep/nested.jpg")],
        &Options::default(),
    );
    assert_eq!(
        dest_of(&r, "sub/deep/nested.jpg"),
        "sub/deep/compressed/nested.jpg"
    );
    assert!(!root.join("compressed").exists());
}

#[test]
fn hidden_files_and_existing_output_folders_are_excluded() {
    let ws = workspace();
    let root = ws.path();
    fs::copy(root.join("photo.jpg"), root.join(".hidden.jpg")).unwrap();
    fs::create_dir_all(root.join(".cache")).unwrap();
    fs::copy(root.join("photo.jpg"), root.join(".cache/a.jpg")).unwrap();
    fs::create_dir_all(root.join("sub/Compressed")).unwrap();
    fs::copy(root.join("photo.jpg"), root.join("sub/Compressed/old.jpg")).unwrap();

    let r = run(root, &[root.to_path_buf()], &Options::default());
    let sources: Vec<_> = r.done.keys().collect();
    assert!(!sources
        .iter()
        .any(|s| s.contains("hidden") || s.contains(".cache") || s.contains("old.jpg")));
}

#[test]
fn overlapping_inputs_processed_once() {
    let ws = workspace();
    let root = ws.path();
    let inputs = [
        root.to_path_buf(),
        root.join("photo.jpg"),
        root.join("sub"),
        root.join("photo.jpg"),
    ];
    let r = run(root, &inputs, &Options::default());
    let single = run(root, &[root.to_path_buf()], &Options::default());
    assert_eq!(r.summary.total, single.summary.total);
    assert_eq!(
        dest_of(&r, "sub/deep/nested.jpg"),
        "compressed/sub/deep/nested.jpg"
    );
}

#[test]
fn non_recursive_ignores_subfolders() {
    let ws = workspace();
    let root = ws.path();
    let opts = Options {
        recursive: false,
        ..Options::default()
    };
    let r = run(root, &[root.to_path_buf()], &opts);
    assert!(!r.done.contains_key("sub/deep/nested.jpg"));
    assert!(r.done.contains_key("photo.jpg"));
}

#[test]
fn corrupt_file_fails_without_aborting_job() {
    let ws = workspace();
    let root = ws.path();
    let jpg = fs::read(root.join("photo.jpg")).unwrap();
    fs::write(root.join("broken.jpg"), &jpg[..300]).unwrap();

    let r = run(root, &[root.to_path_buf()], &Options::default());
    assert!(matches!(r.done["broken.jpg"].1, Outcome::Failed { .. }));
    assert_eq!(r.summary.failed, 1);
    assert_produced(&r, "photo.jpg");
    assert!(!root.join("compressed/broken.jpg").exists());
}

#[test]
fn cancelled_job_writes_nothing() {
    let ws = workspace();
    let root = ws.path();
    let r = run_with_cancel(root, &[root.to_path_buf()], &Options::default(), true);
    assert!(r.summary.cancelled);
    assert_eq!(r.summary.compressed + r.summary.kept_original, 0);
    assert!(matches!(
        r.done["photo.jpg"].1,
        Outcome::Skipped {
            reason: SkipReason::Cancelled
        }
    ));
    assert!(!root.join("compressed").exists());
}

#[test]
fn rename_policy_never_overwrites() {
    let ws = workspace();
    let root = ws.path();
    let opts = Options {
        on_conflict: compress_core::ConflictPolicy::Rename,
        ..Options::default()
    };
    run(root, &[root.join("photo.jpg")], &opts);
    let r = run(root, &[root.join("photo.jpg")], &opts);
    assert_eq!(dest_of(&r, "photo.jpg"), "compressed/photo-1.jpg");
}

#[cfg(feature = "heic")]
#[test]
fn heic_decodes_to_jpeg() {
    let ws = workspace();
    let root = ws.path();
    let r = run(root, &[root.join("photo.heic")], &Options::default());
    assert_eq!(dest_of(&r, "photo.heic"), "compressed/photo.jpg");
    let img = image::open(root.join("compressed/photo.jpg")).unwrap();
    assert_eq!(img.dimensions(), (200, 150));
}
