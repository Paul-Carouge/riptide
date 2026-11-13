use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::engine::OutputPreset;

/// Oldest entries beyond this count are dropped so the file stays small.
pub const MAXIMUM_HISTORY_ENTRIES: usize = 500;

/// One finished download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub title: String,
    pub uploader: Option<String>,
    pub preset: OutputPreset,
    pub output_file: PathBuf,
    pub thumbnail_url: Option<String>,
    pub finished_at_unix_seconds: u64,
}

/// Inserts `entry` first (newest first) and drops entries beyond the cap.
pub fn add_history_entry(history: &mut Vec<HistoryEntry>, entry: HistoryEntry) {
    history.insert(0, entry);
    history.truncate(MAXIMUM_HISTORY_ENTRIES);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{AudioFormat, AudioQuality};
    use crate::store::test_support::TemporaryDirectory;
    use crate::store::{load_history, save_history};

    fn sample_entry(title: &str) -> HistoryEntry {
        HistoryEntry {
            title: title.to_owned(),
            uploader: Some("jawed".to_owned()),
            preset: OutputPreset::Audio(AudioFormat::Flac, AudioQuality::Best),
            output_file: PathBuf::from("/music/Riptide/me-at-the-zoo.flac"),
            thumbnail_url: None,
            finished_at_unix_seconds: 1_700_000_000,
        }
    }

    #[test]
    fn history_round_trip_through_disk() {
        let directory = TemporaryDirectory::new();
        let path = directory.0.join("history.json");
        let history = vec![sample_entry("Me at the zoo"), sample_entry("Café « olé »")];
        save_history(&path, &history).expect("save");
        assert_eq!(load_history(&path), history);
    }

    #[test]
    fn newest_entry_comes_first_and_history_is_capped() {
        let mut history = Vec::new();
        for index in 0..MAXIMUM_HISTORY_ENTRIES + 3 {
            add_history_entry(&mut history, sample_entry(&index.to_string()));
        }
        assert_eq!(history.len(), MAXIMUM_HISTORY_ENTRIES);
        assert_eq!(history[0].title, (MAXIMUM_HISTORY_ENTRIES + 2).to_string());
    }
}
