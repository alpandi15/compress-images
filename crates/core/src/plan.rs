//! Pure planning step: decides every destination path before any pixels are touched
//! (SPEC §3.4, §3.5). No filesystem access — existence checks are injected.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::detect::ImageKind;
use crate::event::SkipReason;
use crate::options::{ConflictPolicy, Options, OutputFormat};
use crate::scan::Discovered;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Encode,
    Skip(SkipReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanItem {
    pub source: PathBuf,
    pub source_size: u64,
    pub input_kind: Option<ImageKind>,
    /// `Some` whenever the item has a destination (encode, or skipped because it exists).
    pub output_kind: Option<ImageKind>,
    pub destination: Option<PathBuf>,
    pub action: Action,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub items: Vec<PlanItem>,
}

impl Plan {
    pub fn encode_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.action == Action::Encode)
            .count()
    }
}

/// Output format for an input under the given setting (SPEC §3.1 / §3.2).
pub fn output_kind(input: ImageKind, format: OutputFormat) -> ImageKind {
    match format {
        OutputFormat::Same => match input {
            ImageKind::Bmp => ImageKind::Png,
            ImageKind::Heic => ImageKind::Jpeg,
            other => other,
        },
        OutputFormat::Jpeg => ImageKind::Jpeg,
        OutputFormat::Png => ImageKind::Png,
        OutputFormat::Webp => ImageKind::Webp,
        OutputFormat::Avif => ImageKind::Avif,
    }
}

/// Builds the plan. `exists` reports whether a path is already present on disk.
pub fn plan(mut found: Vec<Discovered>, opts: &Options, exists: &dyn Fn(&Path) -> bool) -> Plan {
    // Files that keep their format claim their natural names first, so `photo.jpg` stays
    // `photo.jpg` even when `photo.heic` also converts to JPEG. Ties go by source path.
    let converted = |d: &Discovered| match d.kind {
        Some(k) => output_kind(k, opts.output_format) != k,
        None => false,
    };
    found.sort_by(|a, b| (converted(a), &a.source).cmp(&(converted(b), &b.source)));
    // Keys are lowercased so names differing only by case count as collisions: macOS and
    // Windows filesystems are case-insensitive by default.
    let mut taken: HashSet<String> = HashSet::new();
    let mut items = Vec::with_capacity(found.len());

    for d in found {
        // Out-of-scope files get no destination and don't reserve a name.
        let early_skip = match (d.kind, d.skip) {
            (Some(k), None) if k.is_decodable() => None,
            (Some(k), Some(reason)) if k.is_decodable() => Some(reason),
            _ => Some(SkipReason::Unsupported),
        };
        if let Some(reason) = early_skip {
            items.push(PlanItem {
                source: d.source,
                source_size: d.size,
                input_kind: d.kind,
                output_kind: None,
                destination: None,
                action: Action::Skip(reason),
            });
            continue;
        }
        let Some(kind) = d.kind else { continue };

        let out = output_kind(kind, opts.output_format);
        let dir = output_dir(&d.root, &d.rel, opts);
        let stem = d
            .rel
            .file_stem()
            .map(OsStr::to_os_string)
            .unwrap_or_default();
        let ext = out.extension();

        // 1. Resolve collisions with other files in this plan.
        let mut dest = join_name(&dir, &stem, "", ext);
        if taken.contains(&key(&dest)) {
            let src_ext = d
                .rel
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| kind.extension().to_owned());
            let base = format!("-{src_ext}");
            dest = join_name(&dir, &stem, &base, ext);
            let mut n = 2;
            while taken.contains(&key(&dest)) {
                dest = join_name(&dir, &stem, &format!("{base}-{n}"), ext);
                n += 1;
            }
        }

        // 2. Resolve conflicts with files already on disk.
        let mut action = Action::Encode;
        if exists(&dest) {
            match opts.on_conflict {
                ConflictPolicy::Overwrite => {}
                ConflictPolicy::Skip => action = Action::Skip(SkipReason::Exists),
                ConflictPolicy::Rename => {
                    let base_stem = dest
                        .file_stem()
                        .map(OsStr::to_os_string)
                        .unwrap_or_default();
                    let mut n = 1;
                    loop {
                        let candidate = join_name(&dir, &base_stem, &format!("-{n}"), ext);
                        if !exists(&candidate) && !taken.contains(&key(&candidate)) {
                            dest = candidate;
                            break;
                        }
                        n += 1;
                    }
                }
            }
        }

        taken.insert(key(&dest));
        items.push(PlanItem {
            source: d.source,
            source_size: d.size,
            input_kind: Some(kind),
            output_kind: Some(out),
            destination: Some(dest),
            action,
        });
    }

    items.sort_by(|a, b| a.source.cmp(&b.source));
    Plan { items }
}

/// `<root>/<out_dir_name>/<rel parent>`.
fn output_dir(root: &Path, rel: &Path, opts: &Options) -> PathBuf {
    let mut dir = root.join(&opts.out_dir_name);
    if let Some(parent) = rel.parent() {
        dir.push(parent);
    }
    dir
}

fn join_name(dir: &Path, stem: &OsStr, suffix: &str, ext: &str) -> PathBuf {
    let mut name = OsString::from(stem);
    name.push(suffix);
    name.push(".");
    name.push(ext);
    dir.join(name)
}

fn key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::OutputFormat;

    fn disc(root: &str, rel: &str, kind: Option<ImageKind>) -> Discovered {
        Discovered {
            source: Path::new(root).join(rel),
            root: root.into(),
            rel: rel.into(),
            kind,
            skip: None,
            size: 100,
        }
    }

    fn never(_: &Path) -> bool {
        false
    }

    fn dests(p: &Plan) -> Vec<String> {
        p.items
            .iter()
            .map(|i| match (&i.destination, i.action) {
                (Some(d), Action::Encode) => d.to_string_lossy().replace('\\', "/"),
                (Some(d), Action::Skip(r)) => format!("{} [{r:?}]", d.display()),
                (None, Action::Skip(r)) => format!("- [{r:?}]"),
                (None, Action::Encode) => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn mirrors_structure() {
        let p = plan(
            vec![
                disc("/r", "a.jpg", Some(ImageKind::Jpeg)),
                disc("/r", "sub/deep/b.png", Some(ImageKind::Png)),
            ],
            &Options::default(),
            &never,
        );
        assert_eq!(
            dests(&p),
            ["/r/compressed/a.jpg", "/r/compressed/sub/deep/b.png"]
        );
    }

    #[test]
    fn default_format_mapping() {
        assert_eq!(
            output_kind(ImageKind::Bmp, OutputFormat::Same),
            ImageKind::Png
        );
        assert_eq!(
            output_kind(ImageKind::Heic, OutputFormat::Same),
            ImageKind::Jpeg
        );
        assert_eq!(
            output_kind(ImageKind::Tiff, OutputFormat::Same),
            ImageKind::Tiff
        );
        assert_eq!(
            output_kind(ImageKind::Png, OutputFormat::Webp),
            ImageKind::Webp
        );
    }

    #[test]
    fn extension_follows_content_and_format() {
        let p = plan(
            vec![
                disc("/r", "photo.JPEG", Some(ImageKind::Jpeg)),
                disc("/r", "fake.jpg", Some(ImageKind::Png)),
                disc("/r", "scan.bmp", Some(ImageKind::Bmp)),
            ],
            &Options::default(),
            &never,
        );
        assert_eq!(
            dests(&p),
            [
                "/r/compressed/fake.png",
                "/r/compressed/photo.jpg",
                "/r/compressed/scan.png"
            ]
        );
    }

    #[test]
    fn unsupported_is_skipped_without_destination() {
        let p = plan(
            vec![disc("/r", "notes.txt", None)],
            &Options::default(),
            &never,
        );
        assert_eq!(dests(&p), ["- [Unsupported]"]);
        assert_eq!(p.encode_count(), 0);
    }

    #[test]
    fn conversion_collisions_get_source_extension_suffix() {
        let opts = Options {
            output_format: OutputFormat::Webp,
            ..Options::default()
        };
        let p = plan(
            vec![
                disc("/r", "a.png", Some(ImageKind::Png)),
                disc("/r", "a.jpg", Some(ImageKind::Jpeg)),
                disc("/r", "a.webp", Some(ImageKind::Webp)),
            ],
            &opts,
            &never,
        );
        // a.webp keeps its format, so it claims the name; converted files get suffixes.
        // Items are reported sorted by source: a.jpg, a.png, a.webp.
        assert_eq!(
            dests(&p),
            [
                "/r/compressed/a-jpg.webp",
                "/r/compressed/a-png.webp",
                "/r/compressed/a.webp"
            ]
        );
    }

    #[test]
    fn unconverted_file_keeps_its_name_over_converted_one() {
        let p = plan(
            vec![
                disc("/r", "photo.bmp", Some(ImageKind::Bmp)),
                disc("/r", "photo.png", Some(ImageKind::Png)),
            ],
            &Options::default(),
            &never,
        );
        assert_eq!(
            dests(&p),
            ["/r/compressed/photo-bmp.png", "/r/compressed/photo.png"]
        );
    }

    #[cfg(feature = "heic")]
    #[test]
    fn heic_conversion_does_not_steal_jpeg_name() {
        let p = plan(
            vec![
                disc("/r", "photo.heic", Some(ImageKind::Heic)),
                disc("/r", "photo.jpg", Some(ImageKind::Jpeg)),
            ],
            &Options::default(),
            &never,
        );
        assert_eq!(
            dests(&p),
            ["/r/compressed/photo-heic.jpg", "/r/compressed/photo.jpg"]
        );
    }

    #[cfg(not(feature = "heic"))]
    #[test]
    fn heic_is_unsupported_without_feature() {
        let p = plan(
            vec![disc("/r", "photo.heic", Some(ImageKind::Heic))],
            &Options::default(),
            &never,
        );
        assert_eq!(dests(&p), ["- [Unsupported]"]);
    }

    #[test]
    fn collision_suffix_is_numbered_when_still_taken() {
        let opts = Options {
            output_format: OutputFormat::Jpeg,
            ..Options::default()
        };
        // "a-png.jpg" (a real file) claims the name that "a.png" would fall back to.
        let p = plan(
            vec![
                disc("/r", "a.jpg", Some(ImageKind::Jpeg)),
                disc("/r", "a-png.jpg", Some(ImageKind::Jpeg)),
                disc("/r", "a.png", Some(ImageKind::Png)),
            ],
            &opts,
            &never,
        );
        assert_eq!(
            dests(&p),
            [
                "/r/compressed/a-png.jpg",
                "/r/compressed/a.jpg",
                "/r/compressed/a-png-2.jpg"
            ]
        );
    }

    #[test]
    fn collisions_are_case_insensitive() {
        let p = plan(
            vec![
                disc("/r", "A.jpg", Some(ImageKind::Jpeg)),
                disc("/r", "a.jpeg", Some(ImageKind::Jpeg)),
            ],
            &Options::default(),
            &never,
        );
        assert_eq!(
            dests(&p),
            ["/r/compressed/A.jpg", "/r/compressed/a-jpeg.jpg"]
        );
    }

    #[test]
    fn same_name_in_different_folders_does_not_collide() {
        let p = plan(
            vec![
                disc("/r", "x/a.jpg", Some(ImageKind::Jpeg)),
                disc("/r", "y/a.jpg", Some(ImageKind::Jpeg)),
                disc("/other", "a.jpg", Some(ImageKind::Jpeg)),
            ],
            &Options::default(),
            &never,
        );
        assert_eq!(
            dests(&p),
            [
                "/other/compressed/a.jpg",
                "/r/compressed/x/a.jpg",
                "/r/compressed/y/a.jpg"
            ]
        );
    }

    #[test]
    fn header_detected_skip_is_kept() {
        let mut d = disc("/r", "anim.webp", Some(ImageKind::Webp));
        d.skip = Some(SkipReason::Animated);
        let p = plan(vec![d], &Options::default(), &never);
        assert_eq!(p.items[0].action, Action::Skip(SkipReason::Animated));
        assert_eq!(p.items[0].destination, None);
        assert_eq!(p.encode_count(), 0);
    }

    #[test]
    fn conflict_overwrite() {
        let exists = |p: &Path| p.ends_with("compressed/a.jpg");
        let p = plan(
            vec![disc("/r", "a.jpg", Some(ImageKind::Jpeg))],
            &Options::default(),
            &exists,
        );
        assert_eq!(dests(&p), ["/r/compressed/a.jpg"]);
    }

    #[test]
    fn conflict_skip() {
        let exists = |p: &Path| p.ends_with("compressed/a.jpg");
        let opts = Options {
            on_conflict: ConflictPolicy::Skip,
            ..Options::default()
        };
        let p = plan(
            vec![disc("/r", "a.jpg", Some(ImageKind::Jpeg))],
            &opts,
            &exists,
        );
        assert_eq!(p.items[0].action, Action::Skip(SkipReason::Exists));
        assert_eq!(p.encode_count(), 0);
    }

    #[test]
    fn conflict_rename() {
        let exists =
            |p: &Path| p.ends_with("compressed/a.jpg") || p.ends_with("compressed/a-1.jpg");
        let opts = Options {
            on_conflict: ConflictPolicy::Rename,
            ..Options::default()
        };
        let p = plan(
            vec![disc("/r", "a.jpg", Some(ImageKind::Jpeg))],
            &opts,
            &exists,
        );
        assert_eq!(dests(&p), ["/r/compressed/a-2.jpg"]);
    }

    #[test]
    fn custom_out_dir_name() {
        let opts = Options {
            out_dir_name: "min".into(),
            ..Options::default()
        };
        let p = plan(
            vec![disc("/r", "a.jpg", Some(ImageKind::Jpeg))],
            &opts,
            &never,
        );
        assert_eq!(dests(&p), ["/r/min/a.jpg"]);
    }

    #[test]
    fn deterministic_regardless_of_input_order() {
        let opts = Options {
            output_format: OutputFormat::Webp,
            ..Options::default()
        };
        let a = vec![
            disc("/r", "a.png", Some(ImageKind::Png)),
            disc("/r", "a.jpg", Some(ImageKind::Jpeg)),
        ];
        let mut b = a.clone();
        b.reverse();
        assert_eq!(plan(a, &opts, &never), plan(b, &opts, &never));
    }
}
