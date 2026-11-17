<!-- Draft created by the release workflow. Add the changes for this version above the line. -->

---

## Downloads

| System | File |
|---|---|
| macOS 10.15+ (Apple Silicon and Intel) | `Compress-Images-<version>-macos-universal.dmg` |
| Windows 10/11 (x64) | `…-windows-x64-setup.exe` (recommended) or `…-windows-x64.msi` |
| Ubuntu 24.04+ / Debian 13+ | `compress-images_<version>_amd64.deb` |
| Other Linux (x86_64) | `Compress-Images-<version>-x86_64.AppImage` |
| Command line only | `compress-images-cli-<version>-<system>.tar.gz` / `.zip` |

## The app isn't signed yet

- **macOS:** after copying the app to *Applications*, right-click it → **Open** → **Open**.
  If macOS says the app "is damaged", run
  `xattr -dr com.apple.quarantine "/Applications/Compress Images.app"` once.
- **Windows:** SmartScreen may warn about an unknown publisher: **More info** → **Run anyway**.

The Windows `setup.exe` adds the right-click menu automatically. On macOS, Linux, and with
the `.msi`, open **Settings → Right-click menu → Add to menu** once.

Licenses of bundled third-party code: `THIRD_PARTY_LICENSES.html`.
