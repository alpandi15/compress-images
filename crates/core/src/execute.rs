//! Runs a [`Plan`] in parallel (SPEC §3.6, §3.10–§3.12).

use std::fs;
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use rayon::prelude::*;

use crate::decode::decode;
use crate::detect::{unsupported_variant, ImageKind};
use crate::encode::{encode, EncodeParams};
use crate::error::{CoreError, Result};
use crate::event::{Event, Outcome, SkipReason, Summary};
use crate::options::Options;
use crate::plan::{Action, Plan, PlanItem};
use crate::transform;

/// Executes `plan`, reporting progress through `on_event`. Never fails as a whole:
/// per-file problems become [`Outcome::Failed`].
///
/// `cancel` may be set from another thread; files already written are kept, the rest are
/// reported as skipped.
pub fn execute(
    plan: &Plan,
    opts: &Options,
    cancel: &AtomicBool,
    on_event: &(dyn Fn(Event) + Sync),
) -> Summary {
    let started = Instant::now();
    on_event(Event::Started {
        total: plan.items.len(),
    });

    let summary = Mutex::new(Summary {
        total: plan.items.len(),
        ..Summary::default()
    });

    let run = || {
        plan.items.par_iter().enumerate().for_each(|(index, item)| {
            let outcome = match item.action {
                Action::Skip(reason) => Outcome::Skipped { reason },
                Action::Encode if cancel.load(Ordering::Relaxed) => Outcome::Skipped {
                    reason: SkipReason::Cancelled,
                },
                Action::Encode => {
                    on_event(Event::FileStarted {
                        index,
                        source: item.source.clone(),
                    });
                    process_guarded(item, opts, cancel)
                }
            };
            if let Ok(mut s) = summary.lock() {
                s.record(&outcome);
            }
            let destination = match outcome {
                Outcome::Compressed { .. } | Outcome::KeptOriginal { .. } => {
                    item.destination.clone()
                }
                _ => None,
            };
            on_event(Event::FileDone {
                index,
                source: item.source.clone(),
                destination,
                outcome,
            });
        });
    };

    match rayon::ThreadPoolBuilder::new()
        .num_threads(opts.jobs.unwrap_or(0))
        .build()
    {
        Ok(pool) => pool.install(run),
        // Couldn't spawn a dedicated pool (e.g. thread limits): fall back to the global one.
        Err(_) => run(),
    }

    let mut summary = summary.into_inner().unwrap_or_else(|e| e.into_inner());
    summary.cancelled = cancel.load(Ordering::Relaxed);
    summary.elapsed_ms = started.elapsed().as_millis() as u64;
    on_event(Event::Finished {
        summary: summary.clone(),
    });
    summary
}

/// Isolates panics from native codecs so they surface as a per-file failure.
fn process_guarded(item: &PlanItem, opts: &Options, cancel: &AtomicBool) -> Outcome {
    match catch_unwind(AssertUnwindSafe(|| process(item, opts, cancel))) {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(e)) => Outcome::Failed {
            error: e.to_string().trim().to_owned(),
        },
        Err(_) => Outcome::Failed {
            error: "internal error while processing this file".into(),
        },
    }
}

fn process(item: &PlanItem, opts: &Options, cancel: &AtomicBool) -> Result<Outcome> {
    let (Some(input_kind), Some(output_kind), Some(dest)) = (
        item.input_kind,
        item.output_kind,
        item.destination.as_deref(),
    ) else {
        return Err(CoreError::Encode("incomplete plan item".into()));
    };

    let data = fs::read(&item.source)?;
    if let Some(reason) = unsupported_variant(input_kind, &data) {
        return Ok(Outcome::Skipped { reason });
    }

    let decoded = decode(input_kind, &data)?;
    let resized = transform::resize(&decoded.image, &opts.resize)?;
    let was_resized = resized.is_some();
    let image = resized.as_ref().unwrap_or(&decoded.image);

    let params = EncodeParams::new(
        opts,
        input_kind,
        decoded.icc.as_deref(),
        decoded.exif.as_deref(),
    );
    let encoded = encode(output_kind, image, &params)?;

    if cancel.load(Ordering::Relaxed) {
        return Ok(Outcome::Skipped {
            reason: SkipReason::Cancelled,
        });
    }

    let before = data.len() as u64;
    if keep_original(
        input_kind,
        output_kind,
        was_resized,
        encoded.len(),
        data.len(),
    ) {
        write_atomic(dest, &data)?;
        return Ok(Outcome::KeptOriginal { size: before });
    }

    write_atomic(dest, &encoded)?;
    let after = encoded.len() as u64;
    Ok(Outcome::Compressed {
        before,
        after,
        larger_than_source: after > before,
    })
}

/// Never-larger rule (SPEC §3.6). A resized image is never swapped back for the
/// full-size original, since the user explicitly asked for smaller dimensions.
fn keep_original(
    input: ImageKind,
    output: ImageKind,
    resized: bool,
    encoded_len: usize,
    source_len: usize,
) -> bool {
    input == output && !resized && encoded_len >= source_len
}

/// Temp file in the destination directory → fsync → rename (SPEC §3.10).
fn write_atomic(dest: &Path, data: &[u8]) -> Result<()> {
    let dir = dest
        .parent()
        .ok_or_else(|| CoreError::Encode("destination has no parent folder".into()))?;
    fs::create_dir_all(dir)?;
    let prefix = format!(
        ".{}.",
        dest.file_name().unwrap_or_default().to_string_lossy()
    );
    let mut tmp = tempfile::Builder::new()
        .prefix(&prefix)
        .suffix(".tmp")
        .tempfile_in(dir)?;
    tmp.write_all(data)?;
    tmp.as_file().sync_all()?;
    // On failure the temp file is dropped, which deletes it.
    tmp.persist(dest).map_err(|e| CoreError::Io(e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_larger_rule() {
        use ImageKind::*;
        assert!(keep_original(Jpeg, Jpeg, false, 100, 100));
        assert!(keep_original(Png, Png, false, 120, 100));
        assert!(!keep_original(Png, Png, false, 99, 100));
        assert!(
            !keep_original(Png, Webp, false, 200, 100),
            "conversion kept"
        );
        assert!(!keep_original(Jpeg, Jpeg, true, 200, 100), "resize kept");
        assert!(!keep_original(Heic, Jpeg, false, 200, 100));
    }

    #[test]
    fn atomic_write_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("nested/out.bin");
        write_atomic(&dest, b"hello").unwrap();
        write_atomic(&dest, b"world").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"world");
        let names: Vec<_> = fs::read_dir(dest.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["out.bin"]);
    }
}
