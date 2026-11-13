//! Download engine: everything that talks to yt-dlp and ffmpeg. No UI code here.
//!
//! CONTRACT between engine and UI (owned by the lead; change only with the lead's agreement).
//! The UI only uses the items re-exported below. All functions may block or take long:
//! the UI calls them from `cx.background_spawn`, never on the UI thread.

// Remove this `expect` once the Settings tab calls the self-update API.
#[expect(
    dead_code,
    reason = "self-update API is not called by the UI yet (Settings tab)"
)]
mod app_update;
mod download;
mod error;
mod partial;
mod preset;
mod probe;
mod tools;

#[expect(unused_imports, reason = "the Settings tab will import these")]
pub use app_update::{
    AppRelease, RIPTIDE_VERSION, check_for_app_update, install_app_update, restart_app,
};
pub use download::{DownloadEvent, DownloadHandle, DownloadRequest, DownloadState, start_download};
pub use error::EngineError;
pub use partial::purge_stale_partial_downloads;
pub use preset::{AudioFormat, AudioQuality, OutputPreset, VideoFormat, VideoQuality};
pub use probe::{MediaKind, ProbedLink, Track, probe_link, validate_youtube_url};
pub use tools::{ToolStatus, ensure_tools, tool_status, update_yt_dlp};
