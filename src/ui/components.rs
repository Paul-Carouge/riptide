//! Small reusable building blocks: buttons, selector chips, checkboxes, pills, section cards.
//! They return GPUI elements; callers attach `on_click` only when the control is enabled.

use gpui::{Div, ElementId, FontWeight, Rgba, SharedString, Stateful, div, prelude::*};

use super::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Filled burnt orange: the one main action of a zone.
    Primary,
    /// Outlined ink on paper.
    Secondary,
    /// Text only, for low-emphasis actions inside cards.
    Quiet,
    /// Outlined brick red, for cancel.
    Danger,
}

/// A focusable button. Disabled buttons are dimmed and leave the tab order.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    is_enabled: bool,
) -> Stateful<Div> {
    let (background, hover_background, text_color, border_color) = match variant {
        ButtonVariant::Primary => (
            theme::palette().accent,
            theme::palette().accent_hover,
            theme::palette().on_accent,
            theme::palette().accent,
        ),
        ButtonVariant::Secondary => (
            theme::palette().card,
            theme::palette().card_hover,
            theme::palette().ink,
            theme::palette().rule_strong,
        ),
        ButtonVariant::Quiet => (
            theme::TRANSPARENT,
            theme::palette().paper_deep,
            theme::palette().accent_text,
            theme::TRANSPARENT,
        ),
        ButtonVariant::Danger => (
            theme::palette().card,
            theme::palette().danger_soft,
            theme::palette().danger,
            theme::palette().danger,
        ),
    };
    let base = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(theme::SPACE_2)
        .h(theme::CONTROL_HEIGHT)
        .px(theme::SPACE_4)
        .rounded(theme::RADIUS_PILL)
        .border(theme::FOCUS_RING_WIDTH)
        .border_color(border_color)
        .bg(background)
        .text_color(text_color)
        .text_size(theme::TEXT_SM)
        .font_weight(FontWeight::SEMIBOLD)
        .whitespace_nowrap()
        .child(label.into());
    if is_enabled {
        base.tab_index(0)
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background))
            .focus(|style| style.border_color(theme::palette().focus_ring))
    } else {
        base.opacity(0.45)
    }
}

/// One option of a segmented selector (format, quality, audio/video).
pub fn selector_chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    is_selected: bool,
) -> Stateful<Div> {
    let (background, text_color, border_color) = if is_selected {
        (
            theme::palette().ink,
            theme::palette().paper,
            theme::palette().ink,
        )
    } else {
        (
            theme::palette().card,
            theme::palette().ink,
            theme::palette().rule_strong,
        )
    };
    div()
        .id(id)
        .tab_index(0)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(theme::CONTROL_HEIGHT)
        .min_w(theme::CONTROL_HEIGHT)
        .px(theme::SPACE_3)
        .rounded(theme::RADIUS_SM)
        .border(theme::FOCUS_RING_WIDTH)
        .border_color(border_color)
        .bg(background)
        .text_color(text_color)
        .text_size(theme::TEXT_SM)
        .font_family(theme::FONT_MONO)
        .whitespace_nowrap()
        .cursor_pointer()
        .when(!is_selected, |chip| {
            chip.hover(|style| {
                style
                    .bg(theme::palette().card_hover)
                    .border_color(theme::palette().ink_muted)
            })
        })
        .focus(|style| style.border_color(theme::palette().focus_ring))
        .child(label.into())
}

/// A labelled checkbox row; the whole row is the click/keyboard target.
pub fn checkbox_row(
    id: impl Into<ElementId>,
    label: impl IntoElement,
    is_checked: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .tab_index(0)
        .flex()
        .items_center()
        .gap(theme::SPACE_3)
        .min_h(theme::ICON_BUTTON_SIZE)
        .px(theme::SPACE_2)
        .rounded(theme::RADIUS_SM)
        .border(theme::FOCUS_RING_WIDTH)
        .border_color(theme::TRANSPARENT)
        .cursor_pointer()
        .hover(|style| style.bg(theme::palette().paper_deep))
        .focus(|style| style.border_color(theme::palette().focus_ring))
        .child(checkbox_box(is_checked))
        .child(label)
}

fn checkbox_box(is_checked: bool) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(theme::CHECKBOX_SIZE)
        .rounded(theme::RADIUS_SM)
        .border(theme::FOCUS_RING_WIDTH)
        .border_color(if is_checked {
            theme::palette().accent
        } else {
            theme::palette().rule_strong
        })
        .bg(if is_checked {
            theme::palette().accent
        } else {
            theme::palette().card
        })
        .when(is_checked, |box_| box_.child(check_mark().size_full()))
}

/// A small rounded status label.
pub fn pill(label: impl Into<SharedString>, background: Rgba, text_color: Rgba) -> Div {
    div()
        .flex_none()
        .px(theme::SPACE_2)
        .py(theme::SPACE_1 / 2.)
        .rounded(theme::RADIUS_PILL)
        .bg(background)
        .text_color(text_color)
        .text_size(theme::TEXT_XS)
        .font_family(theme::FONT_MONO)
        .whitespace_nowrap()
        .child(label.into())
}

/// Record-sleeve style card that groups one step of the flow.
pub fn section_card() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(theme::SPACE_3)
        .p(theme::SPACE_4)
        .rounded(theme::RADIUS_LG)
        .border(theme::BORDER)
        .border_color(theme::palette().rule)
        .bg(theme::palette().card)
        .shadow_sm()
}

/// Small-caps style overline ("FACE A", "SORTIE").
pub fn overline(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(theme::TEXT_XS)
        .font_family(theme::FONT_MONO)
        .text_color(theme::palette().accent_text)
        .child(text.into())
}

pub fn section_title(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(theme::TEXT_LG)
        .font_family(theme::FONT_DISPLAY)
        .text_color(theme::palette().ink)
        .child(text.into())
}

/// A hand-drawn tick (no icon font needed).
fn check_mark() -> gpui::Canvas<()> {
    gpui::canvas(
        |_, _, _| (),
        |bounds, _, window, _| {
            let width = f32::from(bounds.size.width);
            let at = |x: f32, y: f32| {
                gpui::point(
                    bounds.origin.x + gpui::px(x * width),
                    bounds.origin.y + gpui::px(y * width),
                )
            };
            let mut builder = gpui::PathBuilder::stroke(theme::FOCUS_RING_WIDTH);
            builder.move_to(at(0.22, 0.52));
            builder.line_to(at(0.42, 0.72));
            builder.line_to(at(0.78, 0.30));
            if let Ok(path) = builder.build() {
                window.paint_path(path, theme::palette().on_accent);
            }
        },
    )
}
