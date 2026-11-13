//! The main window's layout: header, link bar, preview, output picker (left, "Face A")
//! and the queue / history panel (right, "Face B").

use std::f32::consts::PI;
use std::ops::Range;
use std::time::Instant;

use gpui::{
    Animation, AnimationExt, AnyElement, Context, Decorations, Div, Entity, Focusable, FontWeight,
    MouseButton, ObjectFit, Pixels, Rgba, SharedString, StyleRefinement, Window, div, img,
    linear_color_stop, linear_gradient, prelude::*, pulsating_between, px, relative, uniform_list,
};

use super::app::{
    AnalyzeLink, Dismiss, FocusNext, FocusPrevious, MediaChoice, OutputChoice, ProbeState,
    QueueEntry, RiptideApp, SidePanelTab, ToolsState,
};
use super::components::{
    ButtonVariant, button, checkbox_row, overline, pill, section_card, section_title, selector_chip,
};
use super::format;
use super::motion::spring_progress;
use super::paper_grain::paper_grain;
use super::region::{self, Region, RegionKind};
use super::theme;
use super::vinyl::vinyl_disc;
use super::window_frame;
use crate::engine::{
    AudioFormat, AudioQuality, DownloadState, MediaKind, VideoFormat, VideoQuality,
};
use crate::store::{HistoryEntry, ThemeMode};

const FFMPEG_INSTALL_HINT: &str = "Installe ffmpeg : sudo pacman -S ffmpeg";

impl Render for RiptideApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let is_animating = self.advance_animations(now);
        if self.is_theme_crossfading() {
            // The whole window changes color: full-rate frames, for the short cross-fade only.
            window.request_animation_frame();
        }
        self.note_frame_drawn(is_animating, cx);
        self.visible_queue_card_ids.clear();
        theme::set_active_palette(self.palette_now(now));
        let paper_grain = self.region(RegionKind::PaperGrain, cx);
        let face_a = self.region(RegionKind::FaceA, cx);
        let tools_pulse = self.region(RegionKind::ToolsPulse, cx);
        let content = div()
            .id("riptide")
            .key_context("Riptide")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &FocusNext, window, _| window.focus_next()))
            .on_action(cx.listener(|_, _: &FocusPrevious, window, _| window.focus_prev()))
            .on_action(cx.listener(|app, action: &AnalyzeLink, window, cx| {
                app.analyze_link(action, window, cx)
            }))
            .on_action(
                cx.listener(|app, action: &Dismiss, window, cx| app.dismiss(action, window, cx)),
            )
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::palette().paper)
            .font_family(theme::FONT_BODY)
            .text_size(theme::TEXT_MD)
            .text_color(theme::palette().ink)
            .child(region::cached(
                &paper_grain,
                StyleRefinement::default().absolute().size_full(),
            ))
            .child(self.render_header(window, cx))
            .children(self.render_tools_banner(&tools_pulse, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap(theme::SPACE_5)
                    .px(theme::SPACE_5)
                    .child(region::cached(
                        &face_a,
                        StyleRefinement::default()
                            .flex_1()
                            .min_w_0()
                            .max_w(theme::LEFT_COLUMN_WIDTH),
                    ))
                    .child(
                        div()
                            .w(theme::BORDER)
                            .my(theme::SPACE_5)
                            .bg(theme::palette().rule),
                    )
                    .child(self.render_side_panel(
                        window.viewport_size().width < theme::COMPACT_WINDOW_WIDTH,
                        cx,
                    )),
            );
        window_frame::client_frame(content, window)
    }
}

impl RiptideApp {
    /// Content of a cached region (see `region.rs`).
    pub fn render_region(
        &self,
        kind: RegionKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = Instant::now();
        match kind {
            RegionKind::PaperGrain => paper_grain().into_any_element(),
            RegionKind::FaceA => div()
                .id("face-a")
                .size_full()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(theme::SPACE_4)
                .py(theme::SPACE_5)
                .child(self.render_link_bar(window, cx))
                .children(self.render_preview(now, cx))
                .child(self.render_output_picker(cx))
                .into_any_element(),
            RegionKind::ToolsPulse => div()
                .size_full()
                .rounded_full()
                .bg(theme::palette().accent)
                .opacity(pulse_opacity(
                    self.pulse_epoch,
                    now,
                    theme::TOOLS_PULSE_OPACITY,
                ))
                .into_any_element(),
            RegionKind::QueueCard(id) => match self.queue.iter().find(|entry| entry.id == id) {
                Some(entry) => self.render_queue_card(entry, now, cx).into_any_element(),
                None => div().into_any_element(),
            },
        }
    }
}

/// Opacity of a looping pulse at `now`: GPUI's `pulsating_between` curve, but computed per
/// frame so it follows the throttled animation ticker instead of forcing 60 Hz redraws.
fn pulse_opacity(epoch: Instant, now: Instant, (minimum, maximum): (f32, f32)) -> f32 {
    let period = theme::LOADING_PULSE_DURATION.as_secs_f32();
    let phase = now.saturating_duration_since(epoch).as_secs_f32() % period / period;
    pulsating_between(minimum, maximum)(phase)
}

fn muted_text(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(theme::TEXT_SM)
        .text_color(theme::palette().ink_muted)
        .child(text.into())
}

fn mono_text(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(theme::TEXT_XS)
        .font_family(theme::FONT_MONO)
        .text_color(theme::palette().ink_muted)
        .whitespace_nowrap()
        .child(text.into())
}

fn thumbnail_frame(width: Pixels, height: Pixels) -> Div {
    div()
        .flex_none()
        .w(width)
        .h(height)
        .rounded(theme::RADIUS_SM)
        .overflow_hidden()
        .bg(theme::palette().paper_deep)
}

fn state_pill(state: DownloadState) -> Div {
    let (label, background, text) = match state {
        DownloadState::Queued => (
            "En attente",
            theme::palette().paper_deep,
            theme::palette().ink_muted,
        ),
        DownloadState::Downloading => (
            "Téléchargement",
            theme::palette().accent_soft,
            theme::palette().accent_text,
        ),
        DownloadState::Converting => (
            "Conversion",
            theme::palette().teal_soft,
            theme::palette().teal,
        ),
        DownloadState::Paused => (
            "En pause",
            theme::palette().paper_deep,
            theme::palette().ink,
        ),
        DownloadState::Done => (
            "Terminé",
            theme::palette().success_soft,
            theme::palette().success,
        ),
        DownloadState::Failed => (
            "Échec",
            theme::palette().danger_soft,
            theme::palette().danger,
        ),
        DownloadState::Cancelled => (
            "Annulé",
            theme::palette().paper_deep,
            theme::palette().ink_muted,
        ),
    };
    pill(label, background, text)
}

impl RiptideApp {
    fn render_header(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (yt_dlp_label, yt_dlp_colors) = match &self.tools {
            ToolsState::Preparing => (
                "yt-dlp · préparation…".to_owned(),
                (theme::palette().paper_deep, theme::palette().ink_muted),
            ),
            ToolsState::Ready(status) => match &status.yt_dlp_version {
                Some(version) => (
                    format!("yt-dlp {version}"),
                    (theme::palette().teal_soft, theme::palette().teal),
                ),
                None => (
                    "yt-dlp absent".to_owned(),
                    (theme::palette().danger_soft, theme::palette().danger),
                ),
            },
            ToolsState::Failed(_) => (
                "yt-dlp indisponible".to_owned(),
                (theme::palette().danger_soft, theme::palette().danger),
            ),
        };
        let ffmpeg_pill = match &self.tools {
            ToolsState::Ready(status) if status.ffmpeg_available => Some(pill(
                "ffmpeg OK",
                theme::palette().success_soft,
                theme::palette().success,
            )),
            ToolsState::Ready(_) => Some(pill(
                "ffmpeg absent",
                theme::palette().danger_soft,
                theme::palette().danger,
            )),
            _ => None,
        };
        let can_update = matches!(self.tools, ToolsState::Ready(_)) && !self.is_updating_yt_dlp;
        let update_label = if self.is_updating_yt_dlp {
            "Mise à jour…"
        } else {
            // Short: it sits next to the yt-dlp version pill, and the header must fit 800 px.
            "Mettre à jour"
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(theme::SPACE_3)
            .px(theme::SPACE_5)
            .border_b(theme::BORDER)
            .border_color(theme::palette().rule)
            .child(vinyl_disc(px(40.), 0.6, None))
            .child(
                div()
                    .id("title-bar-drag-area")
                    .flex_1()
                    .min_w_0()
                    // Vertical padding lives here so the whole header height is draggable.
                    .py(theme::SPACE_3)
                    .flex()
                    .items_center()
                    .on_mouse_down(MouseButton::Left, window_frame::on_title_bar_mouse_down)
                    .whitespace_nowrap()
                    .font_family(theme::FONT_DISPLAY)
                    .text_size(theme::TEXT_DISPLAY)
                    .line_height(relative(1.0))
                    .text_color(theme::palette().ink)
                    .child("Riptide"),
            )
            .child(self.render_theme_selector(cx))
            .child(pill(yt_dlp_label, yt_dlp_colors.0, yt_dlp_colors.1))
            .children(ffmpeg_pill)
            .child(
                button(
                    "update-yt-dlp",
                    update_label,
                    ButtonVariant::Secondary,
                    can_update,
                )
                .when(can_update, |this| {
                    this.on_click(cx.listener(|app, _, _, cx| app.update_yt_dlp(cx)))
                }),
            )
            .when(
                matches!(window.window_decorations(), Decorations::Client { .. }),
                |this| this.child(window_frame::window_controls(window)),
            )
    }

    /// Auto / Clair / Sombre, as a compact segmented control.
    fn render_theme_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.settings.theme_mode;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(theme::SPACE_1)
            .children(ThemeMode::ALL.iter().map(|&mode| {
                let label = match mode {
                    ThemeMode::Auto => "Auto",
                    ThemeMode::Light => "Clair",
                    ThemeMode::Dark => "Sombre",
                };
                selector_chip(("theme-mode", mode as usize), label, current == mode)
                    .h(theme::ICON_BUTTON_SIZE)
                    .px(theme::SPACE_2)
                    .text_size(theme::TEXT_XS)
                    .on_click(cx.listener(move |app, _, _, cx| app.set_theme_mode(mode, cx)))
            }))
    }

    fn render_tools_banner(
        &self,
        pulse: &Entity<Region>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let banner = |background: Rgba, text_color: Rgba| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(theme::SPACE_3)
                .mx(theme::SPACE_5)
                .mt(theme::SPACE_3)
                .px(theme::SPACE_4)
                .py(theme::SPACE_2)
                .rounded(theme::RADIUS_MD)
                .bg(background)
                .text_color(text_color)
                .text_size(theme::TEXT_SM)
        };
        match &self.tools {
            ToolsState::Preparing => Some(
                banner(theme::palette().paper_deep, theme::palette().ink)
                    .child(region::cached(
                        pulse,
                        StyleRefinement::default().flex_none().size(theme::SPACE_2),
                    ))
                    .child("Préparation de yt-dlp… (premier lancement : téléchargement et vérification)")
                    .into_any_element(),
            ),
            ToolsState::Failed(message) => Some(
                banner(theme::palette().danger_soft, theme::palette().danger)
                    .child(div().flex_1().child(format!("Impossible de préparer yt-dlp : {message}")))
                    .child(
                        button("retry-tools", "Réessayer", ButtonVariant::Danger, true)
                            .on_click(cx.listener(|app, _, _, cx| app.prepare_tools(cx))),
                    )
                    .into_any_element(),
            ),
            ToolsState::Ready(status) if !status.ffmpeg_available => Some(
                banner(theme::palette().danger_soft, theme::palette().danger)
                    .child(
                        div()
                            .flex_1()
                            .child(format!("ffmpeg est introuvable : la conversion est impossible. {FFMPEG_INSTALL_HINT}")),
                    )
                    .child(
                        button("recheck-tools", "Revérifier", ButtonVariant::Danger, true)
                            .on_click(cx.listener(|app, _, _, cx| app.recheck_tools(cx))),
                    )
                    .into_any_element(),
            ),
            ToolsState::Ready(_) => self.tools_notice.as_ref().map(|notice| {
                banner(theme::palette().teal_soft, theme::palette().teal)
                    .child(notice.clone())
                    .into_any_element()
            }),
        }
    }

    fn render_link_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_focused = self.link_input.focus_handle(cx).is_focused(window);
        let error = self.link_error.clone();
        let error_generation = self.link_error_generation;
        section_card()
            .child(overline("FACE A · LE LIEN"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(theme::SPACE_2)
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .h(theme::CONTROL_HEIGHT_LARGE)
                            .px(theme::SPACE_3)
                            .rounded(theme::RADIUS_MD)
                            .border(theme::FOCUS_RING_WIDTH)
                            .border_color(if error.is_some() {
                                theme::palette().danger
                            } else if is_focused {
                                theme::palette().focus_ring
                            } else {
                                theme::palette().rule_strong
                            })
                            .bg(theme::palette().paper)
                            .child(self.link_input.clone()),
                    )
                    .child(
                        button("analyze", "Analyser", ButtonVariant::Primary, true)
                            .h(theme::CONTROL_HEIGHT_LARGE)
                            .on_click(cx.listener(|app, _, _, cx| app.start_analysis(cx))),
                    ),
            )
            .when_some(error, |card, message| {
                card.child(
                    div()
                        .relative()
                        .text_size(theme::TEXT_SM)
                        .text_color(theme::palette().danger)
                        .child(message)
                        .with_animation(
                            ("link-error", error_generation as usize),
                            Animation::new(theme::ERROR_SHAKE_DURATION),
                            |text, delta| {
                                let wobble = (delta * PI * 6.0).sin() * (1.0 - delta);
                                text.left(theme::ERROR_SHAKE_AMPLITUDE * wobble)
                            },
                        ),
                )
            })
    }

    fn render_preview(&self, now: Instant, cx: &mut Context<Self>) -> Option<AnyElement> {
        match &self.probe {
            ProbeState::Idle => None,
            ProbeState::Loading => Some(
                section_card()
                    .flex_row()
                    .items_center()
                    .gap(theme::SPACE_4)
                    .child(vinyl_disc(
                        theme::VINYL_CARD_SIZE,
                        self.loading_spin.angle_at(now),
                        None,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(theme::SPACE_1)
                            .child(section_title("Analyse du lien…"))
                            .child(muted_text("On pose le diamant sur le sillon.")),
                    )
                    .into_any_element(),
            ),
            ProbeState::Failed(message) => Some(
                section_card()
                    .border_color(theme::palette().danger)
                    .child(section_title("Lien illisible"))
                    .child(
                        div()
                            .text_size(theme::TEXT_SM)
                            .text_color(theme::palette().danger)
                            .child(message.clone()),
                    )
                    .into_any_element(),
            ),
            ProbeState::Ready(preview) => {
                let link = &preview.link;
                let is_playlist = link.kind == MediaKind::Playlist;
                let subtitle = if is_playlist {
                    format!(
                        "{} · {}",
                        link.uploader.clone().unwrap_or_default(),
                        format::track_count(link.tracks.len())
                    )
                } else {
                    let duration = link
                        .tracks
                        .first()
                        .and_then(|track| track.duration_seconds)
                        .map(format::track_duration)
                        .unwrap_or_default();
                    format!("{} · {duration}", link.uploader.clone().unwrap_or_default())
                };
                let thumbnail = link
                    .thumbnail_url
                    .as_deref()
                    .and_then(|url| self.thumbnails.image(url));
                let unfold_duration = theme::PREVIEW_UNFOLD_DURATION;
                let card = section_card()
                    .relative()
                    .child(overline(if is_playlist {
                        "APERÇU · PLAYLIST"
                    } else {
                        "APERÇU"
                    }))
                    .child(
                        div()
                            .flex()
                            .gap(theme::SPACE_4)
                            .child(
                                thumbnail_frame(
                                    theme::THUMBNAIL_PREVIEW_WIDTH,
                                    theme::THUMBNAIL_PREVIEW_HEIGHT,
                                )
                                .when_some(
                                    thumbnail,
                                    |frame, image| {
                                        frame.child(
                                            img(image).size_full().object_fit(ObjectFit::Cover),
                                        )
                                    },
                                ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(theme::SPACE_1)
                                    .child(
                                        div()
                                            .font_family(theme::FONT_DISPLAY)
                                            .text_size(theme::TEXT_XL)
                                            .line_height(relative(1.15))
                                            .line_clamp(3)
                                            .child(link.title.clone()),
                                    )
                                    .child(muted_text(subtitle)),
                            ),
                    )
                    .when(is_playlist, |card| {
                        card.child(self.render_track_list(preview, cx))
                    });
                Some(
                    card.with_animation(
                        ("preview-unfold", preview.generation as usize),
                        Animation::new(unfold_duration),
                        move |card, linear_delta| {
                            let delta = spring_progress(linear_delta, unfold_duration);
                            card.top(theme::PREVIEW_UNFOLD_OFFSET * (1.0 - delta))
                                .opacity((linear_delta * 2.5).min(1.0))
                        },
                    )
                    .into_any_element(),
                )
            }
        }
    }

    fn render_track_list(
        &self,
        preview: &super::app::Preview,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected_count = preview.selected_track_count();
        let total = preview.is_track_selected.len();
        let are_all_selected = selected_count == total;
        div()
            .flex()
            .flex_col()
            .gap(theme::SPACE_2)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(mono_text(format!(
                        "{selected_count} / {total} SÉLECTIONNÉES"
                    )))
                    .child(
                        button(
                            "toggle-all-tracks",
                            if are_all_selected {
                                "Tout décocher"
                            } else {
                                "Tout cocher"
                            },
                            ButtonVariant::Quiet,
                            true,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.set_all_tracks_selected(!are_all_selected, cx)
                        })),
                    ),
            )
            .child(
                div()
                    .border_t(theme::BORDER)
                    .border_color(theme::palette().rule)
                    .child(
                        uniform_list(
                            "track-list",
                            total,
                            cx.processor(|app, range: Range<usize>, _, cx| {
                                app.render_track_rows(range, cx)
                            }),
                        )
                        .h((theme::TRACK_ROW_HEIGHT * total as f32)
                            .min(theme::TRACK_LIST_MAX_HEIGHT)),
                    ),
            )
    }

    /// Only the visible rows are built, and only their thumbnails are requested.
    fn render_track_rows(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let ProbeState::Ready(preview) = &self.probe else {
            return Vec::new();
        };
        let range =
            range.start.min(preview.link.tracks.len())..range.end.min(preview.link.tracks.len());
        let visible_thumbnails: Vec<String> = preview.link.tracks[range.clone()]
            .iter()
            .filter_map(|track| track.thumbnail_url.clone())
            .collect();
        for url in &visible_thumbnails {
            self.request_thumbnail(url, cx);
        }
        let ProbeState::Ready(preview) = &self.probe else {
            return Vec::new();
        };
        range
            .map(|index| {
                let track = &preview.link.tracks[index];
                let is_selected = preview
                    .is_track_selected
                    .get(index)
                    .copied()
                    .unwrap_or(false);
                let thumbnail = track
                    .thumbnail_url
                    .as_deref()
                    .and_then(|url| self.thumbnails.image(url));
                checkbox_row(
                    ("track", index),
                    div()
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .items_center()
                        .gap(theme::SPACE_3)
                        .child(mono_text(format!("{:02}", index + 1)))
                        .child(
                            thumbnail_frame(
                                theme::TRACK_THUMBNAIL_WIDTH,
                                theme::TRACK_THUMBNAIL_HEIGHT,
                            )
                            .when_some(thumbnail, |frame, image| {
                                frame.child(img(image).size_full().object_fit(ObjectFit::Cover))
                            }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(theme::TEXT_SM)
                                .text_color(if is_selected {
                                    theme::palette().ink
                                } else {
                                    theme::palette().ink_muted
                                })
                                .child(track.title.clone()),
                        )
                        .children(
                            track
                                .duration_seconds
                                .map(|seconds| mono_text(format::track_duration(seconds))),
                        ),
                    is_selected,
                )
                .w_full()
                .h(theme::TRACK_ROW_HEIGHT)
                .on_click(cx.listener(move |app, _, _, cx| app.toggle_track(index, cx)))
                .into_any_element()
            })
            .collect()
    }

    fn render_output_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let output = self.output;
        let selected_count = match &self.probe {
            ProbeState::Ready(preview) => preview.selected_track_count(),
            _ => 0,
        };
        let can_download = selected_count > 0;
        let download_label = if can_download {
            format!("Télécharger · {}", format::track_count(selected_count))
        } else {
            "Télécharger".to_owned()
        };
        let chip_row = || div().flex().flex_wrap().gap(theme::SPACE_2);

        let media_row = chip_row()
            .child(media_chip(
                "media-audio",
                "Audio",
                MediaChoice::Audio,
                output,
                cx,
            ))
            .child(media_chip(
                "media-video",
                "Vidéo",
                MediaChoice::Video,
                output,
                cx,
            ));

        let (format_row, quality_row) = match output.media {
            MediaChoice::Audio => {
                let formats = chip_row().children(AudioFormat::ALL.iter().map(|&format| {
                    selector_chip(
                        ("audio-format", format as usize),
                        format::audio_format_name(format),
                        output.audio_format == format,
                    )
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.set_output(
                            OutputChoice {
                                audio_format: format,
                                ..app.output
                            },
                            cx,
                        )
                    }))
                }));
                let quality = if output.preset().is_lossless() {
                    muted_text("Format sans perte : qualité d'origine, aucun réglage.")
                        .into_any_element()
                } else {
                    chip_row()
                        .children(AudioQuality::ALL.iter().map(|&quality| {
                            selector_chip(
                                ("audio-quality", quality as usize),
                                format::audio_quality_name(quality),
                                output.audio_quality == quality,
                            )
                            .on_click(cx.listener(
                                move |app, _, _, cx| {
                                    app.set_output(
                                        OutputChoice {
                                            audio_quality: quality,
                                            ..app.output
                                        },
                                        cx,
                                    )
                                },
                            ))
                        }))
                        .into_any_element()
                };
                (formats, quality)
            }
            MediaChoice::Video => {
                let formats = chip_row().children(VideoFormat::ALL.iter().map(|&format| {
                    selector_chip(
                        ("video-format", format as usize),
                        format::video_format_name(format),
                        output.video_format == format,
                    )
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.set_output(
                            OutputChoice {
                                video_format: format,
                                ..app.output
                            },
                            cx,
                        )
                    }))
                }));
                let quality = chip_row()
                    .children(VideoQuality::ALL.iter().map(|&quality| {
                        selector_chip(
                            ("video-quality", quality as usize),
                            format::video_quality_name(quality),
                            output.video_quality == quality,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.set_output(
                                OutputChoice {
                                    video_quality: quality,
                                    ..app.output
                                },
                                cx,
                            )
                        }))
                    }))
                    .into_any_element();
                (formats, quality)
            }
        };

        let labelled = |label: &'static str, content: AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap(theme::SPACE_2)
                .child(mono_text(label))
                .child(content)
        };

        section_card()
            .child(overline("SORTIE"))
            .child(labelled("SUPPORT", media_row.into_any_element()))
            .child(labelled("FORMAT", format_row.into_any_element()))
            .child(labelled("QUALITÉ", quality_row))
            .child(labelled(
                "DOSSIER",
                div()
                    .flex()
                    .items_center()
                    .gap(theme::SPACE_3)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(theme::TEXT_SM)
                            .font_family(theme::FONT_MONO)
                            .child(self.settings.output_directory.display().to_string()),
                    )
                    .child(
                        button("change-folder", "Changer", ButtonVariant::Secondary, true)
                            .on_click(cx.listener(|app, _, _, cx| app.choose_output_directory(cx))),
                    )
                    .into_any_element(),
            ))
            .child(
                checkbox_row(
                    "embed-metadata",
                    div()
                        .text_size(theme::TEXT_SM)
                        .child("Intégrer tags et pochette"),
                    self.settings.embed_metadata_and_cover,
                )
                .ml(-theme::SPACE_2)
                .on_click(cx.listener(|app, _, _, cx| app.toggle_embed_metadata(cx))),
            )
            .child(
                button(
                    "download",
                    download_label,
                    ButtonVariant::Primary,
                    can_download,
                )
                .h(theme::CONTROL_HEIGHT_LARGE)
                .text_size(theme::TEXT_MD)
                .when(can_download, |this| {
                    this.on_click(cx.listener(|app, _, _, cx| app.enqueue_selected_tracks(cx)))
                }),
            )
            .when(!can_download, |card| {
                card.child(muted_text(match self.probe {
                    ProbeState::Ready(_) => "Coche au moins une piste.",
                    _ => "Analyse un lien pour choisir les pistes.",
                }))
            })
    }

    fn render_side_panel(&self, is_compact: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = |id: &'static str,
                   label: &'static str,
                   count: usize,
                   value: SidePanelTab,
                   active: SidePanelTab| {
            let is_active = value == active;
            div()
                .id(id)
                .flex_none()
                .tab_index(0)
                .px(theme::SPACE_3)
                .py(theme::SPACE_2)
                .border_b(theme::FOCUS_RING_WIDTH)
                .border_color(if is_active {
                    theme::palette().accent
                } else {
                    theme::TRANSPARENT
                })
                .font_family(theme::FONT_DISPLAY)
                .text_size(theme::TEXT_LG)
                .text_color(if is_active {
                    theme::palette().ink
                } else {
                    theme::palette().ink_muted
                })
                .cursor_pointer()
                .hover(|style| style.text_color(theme::palette().ink))
                .focus(|style| style.bg(theme::palette().teal_soft))
                .flex()
                .items_center()
                .gap(theme::SPACE_2)
                .child(label)
                .child(mono_text(count.to_string()))
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.active_tab = value;
                    cx.notify();
                }))
        };
        let active = self.active_tab;
        let concurrency = self.settings.clamped_concurrent_downloads();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .pt(theme::SPACE_5)
            .gap(theme::SPACE_3)
            .child(overline("FACE B"))
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(theme::SPACE_2)
                    .border_b(theme::BORDER)
                    .border_color(theme::palette().rule)
                    .child(tab(
                        "tab-queue",
                        "File",
                        self.queue.len(),
                        SidePanelTab::Queue,
                        active,
                    ))
                    .child(tab(
                        "tab-history",
                        "Historique",
                        self.history.len(),
                        SidePanelTab::History,
                        active,
                    ))
                    .child(div().flex_1())
                    .when(active == SidePanelTab::Queue, |row| {
                        row.child(
                            div()
                                .flex()
                                .min_w_0()
                                .items_center()
                                .gap(theme::SPACE_1)
                                .pb(theme::SPACE_1)
                                // Dropped in a narrow window so the − / + controls stay visible.
                                .when(!is_compact, |group| group.child(mono_text("SIMULTANÉS")))
                                .child(
                                    button(
                                        "concurrency-down",
                                        "−",
                                        ButtonVariant::Quiet,
                                        concurrency > crate::store::MINIMUM_CONCURRENT_DOWNLOADS,
                                    )
                                    .w(theme::ICON_BUTTON_SIZE)
                                    .px(px(0.))
                                    .on_click(
                                        cx.listener(|app, _, _, cx| app.change_concurrency(-1, cx)),
                                    ),
                                )
                                .child(
                                    div()
                                        .font_family(theme::FONT_MONO)
                                        .text_size(theme::TEXT_MD)
                                        .child(concurrency.to_string()),
                                )
                                .child(
                                    button(
                                        "concurrency-up",
                                        "+",
                                        ButtonVariant::Quiet,
                                        concurrency < crate::store::MAXIMUM_CONCURRENT_DOWNLOADS,
                                    )
                                    .w(theme::ICON_BUTTON_SIZE)
                                    .px(px(0.))
                                    .on_click(
                                        cx.listener(|app, _, _, cx| app.change_concurrency(1, cx)),
                                    ),
                                ),
                        )
                    }),
            )
            .child(match active {
                SidePanelTab::Queue => self.render_queue(cx),
                SidePanelTab::History => div()
                    .id("face-b-history")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pb(theme::SPACE_5)
                    .child(self.render_history(cx))
                    .into_any_element(),
            })
    }

    fn render_queue(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.queue.is_empty() {
            return empty_state(
                "Colle un lien pour commencer",
                "Vidéo, Short ou playlist YouTube : analyse-le, choisis le format, la file se remplit ici.",
            )
            .into_any_element();
        }
        uniform_list(
            "queue",
            self.queue.len(),
            cx.processor(|app, range: Range<usize>, _, cx| app.render_queue_rows(range, cx)),
        )
        .flex_1()
        .min_h_0()
        .pb(theme::SPACE_5)
        .into_any_element()
    }

    /// Only the visible cards are built, and only their thumbnails are requested.
    fn render_queue_rows(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let range = range.start.min(self.queue.len())..range.end.min(self.queue.len());
        let visible_thumbnails: Vec<String> = self.queue[range.clone()]
            .iter()
            .filter_map(|entry| entry.track.thumbnail_url.clone())
            .collect();
        for url in &visible_thumbnails {
            self.request_thumbnail(url, cx);
        }
        let visible_ids: Vec<u64> = self.queue[range].iter().map(|entry| entry.id).collect();
        self.visible_queue_card_ids.extend(&visible_ids);
        visible_ids
            .into_iter()
            .map(|id| {
                let card = self.region(RegionKind::QueueCard(id), cx);
                div()
                    .h(theme::QUEUE_CARD_HEIGHT + theme::QUEUE_ROW_GAP)
                    .pb(theme::QUEUE_ROW_GAP)
                    .child(region::cached(
                        &card,
                        StyleRefinement::default().size_full(),
                    ))
                    .into_any_element()
            })
            .collect()
    }

    fn render_queue_card(
        &self,
        entry: &QueueEntry,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = entry.id;
        let thumbnail = entry
            .track
            .thumbnail_url
            .as_deref()
            .and_then(|url| self.thumbnails.image(url));
        let status_line: AnyElement = match entry.state {
            // The error message takes the groove's place (see below).
            DownloadState::Failed => div().into_any_element(),
            DownloadState::Done => mono_text(
                entry
                    .output_file
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
            .truncate()
            .into_any_element(),
            DownloadState::Queued => mono_text("Attend une place libre").into_any_element(),
            DownloadState::Cancelled => mono_text("Fichiers partiels supprimés").into_any_element(),
            DownloadState::Converting => {
                mono_text("ffmpeg convertit et pose les tags…").into_any_element()
            }
            DownloadState::Downloading | DownloadState::Paused => {
                let mut parts = vec![format::percent(entry.shown_fraction)];
                if entry.state == DownloadState::Downloading {
                    parts.extend(
                        entry
                            .speed_bytes_per_second
                            .map(format::speed_megabytes_per_second),
                    );
                    parts.extend(entry.eta_seconds.map(format::remaining_time));
                }
                mono_text(parts.join(" · ")).into_any_element()
            }
        };

        let actions = div().flex().flex_none().items_center().gap(theme::SPACE_1);
        let actions = match entry.state {
            _ if entry.is_cancelling => actions.child(button(
                ("cancelling", id as usize),
                "Annulation…",
                ButtonVariant::Danger,
                false,
            )),
            DownloadState::Downloading | DownloadState::Paused => actions
                .child(
                    button(
                        ("pause", id as usize),
                        if entry.state == DownloadState::Paused {
                            "Reprendre"
                        } else {
                            "Pause"
                        },
                        ButtonVariant::Secondary,
                        true,
                    )
                    .on_click(cx.listener(move |app, _, _, cx| app.toggle_pause(id, cx))),
                )
                .child(
                    button(
                        ("cancel", id as usize),
                        "Annuler",
                        ButtonVariant::Danger,
                        true,
                    )
                    .on_click(cx.listener(move |app, _, _, cx| app.cancel_entry(id, cx))),
                ),
            DownloadState::Queued | DownloadState::Converting => actions.child(
                button(
                    ("cancel", id as usize),
                    "Annuler",
                    ButtonVariant::Danger,
                    true,
                )
                .on_click(cx.listener(move |app, _, _, cx| app.cancel_entry(id, cx))),
            ),
            DownloadState::Done => {
                let target = entry
                    .output_file
                    .clone()
                    .unwrap_or_else(|| entry.output_directory.clone());
                actions
                    .child(
                        button(
                            ("open", id as usize),
                            "Ouvrir le dossier",
                            ButtonVariant::Secondary,
                            true,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| app.open_folder(&target, cx))),
                    )
                    .child(
                        button(
                            ("remove", id as usize),
                            "Retirer",
                            ButtonVariant::Quiet,
                            true,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| app.remove_entry(id, cx))),
                    )
            }
            DownloadState::Failed => actions
                .child(
                    button(
                        ("retry", id as usize),
                        "Réessayer",
                        ButtonVariant::Secondary,
                        true,
                    )
                    .on_click(cx.listener(move |app, _, _, cx| app.retry_entry(id, cx))),
                )
                .child(
                    button(
                        ("remove", id as usize),
                        "Retirer",
                        ButtonVariant::Quiet,
                        true,
                    )
                    .on_click(cx.listener(move |app, _, _, cx| app.remove_entry(id, cx))),
                ),
            DownloadState::Cancelled => actions.child(
                button(
                    ("remove", id as usize),
                    "Retirer",
                    ButtonVariant::Quiet,
                    true,
                )
                .on_click(cx.listener(move |app, _, _, cx| app.remove_entry(id, cx))),
            ),
        };

        let enter_duration = theme::CARD_ENTER_DURATION;
        div()
            .relative()
            .size_full()
            .flex()
            .items_center()
            .gap(theme::SPACE_4)
            .p(theme::SPACE_3)
            .rounded(theme::RADIUS_LG)
            .border(theme::BORDER)
            .border_color(theme::palette().rule)
            .bg(theme::palette().card)
            .shadow_sm()
            .child(vinyl_disc(
                theme::VINYL_CARD_SIZE,
                entry.spin.angle_at(now),
                thumbnail,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(theme::SPACE_1 / 2.)
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(entry.track.title.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(theme::SPACE_2)
                            .overflow_hidden()
                            .child(pill(
                                entry.preset_label.clone(),
                                theme::palette().paper_deep,
                                theme::palette().ink,
                            ))
                            .child(state_pill(entry.state)),
                    )
                    .child(if entry.state == DownloadState::Failed {
                        // One line: queue rows have a fixed height (virtualized list).
                        div()
                            .text_size(theme::TEXT_SM)
                            .text_color(theme::palette().danger)
                            .truncate()
                            .child(entry.error_message.clone().unwrap_or_default())
                            .into_any_element()
                    } else {
                        let converting_opacity =
                            pulse_opacity(self.pulse_epoch, now, theme::CONVERTING_PULSE_OPACITY);
                        progress_groove(entry, converting_opacity).into_any_element()
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme::SPACE_3)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(status_line),
                            )
                            .child(actions),
                    ),
            )
            .with_animation(
                ("queue-card", id as usize),
                Animation::new(enter_duration),
                move |card, linear_delta| {
                    let delta = spring_progress(linear_delta, enter_duration);
                    card.left(theme::CARD_ENTER_OFFSET * (1.0 - delta))
                        .opacity((linear_delta * 2.5).min(1.0))
                },
            )
    }

    fn render_history(&self, cx: &mut Context<Self>) -> Div {
        if self.history.is_empty() {
            return empty_state(
                "Rien dans les bacs",
                "Les téléchargements terminés s'archivent ici, même après redémarrage.",
            );
        }
        div().flex().flex_col().children(
            self.history
                .iter()
                .enumerate()
                .map(|(index, entry)| history_row(index, entry, self, cx).into_any_element())
                .collect::<Vec<_>>(),
        )
    }
}

fn media_chip(
    id: &'static str,
    label: &'static str,
    media: MediaChoice,
    output: OutputChoice,
    cx: &mut Context<RiptideApp>,
) -> impl IntoElement {
    selector_chip(id, label, output.media == media)
        .px(theme::SPACE_5)
        .font_family(theme::FONT_BODY)
        .on_click(cx.listener(move |app, _, _, cx| {
            app.set_output(
                OutputChoice {
                    media,
                    ..app.output
                },
                cx,
            )
        }))
}

fn progress_groove(entry: &QueueEntry, converting_opacity: f32) -> impl IntoElement {
    let fraction = match entry.state {
        DownloadState::Done | DownloadState::Converting => 1.0,
        _ => entry.shown_fraction,
    };
    let fill_background = match entry.state {
        DownloadState::Done => theme::palette().success.into(),
        DownloadState::Failed | DownloadState::Cancelled => theme::palette().rule_strong.into(),
        DownloadState::Converting => theme::palette().teal.into(),
        DownloadState::Paused => theme::palette().ink_muted.into(),
        _ => linear_gradient(
            90.,
            linear_color_stop(theme::palette().accent, 0.),
            linear_color_stop(theme::palette().amber, 1.),
        ),
    };
    let fill_bar = div()
        .absolute()
        .left_0()
        .top_0()
        .h_full()
        .w(relative(fraction))
        .rounded(theme::RADIUS_PILL)
        .bg(fill_background);
    let fill_bar = fill_bar.when(entry.state == DownloadState::Converting, |bar| {
        bar.opacity(converting_opacity)
    });
    div()
        .relative()
        .w_full()
        .h(theme::PROGRESS_TRACK_HEIGHT)
        .my(theme::SPACE_1)
        .rounded(theme::RADIUS_PILL)
        .bg(theme::palette().paper_deep)
        .child(fill_bar)
}

fn empty_state(title: &'static str, hint: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(theme::SPACE_3)
        .pt(theme::SPACE_6)
        .px(theme::SPACE_5)
        .child(vinyl_disc(theme::VINYL_EMPTY_STATE_SIZE, 0.9, None).opacity(0.9))
        .child(
            div()
                .mt(theme::SPACE_2)
                .font_family(theme::FONT_DISPLAY)
                .text_size(theme::TEXT_XL)
                .text_color(theme::palette().ink)
                .child(title),
        )
        .child(
            div()
                .max_w(px(340.))
                .text_center()
                .text_size(theme::TEXT_SM)
                .text_color(theme::palette().ink_muted)
                .child(hint),
        )
}

fn history_row(
    index: usize,
    entry: &HistoryEntry,
    app: &RiptideApp,
    cx: &mut Context<RiptideApp>,
) -> impl IntoElement {
    let thumbnail = entry
        .thumbnail_url
        .as_deref()
        .and_then(|url| app.thumbnails.image(url));
    let target = entry.output_file.clone();
    div()
        .flex()
        .items_center()
        .gap(theme::SPACE_3)
        .py(theme::SPACE_2)
        .border_b(theme::BORDER)
        .border_color(theme::palette().rule)
        .child(
            thumbnail_frame(theme::TRACK_THUMBNAIL_WIDTH, theme::TRACK_THUMBNAIL_HEIGHT)
                .when_some(thumbnail, |frame, image| {
                    frame.child(img(image).size_full().object_fit(ObjectFit::Cover))
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(theme::TEXT_SM)
                        .font_weight(FontWeight::MEDIUM)
                        .child(entry.title.clone()),
                )
                .child(mono_text(format!(
                    "{} · {}",
                    entry.preset.label_fr(),
                    format::calendar_date(entry.finished_at_unix_seconds)
                ))),
        )
        .child(
            button(
                ("history-open", index),
                "Ouvrir le dossier",
                ButtonVariant::Quiet,
                true,
            )
            .on_click(cx.listener(move |app, _, _, cx| app.open_folder(&target, cx))),
        )
}
