use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// Compression quality, always within `1..=100`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Quality(u8);

impl Quality {
    pub const MIN: u8 = 1;
    pub const MAX: u8 = 100;

    pub fn new(value: u8) -> Result<Self> {
        if (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(CoreError::InvalidOption(format!(
                "quality must be between {} and {}, got {value}",
                Self::MIN,
                Self::MAX
            )))
        }
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

impl Default for Quality {
    fn default() -> Self {
        Preset::Balanced.quality()
    }
}

impl TryFrom<u8> for Quality {
    type Error = CoreError;
    fn try_from(value: u8) -> Result<Self> {
        Self::new(value)
    }
}

impl From<Quality> for u8 {
    fn from(q: Quality) -> u8 {
        q.0
    }
}

/// Named quality presets shown in the UI (SPEC §3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Light,
    Balanced,
    Aggressive,
}

impl Preset {
    pub fn quality(self) -> Quality {
        Quality(match self {
            Preset::Light => 85,
            Preset::Balanced => 70,
            Preset::Aggressive => 50,
        })
    }
}

/// Requested output format. `Same` keeps the input format, except BMP → PNG and HEIC → JPEG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    #[default]
    Same,
    Jpeg,
    Png,
    Webp,
    Avif,
}

/// What to do when the destination file already exists on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    #[default]
    Overwrite,
    Skip,
    Rename,
}

/// Optional downscaling bounds. Aspect ratio is preserved and images are never upscaled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Resize {
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
}

impl Resize {
    pub fn is_noop(&self) -> bool {
        self.max_width.is_none() && self.max_height.is_none()
    }

    /// Target dimensions for an image of `w`×`h`, or `None` if it already fits.
    pub fn target_size(&self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w == 0 || h == 0 {
            return None;
        }
        let sx = self
            .max_width
            .map_or(1.0, |mw| f64::from(mw) / f64::from(w));
        let sy = self
            .max_height
            .map_or(1.0, |mh| f64::from(mh) / f64::from(h));
        let scale = sx.min(sy);
        if scale >= 1.0 {
            return None;
        }
        let nw = ((f64::from(w) * scale).round() as u32).max(1);
        let nh = ((f64::from(h) * scale).round() as u32).max(1);
        Some((nw, nh))
    }
}

/// All settings for a compression job (SPEC §6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub quality: Quality,
    pub output_format: OutputFormat,
    pub recursive: bool,
    pub resize: Resize,
    pub keep_metadata: bool,
    pub out_dir_name: String,
    pub on_conflict: ConflictPolicy,
    /// Worker threads; `None` = number of CPUs.
    pub jobs: Option<usize>,
    /// AVIF encoder speed, 1 (slow, smallest) ..= 10 (fast).
    pub avif_speed: u8,
}

pub const DEFAULT_OUT_DIR_NAME: &str = "compressed";

impl Default for Options {
    fn default() -> Self {
        Self {
            quality: Quality::default(),
            output_format: OutputFormat::Same,
            recursive: true,
            resize: Resize::default(),
            keep_metadata: false,
            out_dir_name: DEFAULT_OUT_DIR_NAME.to_owned(),
            on_conflict: ConflictPolicy::Overwrite,
            jobs: None,
            avif_speed: 6,
        }
    }
}

impl Options {
    /// Checks invariants that the type system can't express.
    pub fn validate(&self) -> Result<()> {
        let name = self.out_dir_name.as_str();
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains(['/', '\\'])
            || name.trim() != name
        {
            return Err(CoreError::InvalidOption(format!(
                "output folder name must be a plain folder name, got {name:?}"
            )));
        }
        if !(1..=10).contains(&self.avif_speed) {
            return Err(CoreError::InvalidOption(format!(
                "AVIF speed must be between 1 and 10, got {}",
                self.avif_speed
            )));
        }
        if self.resize.max_width == Some(0) || self.resize.max_height == Some(0) {
            return Err(CoreError::InvalidOption(
                "resize bounds must be at least 1 pixel".into(),
            ));
        }
        if self.jobs == Some(0) {
            return Err(CoreError::InvalidOption("jobs must be at least 1".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_bounds() {
        assert!(Quality::new(0).is_err());
        assert!(Quality::new(101).is_err());
        assert_eq!(Quality::new(1).unwrap().get(), 1);
        assert_eq!(Quality::new(100).unwrap().get(), 100);
    }

    #[test]
    fn presets() {
        assert_eq!(Preset::Light.quality().get(), 85);
        assert_eq!(Preset::Balanced.quality().get(), 70);
        assert_eq!(Preset::Aggressive.quality().get(), 50);
        assert_eq!(Quality::default().get(), 70);
    }

    #[test]
    fn resize_target() {
        let r = Resize {
            max_width: Some(100),
            max_height: None,
        };
        assert_eq!(r.target_size(400, 200), Some((100, 50)));
        assert_eq!(r.target_size(80, 200), None, "never upscale");

        let r = Resize {
            max_width: Some(100),
            max_height: Some(100),
        };
        assert_eq!(r.target_size(400, 200), Some((100, 50)));
        assert_eq!(r.target_size(200, 400), Some((50, 100)));
        assert_eq!(Resize::default().target_size(10, 10), None);
    }

    #[test]
    fn validate_out_dir_name() {
        for bad in ["", ".", "..", "a/b", "a\\b", " x"] {
            let o = Options {
                out_dir_name: bad.into(),
                ..Options::default()
            };
            assert!(o.validate().is_err(), "{bad:?} should be rejected");
        }
        assert!(Options::default().validate().is_ok());
    }
}
