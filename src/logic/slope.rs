//! How steep a route is: the grade along it (smoothed, the altitudes being read at about 30 m),
//! its classes for colouring, and its climbs.

/// The grade is measured over this many metres around each point.
const SPAN: f32 = 150.0;
/// A stretch of one class shorter than this takes the class of the one before: the altitudes'
/// noise would otherwise flicker between classes.
const MIN_RUN: f32 = 80.0;
/// A climb ends when the route has come down this much from its top.
const DROP: f32 = 10.0;
/// A climb counts from this much gain, at this mean grade.
const MIN_GAIN: f32 = 25.0;
const MIN_GRADE: f32 = 0.03;

/// The grade at each point, rise over run across `SPAN` metres around it (`None` where an
/// altitude is missing).
pub fn grades(along: &[f32], eles: &[Option<f32>]) -> Vec<Option<f32>> {
    let n = along.len().min(eles.len());
    let (mut lo, mut hi) = (0, 0);
    (0..n)
        .map(|i| {
            while along[i] - along[lo] > SPAN / 2.0 {
                lo += 1;
            }
            while hi + 1 < n && along[hi + 1] - along[i] <= SPAN / 2.0 {
                hi += 1;
            }
            let run = along[hi] - along[lo];
            match (eles[lo], eles[hi]) {
                (Some(a), Some(b)) if run > 1.0 => Some((b - a) / run),
                _ => None,
            }
        })
        .collect()
}

/// The class of a grade for colouring, by its steepness up or down: 0 under 3 %, 1 under 6 %,
/// 2 under 10 %, 3 under 15 %, 4 beyond.
pub fn class(grade: f32) -> u8 {
    match grade.abs() {
        g if g < 0.03 => 0,
        g if g < 0.06 => 1,
        g if g < 0.10 => 2,
        g if g < 0.15 => 3,
        _ => 4,
    }
}

/// The route cut in stretches of one class, `(from, to, class)` as indices of its points, one
/// after the other: the shortest stretch under `MIN_RUN` metres joins its longer neighbour, again
/// and again, so that noise doesn't flicker between classes.
pub fn runs(along: &[f32], eles: &[Option<f32>]) -> Vec<(usize, usize, u8)> {
    let n = along.len().min(eles.len());
    let classes: Vec<u8> = grades(along, eles).into_iter().map(|g| g.map_or(0, class)).collect();
    let mut out: Vec<(usize, usize, u8)> = Vec::new();
    for i in 0..n.saturating_sub(1) {
        match out.last_mut() {
            Some(last) if last.2 == classes[i] => last.1 = i + 1,
            _ => out.push((i, i + 1, classes[i])),
        }
    }
    let len = |r: &(usize, usize, u8)| along[r.1] - along[r.0];
    // ponytail: a scan for the shortest at each join, O(stretches²); a heap if long routes show it.
    while out.len() > 1 {
        let (s, short) = out.iter().enumerate().map(|(i, r)| (i, len(r))).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        if short >= MIN_RUN {
            break;
        }
        let into = match (s.checked_sub(1), (s + 1 < out.len()).then_some(s + 1)) {
            (Some(l), Some(r)) => if len(&out[l]) >= len(&out[r]) { l } else { r },
            (Some(l), None) => l,
            (None, Some(r)) => r,
            (None, None) => break,
        };
        let (a, b) = (out[s.min(into)], out[s.max(into)]);
        let k = out[into].2;
        out[s.min(into)] = (a.0, b.1, k);
        out.remove(s.max(into));
        // Neighbours now of one class: one stretch.
        let at = s.min(into);
        if at + 1 < out.len() && out[at + 1].2 == k {
            out[at].1 = out[at + 1].1;
            out.remove(at + 1);
        }
        if at > 0 && out[at - 1].2 == k {
            out[at - 1].1 = out[at].1;
            out.remove(at);
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Climb {
    /// Indices of its foot and its top in the route's points.
    pub from: usize,
    pub to: usize,
    /// Metres along, gained, and its mean and steepest grade (over `SPAN`).
    pub dist: f32,
    pub gain: f32,
    pub grade: f32,
    pub steepest: f32,
}

/// The climbs of a route, in order: from a low point to the top before it comes down `DROP`
/// metres, kept when they gain `MIN_GAIN` metres at `MIN_GRADE` or more.
pub fn climbs(along: &[f32], eles: &[Option<f32>]) -> Vec<Climb> {
    let n = along.len().min(eles.len());
    let grades = grades(along, eles);
    let mut out = Vec::new();
    // The foot of the climb under way, and its top so far.
    let (mut foot, mut top): (Option<(usize, f32)>, Option<(usize, f32)>) = (None, None);
    let close = |foot: (usize, f32), top: (usize, f32), out: &mut Vec<Climb>| {
        let (gain, dist) = (top.1 - foot.1, along[top.0] - along[foot.0]);
        if gain >= MIN_GAIN && dist > 0.0 && gain / dist >= MIN_GRADE {
            let steepest = grades[foot.0..=top.0].iter().flatten().fold(0.0f32, |a, g| a.max(*g));
            out.push(Climb { from: foot.0, to: top.0, dist, gain, grade: gain / dist, steepest });
        }
    };
    for i in 0..n {
        let Some(e) = eles[i] else { continue };
        match (foot, top) {
            (None, _) => (foot, top) = (Some((i, e)), Some((i, e))),
            (Some(f), Some(t)) => {
                if e > t.1 {
                    top = Some((i, e));
                } else if t.1 - e >= DROP {
                    // Come down from the top: the climb is over; the next one starts here.
                    close(f, t, &mut out);
                    (foot, top) = (Some((i, e)), Some((i, e)));
                } else if e <= f.1 {
                    // Flat or down to the foot again: nothing climbed yet, the foot moves here.
                    (foot, top) = (Some((i, e)), Some((i, e)));
                }
            }
            (Some(_), None) => unreachable!(),
        }
    }
    if let (Some(f), Some(t)) = (foot, top) {
        close(f, t, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A route of `n` points every 10 m with these altitudes.
    fn route(eles: impl Fn(f32) -> f32, n: usize) -> (Vec<f32>, Vec<Option<f32>>) {
        let along: Vec<f32> = (0..n).map(|i| i as f32 * 10.0).collect();
        let eles = along.iter().map(|d| Some(eles(*d))).collect();
        (along, eles)
    }

    #[test]
    fn grades_read_rise_over_run() {
        let (along, eles) = route(|d| d * 0.08, 101);
        let g = grades(&along, &eles);
        assert!(g.iter().all(|g| (g.unwrap() - 0.08).abs() < 1e-4));
        // A missing altitude at an end of the span: no grade there.
        let mut holes = eles.clone();
        holes[0] = None;
        assert_eq!(grades(&along, &holes)[0], None);
        assert!(grades(&[], &[]).is_empty());
        assert_eq!(grades(&[0.0], &[Some(5.0)]), vec![None]);
        assert_eq!([0.0, 0.029, -0.05, 0.07, 0.12, -0.3].map(class), [0, 0, 1, 2, 3, 4]);
    }

    #[test]
    fn runs_cover_the_route_without_flicker() {
        // Flat, a 9 % wall of 400 m, flat; with a sawtooth of noise every 20 m.
        let shape = |d: f32| {
            let base = if d < 500.0 { 0.0 } else if d < 900.0 { (d - 500.0) * 0.09 } else { 36.0 };
            base + if (d / 20.0) as i32 % 2 == 0 { 0.0 } else { 1.5 }
        };
        let (along, eles) = route(shape, 151);
        let r = runs(&along, &eles);
        // Joined end to end over the whole route, each long enough but maybe the last.
        assert_eq!(r[0].0, 0);
        assert_eq!(r.last().unwrap().1, 150);
        assert!(r.windows(2).all(|w| w[0].1 == w[1].0 && w[0].2 != w[1].2), "{r:?}");
        assert!(r[..r.len() - 1].iter().all(|(a, b, _)| along[*b] - along[*a] >= MIN_RUN), "{r:?}");
        assert!(r.len() <= 5, "{r:?}");
        assert!(r.iter().any(|(a, b, k)| *k == 2 && along[*b] - along[*a] > 250.0), "{r:?}");
        assert!(runs(&[], &[]).is_empty());
        assert!(runs(&[0.0], &[Some(1.0)]).is_empty());
        assert_eq!(runs(&[0.0, 10.0], &[None, None]), vec![(0, 1, 0)]);
    }

    #[test]
    fn climbs_are_found_from_foot_to_top() {
        // Flat 1 km, up 120 m over 1.5 km (8 %), down 2 km, up 20 m (too little), flat.
        let shape = |d: f32| match d {
            d if d < 1000.0 => 100.0,
            d if d < 2500.0 => 100.0 + (d - 1000.0) * 0.08,
            d if d < 4500.0 => 220.0 - (d - 2500.0) * 0.05,
            d if d < 5000.0 => 120.0 + (d - 4500.0) * 0.04,
            _ => 140.0,
        };
        let (along, eles) = route(shape, 601);
        let c = climbs(&along, &eles);
        assert_eq!(c.len(), 1, "{c:?}");
        let c = c[0];
        assert!((along[c.from] - 1000.0).abs() <= 10.0 && (along[c.to] - 2500.0).abs() <= 10.0, "{c:?}");
        assert!((c.gain - 120.0).abs() < 1.0 && (c.grade - 0.08).abs() < 0.002 && c.steepest >= c.grade);
        // A bump of 5 m in the middle doesn't cut the climb in two.
        let bumpy = |d: f32| shape(d) - if (1700.0..1760.0).contains(&d) { (d - 1700.0).min(1760.0 - d) * 0.3 } else { 0.0 };
        let (along, eles) = route(bumpy, 601);
        assert_eq!(climbs(&along, &eles).len(), 1);
        // A climb to the very end; none on the flat; none without altitude.
        let (along, eles) = route(|d| d * 0.05, 101);
        assert_eq!(climbs(&along, &eles).len(), 1);
        let (along, eles) = route(|_| 50.0, 101);
        assert!(climbs(&along, &eles).is_empty());
        assert!(climbs(&along, &vec![None; along.len()]).is_empty());
        assert!(climbs(&[], &[]).is_empty());
    }
}
