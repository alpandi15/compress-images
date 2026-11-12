//! Per-file outcomes and the progress event stream consumed by the CLI and the desktop app.

use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};

/// Why a file was not compressed. None of these count as errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    Unsupported,
    Animated,
    MultiPage,
    Exists,
    Cancelled,
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SkipReason::Unsupported => "unsupported format",
            SkipReason::Animated => "animated image",
            SkipReason::MultiPage => "multi-page TIFF",
            SkipReason::Exists => "output already exists",
            SkipReason::Cancelled => "cancelled",
        })
    }
}

/// Final state of one planned file (SPEC §3.11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Compressed {
        before: u64,
        after: u64,
        /// Only possible when the format was converted; see the never-larger rule.
        larger_than_source: bool,
    },
    KeptOriginal {
        size: u64,
    },
    Skipped {
        reason: SkipReason,
    },
    Failed {
        error: String,
    },
}

/// Progress events, emitted in order per file but interleaved across files.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Started {
        total: usize,
    },
    FileStarted {
        index: usize,
        #[serde(serialize_with = "lossy_path")]
        source: PathBuf,
    },
    FileDone {
        index: usize,
        #[serde(serialize_with = "lossy_path")]
        source: PathBuf,
        #[serde(serialize_with = "lossy_opt_path")]
        destination: Option<PathBuf>,
        outcome: Outcome,
    },
    Finished {
        summary: Summary,
    },
}

/// Totals for a finished job.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub total: usize,
    pub compressed: usize,
    pub kept_original: usize,
    pub skipped: usize,
    pub failed: usize,
    /// Sum of source sizes for files that produced output.
    pub bytes_before: u64,
    /// Sum of output sizes for files that produced output.
    pub bytes_after: u64,
    pub elapsed_ms: u64,
    pub cancelled: bool,
}

impl Summary {
    pub(crate) fn record(&mut self, outcome: &Outcome) {
        match outcome {
            Outcome::Compressed { before, after, .. } => {
                self.compressed += 1;
                self.bytes_before += before;
                self.bytes_after += after;
            }
            Outcome::KeptOriginal { size } => {
                self.kept_original += 1;
                self.bytes_before += size;
                self.bytes_after += size;
            }
            Outcome::Skipped { .. } => self.skipped += 1,
            Outcome::Failed { .. } => self.failed += 1,
        }
    }

    /// Percentage saved over files that produced output (negative if output grew).
    pub fn saved_percent(&self) -> f64 {
        if self.bytes_before == 0 {
            return 0.0;
        }
        (1.0 - self.bytes_after as f64 / self.bytes_before as f64) * 100.0
    }
}

// Paths may not be UTF-8 (Linux); JSON output uses a lossy rendering rather than failing.
fn lossy_path<S: Serializer>(p: &Path, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&p.to_string_lossy())
}

fn lossy_opt_path<S: Serializer>(p: &Option<PathBuf>, s: S) -> Result<S::Ok, S::Error> {
    match p {
        Some(p) => lossy_path(p, s),
        None => s.serialize_none(),
    }
}
