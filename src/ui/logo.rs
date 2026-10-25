//! The logo: an astrolabe seen from the front. A graduated ring (the limbe) hung from its throne,
//! and the alidade, the sighting rule, turning on the pivot. Drawn in a 100 × 100 box; the same
//! figures are in `assets/icon.svg`.

use gpui_kit::*;
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    Ink,
    Accent,
}

pub struct Stroke {
    pub points: Vec<[f32; 2]>,
    pub role: Role,
    pub width: f32,
}

const CENTER: [f32; 2] = [50.0, 55.0];
const RING: f32 = 36.0;
/// Angle of the alidade, upward to the right.
const SIGHT: f32 = -35.0 * PI / 180.0;

fn at(r: f32, angle: f32) -> [f32; 2] {
    [CENTER[0] + r * angle.cos(), CENTER[1] + r * angle.sin()]
}

fn circle(c: [f32; 2], r: f32) -> Vec<[f32; 2]> {
    (0..=64).map(|i| [c[0] + r * (i as f32 * PI / 32.0).cos(), c[1] + r * (i as f32 * PI / 32.0).sin()]).collect()
}

/// Every line of the drawing. `fine`: the ticks every 10° too, for a size where they can be told
/// apart; otherwise the twelve of the hours.
pub fn strokes(fine: bool) -> Vec<Stroke> {
    let ink = |points, width| Stroke { points, role: Role::Ink, width };
    let mut out = vec![
        ink(circle(CENTER, RING), 5.0), ink(circle([50.0, 10.0], 7.5), 3.5),
        // The rete's ecliptic, off the center: what a stopwatch does not have.
        ink(circle([50.0, 61.0], 14.0), 2.6),
    ];
    for i in 0..36 {
        let major = i % 3 == 0;
        if major || fine {
            let a = i as f32 * PI / 18.0;
            out.push(ink(vec![at(if major { 22.0 } else { 27.5 }, a), at(31.0, a)], if major { 3.4 } else { 1.8 }));
        }
    }
    // The alidade, and a vane across each end.
    let (a, b) = (at(-RING + 6.0, SIGHT), at(RING - 6.0, SIGHT));
    let across = |p: [f32; 2]| {
        let n = [-SIGHT.sin() * 7.0, SIGHT.cos() * 7.0];
        vec![[p[0] - n[0], p[1] - n[1]], [p[0] + n[0], p[1] + n[1]]]
    };
    let accent = |points, width| Stroke { points, role: Role::Accent, width };
    out.extend([accent(vec![a, b], 6.0), accent(across(a), 4.0), accent(across(b), 4.0)]);
    out
}

/// The logo at `size` pixels, in the ink and accent colors of the theme.
pub fn logo(side: f32, ink: Hsla, accent: Hsla) -> Canvas<()> {
    let strokes = strokes(side >= 48.0);
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
                    window.paint_path(path, if s.role == Role::Ink { ink } else { accent });
                }
            }
            let r = px(k * 5.0);
            window.paint_quad(fill(Bounds::new(point(p(CENTER).x - r, p(CENTER).y - r), size(r * 2.0, r * 2.0)), accent).corner_radii(r));
        },
    )
    .size(px(side))
}

#[cfg(test)]
mod tests {
    use super::{Role, strokes};

    #[test]
    fn everything_is_drawn_inside_the_box() {
        for fine in [false, true] {
            for s in strokes(fine) {
                for p in &s.points {
                    assert!(p[0] - s.width / 2.0 >= 0.0 && p[0] + s.width / 2.0 <= 100.0 && p[1] - s.width / 2.0 >= 0.0 && p[1] + s.width / 2.0 <= 100.0, "{p:?}");
                }
            }
        }
    }

    #[test]
    fn the_hours_always_show_and_the_fine_ticks_come_with_size() {
        let ticks = |fine| strokes(fine).iter().filter(|s| s.role == Role::Ink && s.points.len() == 2).count();
        assert_eq!(ticks(false), 12);
        assert_eq!(ticks(true), 36);
        // The alidade is the accent: one rule and two vanes.
        assert_eq!(strokes(false).iter().filter(|s| s.role == Role::Accent).count(), 3);
    }
}
