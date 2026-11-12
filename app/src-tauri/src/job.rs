//! Runs one compression job at a time on a background thread and streams progress to the
//! window that started it. The UI never decides anything: it renders these messages.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use compress_core::{execute, plan, scan, Action, Event, Options, SkipReason};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Event name the frontend listens to.
pub const JOB_EVENT: &str = "job";

#[derive(Debug, Clone, Serialize)]
pub struct PlannedItem {
    pub index: usize,
    pub source: String,
    pub destination: Option<String>,
    pub size: u64,
    /// `None` = will be encoded.
    pub skip: Option<SkipReason>,
}

/// Messages emitted on [`JOB_EVENT`], in this order: input errors, `planned`, then core events.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobMessage {
    InputError { path: String, error: String },
    Planned { items: Vec<PlannedItem> },
    Core { event: Event },
}

struct Active {
    cancel: Arc<AtomicBool>,
    /// Label of the window that started the job and receives its events.
    target: String,
    /// Close `target` once the job has stopped (set when the user tried to close it).
    close_when_done: bool,
}

#[derive(Default)]
pub struct JobManager {
    active: Mutex<Option<Active>>,
}

impl JobManager {
    /// Starts a job whose events go to the window labelled `target`.
    pub fn start(
        &self,
        app: AppHandle,
        target: String,
        paths: Vec<PathBuf>,
        opts: Options,
    ) -> Result<(), String> {
        opts.validate().map_err(|e| e.to_string())?;
        if paths.is_empty() {
            return Err("nothing to compress".into());
        }
        let flag = {
            let mut slot = self.active.lock().map_err(|e| e.to_string())?;
            if slot.is_some() {
                return Err("a compression job is already running".into());
            }
            let flag = Arc::new(AtomicBool::new(false));
            *slot = Some(Active {
                cancel: Arc::clone(&flag),
                target: target.clone(),
                close_when_done: false,
            });
            flag
        };

        tracing::debug!(target_window = %target, inputs = paths.len(), "job starting");
        std::thread::spawn(move || {
            run(&app, &target, &paths, &opts, &flag);
            tracing::debug!(target_window = %target, "job finished");
            let finished = app
                .state::<JobManager>()
                .active
                .lock()
                .ok()
                .and_then(|mut slot| slot.take());
            if let Some(Active {
                close_when_done: true,
                target,
                ..
            }) = finished
            {
                if let Some(w) = app.get_webview_window(&target) {
                    let _ = w.close();
                }
            }
        });
        Ok(())
    }

    pub fn cancel(&self) {
        if let Ok(slot) = self.active.lock() {
            if let Some(a) = slot.as_ref() {
                a.cancel.store(true, Ordering::SeqCst);
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.active.lock().map(|s| s.is_some()).unwrap_or(false)
    }

    /// Called when the user closes `window`. If that window owns the running job, the close
    /// is deferred: the job is cancelled cleanly (no half-written temp files) and the window
    /// closes once it has stopped. Returns `true` if the close must be prevented now.
    pub fn defer_close(&self, window: &str) -> bool {
        let Ok(mut slot) = self.active.lock() else {
            return false;
        };
        match slot.as_mut() {
            Some(a) if a.target == window => {
                a.cancel.store(true, Ordering::SeqCst);
                a.close_when_done = true;
                true
            }
            _ => false,
        }
    }
}

fn run(app: &AppHandle, target: &str, paths: &[PathBuf], opts: &Options, cancel: &AtomicBool) {
    let send = |m: JobMessage| {
        if let Err(e) = app.emit_to(target, JOB_EVENT, m) {
            tracing::warn!("failed to emit job event: {e}");
        }
    };

    let report = scan(paths, opts);
    for err in &report.errors {
        send(JobMessage::InputError {
            path: err.path.to_string_lossy().into_owned(),
            error: err.error.clone(),
        });
    }

    let plan = plan(report.files, opts, &|p| p.exists());
    send(JobMessage::Planned {
        items: plan
            .items
            .iter()
            .enumerate()
            .map(|(index, i)| PlannedItem {
                index,
                source: i.source.to_string_lossy().into_owned(),
                destination: i
                    .destination
                    .as_ref()
                    .map(|d| d.to_string_lossy().into_owned()),
                size: i.source_size,
                skip: match i.action {
                    Action::Encode => None,
                    Action::Skip(r) => Some(r),
                },
            })
            .collect(),
    });

    execute(&plan, opts, cancel, &|event| {
        send(JobMessage::Core { event })
    });
}
