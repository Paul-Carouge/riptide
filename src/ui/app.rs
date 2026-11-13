//! Main window state and behavior: tool preparation, link analysis, output choice, the
//! download queue (at most N concurrent engine downloads) and the persisted history.
//! Rendering lives in `render.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use gpui::{
    App, AppContext, AsyncApp, Context, Entity, FocusHandle, Focusable, KeyBinding,
    PathPromptOptions, Subscription, Task, WeakEntity, Window, WindowAppearance, actions,
};

use super::demo;
use super::motion::VinylSpin;
use super::text_input::TextInput;
use super::theme::{self, Palette};
use super::thumbnails::{ThumbnailCache, fetch_thumbnail};
use crate::engine::{
    self, AudioFormat, AudioQuality, DownloadEvent, DownloadHandle, DownloadRequest, DownloadState,
    EngineError, OutputPreset, ProbedLink, ToolStatus, Track, VideoFormat, VideoQuality,
};
use crate::store::{
    self, HistoryEntry, LatestWriteWins, MAXIMUM_CONCURRENT_DOWNLOADS,
    MINIMUM_CONCURRENT_DOWNLOADS, PersistedState, Settings, ThemeMode,
};

actions!(riptide, [FocusNext, FocusPrevious, AnalyzeLink, Dismiss]);

const TEXT_INPUT_CONTEXT: &str = "TextInput";
/// Debug-build test hooks for end-to-end runs without an input device: `RIPTIDE_LINK`
/// pre-fills the link field; `RIPTIDE_AUTOSTART=1` then analyzes it and queues every track.
/// Release builds ignore both.
const PREFILLED_LINK_ENVIRONMENT_VARIABLE: &str = "RIPTIDE_LINK";
const AUTOSTART_ENVIRONMENT_VARIABLE: &str = "RIPTIDE_AUTOSTART";
const HISTORY_THUMBNAILS_PRELOADED: usize = 50;
const INTERRUPTED_DOWNLOAD_MESSAGE: &str = "Le téléchargement s'est interrompu.";

pub fn bind_application_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrevious, None),
        KeyBinding::new("escape", Dismiss, None),
        KeyBinding::new("enter", AnalyzeLink, Some(TEXT_INPUT_CONTEXT)),
    ]);
}

pub enum ToolsState {
    Preparing,
    Ready(ToolStatus),
    Failed(String),
}

pub enum ProbeState {
    Idle,
    Loading,
    Ready(Preview),
    Failed(String),
}

pub struct Preview {
    pub link: ProbedLink,
    pub is_track_selected: Vec<bool>,
    /// Changes on every new analysis so the unfold animation replays.
    pub generation: u64,
}

impl Preview {
    pub fn selected_track_count(&self) -> usize {
        self.is_track_selected
            .iter()
            .filter(|is_selected| **is_selected)
            .count()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MediaChoice {
    Audio,
    Video,
}

/// The output picker remembers both audio and video choices so toggling loses nothing.
#[derive(Clone, Copy)]
pub struct OutputChoice {
    pub media: MediaChoice,
    pub audio_format: AudioFormat,
    pub audio_quality: AudioQuality,
    pub video_format: VideoFormat,
    pub video_quality: VideoQuality,
}

impl OutputChoice {
    fn from_preset(preset: OutputPreset) -> Self {
        let mut choice = Self {
            media: MediaChoice::Audio,
            audio_format: AudioFormat::Mp3,
            audio_quality: AudioQuality::Kbps320,
            video_format: VideoFormat::Mp4,
            video_quality: VideoQuality::P1080,
        };
        match preset {
            OutputPreset::Audio(format, quality) => {
                choice.audio_format = format;
                choice.audio_quality = quality;
            }
            OutputPreset::Video(format, quality) => {
                choice.media = MediaChoice::Video;
                choice.video_format = format;
                choice.video_quality = quality;
            }
        }
        choice
    }

    pub fn preset(&self) -> OutputPreset {
        match self.media {
            MediaChoice::Audio => OutputPreset::Audio(self.audio_format, self.audio_quality),
            MediaChoice::Video => OutputPreset::Video(self.video_format, self.video_quality),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SidePanelTab {
    Queue,
    History,
}

/// How a queue entry's download is driven.
enum DownloadControl {
    /// Waiting for a free slot, or `start_download` still running.
    NotStarted,
    Engine(Arc<DownloadHandle>),
    /// Fake progress for `RIPTIDE_DEMO=1` screenshots.
    Demo,
}

pub struct QueueEntry {
    pub id: u64,
    pub track: Track,
    pub preset: OutputPreset,
    pub preset_label: String,
    pub output_directory: PathBuf,
    pub embed_metadata_and_cover: bool,
    pub state: DownloadState,
    pub fraction: f32,
    pub shown_fraction: f32,
    pub speed_bytes_per_second: Option<u64>,
    pub eta_seconds: Option<u32>,
    pub error_message: Option<String>,
    pub output_file: Option<PathBuf>,
    pub spin: VinylSpin,
    /// "Annuler" was clicked; the engine has not confirmed yet.
    pub is_cancelling: bool,
    control: DownloadControl,
    _event_task: Option<Task<()>>,
}

impl QueueEntry {
    pub fn is_occupying_a_slot(&self) -> bool {
        matches!(
            self.state,
            DownloadState::Downloading | DownloadState::Converting | DownloadState::Paused
        )
    }

    pub fn is_finished(&self) -> bool {
        matches!(
            self.state,
            DownloadState::Done | DownloadState::Failed | DownloadState::Cancelled
        )
    }

    /// Updates the entry from one engine event. A finished entry is final: late or duplicate
    /// events (e.g. `Paused` arriving after `Cancelled`) are ignored.
    fn apply_event(&mut self, event: DownloadEvent, now: Instant) -> EventOutcome {
        let mut outcome = EventOutcome::default();
        if self.is_finished() {
            return outcome;
        }
        match event {
            DownloadEvent::Progress {
                fraction,
                speed_bytes_per_second,
                eta_seconds,
            } => {
                self.fraction = fraction.clamp(0.0, 1.0);
                self.speed_bytes_per_second = speed_bytes_per_second;
                self.eta_seconds = eta_seconds;
            }
            DownloadEvent::StateChanged(state) => {
                self.state = state;
                outcome.has_freed_a_slot = self.is_finished();
            }
            DownloadEvent::Finished { output_file } => {
                self.state = DownloadState::Done;
                self.fraction = 1.0;
                self.output_file = Some(output_file.clone());
                outcome.finished_history_entry = Some(HistoryEntry {
                    title: self.track.title.clone(),
                    uploader: self.track.uploader.clone(),
                    preset: self.preset,
                    output_file,
                    thumbnail_url: self.track.thumbnail_url.clone(),
                    finished_at_unix_seconds: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|duration| duration.as_secs())
                        .unwrap_or_default(),
                });
                outcome.has_freed_a_slot = true;
            }
            DownloadEvent::Failed(error) => {
                self.state = DownloadState::Failed;
                self.error_message = Some(error.user_message_fr());
                outcome.has_freed_a_slot = true;
            }
        }
        if self.is_finished() {
            self.is_cancelling = false;
            self.eta_seconds = None;
            self.speed_bytes_per_second = None;
        }
        let is_turning = matches!(
            self.state,
            DownloadState::Downloading | DownloadState::Converting
        );
        self.spin.set_playing(is_turning, now);
        outcome
    }

    /// Same track in the same format, still queued or running (double-click guard).
    fn is_duplicate_of(&self, track_id: &str, preset: OutputPreset) -> bool {
        !self.is_finished() && self.track.id == track_id && self.preset == preset
    }

    fn is_waiting_for_slot(&self) -> bool {
        self.state == DownloadState::Queued && matches!(self.control, DownloadControl::NotStarted)
    }
}

#[derive(Default)]
struct EventOutcome {
    has_freed_a_slot: bool,
    finished_history_entry: Option<HistoryEntry>,
}

pub struct RiptideApp {
    pub focus_handle: FocusHandle,
    pub link_input: Entity<TextInput>,
    pub tools: ToolsState,
    pub is_updating_yt_dlp: bool,
    pub tools_notice: Option<String>,
    pub link_error: Option<String>,
    pub link_error_generation: u64,
    pub probe: ProbeState,
    pub probe_generation: u64,
    pub output: OutputChoice,
    pub settings: Settings,
    pub history: Vec<HistoryEntry>,
    pub queue: Vec<QueueEntry>,
    pub active_tab: SidePanelTab,
    pub thumbnails: ThumbnailCache,
    pub loading_spin: VinylSpin,
    pub last_frame_at: Instant,
    pub is_demo: bool,
    pub is_system_dark: bool,
    theme_transition: Option<ThemeTransition>,
    should_autostart: bool,
    next_entry_id: u64,
    settings_writer: Option<Arc<LatestWriteWins>>,
    history_writer: Option<Arc<LatestWriteWins>>,
    _probe_task: Option<Task<()>>,
    _demo_task: Option<Task<()>>,
    _appearance_subscription: Subscription,
}

/// Cross-fade between two palettes after a theme change.
struct ThemeTransition {
    from: Palette,
    started_at: Instant,
}

fn is_dark_appearance(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

fn debug_environment_variable(name: &str) -> Option<String> {
    if cfg!(debug_assertions) {
        std::env::var(name).ok()
    } else {
        None
    }
}

impl Focusable for RiptideApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl RiptideApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, persisted: PersistedState) -> Self {
        let PersistedState {
            settings,
            history,
            settings_path,
            history_path,
        } = persisted;
        let link_input =
            cx.new(|cx| TextInput::new("Colle un lien YouTube : vidéo, Short ou playlist", cx));
        window.focus(&link_input.focus_handle(cx));
        let now = Instant::now();
        let is_demo = demo::is_demo_enabled();
        let appearance_subscription = cx.observe_window_appearance(window, |app, window, cx| {
            let is_system_dark = is_dark_appearance(window.appearance());
            if is_system_dark != app.is_system_dark {
                let previous = app.palette_now(Instant::now());
                app.is_system_dark = is_system_dark;
                app.start_theme_transition(previous);
                cx.notify();
            }
        });
        let mut app = Self {
            focus_handle: cx.focus_handle(),
            link_input,
            tools: ToolsState::Preparing,
            is_updating_yt_dlp: false,
            tools_notice: None,
            link_error: None,
            link_error_generation: 0,
            probe: ProbeState::Idle,
            probe_generation: 0,
            output: OutputChoice::from_preset(settings.default_preset),
            settings,
            history,
            queue: Vec::new(),
            active_tab: SidePanelTab::Queue,
            thumbnails: ThumbnailCache::default(),
            loading_spin: VinylSpin::at_rest(now),
            last_frame_at: now,
            is_demo,
            is_system_dark: is_dark_appearance(window.appearance()),
            theme_transition: None,
            should_autostart: debug_environment_variable(AUTOSTART_ENVIRONMENT_VARIABLE)
                .is_some_and(|value| value == "1"),
            next_entry_id: 0,
            settings_writer: settings_path.map(|path| Arc::new(LatestWriteWins::new(path))),
            history_writer: history_path.map(|path| Arc::new(LatestWriteWins::new(path))),
            _probe_task: None,
            _demo_task: None,
            _appearance_subscription: appearance_subscription,
        };
        if is_demo {
            demo::populate(&mut app, cx);
        } else {
            if let Some(link) = debug_environment_variable(PREFILLED_LINK_ENVIRONMENT_VARIABLE) {
                app.link_input
                    .update(cx, |input, cx| input.set_content(link, cx));
            }
            app.prepare_tools(cx);
            let history_thumbnails: Vec<String> = app
                .history
                .iter()
                .take(HISTORY_THUMBNAILS_PRELOADED)
                .filter_map(|entry| entry.thumbnail_url.clone())
                .collect();
            for url in history_thumbnails {
                app.request_thumbnail(&url, cx);
            }
        }
        app
    }

    // ---- Theme -------------------------------------------------------------------------

    fn target_palette(&self) -> Palette {
        let is_dark = match self.settings.theme_mode {
            ThemeMode::Auto => self.is_system_dark,
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
        };
        if is_dark { theme::DARK } else { theme::LIGHT }
    }

    /// The palette to paint at `now`, cross-fading after a change.
    pub fn palette_now(&self, now: Instant) -> Palette {
        let target = self.target_palette();
        match &self.theme_transition {
            Some(transition) => {
                let progress = now
                    .saturating_duration_since(transition.started_at)
                    .as_secs_f32()
                    / theme::THEME_CROSSFADE_DURATION.as_secs_f32();
                // Ease-out cubic: most of the change happens early, it settles softly.
                let eased = 1.0 - (1.0 - progress.min(1.0)).powi(3);
                transition.from.blended_toward(&target, eased)
            }
            None => target,
        }
    }

    fn start_theme_transition(&mut self, from: Palette) {
        self.theme_transition = Some(ThemeTransition {
            from,
            started_at: Instant::now(),
        });
    }

    pub fn set_theme_mode(&mut self, mode: ThemeMode, cx: &mut Context<Self>) {
        if self.settings.theme_mode == mode {
            return;
        }
        let previous = self.palette_now(Instant::now());
        self.settings.theme_mode = mode;
        self.start_theme_transition(previous);
        self.save_settings(cx);
        cx.notify();
    }

    // ---- Tools -------------------------------------------------------------------------

    pub fn prepare_tools(&mut self, cx: &mut Context<Self>) {
        self.tools = ToolsState::Preparing;
        self.tools_notice = None;
        cx.notify();
        // Leftovers of a crash are only safe to delete while nothing downloads.
        let stale_partial_directories =
            self.directories_safe_to_purge(std::slice::from_ref(&self.settings.output_directory));
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    if !stale_partial_directories.is_empty() {
                        engine::purge_stale_partial_downloads(&stale_partial_directories);
                    }
                    engine::ensure_tools()
                })
                .await;
            this.update(cx, |app, cx| {
                app.tools = match result {
                    Ok(status) => ToolsState::Ready(status),
                    Err(error) => ToolsState::Failed(error.user_message_fr()),
                };
                app.start_queued_downloads(cx);
                if app.should_autostart {
                    app.start_analysis(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The given output directories minus those a queued or running download writes to.
    fn directories_safe_to_purge(&self, directories: &[PathBuf]) -> Vec<PathBuf> {
        if self.is_demo {
            return Vec::new();
        }
        directories
            .iter()
            .filter(|directory| {
                !self
                    .queue
                    .iter()
                    .any(|entry| !entry.is_finished() && &entry.output_directory == *directory)
            })
            .cloned()
            .collect()
    }

    /// Re-checks local tools (no network), e.g. after the user installed ffmpeg.
    pub fn recheck_tools(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let status = cx.background_spawn(async { engine::tool_status() }).await;
            this.update(cx, |app, cx| {
                app.tools = ToolsState::Ready(status);
                app.start_queued_downloads(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn is_ready_to_download(&self) -> bool {
        self.is_demo
            || matches!(&self.tools, ToolsState::Ready(status)
                if status.ffmpeg_available && status.yt_dlp_version.is_some())
    }

    pub fn update_yt_dlp(&mut self, cx: &mut Context<Self>) {
        if self.is_updating_yt_dlp || self.is_demo {
            return;
        }
        self.is_updating_yt_dlp = true;
        self.tools_notice = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async { engine::update_yt_dlp() }).await;
            this.update(cx, |app, cx| {
                app.is_updating_yt_dlp = false;
                match result {
                    Ok(version) => {
                        app.tools_notice = Some(format!("yt-dlp est à jour ({version})."));
                        if let ToolsState::Ready(status) = &mut app.tools {
                            status.yt_dlp_version = Some(version);
                        }
                    }
                    Err(error) => app.tools_notice = Some(error.user_message_fr()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ---- Link analysis -----------------------------------------------------------------

    pub fn analyze_link(&mut self, _: &AnalyzeLink, _: &mut Window, cx: &mut Context<Self>) {
        self.start_analysis(cx);
    }

    pub fn start_analysis(&mut self, cx: &mut Context<Self>) {
        let input = self.link_input.read(cx).content().trim().to_owned();
        if input.is_empty() {
            self.show_link_error("Colle d'abord un lien YouTube.".to_owned(), cx);
            return;
        }
        if self.is_demo {
            self.link_error = None;
            self.show_preview(demo::playlist(), cx);
            return;
        }
        let url = match engine::validate_youtube_url(&input) {
            Ok(url) => url,
            Err(error) => {
                self.show_link_error(error.user_message_fr(), cx);
                return;
            }
        };
        if !matches!(&self.tools, ToolsState::Ready(status) if status.yt_dlp_version.is_some()) {
            self.show_link_error(
                "yt-dlp n'est pas encore prêt, patiente un instant.".to_owned(),
                cx,
            );
            return;
        }
        self.link_error = None;
        self.probe = ProbeState::Loading;
        self.loading_spin.set_playing(true, Instant::now());
        cx.notify();
        self._probe_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { engine::probe_link(&url) })
                .await;
            this.update(cx, |app, cx| match result {
                Ok(link) => app.show_preview(link, cx),
                Err(error) => {
                    app.should_autostart = false;
                    app.loading_spin.set_playing(false, Instant::now());
                    app.probe = ProbeState::Failed(error.user_message_fr());
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn show_link_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.link_error = Some(message);
        self.link_error_generation += 1;
        cx.notify();
    }

    pub fn show_preview(&mut self, link: ProbedLink, cx: &mut Context<Self>) {
        self.loading_spin.set_playing(false, Instant::now());
        self.probe_generation += 1;
        // Track thumbnails load when their rows become visible (see `render_track_list`).
        if let Some(url) = &link.thumbnail_url {
            self.request_thumbnail(url, cx);
        }
        self.probe = ProbeState::Ready(Preview {
            is_track_selected: vec![true; link.tracks.len()],
            link,
            generation: self.probe_generation,
        });
        if std::mem::take(&mut self.should_autostart) {
            self.enqueue_selected_tracks(cx);
        }
        cx.notify();
    }

    pub fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        if self.link_error.take().is_some() {
        } else if !matches!(self.probe, ProbeState::Idle) {
            self.probe = ProbeState::Idle;
            self.loading_spin.set_playing(false, Instant::now());
            self._probe_task = None;
        } else {
            self.link_input
                .update(cx, |input, cx| input.set_content("", cx));
        }
        cx.notify();
    }

    pub fn toggle_track(&mut self, index: usize, cx: &mut Context<Self>) {
        if let ProbeState::Ready(preview) = &mut self.probe
            && let Some(is_selected) = preview.is_track_selected.get_mut(index)
        {
            *is_selected = !*is_selected;
            cx.notify();
        }
    }

    pub fn set_all_tracks_selected(&mut self, is_selected: bool, cx: &mut Context<Self>) {
        if let ProbeState::Ready(preview) = &mut self.probe {
            preview.is_track_selected.fill(is_selected);
            cx.notify();
        }
    }

    // ---- Output choice -----------------------------------------------------------------

    pub fn set_output(&mut self, output: OutputChoice, cx: &mut Context<Self>) {
        self.output = output;
        self.settings.default_preset = output.preset();
        self.save_settings(cx);
        cx.notify();
    }

    pub fn toggle_embed_metadata(&mut self, cx: &mut Context<Self>) {
        self.settings.embed_metadata_and_cover = !self.settings.embed_metadata_and_cover;
        self.save_settings(cx);
        cx.notify();
    }

    pub fn change_concurrency(&mut self, delta: i8, cx: &mut Context<Self>) {
        let current = self.settings.clamped_concurrent_downloads() as i8;
        self.settings.maximum_concurrent_downloads = (current + delta).clamp(
            MINIMUM_CONCURRENT_DOWNLOADS as i8,
            MAXIMUM_CONCURRENT_DOWNLOADS as i8,
        ) as u8;
        self.save_settings(cx);
        self.start_queued_downloads(cx);
        cx.notify();
    }

    pub fn choose_output_directory(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choisir ce dossier".into()),
        });
        cx.spawn(async move |this, cx| {
            let chosen = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    eprintln!("riptide: folder picker failed: {error}");
                    None
                }
            };
            if let Some(directory) = chosen {
                this.update(cx, |app, cx| {
                    let purgeable = app.directories_safe_to_purge(std::slice::from_ref(&directory));
                    if !purgeable.is_empty() {
                        cx.background_spawn(async move {
                            engine::purge_stale_partial_downloads(&purgeable);
                        })
                        .detach();
                    }
                    app.settings.output_directory = directory;
                    app.save_settings(cx);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn save_settings(&self, cx: &mut Context<Self>) {
        if self.is_demo {
            return;
        }
        let Some(writer) = self.settings_writer.clone() else {
            return;
        };
        let revision = writer.next_revision();
        let settings = self.settings.clone();
        cx.background_spawn(async move {
            if let Err(error) = writer.write(revision, &settings) {
                eprintln!("riptide: cannot save settings: {error}");
            }
        })
        .detach();
    }

    fn save_history(&self, cx: &mut Context<Self>) {
        if self.is_demo {
            return;
        }
        let Some(writer) = self.history_writer.clone() else {
            return;
        };
        let revision = writer.next_revision();
        let history = self.history.clone();
        cx.background_spawn(async move {
            if let Err(error) = writer.write(revision, &history) {
                eprintln!("riptide: cannot save history: {error}");
            }
        })
        .detach();
    }

    // ---- Queue -------------------------------------------------------------------------

    /// Adds the checked tracks of the preview to the queue.
    pub fn enqueue_selected_tracks(&mut self, cx: &mut Context<Self>) {
        let ProbeState::Ready(preview) = &self.probe else {
            return;
        };
        let tracks: Vec<Track> = preview
            .link
            .tracks
            .iter()
            .zip(&preview.is_track_selected)
            .filter(|(_, is_selected)| **is_selected)
            .map(|(track, _)| track.clone())
            .collect();
        let preset = self.output.preset();
        for track in tracks {
            let is_already_queued = self
                .queue
                .iter()
                .any(|entry| entry.is_duplicate_of(&track.id, preset));
            if !is_already_queued {
                self.push_queue_entry(track, preset);
            }
        }
        self.active_tab = SidePanelTab::Queue;
        self.start_queued_downloads(cx);
        cx.notify();
    }

    pub fn push_queue_entry(&mut self, track: Track, preset: OutputPreset) -> u64 {
        let id = self.next_entry_id;
        self.next_entry_id += 1;
        self.queue.push(QueueEntry {
            id,
            track,
            preset,
            preset_label: preset.label_fr(),
            output_directory: self.settings.output_directory.clone(),
            embed_metadata_and_cover: self.settings.embed_metadata_and_cover,
            state: DownloadState::Queued,
            fraction: 0.0,
            shown_fraction: 0.0,
            speed_bytes_per_second: None,
            eta_seconds: None,
            error_message: None,
            output_file: None,
            spin: VinylSpin::at_rest(Instant::now()),
            is_cancelling: false,
            control: DownloadControl::NotStarted,
            _event_task: None,
        });
        id
    }

    /// Starts waiting entries while fewer than N downloads occupy a slot.
    pub fn start_queued_downloads(&mut self, cx: &mut Context<Self>) {
        if !self.is_ready_to_download() {
            return;
        }
        let limit = usize::from(self.settings.clamped_concurrent_downloads());
        loop {
            let occupied = self
                .queue
                .iter()
                .filter(|entry| entry.is_occupying_a_slot())
                .count();
            if occupied >= limit {
                return;
            }
            let Some(entry) = self
                .queue
                .iter_mut()
                .find(|entry| entry.is_waiting_for_slot())
            else {
                return;
            };
            entry.state = DownloadState::Downloading;
            entry.spin.set_playing(true, Instant::now());
            if self.is_demo {
                entry.control = DownloadControl::Demo;
                continue;
            }
            let id = entry.id;
            let request = DownloadRequest {
                track: entry.track.clone(),
                preset: entry.preset,
                output_dir: entry.output_directory.clone(),
                embed_metadata_and_cover: entry.embed_metadata_and_cover,
            };
            entry._event_task = Some(cx.spawn(async move |this, cx| {
                run_download(this, id, request, cx).await;
            }));
        }
    }

    fn entry_mut(&mut self, id: u64) -> Option<&mut QueueEntry> {
        self.queue.iter_mut().find(|entry| entry.id == id)
    }

    pub fn apply_download_event(&mut self, id: u64, event: DownloadEvent, cx: &mut Context<Self>) {
        let Some(entry) = self.entry_mut(id) else {
            return;
        };
        let outcome = entry.apply_event(event, Instant::now());
        if let Some(history_entry) = outcome.finished_history_entry {
            store::add_history_entry(&mut self.history, history_entry);
            self.save_history(cx);
        }
        if outcome.has_freed_a_slot {
            self.start_queued_downloads(cx);
        }
        cx.notify();
    }

    /// The engine's event stream ended; anything not finished by then was interrupted.
    fn finish_event_stream(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(entry) = self.entry_mut(id)
            && !entry.is_finished()
        {
            self.apply_download_event(
                id,
                DownloadEvent::Failed(EngineError::DownloadFailed(
                    INTERRUPTED_DOWNLOAD_MESSAGE.to_owned(),
                )),
                cx,
            );
        }
    }

    fn mark_start_failed(&mut self, id: u64, error: EngineError, cx: &mut Context<Self>) {
        self.apply_download_event(id, DownloadEvent::Failed(error), cx);
    }

    pub fn toggle_pause(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(entry) = self.entry_mut(id) else {
            return;
        };
        if entry.is_cancelling || entry.is_finished() {
            return;
        }
        let should_pause = entry.state != DownloadState::Paused;
        match &entry.control {
            // The demo has no engine to report the new state: simulate it.
            DownloadControl::Demo => {
                let state = if should_pause {
                    DownloadState::Paused
                } else {
                    DownloadState::Downloading
                };
                self.apply_download_event(id, DownloadEvent::StateChanged(state), cx);
            }
            // The engine emits StateChanged(Paused / Downloading) itself.
            DownloadControl::Engine(handle) => {
                let handle = handle.clone();
                cx.background_spawn(async move {
                    let result = if should_pause {
                        handle.pause()
                    } else {
                        handle.resume()
                    };
                    if let Err(error) = result {
                        eprintln!("riptide: pause/resume failed: {error}");
                    }
                })
                .detach();
            }
            DownloadControl::NotStarted => {}
        }
    }

    pub fn cancel_entry(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(entry) = self.entry_mut(id) else {
            return;
        };
        if entry.is_finished() || entry.is_cancelling {
            return;
        }
        match &entry.control {
            DownloadControl::Engine(handle) => {
                let handle = handle.clone();
                entry.is_cancelling = true;
                cx.notify();
                cx.spawn(async move |this, cx| {
                    let result = cx.background_spawn(async move { handle.cancel() }).await;
                    // On success the engine cleans up, then reports Cancelled on the channel.
                    if let Err(error) = result {
                        this.update(cx, |app, cx| app.mark_start_failed(id, error, cx))
                            .ok();
                    }
                })
                .detach();
            }
            // A start in flight sees the Cancelled state and cancels its handle on arrival.
            DownloadControl::Demo | DownloadControl::NotStarted => {
                self.apply_download_event(
                    id,
                    DownloadEvent::StateChanged(DownloadState::Cancelled),
                    cx,
                );
            }
        }
    }

    /// Puts a failed entry back in the queue with the same request (track, format, folder).
    pub fn retry_entry(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(entry) = self.entry_mut(id) else {
            return;
        };
        if entry.state != DownloadState::Failed {
            return;
        }
        entry.state = DownloadState::Queued;
        entry.fraction = 0.0;
        entry.shown_fraction = 0.0;
        entry.error_message = None;
        entry.output_file = None;
        entry.is_cancelling = false;
        entry.control = DownloadControl::NotStarted;
        // Dropping the old task stops listening to the failed run's event stream.
        entry._event_task = None;
        self.start_queued_downloads(cx);
        cx.notify();
    }

    pub fn remove_entry(&mut self, id: u64, cx: &mut Context<Self>) {
        self.queue
            .retain(|entry| entry.id != id || !entry.is_finished());
        cx.notify();
    }

    pub fn open_folder(&self, path: &Path, cx: &mut Context<Self>) {
        let path = path.to_path_buf();
        // Argument vector, no shell: the path is passed verbatim to xdg-open.
        cx.background_spawn(async move {
            let directory = if path.is_dir() {
                path
            } else {
                path.parent().map(Path::to_path_buf).unwrap_or_default()
            };
            match std::process::Command::new("xdg-open")
                .arg(&directory)
                .status()
            {
                Ok(status) if status.success() => {}
                Ok(status) => eprintln!("riptide: xdg-open exited with {status}"),
                Err(error) => eprintln!("riptide: cannot run xdg-open: {error}"),
            }
        })
        .detach();
    }

    // ---- Thumbnails and animation ------------------------------------------------------

    /// Asks for a thumbnail; downloads run at most six at a time, newest request first.
    pub fn request_thumbnail(&mut self, url: &str, cx: &mut Context<Self>) {
        let starting = self.thumbnails.request(url);
        self.start_thumbnail_downloads(starting, cx);
    }

    fn start_thumbnail_downloads(&mut self, urls: Vec<String>, cx: &mut Context<Self>) {
        for url in urls {
            cx.spawn(async move |this, cx| {
                let fetch_url = url.clone();
                let result = cx
                    .background_spawn(async move { fetch_thumbnail(&fetch_url) })
                    .await;
                this.update(cx, |app, cx| {
                    let next = app.thumbnails.finish_loading(url, result);
                    app.start_thumbnail_downloads(next, cx);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// Advances per-frame animation state; returns true while something still moves.
    pub fn advance_animations(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last_frame_at);
        self.last_frame_at = now;
        let mut is_animating = self.loading_spin.is_moving(now);
        if let Some(transition) = &self.theme_transition {
            if now.saturating_duration_since(transition.started_at)
                >= theme::THEME_CROSSFADE_DURATION
            {
                self.theme_transition = None;
            } else {
                is_animating = true;
            }
        }
        for entry in &mut self.queue {
            entry.shown_fraction =
                super::motion::smooth_toward(entry.shown_fraction, entry.fraction, elapsed);
            is_animating |= entry.spin.is_moving(now) || entry.shown_fraction != entry.fraction;
        }
        is_animating
    }

    pub fn set_demo_task(&mut self, task: Task<()>) {
        self._demo_task = Some(task);
    }

    pub fn demo_entry_ids(&self) -> Vec<u64> {
        self.queue
            .iter()
            .filter(|entry| matches!(entry.control, DownloadControl::Demo))
            .map(|entry| entry.id)
            .collect()
    }
}

async fn run_download(
    this: WeakEntity<RiptideApp>,
    id: u64,
    request: DownloadRequest,
    cx: &mut AsyncApp,
) {
    let started = cx
        .background_spawn(async move { engine::start_download(request) })
        .await;
    let handle = match started {
        Ok(handle) => Arc::new(handle),
        Err(error) => {
            this.update(cx, |app, cx| app.mark_start_failed(id, error, cx))
                .ok();
            return;
        }
    };
    let events = handle.events();
    let is_still_wanted = this
        .update(cx, |app, _| match app.entry_mut(id) {
            Some(entry) if !entry.is_finished() => {
                entry.control = DownloadControl::Engine(handle.clone());
                true
            }
            _ => false,
        })
        .unwrap_or(false);
    if !is_still_wanted {
        let cancelled = cx.background_spawn(async move { handle.cancel() }).await;
        if let Err(error) = cancelled {
            eprintln!("riptide: cannot cancel a download started after removal: {error}");
        }
        return;
    }
    while let Ok(event) = events.recv().await {
        if this
            .update(cx, |app, cx| app.apply_download_event(id, event, cx))
            .is_err()
        {
            return;
        }
    }
    this.update(cx, |app, cx| app.finish_event_stream(id, cx))
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_entry(state: DownloadState) -> QueueEntry {
        QueueEntry {
            id: 1,
            track: Track {
                id: "jNQXAC9IVRw".to_owned(),
                title: "Me at the zoo".to_owned(),
                uploader: None,
                duration_seconds: Some(19),
                thumbnail_url: None,
                url: "https://www.youtube.com/watch?v=jNQXAC9IVRw".to_owned(),
            },
            preset: OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Kbps320),
            preset_label: String::new(),
            output_directory: PathBuf::from("/music"),
            embed_metadata_and_cover: true,
            state,
            fraction: 0.3,
            shown_fraction: 0.3,
            speed_bytes_per_second: None,
            eta_seconds: None,
            error_message: None,
            output_file: None,
            spin: VinylSpin::at_rest(Instant::now()),
            is_cancelling: false,
            control: DownloadControl::NotStarted,
            _event_task: None,
        }
    }

    #[test]
    fn events_after_a_final_state_are_ignored() {
        for final_state in [
            DownloadState::Cancelled,
            DownloadState::Done,
            DownloadState::Failed,
        ] {
            let mut entry = queue_entry(final_state);
            for event in [
                DownloadEvent::StateChanged(DownloadState::Paused),
                DownloadEvent::StateChanged(DownloadState::Downloading),
                DownloadEvent::Progress {
                    fraction: 0.9,
                    speed_bytes_per_second: Some(1),
                    eta_seconds: Some(1),
                },
                DownloadEvent::Finished {
                    output_file: PathBuf::from("/music/x.mp3"),
                },
            ] {
                let outcome = entry.apply_event(event, Instant::now());
                assert!(!outcome.has_freed_a_slot);
                assert!(outcome.finished_history_entry.is_none());
            }
            assert_eq!(entry.state, final_state);
            assert_eq!(entry.fraction, 0.3);
            assert!(entry.output_file.is_none());
        }
    }

    #[test]
    fn cancel_confirmation_ends_the_cancelling_state() {
        let mut entry = queue_entry(DownloadState::Downloading);
        entry.is_cancelling = true;
        let outcome = entry.apply_event(
            DownloadEvent::StateChanged(DownloadState::Cancelled),
            Instant::now(),
        );
        assert!(outcome.has_freed_a_slot);
        assert!(!entry.is_cancelling);
    }

    #[test]
    fn duplicate_guard_matches_same_track_and_preset_while_unfinished() {
        let entry = queue_entry(DownloadState::Queued);
        let mp3 = OutputPreset::Audio(AudioFormat::Mp3, AudioQuality::Kbps320);
        let flac = OutputPreset::Audio(AudioFormat::Flac, AudioQuality::Best);
        assert!(entry.is_duplicate_of("jNQXAC9IVRw", mp3));
        assert!(!entry.is_duplicate_of("jNQXAC9IVRw", flac));
        assert!(!queue_entry(DownloadState::Done).is_duplicate_of("jNQXAC9IVRw", mp3));
    }
}
