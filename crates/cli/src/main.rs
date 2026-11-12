//! `compress-images` — command-line front end for `compress-core` (SPEC §4.1).

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use compress_core::{
    execute, plan, scan, Action, ConflictPolicy, Event, Options, Outcome, OutputFormat, Preset,
    Quality, Resize, SkipReason, Summary,
};
use serde_json::json;

/// Exit codes (SPEC §4.1).
mod exit {
    pub const OK: u8 = 0;
    pub const SOME_FAILED: u8 = 1;
    pub const USAGE: u8 = 2;
    pub const FATAL: u8 = 3;
    pub const CANCELLED: u8 = 130;
}

#[derive(Parser, Debug)]
#[command(
    name = "compress-images",
    version,
    about = "Compress images in files and folders into a `compressed/` subfolder.",
    long_about = "Compress images in files and folders. Results are written to a \
                  `compressed/` folder next to each file (or inside each selected folder, \
                  mirroring subfolders). Originals are never modified."
)]
struct Cli {
    /// Files and/or folders to compress
    #[arg(required = true, value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// Compression quality, 1 (smallest) to 100 (best)
    #[arg(short, long, default_value_t = 70, value_parser = clap::value_parser!(u8).range(1..=100))]
    quality: u8,

    /// Quality preset (overrides --quality)
    #[arg(short, long, value_enum)]
    preset: Option<PresetArg>,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = FormatArg::Same)]
    format: FormatArg,

    /// Do not descend into subfolders
    #[arg(long)]
    no_recursive: bool,

    /// Downscale to at most this width (pixels)
    #[arg(long, value_name = "PX", value_parser = clap::value_parser!(u32).range(1..))]
    max_width: Option<u32>,

    /// Downscale to at most this height (pixels)
    #[arg(long, value_name = "PX", value_parser = clap::value_parser!(u32).range(1..))]
    max_height: Option<u32>,

    /// Keep EXIF/XMP metadata (GPS location included!)
    #[arg(long)]
    keep_metadata: bool,

    /// Name of the output folder
    #[arg(short, long, value_name = "NAME", default_value = "compressed")]
    out_dir_name: String,

    /// What to do when an output file already exists
    #[arg(long, value_enum, value_name = "POLICY", default_value_t = ConflictArg::Overwrite)]
    on_conflict: ConflictArg,

    /// Worker threads [default: number of CPUs]
    #[arg(short, long, value_name = "N", value_parser = clap::value_parser!(u16).range(1..))]
    jobs: Option<u16>,

    /// AVIF encoder speed, 1 (slow, smaller) to 10 (fast)
    #[arg(long, value_name = "1-10", default_value_t = 6, value_parser = clap::value_parser!(u8).range(1..=10))]
    avif_speed: u8,

    /// Show what would happen without writing anything
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// Machine-readable output: one JSON object per line
    #[arg(long)]
    json: bool,

    /// Verbose output (also lists unsupported files)
    #[arg(short, long)]
    verbose: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum PresetArg {
    Light,
    Balanced,
    Aggressive,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum FormatArg {
    Same,
    #[value(alias = "jpeg")]
    Jpg,
    Png,
    Webp,
    Avif,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum ConflictArg {
    Overwrite,
    Skip,
    Rename,
}

impl Cli {
    fn options(&self) -> Result<Options> {
        let quality = match self.preset {
            Some(PresetArg::Light) => Preset::Light.quality(),
            Some(PresetArg::Balanced) => Preset::Balanced.quality(),
            Some(PresetArg::Aggressive) => Preset::Aggressive.quality(),
            None => Quality::new(self.quality)?,
        };
        let opts = Options {
            quality,
            output_format: match self.format {
                FormatArg::Same => OutputFormat::Same,
                FormatArg::Jpg => OutputFormat::Jpeg,
                FormatArg::Png => OutputFormat::Png,
                FormatArg::Webp => OutputFormat::Webp,
                FormatArg::Avif => OutputFormat::Avif,
            },
            recursive: !self.no_recursive,
            resize: Resize {
                max_width: self.max_width,
                max_height: self.max_height,
            },
            keep_metadata: self.keep_metadata,
            out_dir_name: self.out_dir_name.clone(),
            on_conflict: match self.on_conflict {
                ConflictArg::Overwrite => ConflictPolicy::Overwrite,
                ConflictArg::Skip => ConflictPolicy::Skip,
                ConflictArg::Rename => ConflictPolicy::Rename,
            },
            jobs: self.jobs.map(usize::from),
            avif_speed: self.avif_speed,
        };
        opts.validate()?;
        Ok(opts)
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    let opts = match cli.options() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(exit::USAGE);
        }
    };

    match run(&cli, &opts) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(exit::FATAL)
        }
    }
}

fn init_logging(verbose: bool) {
    let default = if verbose { "debug" } else { "warn" };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .try_init();
}

fn run(cli: &Cli, opts: &Options) -> Result<u8> {
    let out = Printer::new(cli.json, cli.verbose);

    let report = scan(&cli.paths, opts);
    for err in &report.errors {
        out.input_error(&err.path, &err.error);
    }
    if report.files.is_empty()
        && !report.errors.is_empty()
        && report.errors.len() >= cli.paths.len()
    {
        // Nothing usable at all.
        return Ok(exit::FATAL);
    }
    let input_errors = !report.errors.is_empty();

    let plan = plan(report.files, opts, &|p| p.exists());

    if cli.dry_run {
        out.dry_run(&plan);
        return Ok(if input_errors {
            exit::SOME_FAILED
        } else {
            exit::OK
        });
    }

    let cancel = Arc::new(AtomicBool::new(false));
    install_ctrlc(Arc::clone(&cancel), cli.json);

    if !cli.json {
        out.header(plan.encode_count(), opts);
    }
    let summary = execute(&plan, opts, &cancel, &|e| out.event(e));

    Ok(if summary.cancelled {
        exit::CANCELLED
    } else if summary.failed > 0 || input_errors {
        exit::SOME_FAILED
    } else {
        exit::OK
    })
}

/// First Ctrl+C cancels gracefully (finished files are kept); a second one exits at once.
fn install_ctrlc(cancel: Arc<AtomicBool>, quiet: bool) {
    let _ = ctrlc::set_handler(move || {
        if cancel.swap(true, Ordering::SeqCst) {
            std::process::exit(i32::from(exit::CANCELLED));
        }
        if !quiet {
            eprintln!("\nCancelling… (press Ctrl+C again to quit immediately)");
        }
    });
}

struct Printer {
    json: bool,
    verbose: bool,
    cwd: Option<PathBuf>,
}

impl Printer {
    fn new(json: bool, verbose: bool) -> Self {
        Self {
            json,
            verbose,
            // Same canonical form the scanner uses, so relative display works.
            cwd: std::env::current_dir()
                .ok()
                .and_then(|d| dunce::canonicalize(d).ok()),
        }
    }

    /// Paths under the working directory are shown relative to it.
    fn show(&self, p: &Path) -> String {
        self.cwd
            .as_deref()
            .and_then(|cwd| p.strip_prefix(cwd).ok())
            .unwrap_or(p)
            .display()
            .to_string()
    }

    fn json_line(&self, v: &impl serde::Serialize) {
        if let Ok(s) = serde_json::to_string(v) {
            let mut stdout = io::stdout().lock();
            let _ = writeln!(stdout, "{s}");
        }
    }

    fn input_error(&self, path: &Path, error: &str) {
        if self.json {
            self.json_line(&json!({
                "event": "input_error",
                "path": path.to_string_lossy(),
                "error": error,
            }));
        } else {
            eprintln!("error: {}: {error}", path.display());
        }
    }

    fn header(&self, count: usize, opts: &Options) {
        let noun = if count == 1 { "image" } else { "images" };
        println!(
            "Compressing {count} {noun} (quality {}, output folder \"{}\")",
            opts.quality.get(),
            opts.out_dir_name
        );
    }

    fn dry_run(&self, plan: &compress_core::Plan) {
        for item in &plan.items {
            if self.json {
                let (action, reason) = match item.action {
                    Action::Encode => ("encode", None),
                    Action::Skip(r) => ("skip", Some(r)),
                };
                self.json_line(&json!({
                    "event": "planned",
                    "source": item.source.to_string_lossy(),
                    "destination": item.destination.as_ref().map(|d| d.to_string_lossy()),
                    "action": action,
                    "reason": reason,
                }));
                continue;
            }
            match (item.action, &item.destination) {
                (Action::Encode, Some(d)) => {
                    println!("  {} → {}", self.show(&item.source), self.show(d))
                }
                (Action::Skip(SkipReason::Unsupported), _) if !self.verbose => {}
                (Action::Skip(r), _) => {
                    println!("  - {}  (skip: {r})", self.show(&item.source))
                }
                (Action::Encode, None) => {}
            }
        }
        if !self.json {
            let n = plan.encode_count();
            println!("Dry run: {n} image(s) would be compressed. Nothing was written.");
        }
    }

    fn event(&self, e: Event) {
        if self.json {
            self.json_line(&e);
            return;
        }
        match e {
            Event::FileDone {
                source,
                destination,
                outcome,
                ..
            } => self.file_done(&source, destination.as_deref(), &outcome),
            Event::Finished { summary } => self.summary(&summary),
            Event::Started { .. } | Event::FileStarted { .. } => {}
        }
    }

    fn file_done(&self, source: &Path, dest: Option<&Path>, outcome: &Outcome) {
        let src = self.show(source);
        let line = match outcome {
            Outcome::Compressed {
                before,
                after,
                larger_than_source,
            } => {
                let warn = if *larger_than_source {
                    "  (larger than source)"
                } else {
                    ""
                };
                format!(
                    "  ✓ {src} → {}  {} → {}  ({}){warn}",
                    dest.map(|d| self.show(d)).unwrap_or_default(),
                    human(*before),
                    human(*after),
                    percent(*before, *after),
                )
            }
            Outcome::KeptOriginal { size } => format!(
                "  = {src} → {}  {}  (already optimal, original copied)",
                dest.map(|d| self.show(d)).unwrap_or_default(),
                human(*size)
            ),
            Outcome::Skipped {
                reason: SkipReason::Unsupported,
            } if !self.verbose => return,
            Outcome::Skipped { reason } => format!("  - {src}  (skipped: {reason})"),
            Outcome::Failed { error } => {
                eprintln!("  ✗ {src}  failed: {error}");
                return;
            }
        };
        println!("{line}");
    }

    fn summary(&self, s: &Summary) {
        let secs = s.elapsed_ms as f64 / 1000.0;
        let mut parts = vec![format!("{} compressed", s.compressed)];
        if s.kept_original > 0 {
            parts.push(format!("{} kept", s.kept_original));
        }
        if s.skipped > 0 {
            parts.push(format!("{} skipped", s.skipped));
        }
        if s.failed > 0 {
            parts.push(format!("{} failed", s.failed));
        }
        let status = if s.cancelled { "Cancelled" } else { "Done" };
        println!(
            "{status}: {} · {} → {} ({}) in {secs:.1}s",
            parts.join(", "),
            human(s.bytes_before),
            human(s.bytes_after),
            percent(s.bytes_before, s.bytes_after),
        );
    }
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

fn percent(before: u64, after: u64) -> String {
    if before == 0 {
        return "0%".into();
    }
    let p = (after as f64 / before as f64 - 1.0) * 100.0;
    if p <= 0.0 {
        format!("−{:.0}%", -p)
    } else {
        format!("+{p:.0}%")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes() {
        assert_eq!(human(512), "512 B");
        assert_eq!(human(1536), "1.5 KB");
        assert_eq!(human(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn percentages() {
        assert_eq!(percent(100, 25), "−75%");
        assert_eq!(percent(100, 150), "+50%");
        assert_eq!(percent(0, 0), "0%");
    }

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
