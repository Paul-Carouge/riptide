//! GPUI views and design tokens.

mod app;
mod components;
mod demo;
mod format;
mod motion;
mod paper_grain;
mod region;
mod render;
mod text_input;
pub mod theme;
mod thumbnails;
mod vinyl;
mod window_frame;

use std::borrow::Cow;

use gpui::App;

pub use app::RiptideApp;
pub use paper_grain::Assets;

const EMBEDDED_FONTS: [&[u8]; 5] = [
    include_bytes!("../../assets/fonts/YoungSerif-Regular.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexSans-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexMono-Medium.ttf"),
];

/// Registers fonts and key bindings. Call once before opening the window.
pub fn initialize(cx: &mut App) {
    let fonts = EMBEDDED_FONTS
        .iter()
        .map(|bytes| Cow::Borrowed(*bytes))
        .collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("riptide: cannot load embedded fonts, falling back to system fonts: {error}");
    }
    text_input::bind_text_input_keys(cx);
    app::bind_application_keys(cx);
}
