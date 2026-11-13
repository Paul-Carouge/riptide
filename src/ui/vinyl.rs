//! The signature vinyl record: grooves, a static light sheen, and a label (with the track's
//! thumbnail) whose print and groove marks rotate with the given angle.

use std::f32::consts::{FRAC_PI_4, PI, TAU};
use std::sync::Arc;

use gpui::{
    Background, BorderStyle, Bounds, Div, Image, ObjectFit, Pixels, Point, Rgba, Window, canvas,
    div, img, point, prelude::*, px, quad, size, transparent_black,
};

use super::theme;

const GROOVE_RING_COUNT: usize = 9;
const GROOVE_INNER_RATIO: f32 = 0.46;
const GROOVE_OUTER_RATIO: f32 = 0.94;
const LABEL_RATIO: f32 = 0.40;
const LABEL_IMAGE_RADIUS_RATIO: f32 = 0.30;
const LABEL_PRINT_RING_RATIO: f32 = 0.35;
const SPINDLE_RATIO: f32 = 0.045;
const ARC_SEGMENTS: usize = 20;

/// Brighter partial arcs that make rotation visible: (radius ratio, start angle, sweep).
const GROOVE_GLINTS: [(f32, f32, f32); 3] = [(0.58, 0.3, 0.9), (0.74, 2.6, 1.3), (0.88, 4.4, 0.7)];

pub fn vinyl_disc(diameter: Pixels, angle_radians: f32, label_image: Option<Arc<Image>>) -> Div {
    let label_image_diameter = diameter * LABEL_IMAGE_RADIUS_RATIO;
    let image_inset = (diameter - label_image_diameter) / 2.0;
    div()
        .relative()
        .flex_none()
        .size(diameter)
        .rounded_full()
        .shadow_md()
        .child(
            canvas(
                |_, _, _| (),
                |bounds, _, window, _| paint_disc_body(bounds, window),
            )
            .absolute()
            .size_full(),
        )
        .when_some(label_image, |this, image| {
            this.child(
                img(image)
                    .absolute()
                    .top(image_inset)
                    .left(image_inset)
                    .size(label_image_diameter)
                    .rounded_full()
                    .object_fit(ObjectFit::Cover),
            )
        })
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| paint_rotating_marks(bounds, angle_radians, window),
            )
            .absolute()
            .size_full(),
        )
}

fn center_and_radius(bounds: Bounds<Pixels>) -> (Point<Pixels>, Pixels) {
    (
        bounds.center(),
        bounds.size.width.min(bounds.size.height) / 2.0,
    )
}

fn circle_bounds(center: Point<Pixels>, radius: Pixels) -> Bounds<Pixels> {
    Bounds::new(
        point(center.x - radius, center.y - radius),
        size(radius * 2.0, radius * 2.0),
    )
}

fn paint_circle(window: &mut Window, center: Point<Pixels>, radius: Pixels, color: Rgba) {
    window.paint_quad(quad(
        circle_bounds(center, radius),
        radius,
        color,
        px(0.),
        transparent_black(),
        BorderStyle::Solid,
    ));
}

fn paint_ring(window: &mut Window, center: Point<Pixels>, radius: Pixels, color: Rgba) {
    window.paint_quad(quad(
        circle_bounds(center, radius),
        radius,
        Background::from(transparent_black()),
        px(1.),
        color,
        BorderStyle::Solid,
    ));
}

fn paint_arc(
    window: &mut Window,
    center: Point<Pixels>,
    radius: Pixels,
    start_angle: f32,
    sweep: f32,
    width: Pixels,
    color: Rgba,
) {
    let mut builder = gpui::PathBuilder::stroke(width);
    for step in 0..=ARC_SEGMENTS {
        let angle = start_angle + sweep * step as f32 / ARC_SEGMENTS as f32;
        let position = point(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        );
        if step == 0 {
            builder.move_to(position);
        } else {
            builder.line_to(position);
        }
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

fn paint_disc_body(bounds: Bounds<Pixels>, window: &mut Window) {
    let (center, radius) = center_and_radius(bounds);
    paint_circle(window, center, radius, theme::palette().vinyl);
    paint_ring(window, center, radius, theme::palette().vinyl_rim);
    for ring in 0..GROOVE_RING_COUNT {
        let ratio = GROOVE_INNER_RATIO
            + (GROOVE_OUTER_RATIO - GROOVE_INNER_RATIO) * ring as f32
                / (GROOVE_RING_COUNT - 1) as f32;
        paint_ring(
            window,
            center,
            radius * ratio,
            theme::palette().vinyl_groove,
        );
    }
    // The light reflection stays put while the record turns underneath it.
    let sheen_width = radius * 0.22;
    let sheen_radius = radius * 0.7;
    for start in [-FRAC_PI_4 - 0.35, PI - FRAC_PI_4 - 0.35] {
        paint_arc(
            window,
            center,
            sheen_radius,
            start,
            0.7,
            sheen_width,
            theme::palette().vinyl_sheen,
        );
    }
    paint_circle(
        window,
        center,
        radius * LABEL_RATIO,
        theme::palette().vinyl_label,
    );
}

fn paint_rotating_marks(bounds: Bounds<Pixels>, angle: f32, window: &mut Window) {
    let (center, radius) = center_and_radius(bounds);
    let thin = (radius * 0.012).max(px(1.));
    for (ratio, start, sweep) in GROOVE_GLINTS {
        paint_arc(
            window,
            center,
            radius * ratio,
            start + angle,
            sweep,
            thin,
            theme::palette().vinyl_sheen,
        );
    }
    let print_width = (radius * 0.02).max(px(1.));
    paint_arc(
        window,
        center,
        radius * LABEL_PRINT_RING_RATIO,
        angle,
        TAU * 0.62,
        print_width,
        theme::palette().vinyl_label_print,
    );
    let dot_angle = angle + TAU * 0.78;
    let dot_center = point(
        center.x + radius * LABEL_PRINT_RING_RATIO * dot_angle.cos(),
        center.y + radius * LABEL_PRINT_RING_RATIO * dot_angle.sin(),
    );
    paint_circle(
        window,
        dot_center,
        print_width * 1.6,
        theme::palette().vinyl_label_print,
    );
    paint_circle(
        window,
        center,
        radius * SPINDLE_RATIO,
        theme::palette().vinyl_spindle,
    );
}
