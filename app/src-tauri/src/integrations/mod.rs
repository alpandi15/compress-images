//! Context-menu integration with the OS file manager (SPEC §5).
//!
//! Every platform module is split into a *pure* part (what to write, unit-tested on all
//! platforms) and an installer whose target location is injected, so tests never touch the
//! real registry, `~/Library/Services` or file-manager config.

pub mod linux;
pub mod macos;
pub mod windows;

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Menu label shown in every file manager (the product name, so it needs no translation).
pub const MENU_LABEL: &str = "Compress Images";

/// Image extensions that get a context-menu entry (Windows) — mirrors SPEC §3.1 inputs.
pub const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jfif", "png", "webp", "avif", "tif", "tiff", "bmp", "heic", "heif",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntegrationStatus {
    /// `false` on platforms without an integration.
    pub supported: bool,
    pub installed: bool,
    /// Installed, but pointing at another copy of the app (moved or reinstalled elsewhere).
    pub outdated: bool,
    /// Human-readable places the menu appears in, e.g. `["Finder"]`, `["Nautilus", "Dolphin"]`.
    pub targets: Vec<String>,
}

/// The program the menu should launch. For an AppImage this is the `.AppImage` file itself:
/// `current_exe()` points into a temporary mount that changes on every run.
pub fn launcher_path() -> std::io::Result<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(appimage));
    }
    std::env::current_exe()
}

/// Status for the current platform. `home` is the user's home directory.
pub fn status(home: &Path, launcher: &Path) -> IntegrationStatus {
    #[cfg(target_os = "macos")]
    return macos::status(&macos::services_dir(home), launcher);
    #[cfg(target_os = "linux")]
    return linux::status(&linux::Layout::for_home(home), launcher);
    #[cfg(windows)]
    {
        let _ = home;
        return windows::status(&windows::SystemRegistry, launcher);
    }
    #[allow(unreachable_code)]
    {
        let _ = (home, launcher);
        IntegrationStatus {
            supported: false,
            installed: false,
            outdated: false,
            targets: vec![],
        }
    }
}

pub fn install(home: &Path, launcher: &Path) -> Result<IntegrationStatus, String> {
    #[cfg(target_os = "macos")]
    {
        let dir = macos::services_dir(home);
        macos::install(&dir, launcher).map_err(|e| e.to_string())?;
        macos::refresh_services();
    }
    #[cfg(target_os = "linux")]
    linux::install(&linux::Layout::for_home(home), launcher).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    windows::install(&mut windows::SystemRegistry, launcher).map_err(|e| e.to_string())?;
    Ok(status(home, launcher))
}

pub fn uninstall(home: &Path, launcher: &Path) -> Result<IntegrationStatus, String> {
    #[cfg(target_os = "macos")]
    {
        macos::uninstall(&macos::services_dir(home)).map_err(|e| e.to_string())?;
        macos::refresh_services();
    }
    #[cfg(target_os = "linux")]
    linux::uninstall(&linux::Layout::for_home(home)).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    windows::uninstall(&mut windows::SystemRegistry).map_err(|e| e.to_string())?;
    Ok(status(home, launcher))
}

/// Single-quotes a string for POSIX shells: `it's` → `'it'\''s'`.
pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Writes a file atomically (temp file + rename) and optionally marks it executable.
pub(crate) fn write_file(path: &Path, contents: &str, executable: bool) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("path has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".compress-images.")
        .tempfile_in(dir)?;
    tmp.write_all(contents.as_bytes())?;
    tmp.as_file().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o755 } else { 0o644 };
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Removes a file or directory tree, treating "not found" as success.
pub(crate) fn remove_path(path: &Path) -> std::io::Result<()> {
    let res = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match res {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("/a b/c"), "'/a b/c'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn write_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x/y.sh");
        write_file(&f, "echo hi", true).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "echo hi");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        remove_path(&dir.path().join("x")).unwrap();
        remove_path(&dir.path().join("x")).unwrap(); // idempotent
        assert!(!f.exists());
    }
}
