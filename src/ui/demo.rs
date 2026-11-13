//! `RIPTIDE_DEMO=1`: fills the window with fake but animated data, without calling the engine
//! (no yt-dlp, no downloads). Used for screenshots and visual checks. Nothing is persisted.

use std::path::PathBuf;
use std::time::Duration;

use gpui::Context;

use super::app::{RiptideApp, ToolsState};
use crate::engine::{
    AudioFormat, AudioQuality, DownloadEvent, DownloadState, EngineError, MediaKind, OutputPreset,
    ProbedLink, ToolStatus, Track, VideoFormat, VideoQuality,
};

const DEMO_ENVIRONMENT_VARIABLE: &str = "RIPTIDE_DEMO";
const DEMO_LINK: &str = "https://www.youtube.com/playlist?list=PLriptide-demo";
const DEMO_TICK: Duration = Duration::from_millis(350);
const DEMO_PROGRESS_PER_TICK: f32 = 0.012;
const DEMO_BYTES_PER_SECOND: u64 = 2_400_000;
const DEMO_SECONDS_FOR_FULL_TRACK: f32 = 30.0;
/// 2026-11-13, fixed so screenshots are reproducible.
const DEMO_HISTORY_TIMESTAMP: u64 = 1_794_571_200;

/// Tracks in the `RIPTIDE_DEMO=big` playlist (virtualized list stress test).
const BIG_PLAYLIST_TRACK_COUNT: usize = 1500;

/// Debug builds (or the `test-hooks` feature) only. `RIPTIDE_DEMO=1` full scene; `=empty` no
/// preview and empty queue; `=history` opens the history tab; `=big` a 1500-track playlist, all
/// queued.
pub fn is_demo_enabled() -> bool {
    cfg!(any(debug_assertions, feature = "test-hooks"))
        && std::env::var_os(DEMO_ENVIRONMENT_VARIABLE).is_some_and(|value| value != "0")
}

fn demo_scene() -> String {
    std::env::var(DEMO_ENVIRONMENT_VARIABLE).unwrap_or_default()
}

fn demo_track(id: &str, title: &str, uploader: &str, duration_seconds: u32) -> Track {
    Track {
        id: id.to_owned(),
        title: title.to_owned(),
        uploader: Some(uploader.to_owned()),
        duration_seconds: Some(duration_seconds),
        thumbnail_url: Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")),
        url: format!("https://www.youtube.com/watch?v={id}"),
    }
}

pub fn playlist() -> ProbedLink {
    let tracks = vec![
        demo_track("jNQXAC9IVRw", "Me at the zoo", "jawed", 19),
        demo_track(
            "fJ9rUzIMcZQ",
            "Bohemian Rhapsody (Official Video Remastered)",
            "Queen Official",
            359,
        ),
        demo_track(
            "dQw4w9WgXcQ",
            "Never Gonna Give You Up (Official Video)",
            "Rick Astley",
            213,
        ),
        demo_track(
            "9bZkp7q19f0",
            "PSY - GANGNAM STYLE (강남스타일) M/V",
            "officialpsy",
            252,
        ),
        demo_track(
            "kJQP7kiw5Fk",
            "Luis Fonsi - Despacito ft. Daddy Yankee",
            "LuisFonsiVEVO",
            282,
        ),
        demo_track(
            "YQHsXMglC9A",
            "Adele - Hello (Official Music Video)",
            "AdeleVEVO",
            367,
        ),
    ];
    ProbedLink {
        kind: MediaKind::Playlist,
        title: "Soirée vinyle — face A".to_owned(),
        uploader: Some("Riptide démo".to_owned()),
        thumbnail_url: tracks[1].thumbnail_url.clone(),
        tracks,
    }
}

fn big_playlist() -> ProbedLink {
    let mut link = playlist();
    let first_generated = link.tracks.len() + 1;
    for number in first_generated..=BIG_PLAYLIST_TRACK_COUNT {
        link.tracks.push(demo_track(
            &format!("demo{number:05}"),
            &format!("Face cachée n°{number} — prise alternative"),
            "Riptide démo",
            120 + (number as u32 * 37) % 300,
        ));
    }
    link.title = format!("Intégrale démo — {BIG_PLAYLIST_TRACK_COUNT} pistes");
    link
}

pub fn populate(app: &mut RiptideApp, cx: &mut Context<RiptideApp>) {
    app.tools = ToolsState::Ready(ToolStatus {
        yt_dlp_version: Some("2025.10.22".to_owned()),
        ffmpeg_available: true,
    });
    app.settings.output_directory = PathBuf::from("~/Musique/Riptide");
    let scene = demo_scene();
    if scene == "empty" {
        return;
    }
    app.link_input
        .update(cx, |input, cx| input.set_content(DEMO_LINK, cx));
    let is_big = scene == "big";
    let link = if is_big { big_playlist() } else { playlist() };
    app.show_preview(link.clone(), cx);
    if let super::app::ProbeState::Ready(preview) = &mut app.probe {
        preview.is_track_selected[3] = false;
    }

    let mp3 = OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Kbps320);
    let flac = OutputPreset::Audio(AudioFormat::Flac, AudioQuality::Best);
    let mp4 = OutputPreset::Video(VideoFormat::Mp4, VideoQuality::P1080);
    let finished = app.push_queue_entry(link.tracks[0].clone(), mp3);
    let failed = app.push_queue_entry(link.tracks[5].clone(), mp3);
    let downloading = app.push_queue_entry(link.tracks[1].clone(), flac);
    let paused = app.push_queue_entry(link.tracks[2].clone(), mp3);
    let converting = app.push_queue_entry(link.tracks[4].clone(), mp4);
    app.push_queue_entry(link.tracks[3].clone(), mp3);
    if is_big {
        for track in link.tracks.iter().skip(6) {
            app.push_queue_entry(track.clone(), mp3);
        }
    }

    app.settings.maximum_concurrent_downloads = 3;
    app.start_queued_downloads(cx);
    app.apply_download_event(
        finished,
        DownloadEvent::Finished {
            output_file: PathBuf::from("~/Musique/Riptide/Me at the zoo.mp3"),
        },
        cx,
    );
    app.apply_download_event(downloading, progress(0.42), cx);
    app.apply_download_event(paused, progress(0.67), cx);
    app.apply_download_event(
        paused,
        DownloadEvent::StateChanged(DownloadState::Paused),
        cx,
    );
    app.start_queued_downloads(cx);
    app.apply_download_event(converting, progress(1.0), cx);
    app.apply_download_event(
        converting,
        DownloadEvent::StateChanged(DownloadState::Converting),
        cx,
    );
    app.apply_download_event(
        failed,
        DownloadEvent::Failed(EngineError::DownloadFailed("Video unavailable".to_owned())),
        cx,
    );
    for entry in &mut app.queue {
        entry.shown_fraction = entry.fraction;
    }
    if scene == "history" {
        for (index, track) in link.tracks.iter().enumerate().skip(1).rev() {
            crate::store::add_history_entry(
                &mut app.history,
                crate::store::HistoryEntry {
                    title: track.title.clone(),
                    uploader: track.uploader.clone(),
                    preset: if index % 2 == 0 { mp4 } else { flac },
                    output_file: PathBuf::from(format!("~/Musique/Riptide/{}.flac", track.title)),
                    thumbnail_url: track.thumbnail_url.clone(),
                    finished_at_unix_seconds: DEMO_HISTORY_TIMESTAMP - index as u64 * 86_400,
                },
            );
        }
        app.active_tab = super::app::SidePanelTab::History;
    }

    let ticker = cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(DEMO_TICK).await;
            let is_alive = this.update(cx, advance_demo_downloads).is_ok();
            if !is_alive {
                return;
            }
        }
    });
    app.set_demo_task(ticker);
}

fn progress(fraction: f32) -> DownloadEvent {
    DownloadEvent::Progress {
        fraction,
        speed_bytes_per_second: Some(DEMO_BYTES_PER_SECOND),
        eta_seconds: Some(((1.0 - fraction) * DEMO_SECONDS_FOR_FULL_TRACK) as u32),
    }
}

fn advance_demo_downloads(app: &mut RiptideApp, cx: &mut Context<RiptideApp>) {
    for id in app.demo_entry_ids() {
        let Some(entry) = app.queue.iter().find(|entry| entry.id == id) else {
            continue;
        };
        let event = match entry.state {
            DownloadState::Downloading if entry.fraction >= 1.0 => {
                DownloadEvent::StateChanged(DownloadState::Converting)
            }
            DownloadState::Downloading => {
                progress((entry.fraction + DEMO_PROGRESS_PER_TICK).min(1.0))
            }
            // Converting cards stay converting so screenshots always show one.
            _ => continue,
        };
        app.apply_download_event(id, event, cx);
    }
}
