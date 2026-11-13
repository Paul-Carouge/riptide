//! Paper grain: deterministic specks over the whole window, without any image file.
//!
//! The specks of one square tile are generated once as an SVG (served by `Assets`), which
//! GPUI rasterizes once into an alpha mask; the window then paints a dozen tinted copies of
//! that tile. Painting each speck as its own quad cost about a thousand primitives per frame.

use std::borrow::Cow;
use std::sync::LazyLock;

use gpui::{
    AssetSource, Bounds, IntoElement, Pixels, SharedString, Styled, TransformationMatrix, canvas,
    point, px, size,
};

use super::theme;

const GRAIN_TILE_ASSET_PATH: &str = "riptide/paper-grain-tile.svg";
/// Big enough that the repeat is not noticeable, small enough for the sprite atlas.
const GRAIN_TILE_SIZE: f32 = 256.0;

static GRAIN_TILE_SVG: LazyLock<String> = LazyLock::new(grain_tile_svg);

/// The app's asset source: only the generated grain tile.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok((path == GRAIN_TILE_ASSET_PATH).then(|| Cow::Borrowed(GRAIN_TILE_SVG.as_bytes())))
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

/// Fills its bounds with the grain tile, tinted with the palette's speck color.
pub fn paper_grain() -> impl IntoElement {
    canvas(
        |_, _, _| (),
        |bounds: Bounds<Pixels>, _, window, cx| {
            let tile = px(GRAIN_TILE_SIZE);
            let color = theme::palette().grain_speck;
            let mut y = bounds.origin.y;
            while y < bounds.bottom() {
                let mut x = bounds.origin.x;
                while x < bounds.right() {
                    let painted = window.paint_svg(
                        Bounds::new(point(x, y), size(tile, tile)),
                        GRAIN_TILE_ASSET_PATH.into(),
                        TransformationMatrix::unit(),
                        color.into(),
                        cx,
                    );
                    if let Err(error) = painted {
                        eprintln!("riptide: cannot paint the paper grain: {error}");
                        return;
                    }
                    x += tile;
                }
                y += tile;
            }
        },
    )
    .size_full()
}

fn grain_tile_svg() -> String {
    let speck_count = (GRAIN_TILE_SIZE * GRAIN_TILE_SIZE / 10_000.0
        * theme::GRAIN_SPECKS_PER_10K_SQUARE_PIXELS) as u32;
    let speck = f32::from(theme::GRAIN_SPECK_SIZE);
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{GRAIN_TILE_SIZE}" height="{GRAIN_TILE_SIZE}">"#
    );
    for index in 0..speck_count {
        let x = pseudo_random_unit(index * 2) * GRAIN_TILE_SIZE;
        let y = pseudo_random_unit(index * 2 + 1) * GRAIN_TILE_SIZE;
        svg.push_str(&format!(
            r#"<rect x="{x:.2}" y="{y:.2}" width="{speck}" height="{speck}"/>"#
        ));
    }
    svg.push_str("</svg>");
    svg
}

/// Integer hash (lowbias32) mapped to [0, 1).
fn pseudo_random_unit(seed: u32) -> f32 {
    let mut value = seed.wrapping_add(0x9E37_79B9);
    value ^= value >> 16;
    value = value.wrapping_mul(0x7FEB_352D);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846C_A68B);
    value ^= value >> 16;
    (value >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_has_the_token_density_and_stays_inside_the_tile() {
        let svg = grain_tile_svg();
        let expected = (GRAIN_TILE_SIZE * GRAIN_TILE_SIZE / 10_000.0
            * theme::GRAIN_SPECKS_PER_10K_SQUARE_PIXELS) as usize;
        assert_eq!(svg.matches("<rect").count(), expected);
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        assert!((0..1000).all(|seed| (0.0..1.0).contains(&pseudo_random_unit(seed))));
    }

    #[test]
    fn asset_source_serves_only_the_grain_tile() {
        let tile = Assets.load(GRAIN_TILE_ASSET_PATH).ok().flatten();
        assert!(tile.is_some_and(|bytes| bytes.starts_with(b"<svg")));
        assert!(Assets.load("other.svg").ok().flatten().is_none());
    }
}
