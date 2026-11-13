# Engine ↔ UI contract

The UI uses only `crate::engine::*` re-exported in `src/engine/mod.rs`. Every function below may block; call it from a GPUI background task. Async channels use `smol::channel` (GPUI already runs on smol-compatible executors).

```rust
// error.rs
#[derive(Debug, thiserror::Error, Clone)]
pub enum EngineError {
    FfmpegMissing,                  // ffmpeg/ffprobe not on PATH
    ToolDownloadFailed(String),     // network / HTTP error while fetching yt-dlp
    ToolChecksumMismatch,           // SHA-256 of downloaded yt-dlp does not match
    InvalidUrl(String),             // not http(s), not a YouTube host, not a Suno song link
    SunoSongNotDownloadable,        // Suno serves this song only encrypted (non-public song)
    ProbeFailed(String),            // yt-dlp -J failed (private video, removed, geo…)
    DownloadFailed(String),         // yt-dlp/ffmpeg exited non-zero (last stderr line)
    Io(String),
    AppUpdateFailed(String),        // network / GitHub API error during Riptide's self-update
    AppUpdateChecksumMismatch,      // SHA-256 of the downloaded Riptide does not match SHA256SUMS
    AppInstallDirectoryNotWritable(String), // executable's folder is read-only (system install)
}
impl EngineError { pub fn user_message_fr(&self) -> String } // readable French text for the UI

// tools.rs
pub struct ToolStatus { pub yt_dlp_version: Option<String>, pub ffmpeg_available: bool }
pub fn tool_status() -> ToolStatus;                              // fast, no network
pub fn ensure_tools() -> Result<ToolStatus, EngineError>;        // installs pinned yt-dlp if missing, checks ffmpeg
pub fn update_yt_dlp() -> Result<String /*new version*/, EngineError>; // latest release, SHA-256 verified

// app_update.rs (self-update from github.com/Paul-Carouge/riptide releases)
pub const RIPTIDE_VERSION: &str;                                 // Cargo.toml version, e.g. "0.1.0"
pub struct AppRelease { pub version: String /*"1.2.0"*/, pub notes: String /*Markdown*/,
                        pub asset_url: String, pub checksums_url: String }
pub fn check_for_app_update() -> Result<Option<AppRelease>, EngineError>; // Some only if strictly newer; Ok(None) when no release exists
pub fn install_app_update(release: &AppRelease) -> Result<String /*installed version*/, EngineError>;
pub fn restart_app() -> Result<(), EngineError>;                 // spawns the executable again (same argv), detached

// probe.rs
pub fn validate_media_url(input: &str) -> Result<String /*normalized https url*/, EngineError>; // pure, unit-tested
// YouTube hosts as before; Suno: suno.com, www.suno.com, app.suno.ai with /song/<uuid> or /s/<code>.
pub enum MediaKind { Single, Playlist }
pub struct Track {
    pub id: String, pub title: String, pub uploader: Option<String>,
    pub duration_seconds: Option<u32>, pub thumbnail_url: Option<String>, pub url: String,
}
pub struct ProbedLink {
    pub kind: MediaKind, pub title: String, pub uploader: Option<String>,
    pub thumbnail_url: Option<String>, pub tracks: Vec<Track>,
}
pub fn probe_link(url: &str) -> Result<ProbedLink, EngineError>; // YouTube: yt-dlp -J --flat-playlist; Suno: see below

// preset.rs
pub enum AudioFormat { Mp3, M4a, Opus, Flac, Wav, Ogg } // Ogg = Vorbis, cover embedded
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
- YouTube probe and download pass `--use-extractors "youtube.*"`: only YouTube extractors can run.

Suno (`src/engine/suno.rs`):
- yt-dlp refuses suno.com ("will not be supported"), and Suno serves each song's own audio encrypted (`media_urls`, `audio_url` forbidden). The only plain file is the public song's video `https://cdn1.suno.ai/<uuid>.mp4` (AAC audio), listed as `video_url`; non-public (share-link only) songs have none → `SunoSongNotDownloadable`. Riptide never decrypts anything.
- Probe: a `/s/<code>` link is resolved by reading the `Location` of `https://suno.com/s/<code>` (redirects not followed); then `https://studio-api-prod.suno.com/api/clip/<uuid>` (the public JSON the song page loads) gives title, `display_name` (artist), duration, cover (`cdn1`/`cdn2.suno.ai` only) and `video_url`, accepted only if it is exactly `https://cdn1.suno.ai/<uuid>.mp4`.
- The Suno `Track.url` is that CDN file. `start_download` writes an info JSON (title, artist, cover, url) in the working folder and runs `yt-dlp --load-info-json <file> --use-extractors generic`: no extractor runs, yt-dlp only fetches the validated file and the cover, then converts and tags as usual. A Suno page URL given to `start_download` is refused.

Self-update:
- Releases are tags `vX.Y.Z` with two assets, `riptide-x86_64-linux` and `SHA256SUMS` (built by `.github/workflows/release.yml`). Assets must be hosted under `https://github.com/Paul-Carouge/riptide/releases/download/`, else `check_for_app_update` fails.
- `install_app_update` checks the executable's folder is writable before downloading, verifies the SHA-256 in memory, writes a `0600` temp file next to the executable, makes it `0755` and renames it over `current_exe()`. The running process keeps the old inode; the new version runs after `restart_app`.
- UI flow: check, show `version` and `notes`, install on confirmation, then `restart_app()` and quit (`cx.quit()`). Until the Settings tab calls the API, `src/engine/mod.rs` carries `#[expect(dead_code)]` / `#[expect(unused_imports)]` on it: remove them when wiring.

Thumbnails: the UI fetches `thumbnail_url` itself (`src/ui/thumbnails.rs`, ureq, https + YouTube image hosts and `cdn1`/`cdn2.suno.ai` only) because GPUI 0.2.2 ships a null HTTP client; the engine does not fetch images.
