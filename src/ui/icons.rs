//! Line icons drawn as small SVGs, served by `Assets` and tinted with the text color of the
//! `svg()` element (GPUI rasterizes an SVG as an alpha mask), so they follow the palette.
//! Fine strokes on a 24-unit grid, round caps, like the hand-drawn tick of the checkboxes.

pub const MINIMIZE: &str = "riptide/icons/minimize.svg";
pub const MAXIMIZE: &str = "riptide/icons/maximize.svg";
pub const RESTORE: &str = "riptide/icons/restore.svg";
pub const CLOSE: &str = "riptide/icons/close.svg";
pub const SETTINGS: &str = "riptide/icons/settings.svg";
pub const BACK: &str = "riptide/icons/back.svg";

const SVG_OPEN: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">"#;

/// The SVG source of the icon at `path`, if it is one of ours.
pub fn icon_svg(path: &str) -> Option<String> {
    let body = match path {
        MINIMIZE => r#"<path d="M6 12.5h12"/>"#,
        MAXIMIZE => r#"<rect x="6" y="6" width="12" height="12" rx="2.5"/>"#,
        RESTORE => {
            r#"<rect x="5" y="9" width="10" height="10" rx="2"/><path d="M9 9V6.5A1.5 1.5 0 0 1 10.5 5H17a2 2 0 0 1 2 2v6.5A1.5 1.5 0 0 1 17.5 15H15"/>"#
        }
        CLOSE => r#"<path d="M7 7l10 10M17 7L7 17"/>"#,
        // A turntable tone-arm knob: an outer ring with eight short teeth around a hub.
        SETTINGS => {
            r#"<circle cx="12" cy="12" r="3"/><path d="M12 3v2.2M12 18.8V21M3 12h2.2M18.8 12H21M5.6 5.6l1.6 1.6M16.8 16.8l1.6 1.6M5.6 18.4l1.6-1.6M16.8 7.2l1.6-1.6"/><circle cx="12" cy="12" r="6.2"/>"#
        }
        BACK => r#"<path d="M19 12H6M11 6.5L5.5 12l5.5 5.5"/>"#,
        _ => return None,
    };
    Some(format!("{SVG_OPEN}{body}</svg>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_a_complete_svg_and_unknown_paths_are_refused() {
        for path in [MINIMIZE, MAXIMIZE, RESTORE, CLOSE, SETTINGS, BACK] {
            let svg = icon_svg(path).expect(path);
            assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"), "{path}");
        }
        assert!(icon_svg("riptide/icons/nope.svg").is_none());
    }
}
