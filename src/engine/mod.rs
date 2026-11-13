//! Download engine: everything that talks to yt-dlp and ffmpeg. No UI code here.
//!
//! CONTRACT between engine and UI (owned by the lead; change only with the lead's agreement).
//! The UI only uses the items re-exported below. All functions may block or take long:
//! the UI calls them from `cx.background_spawn`, never on the UI thread.

mod download;
mod error;
mod partial;
mod preset;
mod probe;
mod tools;

pub use download::{DownloadEvent, DownloadHandle, DownloadRequest, DownloadState, start_download};
pub use error::EngineError;
pub use partial::purge_stale_partial_downloads;
pub use preset::{AudioFormat, AudioQuality, OutputPreset, VideoFormat, VideoQuality};
pub use probe::{MediaKind, ProbedLink, Track, probe_link, validate_youtube_url};
pub use tools::{ToolStatus, ensure_tools, tool_status, update_yt_dlp};
