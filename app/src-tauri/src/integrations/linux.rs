//! Linux file managers: Nautilus (GNOME Files), Dolphin (KDE) and Nemo (Cinnamon).
//!
//! Dolphin and Nemo entries are always written (they're inert if the file manager isn't
//! installed). For Nautilus, a Python extension gives a top-level menu item but needs the
//! `nautilus-python` plugin; without it, a Nautilus script (under "Scripts") is used instead.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::io;
use std::path::{Path, PathBuf};

use super::{remove_path, shell_quote, write_file, IntegrationStatus, MENU_LABEL};

/// Where each file goes; built from the home dir, overridable in tests.
#[derive(Debug, Clone)]
pub struct Layout {
    pub nautilus_extension: PathBuf,
    pub nautilus_script: PathBuf,
    pub dolphin: PathBuf,
    pub nemo: PathBuf,
    /// Library roots searched for the nautilus-python plugin.
    pub plugin_roots: Vec<PathBuf>,
}

impl Layout {
    pub fn for_home(home: &Path) -> Self {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        Self::under(
            &data,
            ["/usr/lib", "/usr/lib64", "/usr/local/lib"]
                .map(PathBuf::from)
                .to_vec(),
        )
    }

    pub fn under(data: &Path, plugin_roots: Vec<PathBuf>) -> Self {
        Self {
            nautilus_extension: data.join("nautilus-python/extensions/compress_images.py"),
            nautilus_script: data.join("nautilus/scripts").join(MENU_LABEL),
            dolphin: data.join("kio/servicemenus/compress-images.desktop"),
            nemo: data.join("nemo/actions/compress-images.nemo_action"),
            plugin_roots,
        }
    }

    fn all(&self) -> [(&Path, &'static str); 4] {
        [
            (&self.nautilus_extension, "Files (Nautilus)"),
            (&self.nautilus_script, "Files (Nautilus)"),
            (&self.dolphin, "Dolphin"),
            (&self.nemo, "Nemo"),
        ]
    }
}

/// Looks for `…/nautilus/extensions-*/libnautilus-python.so` up to one arch dir deep
/// (e.g. `/usr/lib/x86_64-linux-gnu/nautilus/extensions-4/`).
pub fn has_nautilus_python(roots: &[PathBuf]) -> bool {
    let check = |nautilus_dir: &Path| -> bool {
        let Ok(entries) = std::fs::read_dir(nautilus_dir) else {
            return false;
        };
        entries.flatten().any(|e| {
            e.file_name().to_string_lossy().starts_with("extensions-")
                && e.path().join("libnautilus-python.so").is_file()
        })
    };
    roots.iter().any(|root| {
        check(&root.join("nautilus"))
            || std::fs::read_dir(root)
                .map(|it| it.flatten().any(|e| check(&e.path().join("nautilus"))))
                .unwrap_or(false)
    })
}

/// Desktop Entry `Exec` quoting: double quotes, with `" ` $ \` escaped (twice for `\`,
/// because the key-file layer unescapes once more) and `%` doubled.
pub fn desktop_exec_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '`' | '$' => {
                out.push_str("\\\\");
                out.push(c);
            }
            '\\' => out.push_str("\\\\\\\\"),
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn dolphin_service_menu(launcher: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Service\n\
         MimeType=image/jpeg;image/png;image/webp;image/avif;image/tiff;image/bmp;image/heic;image/heif;inode/directory;\n\
         Actions=compressImages\n\
         X-KDE-Priority=TopLevel\n\
         \n\
         [Desktop Action compressImages]\n\
         Name={MENU_LABEL}\n\
         Icon=compress-images-app\n\
         Exec={} --compress %F\n",
        desktop_exec_quote(launcher)
    )
}

pub fn nemo_action(launcher: &str) -> String {
    // Nemo splits Exec with shell rules, so POSIX single quotes are safe here.
    format!(
        "[Nemo Action]\n\
         Name={MENU_LABEL}\n\
         Comment=Compress the selected images into a compressed/ folder\n\
         Exec={} --compress %F\n\
         Icon-Name=compress-images-app\n\
         Selection=notnone\n\
         Mimetypes=image/*;inode/directory;\n",
        shell_quote(launcher)
    )
}

pub fn nautilus_script(launcher: &str) -> String {
    format!(
        "#!/bin/sh\n\
         # Installed by Compress Images. Nautilus passes the selection (one path per line)\n\
         # in NAUTILUS_SCRIPT_SELECTED_FILE_PATHS; fall back to the arguments, then the folder.\n\
         APP={}\n\
         if [ -n \"$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS\" ]; then\n\
         \x20 IFS='\n'\n\
         \x20 set -f\n\
         \x20 exec \"$APP\" --compress $NAUTILUS_SCRIPT_SELECTED_FILE_PATHS\n\
         fi\n\
         [ $# -eq 0 ] && set -- \"$PWD\"\n\
         exec \"$APP\" --compress \"$@\"\n",
        shell_quote(launcher)
    )
}

pub fn nautilus_extension(launcher: &str) -> String {
    // A JSON string literal is also a valid Python string literal.
    let app = serde_json::to_string(launcher).unwrap_or_else(|_| "\"\"".into());
    let label = serde_json::to_string(MENU_LABEL).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"# Installed by Compress Images. Requires the nautilus-python plugin.
import subprocess

from gi import require_version

try:
    require_version("Nautilus", "4.0")
except ValueError:
    require_version("Nautilus", "3.0")
from gi.repository import GObject, Nautilus  # noqa: E402

APP = {app}
LABEL = {label}


def _paths(files):
    out = []
    for f in files:
        if f.get_uri_scheme() != "file":
            return []
        location = f.get_location()
        path = location.get_path() if location else None
        if path:
            out.append(path)
    return out


def _wanted(f):
    return f.is_directory() or (f.get_mime_type() or "").startswith("image/")


class CompressImagesMenu(GObject.GObject, Nautilus.MenuProvider):
    def _item(self, name, paths):
        item = Nautilus.MenuItem(name=name, label=LABEL, tip="Compress into a compressed/ folder")
        item.connect(
            "activate",
            lambda *_: subprocess.Popen([APP, "--compress", *paths], start_new_session=True),
        )
        return item

    # Nautilus 3 passes (window, files), Nautilus 4 passes (files): take the last argument.
    def get_file_items(self, *args):
        files = args[-1]
        if not files or not all(_wanted(f) for f in files):
            return []
        paths = _paths(files)
        return [self._item("CompressImages::Selection", paths)] if paths else []

    def get_background_items(self, *args):
        paths = _paths([args[-1]])
        return [self._item("CompressImages::Folder", paths)] if paths else []
"#
    )
}

pub fn install(layout: &Layout, launcher: &Path) -> io::Result<()> {
    let app = launcher.to_string_lossy();
    write_file(&layout.dolphin, &dolphin_service_menu(&app), true)?;
    write_file(&layout.nemo, &nemo_action(&app), false)?;
    if has_nautilus_python(&layout.plugin_roots) {
        write_file(&layout.nautilus_extension, &nautilus_extension(&app), false)?;
        remove_path(&layout.nautilus_script)?;
    } else {
        write_file(&layout.nautilus_script, &nautilus_script(&app), true)?;
        remove_path(&layout.nautilus_extension)?;
    }
    Ok(())
}

pub fn uninstall(layout: &Layout) -> io::Result<()> {
    for (path, _) in layout.all() {
        remove_path(path)?;
    }
    Ok(())
}

pub fn status(layout: &Layout, launcher: &Path) -> IntegrationStatus {
    let app = launcher.to_string_lossy();
    let mut installed = false;
    let mut outdated = false;
    let mut targets: Vec<String> = Vec::new();
    for (path, name) in layout.all() {
        if let Ok(content) = std::fs::read_to_string(path) {
            installed = true;
            // Every writer embeds the path verbatim except for rare escaped characters, so
            // compare on a path without them; good enough to notice a moved app.
            outdated |= !content.contains(app.as_ref());
            if !targets.iter().any(|t| t == name) {
                targets.push(name.to_string());
            }
        }
    }
    if targets.is_empty() {
        targets = vec!["Files (Nautilus)".into(), "Dolphin".into(), "Nemo".into()];
    }
    IntegrationStatus {
        supported: true,
        installed,
        outdated,
        targets,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = "/home/me/Applications/Compress_Images.AppImage";

    fn layout(dir: &Path) -> Layout {
        Layout::under(&dir.join("share"), vec![dir.join("usr/lib")])
    }

    #[test]
    fn desktop_quoting() {
        assert_eq!(desktop_exec_quote("/a b/c"), "\"/a b/c\"");
        assert_eq!(desktop_exec_quote("/a$b%c"), "\"/a\\\\$b%%c\"");
        assert!(dolphin_service_menu(APP).contains(&format!("Exec=\"{APP}\" --compress %F")));
        assert!(nemo_action(APP).contains(&format!("Exec='{APP}' --compress %F")));
    }

    #[test]
    fn python_literal_is_escaped() {
        let ext = nautilus_extension("/odd \"path\"/app");
        assert!(ext.contains(r#"APP = "/odd \"path\"/app""#));
        assert!(ext.contains("Nautilus.MenuProvider"));
    }

    #[test]
    fn installs_script_without_nautilus_python() {
        let dir = tempfile::tempdir().unwrap();
        let l = layout(dir.path());
        install(&l, Path::new(APP)).unwrap();
        assert!(l.dolphin.is_file() && l.nemo.is_file());
        assert!(l.nautilus_script.is_file());
        assert!(!l.nautilus_extension.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&l.nautilus_script)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "script must be executable");
        }
        let s = status(&l, Path::new(APP));
        assert!(s.installed && !s.outdated, "{s:?}");
        assert_eq!(s.targets, ["Files (Nautilus)", "Dolphin", "Nemo"]);
    }

    #[test]
    fn installs_extension_when_plugin_present() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir
            .path()
            .join("usr/lib/x86_64-linux-gnu/nautilus/extensions-4");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("libnautilus-python.so"), b"").unwrap();
        let l = layout(dir.path());
        assert!(has_nautilus_python(&l.plugin_roots));
        install(&l, Path::new(APP)).unwrap();
        assert!(l.nautilus_extension.is_file());
        assert!(!l.nautilus_script.exists());
    }

    #[test]
    fn outdated_and_uninstall() {
        let dir = tempfile::tempdir().unwrap();
        let l = layout(dir.path());
        install(&l, Path::new(APP)).unwrap();
        assert!(status(&l, Path::new("/opt/other/app")).outdated);
        uninstall(&l).unwrap();
        let s = status(&l, Path::new(APP));
        assert!(!s.installed);
        for (p, _) in l.all() {
            assert!(!p.exists(), "{}", p.display());
        }
    }

    /// Runs the generated Nautilus script with a fake app to check argument handling.
    #[cfg(unix)]
    #[test]
    fn nautilus_script_passes_selection() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake app.sh");
        let log = dir.path().join("args.txt");
        write_file(
            &fake,
            &format!(
                "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\"; done > {}\n",
                shell_quote(&log.to_string_lossy())
            ),
            true,
        )
        .unwrap();
        let script = dir.path().join("script.sh");
        write_file(&script, &nautilus_script(&fake.to_string_lossy()), true).unwrap();

        let status = std::process::Command::new(&script)
            .env(
                "NAUTILUS_SCRIPT_SELECTED_FILE_PATHS",
                "/p/a b.jpg\n/p/c*.png\n",
            )
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "--compress\n/p/a b.jpg\n/p/c*.png\n"
        );
    }
}
