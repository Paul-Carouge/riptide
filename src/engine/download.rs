use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead as _, BufReader, Read};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use super::error::last_yt_dlp_error_line;
use super::partial::{
    create_working_folder, move_into_folder_without_overwriting, remove_working_folder,
};
use super::probe::{MediaSource, classify_media_url};
use super::suno::{suno_audio_uuid, yt_dlp_info_json};
use super::tools::yt_dlp_command;
use super::{EngineError, OutputPreset, Track};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    pub track: Track,
    pub preset: OutputPreset,
    pub output_dir: PathBuf,
    pub embed_metadata_and_cover: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    Queued,
    Downloading,
    Converting,
    Paused,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DownloadEvent {
    Progress {
        fraction: f32,
        speed_bytes_per_second: Option<u64>,
        eta_seconds: Option<u32>,
    },
    StateChanged(DownloadState),
    Finished {
        output_file: PathBuf,
    },
    Failed(EngineError),
}

const PROGRESS_LINE_PREFIX: &str = "RIPTIDE_PROGRESS ";
const FORMATS_LINE_PREFIX: &str = "RIPTIDE_FORMATS ";
const CONVERTING_LINE_PREFIX: &str = "RIPTIDE_CONVERTING";
const OUTPUT_FILE_LINE_PREFIX: &str = "RIPTIDE_OUTPUT_FILE ";
/// Fields are space separated; yt-dlp prints `NA` for unknown values.
const PROGRESS_TEMPLATE: &str = "download:RIPTIDE_PROGRESS %(info.format_id)s %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.speed)s %(progress.eta)s";
/// Printed after format selection, e.g. `137+140` when video and audio are fetched separately.
const FORMATS_PRINT_TEMPLATE: &str = "video:RIPTIDE_FORMATS %(format_id)s";
/// The `post_process` print fires right before merging / conversion / tagging starts.
const CONVERTING_PRINT_TEMPLATE: &str = "post_process:RIPTIDE_CONVERTING %(id)s";
const OUTPUT_FILE_PRINT_TEMPLATE: &str = "after_move:RIPTIDE_OUTPUT_FILE %(filepath)s";
/// `.150B` caps the title at 150 bytes (UTF-8 safe), leaving room for ` (n)` and the extension.
const OUTPUT_FILE_NAME_TEMPLATE: &str = "%(title).150B.%(ext)s";
const PROGRESS_REPORT_INTERVAL_SECONDS: &str = "0.2";
/// Only YouTube extractors (`youtube`, `youtube:tab`, …): a URL that slipped past validation
/// can never reach the generic extractor or another site.
pub(super) const YOUTUBE_EXTRACTORS: &str = "youtube.*";
/// Suno downloads load a prepared info JSON, so no extractor runs; should yt-dlp re-extract
/// its `webpage_url` (the validated CDN file), only the generic extractor may do it.
const SUNO_EXTRACTORS: &str = "generic";
const SUNO_INFO_JSON_FILE_NAME: &str = "riptide-suno-info.json";
const STDERR_LINES_KEPT: usize = 64;
const PROCESS_GROUP_EXIT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PROCESS_GROUP_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
struct SharedDownloadState {
    /// Set when yt-dlp's stdout closes (it is exiting); the process group id may be reused once
    /// it is reaped, so it is never signalled again. Pause, resume and cancel are then no-ops,
    /// so a late pause cannot report `Paused` for a download that is already over.
    has_exited: bool,
    is_cancel_requested: bool,
    is_paused: bool,
    /// `Downloading` or `Converting`: the state to restore on resume.
    active_phase: DownloadState,
}

/// A running yt-dlp process. Dropping it does not stop the download; call `cancel`.
pub struct DownloadHandle {
    events: smol::channel::Receiver<DownloadEvent>,
    /// Weak, so the channel closes once the worker thread has sent the final event.
    events_sender: smol::channel::WeakSender<DownloadEvent>,
    process_group_id: libc::pid_t,
    shared_state: Arc<Mutex<SharedDownloadState>>,
}

impl DownloadHandle {
    fn send_state_change(&self, state: DownloadState) {
        if let Some(sender) = self.events_sender.upgrade() {
            send_event(&sender, DownloadEvent::StateChanged(state));
        }
    }

    pub fn events(&self) -> smol::channel::Receiver<DownloadEvent> {
        self.events.clone()
    }

    pub fn pause(&self) -> Result<(), EngineError> {
        let mut state = lock_ignoring_poison(&self.shared_state);
        if state.has_exited || state.is_cancel_requested || state.is_paused {
            return Ok(());
        }
        signal_process_group(self.process_group_id, libc::SIGSTOP)?;
        state.is_paused = true;
        self.send_state_change(DownloadState::Paused);
        Ok(())
    }

    pub fn resume(&self) -> Result<(), EngineError> {
        let mut state = lock_ignoring_poison(&self.shared_state);
        if state.has_exited || !state.is_paused {
            return Ok(());
        }
        signal_process_group(self.process_group_id, libc::SIGCONT)?;
        state.is_paused = false;
        self.send_state_change(state.active_phase);
        Ok(())
    }

    /// Stops yt-dlp and ffmpeg; the worker thread then deletes partial files and
    /// sends `StateChanged(Cancelled)`.
    pub fn cancel(&self) -> Result<(), EngineError> {
        let mut state = lock_ignoring_poison(&self.shared_state);
        if state.has_exited || state.is_cancel_requested {
            return Ok(());
        }
        state.is_cancel_requested = true;
        signal_process_group(self.process_group_id, libc::SIGTERM)?;
        if state.is_paused {
            // A stopped process only acts on SIGTERM once continued.
            signal_process_group(self.process_group_id, libc::SIGCONT)?;
            state.is_paused = false;
        }
        Ok(())
    }
}

/// What yt-dlp is pointed at.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DownloadInput {
    /// A validated YouTube URL, passed after `--`.
    YouTubeUrl(String),
    /// A Suno song: an info JSON written by Riptide around a validated `cdn1.suno.ai` file.
    InfoJson(PathBuf),
}

/// Suno tracks carry their validated CDN file as `url`; everything else must be YouTube.
fn validate_track_url(track: &Track) -> Result<Option<String>, EngineError> {
    if suno_audio_uuid(&track.url).is_some() {
        return Ok(None);
    }
    match classify_media_url(&track.url)? {
        (MediaSource::YouTube, url) => Ok(Some(url)),
        (MediaSource::Suno(_), _) => Err(EngineError::InvalidUrl(format!(
            "a Suno page must be probed before download: {}",
            track.url
        ))),
    }
}

/// Spawns yt-dlp now (non-blocking) plus a reader thread that parses progress.
pub fn start_download(request: DownloadRequest) -> Result<DownloadHandle, EngineError> {
    let validated_youtube_url = validate_track_url(&request.track)?;
    let io_error =
        |action: &str, error: std::io::Error| EngineError::Io(format!("{action}: {error}"));
    std::fs::create_dir_all(&request.output_dir)
        .map_err(|error| io_error("create output folder", error))?;
    let output_dir = std::path::absolute(&request.output_dir)
        .map_err(|error| io_error("resolve output folder", error))?;
    let temporary_dir = create_working_folder(&output_dir, &request.track.id)?;
    let input = match validated_youtube_url {
        Some(url) => DownloadInput::YouTubeUrl(url),
        None => {
            let info_json_path = temporary_dir.join(SUNO_INFO_JSON_FILE_NAME);
            let uuid = suno_audio_uuid(&request.track.url).unwrap_or_default();
            let info_json = yt_dlp_info_json(&request.track, &request.track.url, uuid);
            if let Err(error) = std::fs::write(&info_json_path, info_json) {
                remove_working_folder(&temporary_dir);
                return Err(io_error("write Suno info file", error));
            }
            DownloadInput::InfoJson(info_json_path)
        }
    };
    let arguments = yt_dlp_download_arguments(&request, &input, &temporary_dir);

    let spawn_result = yt_dlp_command().and_then(|mut command| {
        command
            .args(&arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Own process group, so pause/cancel also reach the ffmpeg children.
            .process_group(0)
            .spawn()
            .map_err(|error| io_error("cannot start yt-dlp", error))
    });
    let mut child = match spawn_result {
        Ok(child) => child,
        Err(error) => {
            remove_working_folder(&temporary_dir);
            return Err(error);
        }
    };
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        // Unreachable: both pipes were requested above.
        let _ = child.kill();
        let _ = child.wait();
        remove_working_folder(&temporary_dir);
        return Err(EngineError::Io("yt-dlp pipes unavailable".to_string()));
    };
    let process_group_id = match libc::pid_t::try_from(child.id()) {
        Ok(process_id) => process_id,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            remove_working_folder(&temporary_dir);
            return Err(EngineError::Io(
                "yt-dlp process id out of range".to_string(),
            ));
        }
    };

    let (events_sender, events) = smol::channel::unbounded();
    let shared_state = Arc::new(Mutex::new(SharedDownloadState {
        has_exited: false,
        is_cancel_requested: false,
        is_paused: false,
        active_phase: DownloadState::Downloading,
    }));
    send_event(
        &events_sender,
        DownloadEvent::StateChanged(DownloadState::Downloading),
    );

    let stderr_reader = thread::Builder::new()
        .name("riptide-yt-dlp-stderr".to_string())
        .spawn(move || last_stderr_lines(stderr));
    let stderr_reader = match stderr_reader {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            remove_working_folder(&temporary_dir);
            return Err(io_error("cannot start reader thread", error));
        }
    };
    let weak_events_sender = events_sender.downgrade();
    let worker = DownloadWorker {
        child,
        process_group_id,
        stdout,
        stderr_reader,
        temporary_dir,
        output_dir,
        events_sender,
        shared_state: Arc::clone(&shared_state),
    };
    thread::Builder::new()
        .name("riptide-yt-dlp-stdout".to_string())
        .spawn(move || worker.run())
        // The closure is dropped on failure, which leaves yt-dlp running unobserved: stop it.
        .map_err(|error| {
            let _ = signal_process_group(process_group_id, libc::SIGKILL);
            io_error("cannot start worker thread", error)
        })?;

    Ok(DownloadHandle {
        events,
        events_sender: weak_events_sender,
        process_group_id,
        shared_state,
    })
}

fn yt_dlp_download_arguments(
    request: &DownloadRequest,
    input: &DownloadInput,
    temporary_dir: &Path,
) -> Vec<OsString> {
    // Even the finished file stays in the working folder: Riptide moves it out itself so that
    // an existing file with the same name is never overwritten.
    let mut home_path = OsString::from("home:");
    home_path.push(temporary_dir);
    let mut temporary_path = OsString::from("temp:");
    temporary_path.push(temporary_dir);

    let mut arguments: Vec<OsString> = [
        "--no-playlist",
        "--newline",
        "--progress",
        "--no-simulate",
        "--progress-delta",
        PROGRESS_REPORT_INTERVAL_SECONDS,
        "--progress-template",
        PROGRESS_TEMPLATE,
        "--print",
        FORMATS_PRINT_TEMPLATE,
        "--print",
        CONVERTING_PRINT_TEMPLATE,
        "--print",
        OUTPUT_FILE_PRINT_TEMPLATE,
        "--use-extractors",
        match input {
            DownloadInput::YouTubeUrl(_) => YOUTUBE_EXTRACTORS,
            DownloadInput::InfoJson(_) => SUNO_EXTRACTORS,
        },
        "--windows-filenames",
        "-o",
        OUTPUT_FILE_NAME_TEMPLATE,
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    arguments.extend(["-P".into(), home_path, "-P".into(), temporary_path]);
    arguments.extend(
        request
            .preset
            .yt_dlp_format_arguments()
            .into_iter()
            .map(OsString::from),
    );
    if request.embed_metadata_and_cover {
        arguments.push("--embed-metadata".into());
        if request.preset.supports_embedded_cover() {
            arguments
                .extend(["--embed-thumbnail", "--convert-thumbnails", "jpg"].map(OsString::from));
        }
    }
    match input {
        DownloadInput::YouTubeUrl(url) => arguments.extend(["--".into(), url.into()]),
        DownloadInput::InfoJson(path) => {
            arguments.extend(["--load-info-json".into(), path.as_os_str().to_owned()])
        }
    }
    arguments
}

struct DownloadWorker {
    child: Child,
    process_group_id: libc::pid_t,
    stdout: std::process::ChildStdout,
    stderr_reader: thread::JoinHandle<String>,
    /// Working folder inside `output_dir`: yt-dlp writes everything here.
    temporary_dir: PathBuf,
    output_dir: PathBuf,
    events_sender: smol::channel::Sender<DownloadEvent>,
    shared_state: Arc<Mutex<SharedDownloadState>>,
}

impl DownloadWorker {
    fn run(mut self) {
        let output_file = self.forward_stdout_events();
        lock_ignoring_poison(&self.shared_state).has_exited = true;
        let DownloadWorker {
            mut child,
            process_group_id,
            stderr_reader,
            temporary_dir,
            output_dir,
            events_sender,
            shared_state,
            ..
        } = self;
        // The stderr thread only reads lines; a panic there just loses the error detail.
        let stderr_text = stderr_reader.join().unwrap_or_default();
        let exit_status = child.wait();
        wait_for_process_group_to_exit(process_group_id);

        let is_cancel_requested = lock_ignoring_poison(&shared_state).is_cancel_requested;
        let is_success = exit_status.as_ref().is_ok_and(|status| status.success());
        // `after_move` is yt-dlp's last step: once it reported the file, a cancel that lands
        // afterwards changes nothing and the download is kept.
        let finished_file = output_file.filter(|file| {
            (is_success || is_cancel_requested)
                && file.parent() == Some(temporary_dir.as_path())
                && file.is_file()
        });
        let final_events = match (is_success, finished_file) {
            (_, Some(finished_file)) => {
                match move_into_folder_without_overwriting(&finished_file, &output_dir) {
                    Ok(output_file) => vec![
                        DownloadEvent::Finished { output_file },
                        DownloadEvent::StateChanged(DownloadState::Done),
                    ],
                    Err(error) => failure_events(error),
                }
            }
            _ if is_cancel_requested => {
                vec![DownloadEvent::StateChanged(DownloadState::Cancelled)]
            }
            (true, None) => failure_events(EngineError::DownloadFailed(
                "yt-dlp did not report the output file in its working folder".to_string(),
            )),
            (false, _) => failure_events(match exit_status {
                Err(error) => EngineError::Io(format!("waiting for yt-dlp: {error}")),
                Ok(_) => EngineError::DownloadFailed(last_yt_dlp_error_line(&stderr_text)),
            }),
        };
        remove_working_folder(&temporary_dir);
        for event in final_events {
            send_event(&events_sender, event);
        }
    }

    /// Turns yt-dlp's stdout into events until it closes; returns the final file path.
    fn forward_stdout_events(&mut self) -> Option<PathBuf> {
        let mut reader = BufReader::new(&mut self.stdout);
        let mut line_bytes = Vec::new();
        let mut format_ids: Vec<String> = Vec::new();
        let mut highest_fraction = 0.0_f32;
        let mut output_file = None;
        loop {
            line_bytes.clear();
            match reader.read_until(b'\n', &mut line_bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&line_bytes);
            match parse_output_line(line.trim_end_matches(['\r', '\n'])) {
                YtDlpOutputLine::Formats(ids) => format_ids = ids,
                YtDlpOutputLine::Progress(sample) => {
                    let Some(fraction) = sample.overall_fraction(&format_ids) else {
                        continue;
                    };
                    // Size estimates of fragmented streams wobble; the bar must never go back.
                    highest_fraction = highest_fraction.max(fraction);
                    send_event(
                        &self.events_sender,
                        DownloadEvent::Progress {
                            fraction: highest_fraction,
                            speed_bytes_per_second: sample.speed_bytes_per_second,
                            eta_seconds: sample.eta_seconds,
                        },
                    );
                }
                YtDlpOutputLine::Converting => {
                    let mut state = lock_ignoring_poison(&self.shared_state);
                    // A line still in the pipe when the user paused must not replace `Paused`:
                    // `resume` restores `active_phase` instead.
                    let is_new_phase = state.active_phase != DownloadState::Converting;
                    state.active_phase = DownloadState::Converting;
                    if is_new_phase && !state.is_paused {
                        send_event(
                            &self.events_sender,
                            DownloadEvent::StateChanged(DownloadState::Converting),
                        );
                    }
                }
                YtDlpOutputLine::OutputFile(path) => output_file = Some(path),
                YtDlpOutputLine::Unrecognized => {}
            }
        }
        output_file
    }
}

fn failure_events(error: EngineError) -> Vec<DownloadEvent> {
    vec![
        DownloadEvent::Failed(error),
        DownloadEvent::StateChanged(DownloadState::Failed),
    ]
}

fn last_stderr_lines(stderr: impl Read) -> String {
    let mut kept_lines = VecDeque::with_capacity(STDERR_LINES_KEPT);
    let mut reader = BufReader::new(stderr);
    let mut line_bytes = Vec::new();
    while matches!(reader.read_until(b'\n', &mut line_bytes), Ok(read) if read > 0) {
        if kept_lines.len() == STDERR_LINES_KEPT {
            kept_lines.pop_front();
        }
        kept_lines.push_back(String::from_utf8_lossy(&line_bytes).trim_end().to_string());
        line_bytes.clear();
    }
    Vec::from(kept_lines).join("\n")
}

/// yt-dlp is reaped first; its ffmpeg children may still be writing into the temp folder.
fn wait_for_process_group_to_exit(process_group_id: libc::pid_t) {
    let deadline = Instant::now() + PROCESS_GROUP_EXIT_TIMEOUT;
    // Signal 0 only checks existence. A member keeps the group id from being reused.
    while signal_process_group(process_group_id, 0).is_ok() {
        if Instant::now() >= deadline {
            let _ = signal_process_group(process_group_id, libc::SIGKILL);
            return;
        }
        thread::sleep(PROCESS_GROUP_EXIT_POLL_INTERVAL);
    }
}

fn signal_process_group(
    process_group_id: libc::pid_t,
    signal: libc::c_int,
) -> Result<(), EngineError> {
    // SAFETY: killpg only reads two integers and has no memory-safety preconditions. Callers
    // only pass the group id of a yt-dlp child that has not been reaped yet (or that still has
    // live members), so the signal cannot reach an unrelated process group.
    let result = unsafe { libc::killpg(process_group_id, signal) };
    if result == 0 {
        Ok(())
    } else {
        Err(EngineError::Io(format!(
            "cannot signal yt-dlp: {}",
            std::io::Error::last_os_error()
        )))
    }
}

fn lock_ignoring_poison(
    shared_state: &Mutex<SharedDownloadState>,
) -> MutexGuard<'_, SharedDownloadState> {
    // The state stays consistent even if a holder panicked: every field is a plain flag.
    shared_state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn send_event(sender: &smol::channel::Sender<DownloadEvent>, event: DownloadEvent) {
    // Fails only when every receiver is gone, i.e. nobody is listening any more.
    let _ = sender.try_send(event);
}

#[derive(Debug, Clone, PartialEq)]
enum YtDlpOutputLine {
    Formats(Vec<String>),
    Progress(ProgressSample),
    Converting,
    OutputFile(PathBuf),
    Unrecognized,
}

#[derive(Debug, Clone, PartialEq)]
struct ProgressSample {
    format_id: String,
    downloaded_bytes: Option<u64>,
    /// Exact size when known, else yt-dlp's estimate (fragmented streams).
    expected_total_bytes: Option<u64>,
    speed_bytes_per_second: Option<u64>,
    eta_seconds: Option<u32>,
}

impl ProgressSample {
    /// Fraction of the whole download: each separately fetched stream counts equally.
    fn overall_fraction(&self, format_ids: &[String]) -> Option<f32> {
        let downloaded = self.downloaded_bytes?;
        let total = self.expected_total_bytes.filter(|total| *total > 0)?;
        let stream_fraction = (downloaded as f64 / total as f64).clamp(0.0, 1.0);
        let stream_count = format_ids.len().max(1);
        let stream_index = format_ids
            .iter()
            .position(|format_id| *format_id == self.format_id)
            .unwrap_or(0);
        Some(((stream_index as f64 + stream_fraction) / stream_count as f64) as f32)
    }
}

fn parse_output_line(line: &str) -> YtDlpOutputLine {
    if let Some(fields) = line.strip_prefix(PROGRESS_LINE_PREFIX) {
        return parse_progress_fields(fields)
            .map_or(YtDlpOutputLine::Unrecognized, YtDlpOutputLine::Progress);
    }
    if let Some(format_ids) = line.strip_prefix(FORMATS_LINE_PREFIX) {
        let ids = format_ids
            .trim()
            .split('+')
            .filter(|id| !id.is_empty() && *id != "NA")
            .map(str::to_string)
            .collect();
        return YtDlpOutputLine::Formats(ids);
    }
    if line.starts_with(CONVERTING_LINE_PREFIX) {
        return YtDlpOutputLine::Converting;
    }
    if let Some(path) = line.strip_prefix(OUTPUT_FILE_LINE_PREFIX)
        && !path.is_empty()
    {
        return YtDlpOutputLine::OutputFile(PathBuf::from(path));
    }
    YtDlpOutputLine::Unrecognized
}

/// Fields: format id, downloaded, total, total estimate, speed (bytes/s), eta (s).
fn parse_progress_fields(fields: &str) -> Option<ProgressSample> {
    let parts: Vec<&str> = fields.split_whitespace().collect();
    let [format_id, downloaded, total, total_estimate, speed, eta] = parts.as_slice() else {
        return None;
    };
    Some(ProgressSample {
        format_id: (*format_id).to_string(),
        downloaded_bytes: parse_non_negative_number(downloaded),
        expected_total_bytes: parse_non_negative_number(total)
            .or_else(|| parse_non_negative_number(total_estimate)),
        speed_bytes_per_second: parse_non_negative_number(speed),
        eta_seconds: parse_non_negative_number(eta).and_then(|eta| u32::try_from(eta).ok()),
    })
}

/// yt-dlp prints integers, floats (speed, estimates) or `NA`/`None` for unknown values.
fn parse_non_negative_number(field: &str) -> Option<u64> {
    let value: f64 = field.parse().ok()?;
    // Saturating cast is fine: byte counts and speeds never approach u64::MAX.
    (value.is_finite() && value >= 0.0).then(|| value.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{AudioFormat, AudioQuality, VideoFormat, VideoQuality};

    fn progress(line: &str) -> ProgressSample {
        match parse_output_line(line) {
            YtDlpOutputLine::Progress(sample) => sample,
            other => panic!("expected progress, got {other:?}"),
        }
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn parses_a_progress_line_with_exact_total() {
        let sample = progress("RIPTIDE_PROGRESS 251 1024 433081 NA 275177.2998462327 1");
        assert_eq!(
            sample,
            ProgressSample {
                format_id: "251".to_string(),
                downloaded_bytes: Some(1024),
                expected_total_bytes: Some(433081),
                speed_bytes_per_second: Some(275177),
                eta_seconds: Some(1),
            }
        );
    }

    #[test]
    fn falls_back_to_the_estimated_total() {
        let sample = progress("RIPTIDE_PROGRESS 18 500 NA 1000.7 NA NA");
        assert_eq!(sample.expected_total_bytes, Some(1001));
        assert_eq!(sample.speed_bytes_per_second, None);
        assert_eq!(sample.eta_seconds, None);
        let fraction = sample.overall_fraction(&[]).unwrap();
        assert!((fraction - 0.4995).abs() < 0.001, "{fraction}");
    }

    #[test]
    fn unknown_totals_give_no_fraction() {
        let sample = progress("RIPTIDE_PROGRESS 18 500 NA NA 10 3");
        assert_eq!(sample.overall_fraction(&[]), None);
        let zero_total = progress("RIPTIDE_PROGRESS 18 500 0 NA 10 3");
        assert_eq!(zero_total.overall_fraction(&[]), None);
        let no_downloaded = progress("RIPTIDE_PROGRESS 18 NA 100 NA 10 3");
        assert_eq!(no_downloaded.overall_fraction(&[]), None);
    }

    #[test]
    fn fraction_is_clamped_to_one() {
        let sample = progress("RIPTIDE_PROGRESS 18 2000 1000 NA NA NA");
        assert_eq!(sample.overall_fraction(&[]), Some(1.0));
    }

    #[test]
    fn separate_video_and_audio_streams_share_the_bar() {
        let formats = ids(&["137", "140"]);
        let video_half = progress("RIPTIDE_PROGRESS 137 50 100 NA NA NA");
        assert_eq!(video_half.overall_fraction(&formats), Some(0.25));
        let audio_half = progress("RIPTIDE_PROGRESS 140 5 10 NA NA NA");
        assert_eq!(audio_half.overall_fraction(&formats), Some(0.75));
    }

    #[test]
    fn malformed_progress_lines_are_ignored() {
        assert_eq!(
            parse_output_line("RIPTIDE_PROGRESS 1 2 3"),
            YtDlpOutputLine::Unrecognized
        );
        assert_eq!(parse_non_negative_number("-5"), None);
        assert_eq!(parse_non_negative_number("inf"), None);
        assert_eq!(parse_non_negative_number("None"), None);
    }

    #[test]
    fn parses_marker_lines() {
        assert_eq!(
            parse_output_line("RIPTIDE_FORMATS 137+140"),
            YtDlpOutputLine::Formats(ids(&["137", "140"]))
        );
        assert_eq!(
            parse_output_line("RIPTIDE_FORMATS NA"),
            YtDlpOutputLine::Formats(Vec::new())
        );
        assert_eq!(
            parse_output_line("RIPTIDE_CONVERTING jNQXAC9IVRw"),
            YtDlpOutputLine::Converting
        );
        assert_eq!(
            parse_output_line("RIPTIDE_OUTPUT_FILE /music/Me at the zoo.mp3"),
            YtDlpOutputLine::OutputFile(PathBuf::from("/music/Me at the zoo.mp3"))
        );
        assert_eq!(
            parse_output_line("RIPTIDE_OUTPUT_FILE "),
            YtDlpOutputLine::Unrecognized
        );
        assert_eq!(
            parse_output_line("[download] 5%"),
            YtDlpOutputLine::Unrecognized
        );
    }

    fn request(preset: OutputPreset, embed: bool) -> DownloadRequest {
        DownloadRequest {
            track: Track {
                id: "jNQXAC9IVRw".to_string(),
                title: "Me at the zoo".to_string(),
                uploader: None,
                duration_seconds: Some(19),
                thumbnail_url: None,
                url: "https://www.youtube.com/watch?v=jNQXAC9IVRw".to_string(),
            },
            preset,
            output_dir: PathBuf::from("/music"),
            embed_metadata_and_cover: embed,
        }
    }

    fn arguments_for(preset: OutputPreset, embed: bool) -> Vec<String> {
        let request = request(preset, embed);
        yt_dlp_download_arguments(
            &request,
            &DownloadInput::YouTubeUrl(request.track.url.clone()),
            Path::new("/music/.riptide-partial/x"),
        )
        .into_iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect()
    }

    #[test]
    fn url_comes_last_right_after_the_double_dash() {
        let arguments = arguments_for(
            OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Kbps320),
            true,
        );
        let length = arguments.len();
        assert_eq!(arguments[length - 2], "--");
        assert_eq!(
            arguments[length - 1],
            "https://www.youtube.com/watch?v=jNQXAC9IVRw"
        );
        assert_eq!(
            arguments
                .iter()
                .filter(|argument| *argument == "--")
                .count(),
            1
        );
    }

    #[test]
    fn downloads_go_through_a_dedicated_temporary_folder() {
        let arguments = arguments_for(
            OutputPreset::Video(VideoFormat::Mp4, VideoQuality::P720),
            false,
        );
        // Even the finished file is written to the working folder, never straight to /music.
        assert!(arguments.contains(&"home:/music/.riptide-partial/x".to_string()));
        assert!(arguments.contains(&"temp:/music/.riptide-partial/x".to_string()));
        assert!(!arguments.iter().any(|argument| argument == "home:/music"));
        assert!(arguments.contains(&"--no-playlist".to_string()));
        assert!(arguments.contains(&"--windows-filenames".to_string()));
        assert!(!arguments.contains(&"--trim-filenames".to_string()));
        assert!(arguments.contains(&"%(title).150B.%(ext)s".to_string()));
        let extractors_position = arguments
            .iter()
            .position(|argument| argument == "--use-extractors")
            .unwrap();
        assert_eq!(arguments[extractors_position + 1], YOUTUBE_EXTRACTORS);
        assert!(!arguments.contains(&"--embed-metadata".to_string()));
    }

    #[test]
    fn cover_is_embedded_except_for_wav() {
        let mp3 = arguments_for(
            OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Kbps320),
            true,
        );
        assert!(mp3.contains(&"--embed-metadata".to_string()));
        assert!(mp3.contains(&"--embed-thumbnail".to_string()));
        assert!(mp3.contains(&"jpg".to_string()));
        let wav = arguments_for(
            OutputPreset::Audio(AudioFormat::Wav, AudioQuality::Best),
            true,
        );
        assert!(wav.contains(&"--embed-metadata".to_string()));
        assert!(!wav.contains(&"--embed-thumbnail".to_string()));
    }

    #[test]
    fn invalid_urls_are_rejected_before_spawning() {
        let mut bad = request(
            OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Best),
            true,
        );
        bad.track.url = "https://evil.example/watch?v=jNQXAC9IVRw".to_string();
        assert!(matches!(
            start_download(bad),
            Err(EngineError::InvalidUrl(_))
        ));
    }

    #[test]
    fn suno_tracks_load_an_info_json_with_only_the_generic_extractor() {
        let mut suno = request(
            OutputPreset::Audio(AudioFormat::Ogg, AudioQuality::Kbps320),
            true,
        );
        suno.track.url =
            "https://cdn1.suno.ai/0ee28dcc-929d-410b-a60c-6f5076b46f15.mp4".to_string();
        assert_eq!(validate_track_url(&suno.track), Ok(None));
        let arguments: Vec<String> = yt_dlp_download_arguments(
            &suno,
            &DownloadInput::InfoJson(PathBuf::from("/music/.riptide-partial/x/info.json")),
            Path::new("/music/.riptide-partial/x"),
        )
        .into_iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
        let length = arguments.len();
        assert_eq!(arguments[length - 2], "--load-info-json");
        assert_eq!(arguments[length - 1], "/music/.riptide-partial/x/info.json");
        assert!(!arguments.contains(&"--".to_string()));
        let extractors_position = arguments
            .iter()
            .position(|argument| argument == "--use-extractors")
            .unwrap();
        assert_eq!(arguments[extractors_position + 1], SUNO_EXTRACTORS);
    }

    #[test]
    fn suno_pages_and_foreign_cdn_urls_cannot_be_downloaded_directly() {
        let mut track = request(
            OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Best),
            true,
        )
        .track;
        for url in [
            "https://suno.com/song/0ee28dcc-929d-410b-a60c-6f5076b46f15",
            "https://cdn2.suno.ai/0ee28dcc-929d-410b-a60c-6f5076b46f15.mp4",
            "https://d2lwuy8qc234o3.cloudfront.net/1/clip/0ee28dcc-929d-410b-a60c-6f5076b46f15.m4a",
        ] {
            track.url = url.to_string();
            assert!(
                matches!(validate_track_url(&track), Err(EngineError::InvalidUrl(_))),
                "{url}"
            );
        }
    }
}
