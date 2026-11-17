//! Finder Quick Action: an Automator workflow in `~/Library/Services` that runs the app with
//! `--compress` and the selected files/folders.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::io;
use std::path::{Path, PathBuf};

use super::{remove_path, shell_quote, write_file, IntegrationStatus, MENU_LABEL};

pub const WORKFLOW_DIR: &str = "Compress Images.workflow";

pub fn services_dir(home: &Path) -> PathBuf {
    home.join("Library/Services")
}

/// `/Applications/X.app` for `/Applications/X.app/Contents/MacOS/bin`, if inside a bundle.
pub fn app_bundle(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app";
    is_bundle.then(|| bundle.to_path_buf())
}

/// What gets embedded in the workflow to identify the installed copy of the app.
fn target_path(launcher: &Path) -> PathBuf {
    app_bundle(launcher).unwrap_or_else(|| launcher.to_path_buf())
}

/// Shell script run by the Quick Action; `"$@"` are the selected paths.
pub fn script(launcher: &Path) -> String {
    match app_bundle(launcher) {
        // `open -n` returns immediately (no spinning gear in the menu bar) and launches through
        // LaunchServices; the new process forwards to a running instance if there is one.
        Some(bundle) => format!(
            "open -n {} --args --compress \"$@\"\n",
            shell_quote(&bundle.to_string_lossy())
        ),
        // Unbundled dev build: start the binary directly, in the background.
        None => format!(
            "{} --compress \"$@\" >/dev/null 2>&1 &\n",
            shell_quote(&launcher.to_string_lossy())
        ),
    }
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Declares the service: Finder only, for images and folders.
pub fn info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>NSServices</key>
	<array>
		<dict>
			<key>NSBackgroundColorName</key>
			<string>background</string>
			<key>NSIconName</key>
			<string>NSActionTemplate</string>
			<key>NSMenuItem</key>
			<dict>
				<key>default</key>
				<string>{label}</string>
			</dict>
			<key>NSMessage</key>
			<string>runWorkflowAsService</string>
			<key>NSRequiredContext</key>
			<dict>
				<key>NSApplicationIdentifier</key>
				<string>com.apple.finder</string>
			</dict>
			<key>NSSendFileTypes</key>
			<array>
				<string>public.image</string>
				<string>public.folder</string>
			</array>
		</dict>
	</array>
</dict>
</plist>
"#,
        label = xml_escape(MENU_LABEL)
    )
}

/// Automator document with a single "Run Shell Script" action (input passed as arguments).
pub fn document_wflow(script: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AMApplicationBuild</key>
	<string>523</string>
	<key>AMApplicationVersion</key>
	<string>2.10</string>
	<key>AMDocumentVersion</key>
	<string>2</string>
	<key>actions</key>
	<array>
		<dict>
			<key>action</key>
			<dict>
				<key>AMAccepts</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Optional</key>
					<true/>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>AMActionVersion</key>
				<string>2.0.3</string>
				<key>AMApplication</key>
				<array>
					<string>Automator</string>
				</array>
				<key>AMParameterProperties</key>
				<dict>
					<key>COMMAND_STRING</key>
					<dict/>
					<key>CheckedForUserDefaultShell</key>
					<dict/>
					<key>inputMethod</key>
					<dict/>
					<key>shell</key>
					<dict/>
					<key>source</key>
					<dict/>
				</dict>
				<key>AMProvides</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>ActionBundlePath</key>
				<string>/System/Library/Automator/Run Shell Script.action</string>
				<key>ActionName</key>
				<string>Run Shell Script</string>
				<key>ActionParameters</key>
				<dict>
					<key>COMMAND_STRING</key>
					<string>{script}</string>
					<key>CheckedForUserDefaultShell</key>
					<true/>
					<key>inputMethod</key>
					<integer>1</integer>
					<key>shell</key>
					<string>/bin/sh</string>
					<key>source</key>
					<string></string>
				</dict>
				<key>BundleIdentifier</key>
				<string>com.apple.RunShellScript</string>
				<key>CFBundleVersion</key>
				<string>2.0.3</string>
				<key>CanShowSelectedItemsWhenRun</key>
				<false/>
				<key>CanShowWhenRun</key>
				<true/>
				<key>Category</key>
				<array>
					<string>AMCategoryUtilities</string>
				</array>
				<key>Class Name</key>
				<string>RunShellScriptAction</string>
				<key>InputUUID</key>
				<string>6D7C2A43-0F4B-4E0F-9C1E-3B2A5D8E7F01</string>
				<key>Keywords</key>
				<array>
					<string>Shell</string>
					<string>Script</string>
					<string>Command</string>
					<string>Run</string>
					<string>Unix</string>
				</array>
				<key>OutputUUID</key>
				<string>6D7C2A43-0F4B-4E0F-9C1E-3B2A5D8E7F02</string>
				<key>UUID</key>
				<string>6D7C2A43-0F4B-4E0F-9C1E-3B2A5D8E7F03</string>
				<key>UnlocalizedApplications</key>
				<array>
					<string>Automator</string>
				</array>
				<key>arguments</key>
				<dict/>
				<key>isViewVisible</key>
				<integer>1</integer>
				<key>location</key>
				<string>309.000000:253.000000</string>
				<key>nibPath</key>
				<string>/System/Library/Automator/Run Shell Script.action/Contents/Resources/Base.lproj/main.nib</string>
			</dict>
			<key>isViewVisible</key>
			<integer>1</integer>
		</dict>
	</array>
	<key>connectors</key>
	<dict/>
	<key>workflowMetaData</key>
	<dict>
		<key>applicationBundleIDsByPath</key>
		<dict/>
		<key>applicationPaths</key>
		<array/>
		<key>inputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>outputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>presentationMode</key>
		<integer>15</integer>
		<key>processesInput</key>
		<false/>
		<key>serviceApplicationBundleID</key>
		<string>com.apple.finder</string>
		<key>serviceApplicationPath</key>
		<string>/System/Library/CoreServices/Finder.app</string>
		<key>serviceInputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>serviceOutputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>serviceProcessesInput</key>
		<false/>
		<key>systemImageName</key>
		<string>NSActionTemplate</string>
		<key>useAutomaticInputType</key>
		<false/>
		<key>workflowTypeIdentifier</key>
		<string>com.apple.Automator.servicesMenu</string>
	</dict>
</dict>
</plist>
"#,
        script = xml_escape(script)
    )
}

pub fn install(services: &Path, launcher: &Path) -> io::Result<()> {
    let wf = services.join(WORKFLOW_DIR);
    remove_path(&wf)?; // start clean so no stale files linger
    write_file(&wf.join("Contents/Info.plist"), &info_plist(), false)?;
    write_file(
        &wf.join("Contents/document.wflow"),
        &document_wflow(&script(launcher)),
        false,
    )
}

pub fn uninstall(services: &Path) -> io::Result<()> {
    remove_path(&services.join(WORKFLOW_DIR))
}

pub fn status(services: &Path, launcher: &Path) -> IntegrationStatus {
    let doc = std::fs::read_to_string(services.join(WORKFLOW_DIR).join("Contents/document.wflow"));
    let target = xml_escape(&target_path(launcher).to_string_lossy());
    IntegrationStatus {
        supported: true,
        installed: doc.is_ok(),
        outdated: doc.is_ok_and(|d| !d.contains(&target)),
        targets: vec!["Finder".into()],
    }
}

/// Makes Finder pick up added/removed services without logging out.
pub fn refresh_services() {
    let _ = std::process::Command::new("/System/Library/CoreServices/pbs")
        .arg("-flush")
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP_EXE: &str = "/Applications/Compress Images.app/Contents/MacOS/compress-images-app";

    #[test]
    fn detects_bundle() {
        assert_eq!(
            app_bundle(Path::new(APP_EXE)),
            Some(PathBuf::from("/Applications/Compress Images.app"))
        );
        assert_eq!(
            app_bundle(Path::new("/repo/target/debug/compress-images-app")),
            None
        );
    }

    #[test]
    fn script_uses_open_for_bundles_and_direct_exec_otherwise() {
        assert_eq!(
            script(Path::new(APP_EXE)),
            "open -n '/Applications/Compress Images.app' --args --compress \"$@\"\n"
        );
        assert!(script(Path::new("/dev/it's/app")).starts_with(r"'/dev/it'\''s/app' --compress"));
    }

    #[test]
    fn workflow_embeds_escaped_script() {
        let doc = document_wflow("open -n '/A & B.app' --args --compress \"$@\"\n");
        assert!(doc.contains("open -n '/A &amp; B.app' --args --compress &quot;$@&quot;"));
        assert!(
            doc.contains("<integer>1</integer>"),
            "input passed as arguments"
        );
        assert!(info_plist().contains("<string>com.apple.finder</string>"));
    }

    #[test]
    fn install_status_uninstall() {
        let dir = tempfile::tempdir().unwrap();
        let services = dir.path().join("Library/Services");
        let exe = Path::new(APP_EXE);

        assert!(!status(&services, exe).installed);
        install(&services, exe).unwrap();
        let s = status(&services, exe);
        assert!(s.installed && !s.outdated, "{s:?}");
        assert!(services
            .join(WORKFLOW_DIR)
            .join("Contents/Info.plist")
            .is_file());

        let moved =
            Path::new("/Users/me/Apps/Compress Images.app/Contents/MacOS/compress-images-app");
        assert!(status(&services, moved).outdated);

        uninstall(&services).unwrap();
        assert!(!status(&services, exe).installed);
        assert!(!services.join(WORKFLOW_DIR).exists());
    }

    /// Writes a workflow for manual testing without touching `~/Library/Services`:
    /// `WORKFLOW_OUT=/tmp/x WORKFLOW_LAUNCHER=<exe> cargo test -p compress-images-app -- --ignored export_workflow`
    /// then `automator -i <image> "/tmp/x/Compress Images.workflow"`.
    #[test]
    #[ignore = "manual check helper"]
    fn export_workflow() {
        let out = std::env::var_os("WORKFLOW_OUT").expect("set WORKFLOW_OUT");
        let launcher = std::env::var_os("WORKFLOW_LAUNCHER").expect("set WORKFLOW_LAUNCHER");
        install(Path::new(&out), Path::new(&launcher)).unwrap();
    }

    /// Validates the generated plists with the system parser (macOS only).
    #[cfg(target_os = "macos")]
    #[test]
    fn plists_are_valid() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), Path::new(APP_EXE)).unwrap();
        for f in ["Info.plist", "document.wflow"] {
            let p = dir.path().join(WORKFLOW_DIR).join("Contents").join(f);
            let out = std::process::Command::new("plutil")
                .arg("-lint")
                .arg(&p)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{f}: {}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
    }
}
