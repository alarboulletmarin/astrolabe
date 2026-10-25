//! Line charts drawn on a canvas: they take whatever width they're given.

use gpui_kit::*;
use std::sync::Arc;

pub struct Series {
    pub xs: Arc<Vec<f32>>,
    pub ys: Arc<Vec<Option<f32>>>,
    /// `range(&ys)`, kept by the caller: it sorts every value.
    pub scale: Option<(f32, f32)>,
    pub line: Hsla,
    pub fill: Hsla,
    /// Higher values drawn lower (a pace: faster is up).
    pub invert: bool,
}

/// The scale: from the 2nd to the 98th percentile, so a stop or a GPS spike doesn't flatten the
/// rest; `None` if there's nothing to draw.
pub fn range(ys: &[Option<f32>]) -> Option<(f32, f32)> {
    let mut v: Vec<f32> = ys.iter().flatten().copied().filter(|v| v.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    let at = |q: f32| v[((v.len() - 1) as f32 * q).round() as usize];
    Some((at(0.02), at(0.98)))
}

/// Index of the sample nearest to `x` (xs ascending).
pub fn nearest(xs: &[f32], x: f32) -> Option<usize> {
    if xs.is_empty() {
        return None;
    }
    let i = xs.partition_point(|v| *v < x);
    Some(match i {
        0 => 0,
        i if i >= xs.len() => xs.len() - 1,
        i => if (xs[i] - x).abs() < (x - xs[i - 1]).abs() { i } else { i - 1 },
    })
}

/// The chart; `hover`: sample index to mark with a vertical line and a dot.
pub fn line_chart(s: Series, hover: Option<usize>, guide: Hsla) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            let (Some((lo, hi)), Some(&x0), Some(&x1)) = (s.scale, s.xs.first(), s.xs.last()) else { return };
            let span = (hi - lo).max(1e-3);
            // A little air above and below.
            let (lo, hi) = (lo - span * 0.08, hi + span * 0.08);
            let w = b.size.width / px(1.);
            let h = b.size.height / px(1.);
            let px_x = |x: f32| b.origin.x + px(w * (x - x0) / (x1 - x0).max(1e-6));
            let px_y = |y: f32| {
                let f = ((y - lo) / (hi - lo)).clamp(-0.02, 1.02);
                b.origin.y + px(h * if s.invert { f } else { 1.0 - f })
            };
            let bottom = b.origin.y + b.size.height;
            // Runs without a gap, at most one point per pixel.
            let mut runs: Vec<Vec<Point<Pixels>>> = vec![Vec::new()];
            let mut last_px = f32::NEG_INFINITY;
            for (x, y) in s.xs.iter().zip(s.ys.iter()) {
                match y {
                    Some(y) if y.is_finite() => {
                        let p = point(px_x(*x), px_y(*y));
                        let col = p.x / px(1.);
                        if col - last_px >= 1.0 || runs.last().unwrap().is_empty() {
                            runs.last_mut().unwrap().push(p);
                            last_px = col;
                        }
                    }
                    _ if !runs.last().unwrap().is_empty() => runs.push(Vec::new()),
                    _ => {}
                }
            }
            for run in runs.iter().filter(|r| r.len() > 1) {
                let mut area = PathBuilder::fill();
                area.move_to(point(run[0].x, bottom));
                run.iter().for_each(|p| area.line_to(*p));
                area.line_to(point(run.last().unwrap().x, bottom));
                area.close();
                if let Ok(p) = area.build() {
                    window.paint_path(p, s.fill);
                }
                let mut line = PathBuilder::stroke(px(1.5));
                line.move_to(run[0]);
                run[1..].iter().for_each(|p| line.line_to(*p));
                if let Ok(p) = line.build() {
                    window.paint_path(p, s.line);
                }
            }
            if let Some(i) = hover.filter(|i| *i < s.xs.len()) {
                let x = px_x(s.xs[i]);
                window.paint_quad(fill(Bounds::new(point(x, b.origin.y), size(px(1.), b.size.height)), guide));
                if let Some(Some(y)) = s.ys.get(i) {
                    let c = point(x - px(3.5), px_y(*y) - px(3.5));
                    window.paint_quad(fill(Bounds::new(c, size(px(7.), px(7.))), s.line).corner_radii(px(3.5)));
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{nearest, range};

    #[test]
    fn nearest_and_range() {
        assert_eq!(nearest(&[], 1.0), None);
        let xs = [0.0, 1.0, 2.0, 10.0];
        assert_eq!(nearest(&xs, -5.0), Some(0));
        assert_eq!(nearest(&xs, 1.4), Some(1));
        assert_eq!(nearest(&xs, 1.6), Some(2));
        assert_eq!(nearest(&xs, 99.0), Some(3));
        assert_eq!(range(&[]), None);
        assert_eq!(range(&[None, Some(f32::NAN)]), None);
        assert_eq!(range(&[Some(3.0), None, Some(-1.0)]), Some((-1.0, 3.0)));
        // One spike among a hundred values doesn't set the scale.
        let mut ys: Vec<Option<f32>> = (0..100).map(|i| Some(i as f32)).collect();
        ys.push(Some(10_000.0));
        assert_eq!(range(&ys), Some((2.0, 98.0)));
    }
}
