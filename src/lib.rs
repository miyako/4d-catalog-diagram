//! `4d-catalog-diagram` — turn a 4D database catalog (structure XML) into an
//! interactive HTML diagram, a standalone SVG, or a PNG image.
//!
//! The pipeline is deliberately split so each stage can be tested in isolation:
//! `parse` → `select` → `layout` → `render_svg` / `render_html` / `raster`.

pub mod error;
pub mod fonts;
pub mod inspect;
pub mod layout;
pub mod model;
pub mod parse;
pub mod raster;
pub mod render_html;
pub mod render_svg;
pub mod scene;
pub mod select;
pub mod types;

pub use error::{AppError, ExitCode};
