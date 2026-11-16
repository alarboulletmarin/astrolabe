//! The logo: an astrolabe's plate from the front. The limb (a ring), the almucantars (circles of
//! equal altitude, off the center toward the zenith, cut by the limb) and the zenith, the accent.
//! Drawn in a 100 × 100 box; the same figures are in `assets/icon.svg`, and the bold ones for small
//! sizes in `assets/icon-small.svg`.

use gpui_kit::*;
use std::f32::consts::PI;

pub struct Stroke {
    pub points: Vec<[f32; 2]>,
    pub width: f32,
}

const CENTER: [f32; 2] = [50.0, 50.0];
const LIMB: f32 = 42.0;
const ZENITH: [f32; 2] = [50.0, 39.2];

/// One drawing: limb width, almucantar width, the almucantars as (center y, radius), zenith radius.
struct Figure {
    limb: f32,
    line: f32,
    circles: &'static [(f32, f32)],
    zenith: f32,
}

const FINE: Figure = Figure { limb: 7.0, line: 2.4, circles: &[(35.55, 16.37), (37.9, 9.6), (38.97, 3.95), (30.76, 25.7)], zenith: 4.0 };
/// Fewer, thicker lines, for a size where the fine ones would blur together.
const BOLD: Figure = Figure { limb: 9.0, line: 5.0, circles: &[(36.3, 14.52), (38.8, 5.3), (28.87, 28.79)], zenith: 6.0 };

fn figure(bold: bool) -> &'static Figure {
    if bold { &BOLD } else { &FINE }
}

/// The circle (center (50, `cy`), radius `r`) up to the limb's center line. What passes the limb's
/// inner edge lies under the limb, drawn after it.
fn almucantar(cy: f32, r: f32) -> Vec<[f32; 2]> {
    // At angle t, the distance² to the center is r² + d² + 2·d·r·sin t, with d < 0: the point is
    // inside the limb when sin t is at least `low`.
    let d = cy - CENTER[1];
    let low = (LIMB * LIMB - r * r - d * d) / (2.0 * d * r);
    let (from, to) = if low <= -1.0 { (0.0, 2.0 * PI) } else { (low.asin(), PI - low.asin()) };
    arc([CENTER[0], cy], r, from, to)
}

fn arc(c: [f32; 2], r: f32, from: f32, to: f32) -> Vec<[f32; 2]> {
    (0..=64).map(|i| from + (to - from) * i as f32 / 64.0).map(|t| [c[0] + r * t.cos(), c[1] + r * t.sin()]).collect()
}

/// Every ink line, in the order they are painted: the almucantars, then the limb over their ends.
pub fn strokes(bold: bool) -> Vec<Stroke> {
    let f = figure(bold);
    let mut out: Vec<Stroke> = f.circles.iter().map(|&(cy, r)| Stroke { points: almucantar(cy, r), width: f.line }).collect();
    out.push(Stroke { points: arc(CENTER, LIMB, 0.0, 2.0 * PI), width: f.limb });
    out
}

/// The logo at `side` pixels, in the ink and accent colors of the theme.
pub fn logo(side: f32, ink: Hsla, accent: Hsla) -> Canvas<()> {
    let bold = side < 48.0;
    let strokes = strokes(bold);
    let zenith = figure(bold).zenith;
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            // Pixels per unit of the box.
            let k = b.size.width / px(100.0);
            let p = |q: [f32; 2]| point(b.origin.x + px(k * q[0]), b.origin.y + px(k * q[1]));
            for s in &strokes {
                let mut path = PathBuilder::stroke(px(k * s.width));
                path.move_to(p(s.points[0]));
                s.points[1..].iter().for_each(|q| path.line_to(p(*q)));
                if let Ok(path) = path.build() {
                    window.paint_path(path, ink);
                }
            }
            let (c, r) = (p(ZENITH), px(k * zenith));
            window.paint_quad(fill(Bounds::new(point(c.x - r, c.y - r), size(r * 2.0, r * 2.0)), accent).corner_radii(r));
        },
    )
    .size(px(side))
}

#[cfg(test)]
mod tests {
    use super::{CENTER, LIMB, strokes};

    fn dist(p: [f32; 2]) -> f32 {
        (p[0] - CENTER[0]).hypot(p[1] - CENTER[1])
    }

    #[test]
    fn everything_is_drawn_inside_the_box() {
        for bold in [false, true] {
            for s in strokes(bold) {
                for p in &s.points {
                    assert!(p[0] - s.width / 2.0 >= 0.0 && p[0] + s.width / 2.0 <= 100.0 && p[1] - s.width / 2.0 >= 0.0 && p[1] + s.width / 2.0 <= 100.0, "{p:?}");
                }
            }
        }
    }

    #[test]
    fn the_almucantars_stop_under_the_limb() {
        for bold in [false, true] {
            let s = strokes(bold);
            let (limb, lines) = s.split_last().unwrap();
            assert!(limb.points.iter().all(|&p| (dist(p) - LIMB).abs() < 0.01));
            for line in lines {
                assert!(line.points.iter().all(|&p| dist(p) <= LIMB + 0.01));
            }
            // The widest one is cut, and its two ends sit on the limb's center line.
            let cut: Vec<_> = lines.iter().filter(|l| (dist(l.points[0]) - LIMB).abs() < 0.01).collect();
            assert_eq!(cut.len(), 1);
            assert!((dist(*cut[0].points.last().unwrap()) - LIMB).abs() < 0.01);
        }
        assert_eq!(strokes(false).len(), 5);
        assert_eq!(strokes(true).len(), 4);
    }
}
