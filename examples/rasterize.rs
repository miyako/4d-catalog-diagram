//! Development helper: rasterize an arbitrary SVG file with the same pipeline
//! the CLI uses. `cargo run --example rasterize -- in.svg out.png [scale]`
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let svg = std::fs::read_to_string(&args[1]).unwrap();
    let scale: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(4.0);
    let png = catalog_diagram::raster::svg_to_png(&svg, scale).unwrap();
    std::fs::write(&args[2], png).unwrap();
}
