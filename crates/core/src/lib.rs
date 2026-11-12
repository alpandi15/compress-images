//! `compress-core`: the image compression engine behind Compress Images.
//!
//! The pipeline is `scan → plan → execute` (see `docs/SPEC.md` §6.2):
//!
//! ```no_run
//! use std::sync::atomic::AtomicBool;
//! use compress_core::{execute, plan, scan, Options};
//!
//! let opts = Options::default();
//! let report = scan(&["./photos".into()], &opts);
//! let plan = plan(report.files, &opts, &|p| p.exists());
//! let summary = execute(&plan, &opts, &AtomicBool::new(false), &|event| {
//!     println!("{event:?}");
//! });
//! println!("saved {:.1}%", summary.saved_percent());
//! ```
//!
//! The crate never writes outside `<root>/<out_dir_name>/` and never modifies source files.
//! It does not print; progress is reported through [`Event`]s and diagnostics via `tracing`.

mod decode;
pub mod detect;
mod encode;
pub mod error;
pub mod event;
mod execute;
pub mod options;
pub mod plan;
pub mod scan;
mod transform;

pub use detect::ImageKind;
pub use error::CoreError;
pub use event::{Event, Outcome, SkipReason, Summary};
pub use execute::execute;
pub use options::{ConflictPolicy, Options, OutputFormat, Preset, Quality, Resize};
pub use plan::{plan, Action, Plan, PlanItem};
pub use scan::{scan, Discovered, InputError, ScanReport};
