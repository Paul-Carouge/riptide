//! Design tokens: every color, size, radius, duration and spring constant used by the views.
//! See `docs/design/DESIGN.md` for the rationale. Views never use literal values.

use std::cell::Cell;
use std::time::Duration;

use gpui::{Pixels, Rgba, px};

const fn hex(value: u32) -> Rgba {
    Rgba {
        r: ((value >> 16) & 0xFF) as f32 / 255.0,
        g: ((value >> 8) & 0xFF) as f32 / 255.0,
        b: (value & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

const fn with_alpha(color: Rgba, alpha: f32) -> Rgba {
    Rgba { a: alpha, ..color }
}

/// Every color the views use. `LIGHT` is kraft paper under daylight, `DARK` the same sleeve
/// under a lamp at night ("vinyle de nuit"): warm brown-black, cream ink, same burnt orange.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub paper: Rgba,
    pub paper_deep: Rgba,
    pub card: Rgba,
    pub card_hover: Rgba,
    pub rule: Rgba,
    pub rule_strong: Rgba,
    pub ink: Rgba,
    pub ink_muted: Rgba,
    /// Placeholders and hints; still 4.5:1 on paper.
    pub ink_faint: Rgba,
    pub grain_speck: Rgba,
    pub accent: Rgba,
    pub accent_hover: Rgba,
    /// Accent used as text on paper (AA).
    pub accent_text: Rgba,
    pub accent_soft: Rgba,
    pub amber: Rgba,
    pub on_accent: Rgba,
    pub success: Rgba,
    pub success_soft: Rgba,
    pub teal: Rgba,
    pub teal_soft: Rgba,
    pub danger: Rgba,
    pub danger_soft: Rgba,
    pub focus_ring: Rgba,
    pub selection: Rgba,
    pub vinyl: Rgba,
    pub vinyl_groove: Rgba,
    pub vinyl_rim: Rgba,
    pub vinyl_sheen: Rgba,
    pub vinyl_label: Rgba,
    pub vinyl_label_print: Rgba,
    pub vinyl_spindle: Rgba,
    /// Thin outer edge of the floating window (sleeve edge, never a cold grey).
    pub window_edge: Rgba,
    /// Warm drop shadow of the floating window.
    pub window_shadow: Rgba,
}

pub const LIGHT: Palette = Palette {
    paper: hex(0xF3E9D2),
    paper_deep: hex(0xEADBBB),
    card: hex(0xFBF5E6),
    card_hover: hex(0xFFFAEE),
    rule: hex(0xD8C49C),
    rule_strong: hex(0xBFA77A),
    ink: hex(0x2A1E16),
    ink_muted: hex(0x6B5646),
    ink_faint: hex(0x7A6553),
    grain_speck: with_alpha(hex(0x5A4330), 0.10),
    accent: hex(0xB4481A),
    accent_hover: hex(0x9E3D14),
    accent_text: hex(0x9E3D14),
    accent_soft: hex(0xF2D3B3),
    amber: hex(0xE39B2D),
    on_accent: hex(0xFFF6E5),
    success: hex(0x56622A),
    success_soft: hex(0xE1E3C2),
    teal: hex(0x1F6F6B),
    teal_soft: hex(0xCFE2DC),
    danger: hex(0xA23022),
    danger_soft: hex(0xF4D5C9),
    focus_ring: hex(0x1F6F6B),
    selection: with_alpha(hex(0xE39B2D), 0.35),
    vinyl: hex(0x1C1714),
    vinyl_groove: hex(0x342B26),
    vinyl_rim: with_alpha(hex(0x1C1714), 0.0),
    vinyl_sheen: with_alpha(hex(0xFFF1D6), 0.22),
    vinyl_label: hex(0xC4541C),
    vinyl_label_print: with_alpha(hex(0xFFF6E5), 0.85),
    vinyl_spindle: hex(0xF3E9D2),
    window_edge: hex(0xC9B088),
    window_shadow: with_alpha(hex(0x4A2E18), 0.30),
};

pub const DARK: Palette = Palette {
    paper: hex(0x1D1611),
    paper_deep: hex(0x34291F),
    card: hex(0x261D16),
    card_hover: hex(0x30251C),
    rule: hex(0x3B2E24),
    rule_strong: hex(0x806A55),
    ink: hex(0xF2E6CD),
    ink_muted: hex(0xC4B094),
    ink_faint: hex(0x9C8872),
    grain_speck: with_alpha(hex(0xF2E6CD), 0.05),
    accent: hex(0xB84A1B),
    accent_hover: hex(0xA3411A),
    accent_text: hex(0xEE8A52),
    accent_soft: hex(0x4A2716),
    amber: hex(0xE9A640),
    on_accent: hex(0xFFF6E5),
    success: hex(0xAEBD72),
    success_soft: hex(0x2F3419),
    teal: hex(0x76C6BC),
    teal_soft: hex(0x183633),
    danger: hex(0xF2907A),
    danger_soft: hex(0x45201A),
    focus_ring: hex(0x76C6BC),
    selection: with_alpha(hex(0xE9A640), 0.35),
    vinyl: hex(0x110D0A),
    vinyl_groove: hex(0x2E2620),
    // On a dark table the record's edge needs a lamp-lit rim to separate it from the paper.
    vinyl_rim: with_alpha(hex(0xF2E6CD), 0.16),
    vinyl_sheen: with_alpha(hex(0xFFF1D6), 0.16),
    vinyl_label: hex(0xC4541C),
    vinyl_label_print: with_alpha(hex(0xFFF6E5), 0.85),
    vinyl_spindle: hex(0x1D1611),
    window_edge: hex(0x4A3A2D),
    window_shadow: with_alpha(hex(0x050302), 0.60),
};

pub const TRANSPARENT: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

impl Palette {
    /// Component-wise blend, `amount` 0 = `self`, 1 = `target` (theme cross-fade).
    pub fn blended_toward(&self, target: &Palette, amount: f32) -> Palette {
        let amount = amount.clamp(0.0, 1.0);
        let mix = |from: Rgba, to: Rgba| Rgba {
            r: from.r + (to.r - from.r) * amount,
            g: from.g + (to.g - from.g) * amount,
            b: from.b + (to.b - from.b) * amount,
            a: from.a + (to.a - from.a) * amount,
        };
        Palette {
            paper: mix(self.paper, target.paper),
            paper_deep: mix(self.paper_deep, target.paper_deep),
            card: mix(self.card, target.card),
            card_hover: mix(self.card_hover, target.card_hover),
            rule: mix(self.rule, target.rule),
            rule_strong: mix(self.rule_strong, target.rule_strong),
            ink: mix(self.ink, target.ink),
            ink_muted: mix(self.ink_muted, target.ink_muted),
            ink_faint: mix(self.ink_faint, target.ink_faint),
            grain_speck: mix(self.grain_speck, target.grain_speck),
            accent: mix(self.accent, target.accent),
            accent_hover: mix(self.accent_hover, target.accent_hover),
            accent_text: mix(self.accent_text, target.accent_text),
            accent_soft: mix(self.accent_soft, target.accent_soft),
            amber: mix(self.amber, target.amber),
            on_accent: mix(self.on_accent, target.on_accent),
            success: mix(self.success, target.success),
            success_soft: mix(self.success_soft, target.success_soft),
            teal: mix(self.teal, target.teal),
            teal_soft: mix(self.teal_soft, target.teal_soft),
            danger: mix(self.danger, target.danger),
            danger_soft: mix(self.danger_soft, target.danger_soft),
            focus_ring: mix(self.focus_ring, target.focus_ring),
            selection: mix(self.selection, target.selection),
            vinyl: mix(self.vinyl, target.vinyl),
            vinyl_groove: mix(self.vinyl_groove, target.vinyl_groove),
            vinyl_rim: mix(self.vinyl_rim, target.vinyl_rim),
            vinyl_sheen: mix(self.vinyl_sheen, target.vinyl_sheen),
            vinyl_label: mix(self.vinyl_label, target.vinyl_label),
            vinyl_label_print: mix(self.vinyl_label_print, target.vinyl_label_print),
            vinyl_spindle: mix(self.vinyl_spindle, target.vinyl_spindle),
            window_edge: mix(self.window_edge, target.window_edge),
            window_shadow: mix(self.window_shadow, target.window_shadow),
        }
    }
}

thread_local! {
    // GPUI renders and paints on the main thread only, so a thread-local is the active theme.
    static ACTIVE_PALETTE: Cell<Palette> = const { Cell::new(LIGHT) };
}

/// The palette of the frame being rendered (set by the root view at the start of `render`).
pub fn palette() -> Palette {
    ACTIVE_PALETTE.with(Cell::get)
}

pub fn set_active_palette(palette: Palette) {
    ACTIVE_PALETTE.with(|active| active.set(palette));
}

// Typography.
pub const FONT_DISPLAY: &str = "Young Serif";
pub const FONT_BODY: &str = "IBM Plex Sans";
pub const FONT_MONO: &str = "IBM Plex Mono";
pub const TEXT_XS: Pixels = px(12.);
pub const TEXT_SM: Pixels = px(13.);
pub const TEXT_MD: Pixels = px(15.);
pub const TEXT_LG: Pixels = px(18.);
pub const TEXT_XL: Pixels = px(24.);
pub const TEXT_DISPLAY: Pixels = px(34.);

// Spacing (4 px grid).
pub const SPACE_1: Pixels = px(4.);
pub const SPACE_2: Pixels = px(8.);
pub const SPACE_3: Pixels = px(12.);
pub const SPACE_4: Pixels = px(16.);
pub const SPACE_5: Pixels = px(24.);
pub const SPACE_6: Pixels = px(32.);

// Radii.
pub const RADIUS_SM: Pixels = px(6.);
pub const RADIUS_MD: Pixels = px(10.);
pub const RADIUS_LG: Pixels = px(16.);
pub const RADIUS_PILL: Pixels = px(999.);

// Sizes.
pub const BORDER: Pixels = px(1.);
pub const FOCUS_RING_WIDTH: Pixels = px(2.);
pub const CONTROL_HEIGHT: Pixels = px(36.);
pub const CONTROL_HEIGHT_LARGE: Pixels = px(44.);
pub const ICON_BUTTON_SIZE: Pixels = px(32.);
pub const CHECKBOX_SIZE: Pixels = px(18.);
/// Round minimize / maximize / close knobs and their line icons.
pub const WINDOW_CONTROL_SIZE: Pixels = px(30.);
pub const WINDOW_CONTROL_ICON_SIZE: Pixels = px(14.);
pub const ICON_SIZE: Pixels = px(16.);
/// "Update available" dot on the settings button.
pub const BADGE_DOT_SIZE: Pixels = px(8.);
pub const SETTINGS_COLUMN_MIN_WIDTH: Pixels = px(320.);
pub const SETTINGS_MAX_WIDTH: Pixels = px(980.);
pub const RELEASE_NOTES_MAX_HEIGHT: Pixels = px(140.);
pub const THUMBNAIL_PREVIEW_WIDTH: Pixels = px(176.);
pub const THUMBNAIL_PREVIEW_HEIGHT: Pixels = px(99.);
pub const TRACK_THUMBNAIL_WIDTH: Pixels = px(48.);
pub const TRACK_THUMBNAIL_HEIGHT: Pixels = px(27.);
pub const VINYL_CARD_SIZE: Pixels = px(64.);
pub const VINYL_EMPTY_STATE_SIZE: Pixels = px(132.);
pub const PROGRESS_TRACK_HEIGHT: Pixels = px(6.);
pub const LEFT_COLUMN_WIDTH: Pixels = px(500.);
pub const TRACK_LIST_MAX_HEIGHT: Pixels = px(240.);
/// Fixed row heights: the playlist and the queue are virtualized (`uniform_list`).
pub const TRACK_ROW_HEIGHT: Pixels = px(36.);
pub const QUEUE_CARD_HEIGHT: Pixels = px(124.);
pub const QUEUE_ROW_GAP: Pixels = px(12.);
pub const WINDOW_WIDTH: Pixels = px(1100.);
pub const WINDOW_HEIGHT: Pixels = px(760.);
pub const WINDOW_MIN_WIDTH: Pixels = px(800.);
pub const WINDOW_MIN_HEIGHT: Pixels = px(600.);
/// Below this window width, secondary labels are dropped.
pub const COMPACT_WINDOW_WIDTH: Pixels = px(960.);

// Grain: deterministic specks painted over the paper.
pub const GRAIN_SPECKS_PER_10K_SQUARE_PIXELS: f32 = 14.0;
pub const GRAIN_SPECK_SIZE: Pixels = px(1.5);

// Motion.
pub const CARD_ENTER_DURATION: Duration = Duration::from_millis(620);
pub const CARD_ENTER_OFFSET: Pixels = px(28.);
pub const PREVIEW_UNFOLD_DURATION: Duration = Duration::from_millis(560);
pub const PREVIEW_UNFOLD_OFFSET: Pixels = px(-14.);
/// Settings panel and Face A/B swapping places: spring slide plus fade.
pub const PANEL_SWAP_DURATION: Duration = Duration::from_millis(520);
pub const PANEL_SWAP_OFFSET: Pixels = px(18.);
pub const LOADING_PULSE_DURATION: Duration = Duration::from_millis(1400);
/// Opacity range of the looping pulses (tools preparation dot, converting groove).
pub const TOOLS_PULSE_OPACITY: (f32, f32) = (0.25, 1.0);
pub const CONVERTING_PULSE_OPACITY: (f32, f32) = (0.45, 1.0);
/// Cadence of continuous animations (spinning discs, progress catch-up, pulses): ~30 fps
/// is smooth for a slow 33⅓ rpm turn and costs half of a 60 Hz redraw.
pub const ANIMATION_FRAME_INTERVAL: Duration = Duration::from_millis(33);
pub const ERROR_SHAKE_DURATION: Duration = Duration::from_millis(380);
pub const ERROR_SHAKE_AMPLITUDE: Pixels = px(6.);
/// Damped spring used by entrance animations (natural frequency in Hz, damping ratio).
pub const SPRING_FREQUENCY_HZ: f32 = 2.1;
pub const SPRING_DAMPING_RATIO: f32 = 0.58;
/// 33⅓ rpm, in radians per second.
pub const VINYL_PLAYING_RADIANS_PER_SECOND: f32 = 33.333 / 60.0 * std::f32::consts::TAU;
/// Time constant of the turntable motor when spinning up / braking.
pub const VINYL_SPIN_UP_SECONDS: f32 = 0.55;
pub const VINYL_SPIN_DOWN_SECONDS: f32 = 0.9;
/// Progress bar follows the real fraction with this time constant.
pub const PROGRESS_SMOOTHING_SECONDS: f32 = 0.25;
/// Cross-fade between light and dark palettes.
pub const THEME_CROSSFADE_DURATION: Duration = Duration::from_millis(320);
