//! Real-network check of OGG output and Suno links.
//!
//! `cargo run --example ogg_and_suno -- <output folder> <public Suno link> [other Suno link…]`
//!
//! Downloads a YouTube video to OGG and MP3, then each Suno link to MP3 and OGG, and prints
//! ffprobe's codec and tags for every file. A Suno link Suno serves only encrypted must fail
//! with `SunoSongNotDownloadable`, which is printed, not treated as a failure.

#[allow(dead_code, unused_imports)]
#[path = "../src/engine/mod.rs"]
mod engine;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use engine::{
    AudioFormat, AudioQuality, DownloadEvent, DownloadRequest, EngineError, OutputPreset, Track,
};

const YOUTUBE_URL: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let Some(output_dir) = arguments.next().map(PathBuf::from) else {
        eprintln!("usage: ogg_and_suno <output folder> <Suno link>…");
        return ExitCode::FAILURE;
    };
    let suno_links: Vec<String> = arguments.collect();
    match run(&output_dir, &suno_links) {
        Ok(()) => {
            println!("OGG AND SUNO OK");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("OGG AND SUNO FAILED: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(output_dir: &Path, suno_links: &[String]) -> Result<(), String> {
    let fail = |error: EngineError| format!("{error} / {}", error.user_message_fr());
    engine::ensure_tools().map_err(fail)?;
    let ogg = OutputPreset::Audio(AudioFormat::Ogg, AudioQuality::Kbps320);
    let mp3 = OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Kbps320);

    let youtube = engine::probe_link(YOUTUBE_URL).map_err(fail)?;
    for preset in [ogg, mp3] {
        check_file(&download(&youtube.tracks[0], preset, output_dir)?)?;
    }

    for link in suno_links {
        println!("--- Suno {link}");
        let probed = match engine::probe_link(link) {
            Err(EngineError::SunoSongNotDownloadable) => {
                println!(
                    "not downloadable (expected for non-public songs): {}",
                    EngineError::SunoSongNotDownloadable.user_message_fr()
                );
                continue;
            }
            other => other.map_err(fail)?,
        };
        println!("probe: {probed:#?}");
        for preset in [mp3, ogg] {
            check_file(&download(&probed.tracks[0], preset, output_dir)?)?;
        }
    }
    Ok(())
}

fn download(track: &Track, preset: OutputPreset, output_dir: &Path) -> Result<PathBuf, String> {
    println!("--- download « {} » as {}", track.title, preset.label_fr());
    let handle = engine::start_download(DownloadRequest {
        track: track.clone(),
        preset,
        output_dir: output_dir.to_path_buf(),
        embed_metadata_and_cover: true,
    })
    .map_err(|error| error.user_message_fr())?;
    let events = handle.events();
    let mut output_file = None;
    let mut last_fraction = None;
    while let Ok(event) = events.recv_blocking() {
        match event {
            DownloadEvent::Finished { output_file: path } => output_file = Some(path),
            DownloadEvent::Failed(error) => return Err(error.user_message_fr()),
            DownloadEvent::Progress { fraction, .. } => last_fraction = Some(fraction),
            other => println!("{other:?}"),
        }
    }
    println!("last progress fraction: {last_fraction:?}");
    output_file.ok_or_else(|| "channel closed without a result".to_string())
}

/// Prints codecs (a `mjpeg`/`png` stream is the embedded cover) and title/artist tags.
fn check_file(file: &Path) -> Result<(), String> {
    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name:format_tags=title,artist:stream_tags=title,artist",
            "-of",
            "compact",
        ])
        .arg(file)
        .output()
        .map_err(|error| format!("ffprobe: {error}"))?;
    if !output.status.success() {
        return Err(format!("ffprobe rejected {}", file.display()));
    }
    println!(
        "{}\n{}",
        file.display(),
        String::from_utf8_lossy(&output.stdout).trim()
    );
    Ok(())
}
