//! Turns user inputs (files and folders) into a deduplicated list of candidate files (SPEC §3.3).

use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use walkdir::WalkDir;

use crate::detect::{sniff_file, ImageKind};
use crate::event::SkipReason;
use crate::options::Options;

/// A regular file found while scanning, with the root its output is relative to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    pub source: PathBuf,
    /// Folder input, or the parent directory of a file input.
    pub root: PathBuf,
    /// Path of `source` relative to `root`, including the file name.
    pub rel: PathBuf,
    /// `None` = not an image (or a format we don't handle at all).
    pub kind: Option<ImageKind>,
    /// Set when the header already shows the file is out of scope (e.g. animated).
    pub skip: Option<SkipReason>,
    pub size: u64,
}

/// An input or entry that couldn't be read. Counts as an error for exit-code purposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputError {
    pub path: PathBuf,
    pub error: String,
}

#[derive(Debug, Default)]
pub struct ScanReport {
    pub files: Vec<Discovered>,
    pub errors: Vec<InputError>,
}

/// Scans `inputs`. Never fails as a whole: unreadable inputs are reported in `errors`.
pub fn scan(inputs: &[PathBuf], opts: &Options) -> ScanReport {
    let mut report = ScanReport::default();
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for input in inputs {
        let resolved = dunce::canonicalize(input).and_then(|p| fs::metadata(&p).map(|m| (p, m)));
        match resolved {
            Ok((p, m)) if m.is_dir() => dirs.push(p),
            Ok((p, m)) if m.is_file() => files.push((p, m.len())),
            Ok(_) => report.errors.push(InputError {
                path: input.clone(),
                error: "not a regular file or folder".into(),
            }),
            Err(e) => report.errors.push(InputError {
                path: input.clone(),
                error: e.to_string(),
            }),
        }
    }

    dirs.sort();
    dirs.dedup();
    // A folder already reached by walking another selected folder is processed once, under
    // the outer root.
    let all_dirs = dirs.clone();
    dirs.retain(|d| !all_dirs.iter().any(|o| walk_reaches(o, d, true, opts)));

    files.sort();
    files.dedup();
    files.retain(|(f, _)| !dirs.iter().any(|d| walk_reaches(d, f, false, opts)));

    for (path, size) in files {
        let (Some(root), Some(name)) = (path.parent(), path.file_name()) else {
            continue;
        };
        let (root, rel) = (root.to_path_buf(), PathBuf::from(name));
        push_file(&mut report, path, root, rel, size);
    }

    for dir in dirs {
        walk_dir(&dir, opts, &mut report);
    }

    report.files.sort_by(|a, b| a.source.cmp(&b.source));
    report
}

fn walk_dir(dir: &Path, opts: &Options, report: &mut ScanReport) {
    let max_depth = if opts.recursive { usize::MAX } else { 1 };
    let walker = WalkDir::new(dir)
        .follow_links(false)
        .min_depth(1)
        .max_depth(max_depth)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| !is_excluded_entry(e, opts));

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                report.errors.push(InputError {
                    path: e.path().unwrap_or(dir).to_path_buf(),
                    error: e.to_string(),
                });
                continue;
            }
        };
        let ft = entry.file_type();
        if ft.is_symlink() || !ft.is_file() {
            continue;
        }
        let size = match entry.metadata() {
            Ok(m) => m.len(),
            Err(e) => {
                report.errors.push(InputError {
                    path: entry.path().to_path_buf(),
                    error: e.to_string(),
                });
                continue;
            }
        };
        if size == 0 {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(dir) else {
            continue;
        };
        let rel = rel.to_path_buf();
        push_file(report, entry.into_path(), dir.to_path_buf(), rel, size);
    }
}

fn push_file(report: &mut ScanReport, source: PathBuf, root: PathBuf, rel: PathBuf, size: u64) {
    match sniff_file(&source) {
        Ok((kind, skip)) => report.files.push(Discovered {
            source,
            root,
            rel,
            kind,
            skip,
            size,
        }),
        Err(e) => report.errors.push(InputError {
            path: source,
            error: e.to_string(),
        }),
    }
}

/// Entries the folder walk never enters or yields (SPEC §3.3 step 3).
fn is_excluded_entry(e: &walkdir::DirEntry, opts: &Options) -> bool {
    let name = e.file_name();
    if is_hidden_name(name) || is_hidden_attr(e) {
        return true;
    }
    e.file_type().is_dir() && is_out_dir_name(name, opts)
}

pub(crate) fn is_hidden_name(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

pub(crate) fn is_out_dir_name(name: &OsStr, opts: &Options) -> bool {
    name.to_string_lossy().to_lowercase() == opts.out_dir_name.to_lowercase()
}

#[cfg(windows)]
fn is_hidden_attr(e: &walkdir::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    e.metadata()
        .map(|m| m.file_attributes() & (HIDDEN | SYSTEM) != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn is_hidden_attr(_: &walkdir::DirEntry) -> bool {
    false
}

/// Would walking folder input `dir` reach `target`? Used to deduplicate overlapping inputs.
fn walk_reaches(dir: &Path, target: &Path, target_is_dir: bool, opts: &Options) -> bool {
    let Ok(rel) = target.strip_prefix(dir) else {
        return false;
    };
    let comps: Vec<&OsStr> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s),
            _ => None,
        })
        .collect();
    let Some((last, parents)) = comps.split_last() else {
        return false; // same path
    };
    if !opts.recursive && (target_is_dir || !parents.is_empty()) {
        return false;
    }
    if parents
        .iter()
        .any(|p| is_hidden_name(p) || is_out_dir_name(p, opts))
    {
        return false;
    }
    if is_hidden_name(last) {
        return false;
    }
    !(target_is_dir && is_out_dir_name(last, opts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaches_rules() {
        let o = Options::default();
        let d = Path::new("/r");
        assert!(walk_reaches(d, Path::new("/r/a.jpg"), false, &o));
        assert!(walk_reaches(d, Path::new("/r/sub/a.jpg"), false, &o));
        assert!(walk_reaches(d, Path::new("/r/sub"), true, &o));
        assert!(!walk_reaches(d, Path::new("/r"), true, &o));
        assert!(!walk_reaches(d, Path::new("/other/a.jpg"), false, &o));
        assert!(!walk_reaches(
            d,
            Path::new("/r/compressed/a.jpg"),
            false,
            &o
        ));
        assert!(!walk_reaches(d, Path::new("/r/Compressed"), true, &o));
        assert!(!walk_reaches(d, Path::new("/r/.hidden/a.jpg"), false, &o));
        assert!(!walk_reaches(d, Path::new("/r/.a.jpg"), false, &o));

        let flat = Options {
            recursive: false,
            ..Options::default()
        };
        assert!(walk_reaches(d, Path::new("/r/a.jpg"), false, &flat));
        assert!(!walk_reaches(d, Path::new("/r/sub/a.jpg"), false, &flat));
        assert!(!walk_reaches(d, Path::new("/r/sub"), true, &flat));
    }
}
