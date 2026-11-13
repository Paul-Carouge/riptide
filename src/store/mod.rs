//! Persistence of user settings and download history as JSON files.
//!
//! Reads never fail: a missing file falls back to defaults; a corrupt one is set aside as
//! `<file>.corrupt-<unix seconds>` (so nothing is silently overwritten) and defaults are used.
//! Writes are atomic (unique temp file, `sync_all`, rename) and serialized per file by
//! [`LatestWriteWins`], so an older snapshot can never overwrite a newer one.

mod history;
mod settings;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;

pub use history::{HistoryEntry, add_history_entry};
pub use settings::{
    MAXIMUM_CONCURRENT_DOWNLOADS, MINIMUM_CONCURRENT_DOWNLOADS, Settings, ThemeMode,
};

const APPLICATION_DIRECTORY_NAME: &str = "riptide";
const SETTINGS_FILE_NAME: &str = "settings.json";
const HISTORY_FILE_NAME: &str = "history.json";

/// `$XDG_CONFIG_HOME/riptide/settings.json`, or `None` when no home directory is known.
pub fn settings_file_path() -> Option<PathBuf> {
    dirs::config_dir().map(|directory| {
        directory
            .join(APPLICATION_DIRECTORY_NAME)
            .join(SETTINGS_FILE_NAME)
    })
}

/// `$XDG_DATA_HOME/riptide/history.json`, or `None` when no home directory is known.
pub fn history_file_path() -> Option<PathBuf> {
    dirs::data_dir().map(|directory| {
        directory
            .join(APPLICATION_DIRECTORY_NAME)
            .join(HISTORY_FILE_NAME)
    })
}

/// Everything read from disk at startup. Blocking: `main` loads it before the event loop
/// starts, so the UI thread never waits on the disk.
pub struct PersistedState {
    pub settings: Settings,
    pub history: Vec<HistoryEntry>,
    pub settings_path: Option<PathBuf>,
    pub history_path: Option<PathBuf>,
}

pub fn load_persisted_state() -> PersistedState {
    let settings_path = settings_file_path();
    let history_path = history_file_path();
    PersistedState {
        settings: settings_path
            .as_deref()
            .map(load_settings)
            .unwrap_or_default(),
        history: history_path
            .as_deref()
            .map(load_history)
            .unwrap_or_default(),
        settings_path,
        history_path,
    }
}

pub fn load_settings(path: &Path) -> Settings {
    read_json_or_default(path)
}

#[cfg(test)]
pub fn save_settings(path: &Path, settings: &Settings) -> io::Result<()> {
    write_json_atomically(path, settings)
}

pub fn load_history(path: &Path) -> Vec<HistoryEntry> {
    read_json_or_default(path)
}

#[cfg(test)]
pub fn save_history(path: &Path, history: &[HistoryEntry]) -> io::Result<()> {
    write_json_atomically(path, &history)
}

/// Serializes the writes of one file: each save takes a revision on the UI thread, and a
/// background write only happens if no newer revision was taken since (last revision wins).
pub struct LatestWriteWins {
    path: PathBuf,
    latest_revision: AtomicU64,
    write_lock: Mutex<()>,
}

impl LatestWriteWins {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            latest_revision: AtomicU64::new(0),
            write_lock: Mutex::new(()),
        }
    }

    /// Call on the UI thread, in the order the snapshots were taken.
    pub fn next_revision(&self) -> u64 {
        self.latest_revision.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Blocking. Writes `value` unless a newer revision exists; returns whether it wrote.
    pub fn write<T: Serialize + ?Sized>(&self, revision: u64, value: &T) -> io::Result<bool> {
        // A panic while holding the lock leaves no shared state to repair: keep going.
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if revision != self.latest_revision.load(Ordering::SeqCst) {
            return Ok(false);
        }
        write_json_atomically(&self.path, value)?;
        Ok(true)
    }
}

fn read_json_or_default<T: DeserializeOwned + Default>(path: &Path) -> T {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return T::default(),
        Err(error) => {
            eprintln!(
                "riptide: cannot read {}: {error}; using defaults",
                path.display()
            );
            return T::default();
        }
    };
    serde_json::from_slice(&contents).unwrap_or_else(|error| {
        let unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        let backup_path = corrupt_backup_path(path, unix_seconds);
        match fs::rename(path, &backup_path) {
            Ok(()) => eprintln!(
                "riptide: corrupt file {}: {error}; kept as {}, using defaults",
                path.display(),
                backup_path.display()
            ),
            Err(rename_error) => eprintln!(
                "riptide: corrupt file {}: {error}; cannot set it aside ({rename_error}), using defaults",
                path.display()
            ),
        }
        T::default()
    })
}

fn corrupt_backup_path(path: &Path, unix_seconds: u64) -> PathBuf {
    let mut backup = path.as_os_str().to_owned();
    backup.push(format!(".corrupt-{unix_seconds}"));
    PathBuf::from(backup)
}

fn write_json_atomically<T: Serialize + ?Sized>(path: &Path, value: &T) -> io::Result<()> {
    static TEMPORARY_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
    let directory = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    fs::create_dir_all(directory)?;
    let serialized = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let mut temporary_name = std::ffi::OsString::from(".");
    temporary_name.push(file_name);
    temporary_name.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        TEMPORARY_FILE_COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let temporary_path = directory.join(temporary_name);
    let written = (|| {
        let mut file = fs::File::create_new(&temporary_path)?;
        file.write_all(&serialized)?;
        file.sync_all()?;
        fs::rename(&temporary_path, path)
    })();
    if written.is_err()
        && let Err(error) = fs::remove_file(&temporary_path)
        && error.kind() != io::ErrorKind::NotFound
    {
        eprintln!(
            "riptide: cannot remove {}: {error}",
            temporary_path.display()
        );
    }
    written
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A fresh, empty directory under the system temp dir, removed on drop.
    pub struct TemporaryDirectory(pub PathBuf);

    impl TemporaryDirectory {
        pub fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "riptide-test-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TemporaryDirectory;
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let directory = TemporaryDirectory::new();
        let settings = load_settings(&directory.0.join("absent.json"));
        assert_eq!(settings, Settings::default());
        assert!(load_history(&directory.0.join("absent.json")).is_empty());
    }

    fn corrupt_backups(directory: &Path, file_name: &str) -> Vec<(PathBuf, Vec<u8>)> {
        std::fs::read_dir(directory)
            .expect("read dir")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{file_name}.corrupt-"))
            })
            .map(|entry| (entry.path(), std::fs::read(entry.path()).expect("read")))
            .collect()
    }

    #[test]
    fn corrupt_file_gives_defaults_and_is_set_aside() {
        let directory = TemporaryDirectory::new();
        let path = directory.0.join("settings.json");
        std::fs::write(&path, b"{ not json").expect("write");
        assert_eq!(load_settings(&path), Settings::default());
        assert!(!path.exists(), "the corrupt file must be moved away");
        let backups = corrupt_backups(&directory.0, "settings.json");
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].1, b"{ not json");

        let history_path = directory.0.join("history.json");
        std::fs::write(&history_path, b"[1, 2").expect("write");
        assert!(load_history(&history_path).is_empty());
        assert_eq!(corrupt_backups(&directory.0, "history.json").len(), 1);
    }

    #[test]
    fn corrupt_backup_name_carries_the_timestamp() {
        assert_eq!(
            corrupt_backup_path(Path::new("/a/settings.json"), 1_700_000_000),
            PathBuf::from("/a/settings.json.corrupt-1700000000")
        );
    }

    #[test]
    fn older_revision_never_overwrites_a_newer_one() {
        let directory = TemporaryDirectory::new();
        let path = directory.0.join("settings.json");
        let writer = LatestWriteWins::new(path.clone());
        let older = writer.next_revision();
        let newer = writer.next_revision();
        let newer_settings = Settings {
            maximum_concurrent_downloads: 7,
            ..Settings::default()
        };
        assert!(writer.write(newer, &newer_settings).expect("write newer"));
        assert!(
            !writer
                .write(older, &Settings::default())
                .expect("skip older")
        );
        assert_eq!(load_settings(&path), newer_settings);
    }

    #[test]
    fn atomic_write_creates_parent_and_leaves_no_temp_file() {
        let directory = TemporaryDirectory::new();
        let path = directory.0.join("nested").join("settings.json");
        save_settings(&path, &Settings::default()).expect("save");
        assert!(path.exists());
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().expect("parent"))
            .expect("read dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }
}
