//! End-to-end check of the engine against the real network, yt-dlp and ffmpeg.
//!
//! `cargo run --example smoke -- <output folder> [update-first]`
//!
//! With `update-first`, `update_yt_dlp` runs before `ensure_tools`, which exercises the
//! GitHub API + `SHA2-256SUMS` path when no yt-dlp is installed yet.

// The crate is a binary, so the engine is compiled into this example directly.
// The smoke run does not touch every public item (e.g. `DownloadState::Queued`, `ToolStatus`).
#[allow(dead_code, unused_imports)]
#[path = "../src/engine/mod.rs"]
mod engine;

use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use engine::{
    AudioFormat, AudioQuality, DownloadEvent, DownloadHandle, DownloadRequest, DownloadState,
    MediaKind, OutputPreset, VideoFormat, VideoQuality,
};

const SHORT_VIDEO_URL: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";
const SHORT_PLAYLIST_URL: &str =
    "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI";
/// A real YouTube Short (from the @YouTube channel's Shorts tab).
const SHORT_FORM_VIDEO_URL: &str = "https://www.youtube.com/shorts/n0FM32lZA8s";
/// About eleven minutes: long enough to cancel mid-download.
const LONG_VIDEO_URL: &str = "https://www.youtube.com/watch?v=NFvDHYMzj9U";
const CANCEL_AFTER_FRACTION: f32 = 0.05;
const PAUSE_DURATION: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let Some(output_dir) = arguments.next().map(PathBuf::from) else {
        eprintln!("usage: smoke <output folder> [update-first]");
        return ExitCode::FAILURE;
    };
    let should_update_first = arguments.next().as_deref() == Some("update-first");
    match run(&output_dir, should_update_first) {
        Ok(()) => {
            println!("SMOKE OK");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("SMOKE FAILED: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(output_dir: &Path, should_update_first: bool) -> Result<(), String> {
    let fail = |error: engine::EngineError| format!("{error} / {}", error.user_message_fr());

    println!("tool_status before: {:?}", engine::tool_status());
    if should_update_first {
        println!(
            "update_yt_dlp: {:?}",
            engine::update_yt_dlp().map_err(fail)?
        );
    }
    engine::purge_stale_partial_downloads(&[output_dir.to_path_buf()]);
    println!("ensure_tools: {:?}", engine::ensure_tools().map_err(fail)?);
    println!(
        "update_yt_dlp: {:?}",
        engine::update_yt_dlp().map_err(fail)?
    );

    let single = engine::probe_link(SHORT_VIDEO_URL).map_err(fail)?;
    println!("probe single: {single:#?}");
    if single.kind != MediaKind::Single || single.tracks.len() != 1 {
        return Err("single video probe returned the wrong shape".to_string());
    }
    let playlist = engine::probe_link(SHORT_PLAYLIST_URL).map_err(fail)?;
    println!(
        "probe playlist: {:?} « {} » by {:?}, {} tracks, first = {:?}",
        playlist.kind,
        playlist.title,
        playlist.uploader,
        playlist.tracks.len(),
        playlist.tracks.first()
    );
    if playlist.kind != MediaKind::Playlist || playlist.tracks.is_empty() {
        return Err("playlist probe returned the wrong shape".to_string());
    }
    if let Err(error) = engine::probe_link("https://vimeo.com/1") {
        println!("invalid url rejected: {}", error.user_message_fr());
    }

    let track = single.tracks[0].clone();
    let mut finished_files = Vec::new();
    for preset in [
        OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Kbps320),
        OutputPreset::Video(VideoFormat::Mp4, VideoQuality::P720),
        OutputPreset::Audio(AudioFormat::M4a, AudioQuality::Kbps192),
        OutputPreset::Audio(AudioFormat::Opus, AudioQuality::Best),
        OutputPreset::Audio(AudioFormat::Flac, AudioQuality::Best),
        OutputPreset::Audio(AudioFormat::Wav, AudioQuality::Best),
        OutputPreset::Video(VideoFormat::Webm, VideoQuality::Best),
    ] {
        println!("--- download {}", preset.label_fr());
        let output_file = download(&track, preset, output_dir)?;
        println!("finished: {}", output_file.display());
        finished_files.push(output_file);
    }

    println!("--- same title twice: the first file must stay intact");
    let first_mp3 = output_dir.join(format!("{}.mp3", track.title));
    if finished_files[0] != first_mp3 {
        return Err(format!(
            "first MP3 landed at {}",
            finished_files[0].display()
        ));
    }
    let first_mp3_digest_before = sha256_hex(&first_mp3)?;
    let second_mp3 = download(
        &track,
        OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Kbps320),
        output_dir,
    )?;
    let expected_second_mp3 = output_dir.join(format!("{} (2).mp3", track.title));
    if second_mp3 != expected_second_mp3 || !second_mp3.is_file() {
        return Err(format!("second MP3 landed at {}", second_mp3.display()));
    }
    let first_mp3_digest_after = sha256_hex(&first_mp3)?;
    println!("first MP3 sha256 before {first_mp3_digest_before} after {first_mp3_digest_after}");
    if first_mp3_digest_before != first_mp3_digest_after {
        return Err("the first MP3 was modified".to_string());
    }

    println!("--- MP4 720p stream check");
    let mp4_height = ffprobe_video_height(&finished_files[1])?;
    println!(
        "ffprobe height of {}: {mp4_height}",
        finished_files[1].display()
    );
    if mp4_height == 0 || mp4_height > 720 {
        return Err(format!("unexpected MP4 height {mp4_height}"));
    }

    println!("--- Short");
    let short = engine::probe_link(SHORT_FORM_VIDEO_URL).map_err(fail)?;
    println!("probe short: {:?} « {} »", short.kind, short.title);
    if short.kind != MediaKind::Single || short.tracks.len() != 1 {
        return Err("short probe returned the wrong shape".to_string());
    }
    let short_dir = output_dir.join("short");
    let short_file = download(
        &short.tracks[0],
        OutputPreset::Video(VideoFormat::Mp4, VideoQuality::Best),
        &short_dir,
    )?;
    println!(
        "short finished: {} (height {})",
        short_file.display(),
        ffprobe_video_height(&short_file)?
    );

    println!("--- cancel test");
    let long_track = engine::probe_link(LONG_VIDEO_URL)
        .map_err(fail)?
        .tracks
        .remove(0);
    let cancel_dir = output_dir.join("cancel");
    let handle = engine::start_download(DownloadRequest {
        track: long_track,
        preset: OutputPreset::Video(VideoFormat::Mp4, VideoQuality::P1080),
        output_dir: cancel_dir.clone(),
        embed_metadata_and_cover: true,
    })
    .map_err(fail)?;
    match wait_for_finish(&handle, Some(CANCEL_AFTER_FRACTION)) {
        Err(message) if message == "cancelled" => {}
        other => return Err(format!("cancel test ended with {other:?}")),
    }
    // Hidden entries included: `.riptide-partial` must be gone too.
    let leftovers: Vec<_> = std::fs::read_dir(&cancel_dir)
        .map_err(|error| error.to_string())?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    if !leftovers.is_empty() {
        return Err(format!("cancel left files behind: {leftovers:?}"));
    }
    println!("cancel left no files in {}", cancel_dir.display());

    for folder in [output_dir, short_dir.as_path()] {
        if folder.join(".riptide-partial").exists() {
            return Err(format!("{} still has .riptide-partial", folder.display()));
        }
    }
    Ok(())
}

fn download(
    track: &engine::Track,
    preset: OutputPreset,
    output_dir: &Path,
) -> Result<PathBuf, String> {
    let handle = engine::start_download(DownloadRequest {
        track: track.clone(),
        preset,
        output_dir: output_dir.to_path_buf(),
        embed_metadata_and_cover: true,
    })
    .map_err(|error| error.user_message_fr())?;
    wait_for_finish(&handle, None)
}

fn sha256_hex(file: &Path) -> Result<String, String> {
    let bytes = std::fs::read(file).map_err(|error| format!("{}: {error}", file.display()))?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn ffprobe_video_height(file: &Path) -> Result<u32, String> {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=height",
            "-of",
            "csv=p=0",
        ])
        .arg(file)
        .output()
        .map_err(|error| format!("ffprobe: {error}"))?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .map_err(|error| format!("ffprobe height of {}: {error}", file.display()))
}

/// Prints every event; with `cancel_after`, pauses, resumes then cancels past that fraction.
fn wait_for_finish(handle: &DownloadHandle, cancel_after: Option<f32>) -> Result<PathBuf, String> {
    let events = handle.events();
    let started = Instant::now();
    let mut output_file = None;
    let mut has_cancelled = false;
    let mut last_printed_tenth = -1;
    while let Ok(event) = events.recv_blocking() {
        match &event {
            DownloadEvent::Progress { fraction, .. } => {
                let tenth = (fraction * 10.0) as i32;
                if tenth != last_printed_tenth {
                    last_printed_tenth = tenth;
                    println!("{:>6.1}s {event:?}", started.elapsed().as_secs_f32());
                }
                if let Some(threshold) = cancel_after
                    && *fraction >= threshold
                    && !has_cancelled
                {
                    has_cancelled = true;
                    handle.pause().map_err(|error| error.to_string())?;
                    std::thread::sleep(PAUSE_DURATION);
                    handle.resume().map_err(|error| error.to_string())?;
                    handle.pause().map_err(|error| error.to_string())?;
                    // Cancelling a paused download must still work.
                    handle.cancel().map_err(|error| error.to_string())?;
                }
            }
            _ => println!("{:>6.1}s {event:?}", started.elapsed().as_secs_f32()),
        }
        match event {
            DownloadEvent::Finished { output_file: path } => output_file = Some(path),
            DownloadEvent::Failed(error) => return Err(error.user_message_fr()),
            DownloadEvent::StateChanged(DownloadState::Cancelled) => {
                return Err("cancelled".to_string());
            }
            _ => {}
        }
    }
    // The channel closes after the final event.
    output_file.ok_or_else(|| "channel closed without a result".to_string())
}
