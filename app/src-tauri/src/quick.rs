//! Quick mode (SPEC §4.3): `compress-images-app --compress <PATH>...`, used by the OS
//! context menu. File managers often launch one process per selected item, so paths that
//! arrive within a short debounce window are merged into a single batch.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const FLAG: &str = "--compress";
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// Extracts the paths after `--compress` (resolved against `cwd`), or `None` if the flag is
/// absent. `argv[0]` (the program) is ignored.
pub fn parse_args<I, S>(argv: I, cwd: &Path) -> Option<Vec<PathBuf>>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args: Vec<OsString> = argv.into_iter().skip(1).map(Into::into).collect();
    let pos = args.iter().position(|a| a == FLAG)?;
    Some(
        args[pos + 1..]
            .iter()
            .filter(|a| !a.is_empty())
            .map(|a| {
                let p = PathBuf::from(a);
                if p.is_absolute() {
                    p
                } else {
                    cwd.join(p)
                }
            })
            .collect(),
    )
}

#[derive(Default)]
struct State {
    /// Paths still inside the debounce window.
    pending: Vec<PathBuf>,
    /// Bumped on every arrival; a timer only fires if no newer arrival happened.
    generation: u64,
    /// Batches ready for the quick window to pick up.
    ready: Vec<PathBuf>,
}

/// Debounced collector. `on_ready` runs (on a background thread) whenever a batch settles.
#[derive(Clone)]
pub struct QuickQueue {
    state: Arc<Mutex<State>>,
    delay: Duration,
}

impl QuickQueue {
    pub fn new(delay: Duration) -> Self {
        Self {
            state: Arc::default(),
            delay,
        }
    }

    pub fn push(&self, paths: Vec<PathBuf>, on_ready: impl FnOnce() + Send + 'static) {
        if paths.is_empty() {
            return;
        }
        let generation = {
            let Ok(mut s) = self.state.lock() else { return };
            s.pending.extend(paths);
            s.generation += 1;
            s.generation
        };
        let this = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(this.delay);
            let settled = {
                let Ok(mut s) = this.state.lock() else { return };
                if s.generation != generation {
                    return; // a newer arrival restarted the window
                }
                let batch = std::mem::take(&mut s.pending);
                s.ready.extend(batch);
                true
            };
            if settled {
                on_ready();
            }
        });
    }

    /// Takes the settled batch (deduplicated, order kept), if any.
    pub fn take_ready(&self) -> Vec<PathBuf> {
        let Ok(mut s) = self.state.lock() else {
            return vec![];
        };
        let mut seen = std::collections::HashSet::new();
        std::mem::take(&mut s.ready)
            .into_iter()
            .filter(|p| seen.insert(p.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn parses_compress_flag() {
        let cwd = Path::new("/work");
        assert_eq!(parse_args(["app"], cwd), None);
        assert_eq!(parse_args(["app", "--other"], cwd), None);
        assert_eq!(parse_args(["app", FLAG], cwd), Some(vec![]));
        assert_eq!(
            parse_args(["app", FLAG, "/abs/a.jpg", "rel/b.png", ""], cwd),
            Some(vec![
                PathBuf::from("/abs/a.jpg"),
                PathBuf::from("/work/rel/b.png")
            ])
        );
    }

    #[test]
    fn merges_arrivals_within_window_into_one_batch() {
        let q = QuickQueue::new(Duration::from_millis(80));
        let fired = Arc::new(AtomicUsize::new(0));
        for p in ["a", "b", "c", "a"] {
            let f = Arc::clone(&fired);
            q.push(vec![PathBuf::from(p)], move || {
                f.fetch_add(1, Ordering::SeqCst);
            });
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(fired.load(Ordering::SeqCst), 1, "only the last timer fires");
        assert_eq!(
            q.take_ready(),
            ["a", "b", "c"].map(PathBuf::from).to_vec(),
            "deduplicated, order kept"
        );
        assert!(q.take_ready().is_empty());
    }

    #[test]
    fn separate_bursts_make_separate_batches() {
        let q = QuickQueue::new(Duration::from_millis(30));
        q.push(vec![PathBuf::from("a")], || {});
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(q.take_ready(), vec![PathBuf::from("a")]);
        q.push(vec![PathBuf::from("b")], || {});
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(q.take_ready(), vec![PathBuf::from("b")]);
    }
}
