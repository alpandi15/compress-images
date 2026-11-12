//! CLI integration tests (SPEC §4.1): exit codes, dry run, JSON output, safety.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dst = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dst);
        } else {
            fs::copy(e.path(), dst).unwrap();
        }
    }
}

fn workspace() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixtures(), dir.path());
    dir
}

fn bin() -> Command {
    Command::cargo_bin("compress-images").unwrap()
}

fn originals(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut v = Vec::new();
    for e in fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        if p.is_file() {
            v.push((p.clone(), fs::read(&p).unwrap()));
        }
    }
    v.sort();
    v
}

#[test]
fn compresses_folder_exit_0_and_keeps_originals() {
    let ws = workspace();
    let before = originals(ws.path());
    bin()
        .arg(ws.path())
        .assert()
        .code(0)
        .stdout(predicate::str::contains("Done:"));
    assert_eq!(originals(ws.path()), before);
    assert!(ws.path().join("compressed/photo.jpg").is_file());
    assert!(ws.path().join("compressed/sub/deep/nested.jpg").is_file());
}

#[test]
fn dry_run_writes_nothing() {
    let ws = workspace();
    bin()
        .args(["--dry-run", "-f", "webp"])
        .arg(ws.path())
        .assert()
        .code(0)
        .stdout(predicate::str::contains("photo.webp"))
        .stdout(predicate::str::contains("Nothing was written"));
    assert!(!ws.path().join("compressed").exists());
}

#[test]
fn usage_errors_exit_2() {
    let ws = workspace();
    bin().args(["-q", "0"]).arg(ws.path()).assert().code(2);
    bin().args(["-q", "101"]).arg(ws.path()).assert().code(2);
    bin().args(["-f", "gif"]).arg(ws.path()).assert().code(2);
    bin()
        .args(["--out-dir-name", "a/b"])
        .arg(ws.path())
        .assert()
        .code(2);
    bin().assert().code(2); // no paths
}

#[test]
fn missing_input_only_exits_3() {
    let ws = workspace();
    bin()
        .arg(ws.path().join("does-not-exist"))
        .assert()
        .code(3)
        .stderr(predicate::str::contains("does-not-exist"));
}

#[test]
fn missing_input_alongside_valid_one_exits_1() {
    let ws = workspace();
    bin()
        .arg(ws.path().join("photo.jpg"))
        .arg(ws.path().join("does-not-exist"))
        .assert()
        .code(1);
    assert!(ws.path().join("compressed/photo.jpg").is_file());
}

#[test]
fn corrupt_file_exits_1_but_processes_others() {
    let ws = workspace();
    let jpg = fs::read(ws.path().join("photo.jpg")).unwrap();
    fs::write(ws.path().join("broken.jpg"), &jpg[..300]).unwrap();
    bin()
        .arg(ws.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("broken.jpg"));
    assert!(ws.path().join("compressed/photo.jpg").is_file());
}

#[test]
fn json_output_is_one_object_per_line() {
    let ws = workspace();
    let out = bin()
        .arg("--json")
        .arg(ws.path().join("photo.jpg"))
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let lines: Vec<serde_json::Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("valid JSON line"))
        .collect();
    assert_eq!(lines.first().unwrap()["event"], "started");
    let done = lines.iter().find(|v| v["event"] == "file_done").unwrap();
    assert_eq!(done["outcome"]["status"], "compressed");
    let last = lines.last().unwrap();
    assert_eq!(last["event"], "finished");
    assert_eq!(last["summary"]["compressed"], 1);
}

#[test]
fn preset_and_format_flags() {
    let ws = workspace();
    bin()
        .args(["-p", "aggressive", "-f", "jpeg", "--max-width", "50"])
        .arg(ws.path().join("photo.png"))
        .assert()
        .code(0);
    let img = image_dims(&ws.path().join("compressed/photo.jpg"));
    assert_eq!(img, (50, 38));
}

#[test]
fn skip_policy_leaves_existing_output() {
    let ws = workspace();
    fs::create_dir_all(ws.path().join("compressed")).unwrap();
    fs::write(ws.path().join("compressed/photo.jpg"), b"keep me").unwrap();
    bin()
        .args(["--on-conflict", "skip", "-v"])
        .arg(ws.path().join("photo.jpg"))
        .assert()
        .code(0)
        .stdout(predicate::str::contains("output already exists"));
    assert_eq!(
        fs::read(ws.path().join("compressed/photo.jpg")).unwrap(),
        b"keep me"
    );
}

/// Reads width/height from a baseline/progressive JPEG SOF marker (no image crate needed).
fn image_dims(path: &Path) -> (u32, u32) {
    let d = fs::read(path).unwrap();
    let mut i = 2;
    while i + 9 < d.len() {
        assert_eq!(d[i], 0xFF);
        let marker = d[i + 1];
        let len = u16::from_be_bytes([d[i + 2], d[i + 3]]) as usize;
        if (0xC0..=0xC2).contains(&marker) {
            let h = u16::from_be_bytes([d[i + 5], d[i + 6]]) as u32;
            let w = u16::from_be_bytes([d[i + 7], d[i + 8]]) as u32;
            return (w, h);
        }
        i += 2 + len;
    }
    panic!("no SOF marker in {}", path.display());
}
