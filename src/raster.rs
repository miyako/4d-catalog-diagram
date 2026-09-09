//! Headless SVG → PNG rasterization.
//!
//! `resvg` + `tiny-skia` are pure Rust and statically linked, and the fonts
//! come from [`crate::fonts`], so this path needs no system Skia, no cairo, and
//! no fontconfig at runtime.

use crate::error::{AppError, Result};

/// Rasterize `svg` at `scale`× its intrinsic size and return PNG bytes.
pub fn svg_to_png(svg: &str, scale: f64) -> Result<Vec<u8>> {
    if !(scale.is_finite() && scale > 0.0) {
        return Err(AppError::Usage(format!(
            "--scale must be a positive number, got {scale}"
        )));
    }

    let mut options = usvg::Options {
        // No filesystem or network resolution of any kind.
        resources_dir: None,
        ..usvg::Options::default()
    };
    options.fontdb = std::sync::Arc::new(crate::fonts::font_database());
    options.font_family = "DejaVu Sans".to_string();

    let tree = usvg::Tree::from_str(svg, &options)
        .map_err(|e| AppError::Render(format!("could not rasterize the generated SVG: {e}")))?;

    let size = tree.size();
    let width = (size.width() as f64 * scale).round().max(1.0);
    let height = (size.height() as f64 * scale).round().max(1.0);
    if width > 20_000.0 || height > 20_000.0 {
        return Err(AppError::Render(format!(
            "the requested PNG would be {width}×{height} pixels; lower --scale or narrow the selection"
        )));
    }

    let mut pixmap = tiny_skia::Pixmap::new(width as u32, height as u32).ok_or_else(|| {
        AppError::Render("could not allocate a pixmap for the PNG output".to_string())
    })?;

    let transform = tiny_skia::Transform::from_scale(scale as f32, scale as f32);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    pixmap
        .encode_png()
        .map_err(|e| AppError::Render(format!("could not encode the PNG: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rasterizes_a_trivial_svg() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="#123456"/></svg>"##;
        let png = svg_to_png(svg, 2.0).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn rejects_a_nonsense_scale() {
        assert!(svg_to_png(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>",
            0.0
        )
        .is_err());
    }
}
