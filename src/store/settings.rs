use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::engine::{AudioFormat, AudioQuality, OutputPreset};

pub const MINIMUM_CONCURRENT_DOWNLOADS: u8 = 1;
pub const MAXIMUM_CONCURRENT_DOWNLOADS: u8 = 8;
const DEFAULT_CONCURRENT_DOWNLOADS: u8 = 3;
const OUTPUT_SUBDIRECTORY_NAME: &str = "Riptide";

/// User preferences. Unknown or missing fields fall back to defaults so old files keep loading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub output_directory: PathBuf,
    pub default_preset: OutputPreset,
    pub maximum_concurrent_downloads: u8,
    pub embed_metadata_and_cover: bool,
    pub theme_mode: ThemeMode,
}

/// Light / dark choice. `Auto` follows the desktop's appearance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Auto,
    Light,
    Dark,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 3] = [ThemeMode::Auto, ThemeMode::Light, ThemeMode::Dark];
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_directory: default_output_directory(),
            default_preset: OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Kbps320),
            maximum_concurrent_downloads: DEFAULT_CONCURRENT_DOWNLOADS,
            embed_metadata_and_cover: true,
            theme_mode: ThemeMode::Auto,
        }
    }
}

impl Settings {
    /// Concurrency clamped to the supported range (a hand-edited file may hold anything).
    pub fn clamped_concurrent_downloads(&self) -> u8 {
        self.maximum_concurrent_downloads
            .clamp(MINIMUM_CONCURRENT_DOWNLOADS, MAXIMUM_CONCURRENT_DOWNLOADS)
    }
}

/// `XDG_MUSIC_DIR/Riptide`, else `~/Music/Riptide`, else `./Riptide`.
pub fn default_output_directory() -> PathBuf {
    dirs::audio_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Music")))
        .unwrap_or_default()
        .join(OUTPUT_SUBDIRECTORY_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{VideoFormat, VideoQuality};
    use crate::store::test_support::TemporaryDirectory;
    use crate::store::{load_settings, save_settings};

    #[test]
    fn defaults_match_product_spec() {
        let settings = Settings::default();
        assert_eq!(
            settings.default_preset,
            OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Kbps320)
        );
        assert_eq!(settings.maximum_concurrent_downloads, 3);
        assert!(settings.embed_metadata_and_cover);
        assert!(settings.output_directory.ends_with("Riptide"));
        assert_eq!(settings.theme_mode, ThemeMode::Auto);
    }

    #[test]
    fn settings_round_trip_through_disk() {
        let directory = TemporaryDirectory::new();
        let path = directory.0.join("settings.json");
        let settings = Settings {
            output_directory: PathBuf::from("/tmp/somewhere"),
            default_preset: OutputPreset::Video(VideoFormat::Webm, VideoQuality::P720),
            maximum_concurrent_downloads: 5,
            embed_metadata_and_cover: false,
            theme_mode: ThemeMode::Dark,
        };
        save_settings(&path, &settings).expect("save");
        assert_eq!(load_settings(&path), settings);
    }

    #[test]
    fn partial_file_keeps_known_fields_and_defaults_the_rest() {
        let directory = TemporaryDirectory::new();
        let path = directory.0.join("settings.json");
        std::fs::write(&path, br#"{"maximum_concurrent_downloads": 6}"#).expect("write");
        let settings = load_settings(&path);
        assert_eq!(settings.maximum_concurrent_downloads, 6);
        assert!(settings.embed_metadata_and_cover);
    }

    #[test]
    fn concurrency_is_clamped() {
        let settings = Settings {
            maximum_concurrent_downloads: 0,
            ..Settings::default()
        };
        assert_eq!(settings.clamped_concurrent_downloads(), 1);
        let settings = Settings {
            maximum_concurrent_downloads: 200,
            ..Settings::default()
        };
        assert_eq!(settings.clamped_concurrent_downloads(), 8);
    }
}
