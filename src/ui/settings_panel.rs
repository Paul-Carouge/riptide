//! The settings panel ("Paramètres"): appearance, download defaults, updates and about. It
//! replaces Face A / Face B while open and is drawn as a cached region (`RegionKind::Settings`).

use gpui::{AnyElement, Context, Div, FontWeight, IntoElement, div, prelude::*, relative, svg};

use super::app::{AppUpdateState, RiptideApp, ToolsState};
use super::components::{ButtonVariant, button, overline, pill, section_card, selector_chip};
use super::icons;
use super::render::{labelled_row, mono_text, muted_text};
use super::theme;
use crate::engine::RIPTIDE_VERSION;
use crate::store::ThemeMode;

/// Long release notes are cut here (on a character boundary); the box scrolls for the rest.
const RELEASE_NOTES_MAXIMUM_CHARACTERS: usize = 1500;
const REPOSITORY_LABEL: &str = "github.com/Paul-Carouge/riptide";

/// Release notes as shown: Markdown headings and list markers turned into plain text, trimmed,
/// at most `maximum_characters`, with an ellipsis when cut.
fn release_notes_excerpt(notes: &str, maximum_characters: usize) -> String {
    let plain: Vec<String> = notes
        .lines()
        .map(|line| {
            let line = line.trim_end();
            if let Some(item) = line
                .trim_start()
                .strip_prefix("- ")
                .or_else(|| line.trim_start().strip_prefix("* "))
            {
                format!("• {item}")
            } else {
                line.trim_start_matches('#').trim_start().to_owned()
            }
        })
        .collect();
    let plain = plain.join("\n");
    let notes = plain.trim();
    match notes.char_indices().nth(maximum_characters) {
        Some((cut, _)) => format!("{}…", notes[..cut].trim_end()),
        None => notes.to_owned(),
    }
}

fn setting_block_title(text: &'static str) -> Div {
    div()
        .font_family(theme::FONT_DISPLAY)
        .text_size(theme::TEXT_LG)
        .text_color(theme::palette().ink)
        .child(text)
}

/// One tool line of the updates card: name, status pill, then its action.
fn tool_row(name: &'static str, status: Div, action: Option<AnyElement>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(theme::SPACE_3)
        .pt(theme::SPACE_3)
        .border_t(theme::BORDER)
        .border_color(theme::palette().rule)
        .child(setting_block_title(name))
        .child(status)
        .child(div().flex_1())
        .children(action)
}

impl RiptideApp {
    pub fn render_settings_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let column = || {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(theme::SETTINGS_COLUMN_MIN_WIDTH)
                .gap(theme::SPACE_4)
        };
        div()
            .id("settings-panel")
            .size_full()
            .overflow_y_scroll()
            .px(theme::SPACE_5)
            .py(theme::SPACE_5)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(theme::SPACE_4)
                    .max_w(theme::SETTINGS_MAX_WIDTH)
                    .child(self.render_settings_heading(cx))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_start()
                            .gap(theme::SPACE_5)
                            .child(
                                column()
                                    .child(self.render_appearance_card(cx))
                                    .child(self.render_downloads_card(cx)),
                            )
                            .child(
                                column()
                                    .child(self.render_updates_card(cx))
                                    .child(self.render_about_card(cx)),
                            ),
                    ),
            )
    }

    fn render_settings_heading(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(theme::SPACE_4)
            .child(
                button("settings-back", "Retour", ButtonVariant::Secondary, true)
                    .child(
                        svg()
                            .path(icons::BACK)
                            .size(theme::ICON_SIZE)
                            .text_color(theme::palette().ink),
                    )
                    // The icon reads first: put it before the label.
                    .flex_row_reverse()
                    .on_click(
                        cx.listener(|app, _, window, cx| app.set_settings_open(false, window, cx)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(theme::SPACE_1)
                    .child(overline("FACE C · RÉGLAGES"))
                    .child(
                        div()
                            .font_family(theme::FONT_DISPLAY)
                            .text_size(theme::TEXT_XL)
                            .line_height(relative(1.0))
                            .child("Paramètres"),
                    ),
            )
            .child(div().flex_1())
            .child(mono_text("ÉCHAP POUR REVENIR"))
    }

    fn render_appearance_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.settings.theme_mode;
        section_card()
            .child(overline("APPARENCE"))
            .child(labelled_row(
                "THÈME",
                div()
                    .flex()
                    .flex_wrap()
                    .gap(theme::SPACE_2)
                    .children(ThemeMode::ALL.iter().map(|&mode| {
                        let label = match mode {
                            ThemeMode::Auto => "Auto",
                            ThemeMode::Light => "Clair",
                            ThemeMode::Dark => "Sombre",
                        };
                        selector_chip(("theme-mode", mode as usize), label, current == mode)
                            .px(theme::SPACE_4)
                            .font_family(theme::FONT_BODY)
                            .on_click(
                                cx.listener(move |app, _, _, cx| app.set_theme_mode(mode, cx)),
                            )
                    }))
                    .into_any_element(),
            ))
            .child(muted_text(
                "Auto suit l'apparence du bureau, jour comme nuit.",
            ))
    }

    fn render_downloads_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        section_card()
            .child(overline("TÉLÉCHARGEMENTS"))
            .child(muted_text(
                "Réglages par défaut, partagés avec la Face A et gardés d'une session à l'autre.",
            ))
            .child(self.render_output_choice_rows(cx))
            .child(labelled_row(
                "DOSSIER",
                self.render_output_folder_row(cx).into_any_element(),
            ))
            .child(labelled_row(
                "TÉLÉCHARGEMENTS SIMULTANÉS (1 À 8)",
                self.render_concurrency_stepper(cx)
                    .ml(-theme::SPACE_2)
                    .into_any_element(),
            ))
            .child(self.render_embed_metadata_row(cx))
    }

    fn render_updates_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        section_card()
            .child(overline("MISES À JOUR"))
            .child(self.render_riptide_update(cx))
            .child(self.render_yt_dlp_row(cx))
            .child(self.render_ffmpeg_row(cx))
    }

    fn render_riptide_update(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let check_button = |label: &'static str, is_enabled: bool| {
            button(
                "check-app-update",
                label,
                ButtonVariant::Secondary,
                is_enabled,
            )
            .when(is_enabled, |this| {
                this.on_click(cx.listener(|app, _, _, cx| app.check_for_app_update(false, cx)))
            })
        };
        let status_text = |text: String, color| {
            div()
                .text_size(theme::TEXT_SM)
                .text_color(color)
                .child(text)
        };
        let palette = theme::palette();
        let (status, notes, action): (Div, Option<String>, AnyElement) = match &self.app_update {
            AppUpdateState::Idle => (
                status_text(
                    "Vérifiée au démarrage ; tu peux relancer la recherche.".to_owned(),
                    palette.ink_muted,
                ),
                None,
                check_button("Rechercher une mise à jour", true).into_any_element(),
            ),
            AppUpdateState::Checking => (
                status_text("Recherche…".to_owned(), palette.ink_muted),
                None,
                check_button("Recherche…", false).into_any_element(),
            ),
            AppUpdateState::UpToDate => (
                status_text("Riptide est à jour.".to_owned(), palette.success),
                None,
                check_button("Rechercher une mise à jour", true).into_any_element(),
            ),
            AppUpdateState::Available(release) => (
                status_text(
                    format!("Nouvelle version {} disponible.", release.version),
                    palette.accent_text,
                ),
                Some(release.notes.clone()),
                button(
                    "install-app-update",
                    "Installer et redémarrer",
                    ButtonVariant::Primary,
                    true,
                )
                .on_click(cx.listener(|app, _, _, cx| app.install_app_update(cx)))
                .into_any_element(),
            ),
            AppUpdateState::Installing(release) => (
                status_text(
                    format!(
                        "Installation de la version {}… Riptide redémarre ensuite.",
                        release.version
                    ),
                    palette.ink_muted,
                ),
                None,
                button(
                    "install-app-update",
                    "Installation…",
                    ButtonVariant::Primary,
                    false,
                )
                .into_any_element(),
            ),
            AppUpdateState::Failed(message) => (
                status_text(message.clone(), palette.danger),
                None,
                check_button("Réessayer", true).into_any_element(),
            ),
        };
        div()
            .flex()
            .flex_col()
            .gap(theme::SPACE_2)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(theme::SPACE_3)
                    .child(setting_block_title("Riptide"))
                    .child(pill(
                        format!("v{RIPTIDE_VERSION}"),
                        palette.paper_deep,
                        palette.ink,
                    )),
            )
            .child(status)
            .children(notes.filter(|notes| !notes.trim().is_empty()).map(|notes| {
                div()
                    .id("release-notes")
                    .max_h(theme::RELEASE_NOTES_MAX_HEIGHT)
                    .overflow_y_scroll()
                    .p(theme::SPACE_3)
                    .rounded(theme::RADIUS_MD)
                    .bg(palette.paper)
                    .border(theme::BORDER)
                    .border_color(palette.rule)
                    .text_size(theme::TEXT_SM)
                    .text_color(palette.ink_muted)
                    .child(release_notes_excerpt(
                        &notes,
                        RELEASE_NOTES_MAXIMUM_CHARACTERS,
                    ))
            }))
            .child(div().flex().child(action))
    }

    fn render_yt_dlp_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette();
        let status = match &self.tools {
            ToolsState::Preparing => pill("préparation…", palette.paper_deep, palette.ink_muted),
            ToolsState::Ready(status) => match &status.yt_dlp_version {
                Some(version) => pill(version.clone(), palette.teal_soft, palette.teal),
                None => pill("absent", palette.danger_soft, palette.danger),
            },
            ToolsState::Failed(_) => pill("indisponible", palette.danger_soft, palette.danger),
        };
        let can_update = matches!(self.tools, ToolsState::Ready(_)) && !self.is_updating_yt_dlp;
        let label = if self.is_updating_yt_dlp {
            "Mise à jour…"
        } else {
            "Mettre à jour yt-dlp"
        };
        tool_row(
            "yt-dlp",
            status,
            Some(
                button("update-yt-dlp", label, ButtonVariant::Secondary, can_update)
                    .when(can_update, |this| {
                        this.on_click(cx.listener(|app, _, _, cx| app.update_yt_dlp(cx)))
                    })
                    .into_any_element(),
            ),
        )
    }

    fn render_ffmpeg_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette();
        let (status, can_recheck) = match &self.tools {
            ToolsState::Ready(status) if status.ffmpeg_available => (
                pill("disponible", palette.success_soft, palette.success),
                true,
            ),
            ToolsState::Ready(_) => (pill("absent", palette.danger_soft, palette.danger), true),
            _ => (pill("—", palette.paper_deep, palette.ink_muted), false),
        };
        tool_row(
            "ffmpeg",
            status,
            Some(
                button(
                    "recheck-ffmpeg",
                    "Revérifier",
                    ButtonVariant::Quiet,
                    can_recheck,
                )
                .when(can_recheck, |this| {
                    this.on_click(cx.listener(|app, _, _, cx| app.recheck_tools(cx)))
                })
                .into_any_element(),
            ),
        )
    }

    fn render_about_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        section_card()
            .child(overline("À PROPOS"))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(theme::SPACE_3)
                    .child(
                        div()
                            .font_family(theme::FONT_DISPLAY)
                            .text_size(theme::TEXT_XL)
                            .child("Riptide"),
                    )
                    .child(mono_text(format!(
                        "VERSION {RIPTIDE_VERSION} · LICENCE MIT"
                    ))),
            )
            .child(muted_text(
                "Téléchargeur YouTube pour Linux, en audio ou en vidéo, avec yt-dlp et ffmpeg.",
            ))
            .child(
                div().flex().child(
                    button(
                        "open-repository",
                        REPOSITORY_LABEL,
                        ButtonVariant::Quiet,
                        true,
                    )
                    .ml(-theme::SPACE_4)
                    .font_weight(FontWeight::MEDIUM)
                    .on_click(cx.listener(|app, _, _, cx| app.open_repository_page(cx))),
                ),
            )
            .child(
                div()
                    .pt(theme::SPACE_3)
                    .border_t(theme::BORDER)
                    .border_color(theme::palette().rule)
                    .text_size(theme::TEXT_SM)
                    .text_color(theme::palette().ink_muted)
                    .child(
                        "Respecte les droits d'auteur : ne télécharge que ce que tu as le droit \
                         de copier, dans le respect des conditions de YouTube.",
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_notes_are_trimmed_and_cut_on_a_character_boundary() {
        assert_eq!(release_notes_excerpt("  court  \n", 10), "court");
        assert_eq!(release_notes_excerpt("éééééé", 3), "ééé…");
        assert_eq!(release_notes_excerpt("abc def", 4), "abc…");
        assert_eq!(release_notes_excerpt("", 4), "");
        assert_eq!(
            release_notes_excerpt("## Nouveautés\n\n- un\n* deux", 100),
            "Nouveautés\n\n• un\n• deux"
        );
    }
}
