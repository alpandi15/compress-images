//! Race-free single-instance handling for macOS.
//!
//! `tauri-plugin-single-instance` on macOS decides "am I first?" by failing to connect to a
//! Unix socket, then binds it asynchronously. Two processes launched at nearly the same
//! moment (e.g. several context-menu clicks) can both fail to connect and both become
//! primary, so their files end up in separate jobs. Windows (named mutex) and Linux (D-Bus
//! name) are atomic and keep using the plugin.
//!
//! Here an exclusive file lock decides atomically. The primary binds the socket only while
//! holding the lock; other instances retry connecting until the primary is listening. The OS
//! releases the lock when the process exits, even after a crash.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a secondary instance waits for the primary to start listening.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_EVERY: Duration = Duration::from_millis(25);

pub enum Role {
    /// This process is the single instance; call [`Primary::listen`] once the app is up.
    Primary(Primary),
    /// Arguments were handed to the running instance; this process should exit.
    Forwarded,
}

pub struct Primary {
    _lock: File,
    listener: UnixListener,
    socket: PathBuf,
}

impl Drop for Primary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
    }
}

/// Lock and socket paths for an app identifier, in the per-user temp dir (Unix socket paths
/// must stay under ~104 bytes on macOS, which the temp dir comfortably allows).
pub fn paths_for(identifier: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir();
    let id = identifier.replace(['.', '-'], "_");
    (
        base.join(format!("{id}.lock")),
        base.join(format!("{id}.sock")),
    )
}

/// Becomes the primary instance, or forwards `cwd` + `argv` to the existing one.
pub fn acquire(
    lock_path: &Path,
    socket_path: &Path,
    cwd: &str,
    argv: &[String],
) -> io::Result<Role> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    match lock.try_lock() {
        Ok(()) => {
            // We hold the lock, so any socket file left behind is stale (crashed primary).
            let _ = fs::remove_file(socket_path);
            let listener = UnixListener::bind(socket_path)?;
            Ok(Role::Primary(Primary {
                _lock: lock,
                listener,
                socket: socket_path.to_path_buf(),
            }))
        }
        Err(fs::TryLockError::WouldBlock) => {
            forward(socket_path, cwd, argv)?;
            Ok(Role::Forwarded)
        }
        Err(fs::TryLockError::Error(e)) => Err(e),
    }
}

fn forward(socket_path: &Path, cwd: &str, argv: &[String]) -> io::Result<()> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let mut stream = loop {
        match UnixStream::connect(socket_path) {
            Ok(s) => break s,
            // The primary holds the lock but may not be listening yet.
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => std::thread::sleep(RETRY_EVERY),
        }
    };
    stream.write_all(&encode(cwd, argv))?;
    stream.flush()
}

/// `cwd \0\0 arg0 \0 arg1 ...` (NUL cannot occur in macOS paths or arguments).
fn encode(cwd: &str, argv: &[String]) -> Vec<u8> {
    let mut out = cwd.as_bytes().to_vec();
    out.extend_from_slice(b"\0\0");
    out.extend_from_slice(argv.join("\0").as_bytes());
    out
}

fn decode(data: &str) -> (Vec<String>, String) {
    let (cwd, args) = data.split_once("\0\0").unwrap_or(("", data));
    let argv = if args.is_empty() {
        vec![]
    } else {
        args.split('\0').map(String::from).collect()
    };
    (argv, cwd.to_owned())
}

impl Primary {
    /// Handles forwarded launches on a background thread for the rest of the process life.
    pub fn listen(self, on_launch: impl Fn(Vec<String>, String) + Send + 'static) {
        std::thread::spawn(move || {
            for conn in self.listener.incoming() {
                let mut stream = match conn {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("single instance: accept failed: {e}");
                        continue;
                    }
                };
                let mut data = String::new();
                match stream.read_to_string(&mut data) {
                    Ok(_) => {
                        let (argv, cwd) = decode(&data);
                        on_launch(argv, cwd);
                    }
                    Err(e) => tracing::warn!("single instance: bad message: {e}"),
                }
            }
            // `self` (lock + socket) lives as long as this thread, i.e. the whole process.
            drop(self);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn round_trip_encoding() {
        let (argv, cwd) = decode(
            std::str::from_utf8(&encode("/w", &args(&["app", "--compress", "a b.jpg"]))).unwrap(),
        );
        assert_eq!(cwd, "/w");
        assert_eq!(argv, args(&["app", "--compress", "a b.jpg"]));
    }

    #[test]
    fn second_instance_forwards_to_first() {
        let dir = tempfile::tempdir().unwrap();
        let (lock, sock) = (dir.path().join("l"), dir.path().join("s"));

        let Role::Primary(primary) = acquire(&lock, &sock, "/w", &args(&["app"])).unwrap() else {
            panic!("first must be primary");
        };
        let (tx, rx) = mpsc::channel();
        primary.listen(move |argv, cwd| tx.send((argv, cwd)).unwrap());

        for f in ["a.jpg", "b.jpg", "c.jpg"] {
            let role = acquire(&lock, &sock, "/w", &args(&["app", "--compress", f])).unwrap();
            assert!(matches!(role, Role::Forwarded));
        }
        let got: Vec<_> = (0..3)
            .map(|_| rx.recv_timeout(Duration::from_secs(2)).unwrap())
            .collect();
        assert!(got.iter().all(|(_, cwd)| cwd == "/w"));
        assert_eq!(got[0].0, args(&["app", "--compress", "a.jpg"]));
    }

    #[test]
    fn secondary_waits_for_slow_primary() {
        let dir = tempfile::tempdir().unwrap();
        let (lock_path, sock) = (dir.path().join("l"), dir.path().join("s"));

        // Simulate a primary that holds the lock but starts listening late.
        let lock = File::create(&lock_path).unwrap();
        lock.try_lock().unwrap();
        let sock2 = sock.clone();
        let late = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let listener = UnixListener::bind(&sock2).unwrap();
            let (mut s, _) = listener.accept().unwrap();
            let mut data = String::new();
            s.read_to_string(&mut data).unwrap();
            decode(&data)
        });

        let role = acquire(
            &lock_path,
            &sock,
            "/w",
            &args(&["app", "--compress", "x.jpg"]),
        )
        .unwrap();
        assert!(matches!(role, Role::Forwarded));
        let (argv, _) = late.join().unwrap();
        assert_eq!(argv, args(&["app", "--compress", "x.jpg"]));
        drop(lock);
    }

    #[test]
    fn stale_socket_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let (lock, sock) = (dir.path().join("l"), dir.path().join("s"));
        fs::write(&sock, b"stale").unwrap();
        let role = acquire(&lock, &sock, "/w", &args(&["app"])).unwrap();
        assert!(matches!(role, Role::Primary(_)));
    }
}
