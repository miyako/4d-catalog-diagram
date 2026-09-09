//! Positioning.
//!
//! Two modes, both fully deterministic:
//!
//! * `as-designed` reuses the `left`/`top` the human designer arranged in 4D's
//!   own structure editor. Only the *position* is taken from the XML — card
//!   size is always recomputed from our own text metrics, because the stored
//!   `width`/`height` were measured with 4D's fonts, not ours. Tables that
//!   carry no coordinates are auto-placed into free space below the rest.
//! * `auto` ignores stored coordinates and runs a fixed-seed
//!   Fruchterman-Reingold relaxation over the relation graph.
//!
//! Either way, a final separation pass guarantees no two cards overlap.

use crate::model::Coordinates;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    AsDesigned,
    Auto,
}

impl LayoutMode {
    pub fn parse(value: &str) -> Option<LayoutMode> {
        match value {
            "as-designed" => Some(LayoutMode::AsDesigned),
            "auto" => Some(LayoutMode::Auto),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LayoutMode::AsDesigned => "as-designed",
            LayoutMode::Auto => "auto",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Size {
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Pos {
    pub x: f64,
    pub y: f64,
}

/// Minimum empty space kept between two cards.
pub const GAP: f64 = 28.0;

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(values[values.len() / 2])
}

pub fn as_designed(coords: &[Option<Coordinates>], sizes: &[Size]) -> Vec<Pos> {
    debug_assert_eq!(coords.len(), sizes.len());

    // 4D's editor units are close to pixels, but its cards are narrower than
    // ours because it uses a denser font. Rescale positions by the ratio of
    // typical widths so the designer's spacing survives our larger cards.
    let ratio = |pick: fn(&Coordinates) -> f64, ours: fn(&Size) -> f64| -> f64 {
        let theirs: Vec<f64> = coords
            .iter()
            .flatten()
            .map(pick)
            .filter(|v| *v > 1.0)
            .collect();
        let mine: Vec<f64> = sizes.iter().map(ours).filter(|v| *v > 1.0).collect();
        match (median(theirs), median(mine)) {
            (Some(t), Some(m)) if t > 1.0 => (m / t).clamp(1.0, 3.0),
            _ => 1.0,
        }
    };
    let scale_x = ratio(|c| c.width, |s| s.w);
    let scale_y = ratio(|c| c.height, |s| s.h);

    let mut positions = vec![Pos::default(); sizes.len()];
    let mut placed = vec![false; sizes.len()];
    for (i, coord) in coords.iter().enumerate() {
        if let Some(c) = coord {
            positions[i] = Pos {
                x: c.left * scale_x,
                y: c.top * scale_y,
            };
            placed[i] = true;
        }
    }

    let missing: Vec<usize> = (0..sizes.len()).filter(|&i| !placed[i]).collect();
    if !missing.is_empty() {
        // Park the unpositioned cards in a grid underneath everything that has
        // real coordinates, then let the separation pass tidy up.
        let base_y = positions
            .iter()
            .enumerate()
            .filter(|(i, _)| placed[*i])
            .map(|(i, p)| p.y + sizes[i].h)
            .fold(0.0_f64, f64::max);
        let base_x = positions
            .iter()
            .enumerate()
            .filter(|(i, _)| placed[*i])
            .map(|(_, p)| p.x)
            .fold(f64::INFINITY, f64::min);
        let base_x = if base_x.is_finite() { base_x } else { 0.0 };

        let columns = (missing.len() as f64).sqrt().ceil().max(1.0) as usize;
        let column_width = missing.iter().map(|&i| sizes[i].w).fold(0.0_f64, f64::max) + GAP;
        let mut row_y = base_y + GAP * 2.0;
        let mut row_height = 0.0_f64;
        for (n, &i) in missing.iter().enumerate() {
            let column = n % columns;
            if column == 0 && n > 0 {
                row_y += row_height + GAP;
                row_height = 0.0;
            }
            positions[i] = Pos {
                x: base_x + column as f64 * column_width,
                y: row_y,
            };
            row_height = row_height.max(sizes[i].h);
        }
    }

    resolve_overlaps(&mut positions, sizes);
    positions
}

pub fn auto(sizes: &[Size], edges: &[(usize, usize)]) -> Vec<Pos> {
    let n = sizes.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![Pos::default()];
    }

    let total_area: f64 = sizes.iter().map(|s| (s.w + GAP) * (s.h + GAP)).sum();
    let area = total_area * 3.0;
    let k = (area / n as f64).sqrt();

    // Seeded deterministically from the node index — no RNG, no hash order.
    let radius = k * (n as f64).sqrt() * 0.5;
    let mut px: Vec<f64> = Vec::with_capacity(n);
    let mut py: Vec<f64> = Vec::with_capacity(n);
    for i in 0..n {
        let angle = std::f64::consts::TAU * i as f64 / n as f64;
        // A slight golden-ratio radial jitter avoids a perfectly symmetric
        // start, which relaxes poorly, while staying reproducible.
        let jitter = 0.6 + 0.4 * ((i as f64 * 0.6180339887).fract());
        px.push(radius * jitter * angle.cos());
        py.push(radius * jitter * angle.sin());
    }

    const ITERATIONS: usize = 300;
    let mut temperature = k * 0.8;
    let cooling = temperature / (ITERATIONS as f64 + 1.0);

    let mut dx = vec![0.0; n];
    let mut dy = vec![0.0; n];

    for _ in 0..ITERATIONS {
        dx.iter_mut().for_each(|v| *v = 0.0);
        dy.iter_mut().for_each(|v| *v = 0.0);

        for i in 0..n {
            for j in (i + 1)..n {
                let mut ex = px[i] - px[j];
                let mut ey = py[i] - py[j];
                let mut dist = (ex * ex + ey * ey).sqrt();
                if dist < 0.01 {
                    // Deterministic nudge for coincident nodes.
                    ex = 0.01 * (1 + i) as f64;
                    ey = 0.01 * (1 + j) as f64;
                    dist = (ex * ex + ey * ey).sqrt();
                }
                let force = k * k / dist;
                let ux = ex / dist * force;
                let uy = ey / dist * force;
                dx[i] += ux;
                dy[i] += uy;
                dx[j] -= ux;
                dy[j] -= uy;
            }
        }

        for &(a, b) in edges {
            if a == b || a >= n || b >= n {
                continue;
            }
            let ex = px[a] - px[b];
            let ey = py[a] - py[b];
            let dist = (ex * ex + ey * ey).sqrt().max(0.01);
            let force = dist * dist / k;
            let ux = ex / dist * force;
            let uy = ey / dist * force;
            dx[a] -= ux;
            dy[a] -= uy;
            dx[b] += ux;
            dy[b] += uy;
        }

        for i in 0..n {
            let dist = (dx[i] * dx[i] + dy[i] * dy[i]).sqrt();
            if dist > 0.0 {
                let step = dist.min(temperature);
                px[i] += dx[i] / dist * step;
                py[i] += dy[i] / dist * step;
            }
        }
        temperature -= cooling;
    }

    // Fruchterman-Reingold only settles the *relative* structure; its absolute
    // spread is a function of `k` and of how many springs pull inwards, so a
    // small sparsely connected catalog drifts apart over tens of thousands of
    // pixels. Rescale the whole cloud to a target extent derived from real card
    // sizes and let `resolve_overlaps` open it back up only where it must.
    let avg_w = sizes.iter().map(|s| s.w).sum::<f64>() / n as f64;
    let avg_h = sizes.iter().map(|s| s.h).sum::<f64>() / n as f64;
    let root = (n as f64).sqrt();
    let target_x = root * (avg_w + GAP) * 1.25;
    let target_y = root * (avg_h + GAP) * 1.25;
    let span_x = span(&px);
    let span_y = span(&py);
    let scale = match (span_x > 1.0, span_y > 1.0) {
        (true, true) => (target_x / span_x).min(target_y / span_y),
        (true, false) => target_x / span_x,
        (false, true) => target_y / span_y,
        (false, false) => 1.0,
    };
    for i in 0..n {
        px[i] *= scale;
        py[i] *= scale;
    }

    // Diagrams read better wide than tall.
    if span_y > span_x * 1.35 {
        std::mem::swap(&mut px, &mut py);
    }

    let mut positions: Vec<Pos> = (0..n)
        .map(|i| Pos {
            x: px[i] - sizes[i].w / 2.0,
            y: py[i] - sizes[i].h / 2.0,
        })
        .collect();
    resolve_overlaps(&mut positions, sizes);
    positions
}

fn span(values: &[f64]) -> f64 {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if min.is_finite() && max.is_finite() {
        max - min
    } else {
        0.0
    }
}

/// Push overlapping cards apart along their axis of least penetration until
/// every pair is separated by at least [`GAP`]. Pairs are visited in a fixed
/// order so the result never depends on iteration order.
pub fn resolve_overlaps(positions: &mut [Pos], sizes: &[Size]) {
    const MAX_PASSES: usize = 600;
    let n = positions.len();
    for _ in 0..MAX_PASSES {
        let mut moved = false;
        for i in 0..n {
            for j in (i + 1)..n {
                let ax = positions[i].x;
                let ay = positions[i].y;
                let bx = positions[j].x;
                let by = positions[j].y;
                let aw = sizes[i].w + GAP;
                let ah = sizes[i].h + GAP;
                let bw = sizes[j].w;
                let bh = sizes[j].h;

                let overlap_x = (ax + aw).min(bx + bw) - ax.max(bx);
                let overlap_y = (ay + ah).min(by + bh) - ay.max(by);
                if overlap_x <= 0.0 || overlap_y <= 0.0 {
                    continue;
                }
                moved = true;

                if overlap_x <= overlap_y {
                    let shift = overlap_x / 2.0 + 0.5;
                    if ax <= bx {
                        positions[i].x -= shift;
                        positions[j].x += shift;
                    } else {
                        positions[i].x += shift;
                        positions[j].x -= shift;
                    }
                } else {
                    let shift = overlap_y / 2.0 + 0.5;
                    if ay <= by {
                        positions[i].y -= shift;
                        positions[j].y += shift;
                    } else {
                        positions[i].y += shift;
                        positions[j].y -= shift;
                    }
                }
            }
        }
        if !moved {
            break;
        }
    }
}

/// Shift everything so the bounding box starts at `(margin, margin)`, and
/// return the resulting canvas size.
pub fn normalize(positions: &mut [Pos], sizes: &[Size], margin: f64) -> (f64, f64) {
    if positions.is_empty() {
        return (margin * 2.0, margin * 2.0);
    }
    let min_x = positions.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let min_y = positions.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    for p in positions.iter_mut() {
        p.x = p.x - min_x + margin;
        p.y = p.y - min_y + margin;
    }
    let width = positions
        .iter()
        .zip(sizes)
        .map(|(p, s)| p.x + s.w)
        .fold(0.0_f64, f64::max)
        + margin;
    let height = positions
        .iter()
        .zip(sizes)
        .map(|(p, s)| p.y + s.h)
        .fold(0.0_f64, f64::max)
        + margin;
    (round2(width), round2(height))
}

/// Two decimal places is plenty for SVG geometry and keeps output compact and
/// byte-stable across platforms.
pub fn round2(value: f64) -> f64 {
    let v = (value * 100.0).round() / 100.0;
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes(n: usize) -> Vec<Size> {
        (0..n).map(|_| Size { w: 200.0, h: 120.0 }).collect()
    }

    #[test]
    fn overlaps_are_always_resolved() {
        let sizes = sizes(6);
        let mut positions = vec![Pos { x: 0.0, y: 0.0 }; 6];
        resolve_overlaps(&mut positions, &sizes);
        for i in 0..6 {
            for j in (i + 1)..6 {
                let x_apart = positions[i].x + sizes[i].w <= positions[j].x
                    || positions[j].x + sizes[j].w <= positions[i].x;
                let y_apart = positions[i].y + sizes[i].h <= positions[j].y
                    || positions[j].y + sizes[j].h <= positions[i].y;
                assert!(x_apart || y_apart, "cards {i} and {j} still overlap");
            }
        }
    }

    #[test]
    fn auto_layout_is_deterministic() {
        let sizes = sizes(8);
        let edges = vec![(0, 1), (1, 2), (2, 3), (3, 4), (0, 5), (5, 6), (6, 7)];
        let a = auto(&sizes, &edges);
        let b = auto(&sizes, &edges);
        assert_eq!(a, b);
    }

    #[test]
    fn tables_without_coordinates_are_auto_placed() {
        let coords = vec![
            Some(Coordinates {
                left: 0.0,
                top: 0.0,
                width: 144.0,
                height: 200.0,
            }),
            None,
        ];
        let sizes = sizes(2);
        let positions = as_designed(&coords, &sizes);
        assert!(positions[1].y > positions[0].y);
    }

    #[test]
    fn normalize_moves_content_to_the_margin() {
        let mut positions = vec![Pos { x: -50.0, y: -20.0 }, Pos { x: 300.0, y: 400.0 }];
        let sizes = sizes(2);
        let (w, h) = normalize(&mut positions, &sizes, 40.0);
        assert_eq!(positions[0], Pos { x: 40.0, y: 40.0 });
        assert!(w > 0.0 && h > 0.0);
    }
}
