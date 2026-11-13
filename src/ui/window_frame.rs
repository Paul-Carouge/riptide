//! Client-side window frame: rounded paper window with a soft shadow, resize edges, and the
//! title-bar behaviors (drag, double-click to maximize, right-click window menu, and the round
//! minimize/maximize/close knobs).

use gpui::{
    AnyElement, Bounds, CursorStyle, Decorations, Div, HitboxBehavior, MouseButton, MouseDownEvent,
    Pixels, Point, ResizeEdge, Size, Stateful, Window, canvas, div, point, prelude::*, px, svg,
};

use super::{icons, theme};

/// Invisible band around the window where the pointer resizes it; also holds the shadow.
const RESIZE_BAND: Pixels = px(10.);
const WINDOW_CORNER_RADIUS: Pixels = px(12.);
/// Two layers: a tight contact shadow and a wide soft one, both inside `RESIZE_BAND`.
const WINDOW_CONTACT_SHADOW_BLUR: Pixels = px(2.);
const WINDOW_SOFT_SHADOW_BLUR: Pixels = px(9.);
const WINDOW_SOFT_SHADOW_OFFSET: Pixels = px(2.);
const WINDOW_CONTACT_SHADOW_ALPHA_SCALE: f32 = 0.6;

/// Wraps the app content in the custom frame. With server decorations it is a no-op.
pub fn client_frame(content: impl IntoElement, window: &mut Window) -> AnyElement {
    let Decorations::Client { tiling } = window.window_decorations() else {
        return div().size_full().child(content).into_any_element();
    };
    window.set_client_inset(RESIZE_BAND);
    let is_floating = !tiling.is_tiled();
    let round_top_left = !(tiling.top || tiling.left);
    let round_top_right = !(tiling.top || tiling.right);
    let round_bottom_left = !(tiling.bottom || tiling.left);
    let round_bottom_right = !(tiling.bottom || tiling.right);

    div()
        .id("window-backdrop")
        .size_full()
        .bg(theme::TRANSPARENT)
        .child(
            canvas(
                |_bounds, window, _cx| {
                    let size = window.window_bounds().get_bounds().size;
                    RESIZE_EDGES.map(|edge| {
                        window.insert_hitbox(resize_band_bounds(edge, size), HitboxBehavior::Normal)
                    })
                },
                // One hitbox per edge with its own cursor: GPUI swaps the cursor when the hovered
                // hitbox changes, so moving the pointer never has to redraw the window.
                |_bounds, hitboxes, window, _cx| {
                    for (edge, hitbox) in RESIZE_EDGES.iter().zip(&hitboxes) {
                        window.set_cursor_style(cursor_for_edge(*edge), hitbox);
                    }
                },
            )
            .size_full()
            .absolute(),
        )
        .when(!tiling.top, |this| this.pt(RESIZE_BAND))
        .when(!tiling.bottom, |this| this.pb(RESIZE_BAND))
        .when(!tiling.left, |this| this.pl(RESIZE_BAND))
        .when(!tiling.right, |this| this.pr(RESIZE_BAND))
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            let size = window.window_bounds().get_bounds().size;
            if let Some(edge) = resize_edge(event.position, size) {
                window.start_window_resize(edge);
            }
        })
        .child(
            div()
                .size_full()
                .overflow_hidden()
                .cursor(CursorStyle::Arrow)
                .border_color(theme::palette().window_edge)
                .when(!tiling.top, |this| this.border_t(theme::BORDER))
                .when(!tiling.bottom, |this| this.border_b(theme::BORDER))
                .when(!tiling.left, |this| this.border_l(theme::BORDER))
                .when(!tiling.right, |this| this.border_r(theme::BORDER))
                .when(round_top_left, |this| this.rounded_tl(WINDOW_CORNER_RADIUS))
                .when(round_top_right, |this| {
                    this.rounded_tr(WINDOW_CORNER_RADIUS)
                })
                .when(round_bottom_left, |this| {
                    this.rounded_bl(WINDOW_CORNER_RADIUS)
                })
                .when(round_bottom_right, |this| {
                    this.rounded_br(WINDOW_CORNER_RADIUS)
                })
                .when(is_floating, |this| {
                    let shadow = theme::palette().window_shadow;
                    let contact = gpui::Rgba {
                        a: shadow.a * WINDOW_CONTACT_SHADOW_ALPHA_SCALE,
                        ..shadow
                    };
                    this.shadow(vec![
                        gpui::BoxShadow {
                            color: contact.into(),
                            blur_radius: WINDOW_CONTACT_SHADOW_BLUR,
                            spread_radius: px(0.),
                            offset: point(px(0.), px(0.)),
                        },
                        gpui::BoxShadow {
                            color: shadow.into(),
                            blur_radius: WINDOW_SOFT_SHADOW_BLUR,
                            spread_radius: px(0.),
                            offset: point(px(0.), WINDOW_SOFT_SHADOW_OFFSET),
                        },
                    ])
                })
                .child(content),
        )
        .into_any_element()
}

/// Mouse-down handler for the title-bar drag area: drag moves, double-click maximizes.
pub fn on_title_bar_mouse_down(event: &MouseDownEvent, window: &mut Window, _cx: &mut gpui::App) {
    if event.click_count >= 2 {
        window.zoom_window();
    } else {
        window.start_window_move();
    }
}

/// Right-click on the title bar: the desktop's own window menu.
pub fn on_title_bar_right_mouse_down(
    event: &MouseDownEvent,
    window: &mut Window,
    _cx: &mut gpui::App,
) {
    window.show_window_menu(event.position);
}

/// Minimize / maximize-restore / close, as round turntable knobs with line icons.
pub fn window_controls(window: &Window) -> impl IntoElement {
    let is_maximized = window.is_maximized();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(theme::SPACE_2)
        .child(
            window_control_button("window-minimize", icons::MINIMIZE, false)
                .on_click(|_, window, _| window.minimize_window()),
        )
        .child(
            window_control_button(
                "window-maximize",
                if is_maximized {
                    icons::RESTORE
                } else {
                    icons::MAXIMIZE
                },
                false,
            )
            .on_click(|_, window, _| window.zoom_window()),
        )
        .child(
            window_control_button("window-close", icons::CLOSE, true)
                .on_click(|_, window, _| window.remove_window()),
        )
}

fn window_control_button(id: &'static str, icon: &'static str, is_close: bool) -> Stateful<Div> {
    let palette = theme::palette();
    let (hover_background, hover_border, hover_icon, pressed_background) = if is_close {
        (
            palette.accent,
            palette.accent,
            palette.on_accent,
            palette.accent_hover,
        )
    } else {
        (
            palette.paper_deep,
            palette.rule_strong,
            palette.ink,
            palette.rule,
        )
    };
    div()
        .id(id)
        .group(id)
        .tab_index(0)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(theme::WINDOW_CONTROL_SIZE)
        .rounded(theme::RADIUS_PILL)
        .border(theme::FOCUS_RING_WIDTH)
        .border_color(palette.rule)
        .bg(palette.card)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background).border_color(hover_border))
        .active(move |style| style.bg(pressed_background))
        .focus(|style| style.border_color(theme::palette().focus_ring))
        // Stops the title bar underneath from starting a window move.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            svg()
                .path(icon)
                .size(theme::WINDOW_CONTROL_ICON_SIZE)
                .text_color(palette.ink_muted)
                .group_hover(id, move |style| style.text_color(hover_icon)),
        )
}

const RESIZE_EDGES: [ResizeEdge; 8] = [
    ResizeEdge::TopLeft,
    ResizeEdge::TopRight,
    ResizeEdge::BottomLeft,
    ResizeEdge::BottomRight,
    ResizeEdge::Top,
    ResizeEdge::Bottom,
    ResizeEdge::Left,
    ResizeEdge::Right,
];

/// The part of the resize band where `resize_edge` answers `edge`.
fn resize_band_bounds(edge: ResizeEdge, size: Size<Pixels>) -> Bounds<Pixels> {
    let band = RESIZE_BAND;
    let inner_width = size.width - band * 2.0;
    let inner_height = size.height - band * 2.0;
    let (origin, extent) = match edge {
        ResizeEdge::TopLeft => (point(px(0.), px(0.)), gpui::size(band, band)),
        ResizeEdge::TopRight => (point(size.width - band, px(0.)), gpui::size(band, band)),
        ResizeEdge::BottomLeft => (point(px(0.), size.height - band), gpui::size(band, band)),
        ResizeEdge::BottomRight => (
            point(size.width - band, size.height - band),
            gpui::size(band, band),
        ),
        ResizeEdge::Top => (point(band, px(0.)), gpui::size(inner_width, band)),
        ResizeEdge::Bottom => (
            point(band, size.height - band),
            gpui::size(inner_width, band),
        ),
        ResizeEdge::Left => (point(px(0.), band), gpui::size(band, inner_height)),
        ResizeEdge::Right => (
            point(size.width - band, band),
            gpui::size(band, inner_height),
        ),
    };
    Bounds::new(origin, extent)
}

fn resize_edge(position: Point<Pixels>, size: Size<Pixels>) -> Option<ResizeEdge> {
    let near_top = position.y < RESIZE_BAND;
    let near_bottom = position.y > size.height - RESIZE_BAND;
    let near_left = position.x < RESIZE_BAND;
    let near_right = position.x > size.width - RESIZE_BAND;
    match (near_top, near_bottom, near_left, near_right) {
        (true, _, true, _) => Some(ResizeEdge::TopLeft),
        (true, _, _, true) => Some(ResizeEdge::TopRight),
        (_, true, true, _) => Some(ResizeEdge::BottomLeft),
        (_, true, _, true) => Some(ResizeEdge::BottomRight),
        (true, _, _, _) => Some(ResizeEdge::Top),
        (_, true, _, _) => Some(ResizeEdge::Bottom),
        (_, _, true, _) => Some(ResizeEdge::Left),
        (_, _, _, true) => Some(ResizeEdge::Right),
        _ => None,
    }
}

fn cursor_for_edge(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    #[test]
    fn detects_corners_edges_and_inside() {
        let window_size = size(px(800.), px(600.));
        assert_eq!(
            resize_edge(point(px(2.), px(2.)), window_size),
            Some(ResizeEdge::TopLeft)
        );
        assert_eq!(
            resize_edge(point(px(798.), px(598.)), window_size),
            Some(ResizeEdge::BottomRight)
        );
        assert_eq!(
            resize_edge(point(px(400.), px(2.)), window_size),
            Some(ResizeEdge::Top)
        );
        assert_eq!(
            resize_edge(point(px(798.), px(300.)), window_size),
            Some(ResizeEdge::Right)
        );
        assert_eq!(resize_edge(point(px(400.), px(300.)), window_size), None);
    }

    #[test]
    fn each_band_hitbox_matches_its_edge() {
        let window_size = size(px(800.), px(600.));
        for edge in RESIZE_EDGES {
            let band = resize_band_bounds(edge, window_size);
            let inset = point(px(0.5), px(0.5));
            for corner in [band.origin + inset, band.bottom_right() - inset] {
                assert_eq!(resize_edge(corner, window_size), Some(edge), "{edge:?}");
            }
        }
    }
}
