# Engine ↔ UI contract

The UI uses only `crate::engine::*` re-exported in `src/engine/mod.rs`. Every function below may block; call it from a GPUI background task. Async channels use `smol::channel` (GPUI already runs on smol-compatible executors).

```rust
// error.rs
#[derive(Debug, thiserror::Error, Clone)]
pub enum EngineError {
    FfmpegMissing,                  // ffmpeg/ffprobe not on PATH
    ToolDownloadFailed(String),     // network / HTTP error while fetching yt-dlp
    ToolChecksumMismatch,           // SHA-256 of downloaded yt-dlp does not match
    InvalidUrl(String),             // not http(s) or not a YouTube host
    ProbeFailed(String),            // yt-dlp -J failed (private video, removed, geo…)
    DownloadFailed(String),         // yt-dlp/ffmpeg exited non-zero (last stderr line)
    Io(String),
}
impl EngineError { pub fn user_message_fr(&self) -> String } // readable French text for the UI

// tools.rs
pub struct ToolStatus { pub yt_dlp_version: Option<String>, pub ffmpeg_available: bool }
pub fn tool_status() -> ToolStatus;                              // fast, no network
pub fn ensure_tools() -> Result<ToolStatus, EngineError>;        // installs pinned yt-dlp if missing, checks ffmpeg
pub fn update_yt_dlp() -> Result<String /*new version*/, EngineError>; // latest release, SHA-256 verified

// probe.rs
pub fn validate_youtube_url(input: &str) -> Result<String /*normalized url*/, EngineError>; // pure, unit-tested
pub enum MediaKind { Single, Playlist }
pub struct Track {
    pub id: String, pub title: String, pub uploader: Option<String>,
    pub duration_seconds: Option<u32>, pub thumbnail_url: Option<String>, pub url: String,
}
pub struct ProbedLink {
    pub kind: MediaKind, pub title: String, pub uploader: Option<String>,
    pub thumbnail_url: Option<String>, pub tracks: Vec<Track>,
}
pub fn probe_link(url: &str) -> Result<ProbedLink, EngineError>; // yt-dlp -J --flat-playlist

// preset.rs
pub enum AudioFormat { Mp3, M4a, Opus, Flac, Wav }
pub enum AudioQuality { Best, Kbps320, Kbps256, Kbps192, Kbps128 } // ignored for Flac/Wav
pub enum VideoFormat { Mp4, Webm }
pub enum VideoQuality { Best, P2160, P1440, P1080, P720, P480 }
pub enum OutputPreset { Audio(AudioFormat, AudioQuality), Video(VideoFormat, VideoQuality) }
impl OutputPreset { pub fn label_fr(&self) -> String; pub fn is_lossless(&self) -> bool }
// All enums: Debug, Clone, Copy, PartialEq, Eq, serde Serialize/Deserialize, with `ALL` const slices for pickers.

// download.rs
pub struct DownloadRequest {
    pub track: Track, pub preset: OutputPreset, pub output_dir: std::path::PathBuf,
    pub embed_metadata_and_cover: bool,
}
pub enum DownloadState { Queued, Downloading, Converting, Paused, Done, Failed, Cancelled }
pub enum DownloadEvent {
    Progress { fraction: f32, speed_bytes_per_second: Option<u64>, eta_seconds: Option<u32> },
    StateChanged(DownloadState),
    Finished { output_file: std::path::PathBuf },
    Failed(EngineError),
}
pub struct DownloadHandle { /* child pid, events receiver */ }
impl DownloadHandle {
    pub fn events(&self) -> smol::channel::Receiver<DownloadEvent>; // clone of the receiver
    pub fn pause(&self) -> Result<(), EngineError>;   // SIGSTOP on the process group
    pub fn resume(&self) -> Result<(), EngineError>;  // SIGCONT
    pub fn cancel(&self) -> Result<(), EngineError>;  // SIGTERM group, then removes partial files
}
pub fn start_download(request: DownloadRequest) -> Result<DownloadHandle, EngineError>;
// Spawns yt-dlp immediately (non-blocking) and a reader thread that parses progress.
// Concurrency limit is the UI's job (queue starts at most N handles).

// partial.rs
pub fn purge_stale_partial_downloads(output_directories: &[std::path::PathBuf]);
// Call once at startup (background task) with every output folder the user may have used.
// Deletes `<dir>/.riptide-partial/<id>-<pid>-<n>/` (and the pre-1.0 `$XDG_CACHE_HOME/riptide/partial/*`)
// only when that pid is dead, so a second running Riptide keeps its downloads. Best effort, never fails.
```

Download file handling:
- Each download works in `<output_dir>/.riptide-partial/<id>-<pid>-<n>/` (same file system as the output). yt-dlp writes everything there, the finished file included (`-o "%(title).150B.%(ext)s"`).
- On success Riptide moves the file into `output_dir` with `renameat2(RENAME_NOREPLACE)` (fallback `link` + `unlink`): `Title.ext`, else `Title (2).ext`, `Title (3).ext`… An existing file is never overwritten. Names are sanitized (no `/`, NUL, empty, `.` or `..`) and cut on a UTF-8 boundary to 255 bytes.
- `Finished { output_file }` carries that final path. The working folder (and `.riptide-partial` once empty) is removed on success, failure and cancel.
- A cancel that lands after yt-dlp reported the finished file yields `Finished` + `Done`, not `Cancelled`.
- `StateChanged(Converting)` is never sent while paused (resume restores it). Pause/resume/cancel are no-ops once yt-dlp's output has closed.
- Probe and download pass `--use-extractors "youtube.*"`: only YouTube extractors can run.

Thumbnails: the UI fetches `thumbnail_url` itself (`src/ui/thumbnails.rs`, ureq, https + ytimg hosts only) because GPUI 0.2.2 ships a null HTTP client; the engine does not fetch images.
