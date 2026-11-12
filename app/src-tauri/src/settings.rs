//! Persistent user settings stored as TOML in the app config dir (SPEC §4.4).
//!
//! Loading is lenient: unknown keys are ignored and each invalid value falls back to its
//! default individually, so one bad entry never resets everything else.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use compress_core::{Options, Preset};
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

pub const FILE_NAME: &str = "config.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// Follow the OS language (Indonesian if it starts with `id`, otherwise English).
    #[default]
    Auto,
    En,
    Id,
}

/// Quality preset selected in the UI. `Custom` is an explicit variant (not `Option::None`)
/// because TOML cannot store `None`: an omitted key would load back as the default preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetChoice {
    Light,
    #[default]
    Balanced,
    Aggressive,
    Custom,
}

impl PresetChoice {
    pub fn preset(self) -> Option<Preset> {
        match self {
            PresetChoice::Light => Some(Preset::Light),
            PresetChoice::Balanced => Some(Preset::Balanced),
            PresetChoice::Aggressive => Some(Preset::Aggressive),
            PresetChoice::Custom => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub options: Options,
    pub preset: PresetChoice,
    pub language: Language,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            options: Options::default(),
            preset: PresetChoice::Balanced,
            language: Language::Auto,
        }
    }
}

impl Settings {
    fn is_valid(&self) -> bool {
        self.options.validate().is_ok()
    }

    /// A named preset must match the stored quality. If the file was edited by hand and they
    /// disagree, the explicit number wins and the preset becomes `Custom`.
    fn normalized(mut self) -> Self {
        if self
            .preset
            .preset()
            .is_some_and(|p| p.quality() != self.options.quality)
        {
            self.preset = PresetChoice::Custom;
        }
        self
    }
}

/// Loads settings, returning warnings for anything that had to be ignored.
pub fn load(path: &Path) -> (Settings, Vec<String>) {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Settings::default(), vec![]),
        Err(e) => {
            return (
                Settings::default(),
                vec![format!("could not read {}: {e}", path.display())],
            )
        }
    };
    parse(&text)
}

/// Lenient parse (see module docs).
pub fn parse(text: &str) -> (Settings, Vec<String>) {
    let user: Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            return (
                Settings::default(),
                vec![format!("invalid settings file: {e}")],
            )
        }
    };
    let mut merged = match Table::try_from(Settings::default()) {
        Ok(t) => t,
        Err(e) => return (Settings::default(), vec![e.to_string()]),
    };
    let mut warnings = Vec::new();
    overlay(&mut merged, &[], &user, &mut warnings);
    let settings = to_settings(&merged).unwrap_or_default().normalized();
    (settings, warnings)
}

fn to_settings(t: &Table) -> Option<Settings> {
    let s = Settings::deserialize(Value::Table(t.clone())).ok()?;
    s.is_valid().then_some(s)
}

/// Applies `user` onto `root` one leaf at a time, keeping only values that leave the whole
/// document valid.
fn overlay(root: &mut Table, path: &[String], user: &Table, warnings: &mut Vec<String>) {
    for (key, value) in user {
        let mut p = path.to_vec();
        p.push(key.clone());
        let base_is_table = get_at(root, &p).is_some_and(Value::is_table);
        if let (true, Value::Table(sub)) = (base_is_table, value) {
            overlay(root, &p, sub, warnings);
            continue;
        }
        let mut candidate = root.clone();
        set_at(&mut candidate, &p, value.clone());
        if to_settings(&candidate).is_some() {
            *root = candidate;
        } else {
            warnings.push(format!("ignoring invalid setting `{}`", p.join(".")));
        }
    }
}

fn get_at<'a>(t: &'a Table, path: &[String]) -> Option<&'a Value> {
    let (last, parents) = path.split_last()?;
    let mut cur = t;
    for p in parents {
        cur = cur.get(p)?.as_table()?;
    }
    cur.get(last)
}

fn set_at(t: &mut Table, path: &[String], value: Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut cur = t;
    for p in parents {
        let entry = cur
            .entry(p.clone())
            .or_insert_with(|| Value::Table(Table::new()));
        if !entry.is_table() {
            *entry = Value::Table(Table::new());
        }
        let Value::Table(next) = entry else { return };
        cur = next;
    }
    cur.insert(last.clone(), value);
}

/// Validates and writes settings atomically.
pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    settings.options.validate().map_err(|e| e.to_string())?;
    let text = toml::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let dir = path
        .parent()
        .ok_or_else(|| "settings path has no parent folder".to_string())?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".config.")
        .suffix(".tmp")
        .tempfile_in(dir)
        .map_err(|e| e.to_string())?;
    tmp.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    tmp.as_file().sync_all().map_err(|e| e.to_string())?;
    tmp.persist(path).map_err(|e| e.error.to_string())?;
    Ok(())
}

pub fn path_in(config_dir: PathBuf) -> PathBuf {
    config_dir.join(FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use compress_core::{OutputFormat, Quality, Resize};

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let (s, w) = load(&dir.path().join("nope.toml"));
        assert_eq!(s, Settings::default());
        assert!(w.is_empty());
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/config.toml");
        let s = Settings {
            options: Options {
                quality: Quality::new(42).unwrap(),
                output_format: OutputFormat::Webp,
                recursive: false,
                resize: Resize {
                    max_width: Some(1920),
                    max_height: None,
                },
                jobs: Some(3),
                ..Options::default()
            },
            preset: PresetChoice::Custom,
            language: Language::Id,
        };
        save(&path, &s).unwrap();
        let (loaded, w) = load(&path);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(loaded, s);
    }

    #[test]
    fn invalid_values_fall_back_individually() {
        let (s, w) = parse(
            r#"
            language = "id"
            unknown_key = 1
            [options]
            quality = 0
            output_format = "webp"
            avif_speed = 99
            out_dir_name = "min"
            [options.resize]
            max_width = 800
            "#,
        );
        assert_eq!(s.language, Language::Id);
        assert_eq!(s.options.quality, Quality::default());
        assert_eq!(s.options.output_format, OutputFormat::Webp);
        assert_eq!(s.options.avif_speed, 6);
        assert_eq!(s.options.out_dir_name, "min");
        assert_eq!(s.options.resize.max_width, Some(800));
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w.iter().any(|m| m.contains("options.quality")));
        assert!(w.iter().any(|m| m.contains("options.avif_speed")));
    }

    #[test]
    fn preset_conflicting_with_quality_is_ignored() {
        let (s, w) = parse(
            r#"
            preset = "light"
            [options]
            quality = 33
            "#,
        );
        // The explicit number wins; "light" (85) would contradict it.
        assert_eq!(s.options.quality, Quality::new(33).unwrap());
        assert_eq!(s.preset, PresetChoice::Custom);
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn garbage_file_gives_defaults_with_warning() {
        let (s, w) = parse("this is = = not toml");
        assert_eq!(s, Settings::default());
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn save_rejects_invalid_options() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        s.options.out_dir_name = "a/b".into();
        assert!(save(&dir.path().join("config.toml"), &s).is_err());
        assert!(!dir.path().join("config.toml").exists());
    }
}
