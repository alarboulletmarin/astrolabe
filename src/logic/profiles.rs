//! An activity's profiles: pace (or speed), heart rate and altitude against distance or time.

use crate::logic::activity::Sport;
use crate::logic::fit::Track;
use std::sync::Arc;

/// What the charts plot, sample by sample.
pub struct Profiles {
    /// Kilometres, or minutes when nothing recorded a distance.
    pub xs: Arc<Vec<f32>>,
    pub by_distance: bool,
    /// Seconds per km (per 100 m swimming), or km/h on a bike.
    pub speed: Arc<Vec<Option<f32>>>,
    pub hr: Arc<Vec<Option<f32>>>,
    pub alt: Arc<Vec<Option<f32>>>,
}

pub fn profiles(track: &Track, sport: Sport) -> Profiles {
    let by_distance = track.samples.iter().filter(|s| s.dist.is_some()).count() * 2 > track.samples.len();
    let xs: Vec<f32> = track
        .samples
        .iter()
        .scan(0.0f32, |last, s| {
            let x = if by_distance { s.dist.map(|d| d / 1000.0).unwrap_or(*last) } else { s.t / 60.0 };
            // Never backwards: the charts need ascending x.
            *last = x.max(*last);
            Some(*last)
        })
        .collect();
    // Speed as the median of a window: ±100 m along a distance axis, ±30 samples along time. GPS
    // speed is jumpy, and on a distance axis a stop takes no width at all, so it was drawn as a
    // needle down to the bottom. A median over distance drops stops and short dips; a slow
    // stretch (a steep climb walked) keeps its real width.
    let raw: Vec<Option<f32>> = track.samples.iter().map(|s| s.speed).collect();
    let (mut lo, mut hi) = (0, 0);
    let smooth: Vec<Option<f32>> = (0..raw.len())
        .map(|i| {
            if by_distance {
                while xs[lo] < xs[i] - 0.1 {
                    lo += 1;
                }
                while hi < xs.len() && xs[hi] <= xs[i] + 0.1 {
                    hi += 1;
                }
            } else {
                (lo, hi) = (i.saturating_sub(30), (i + 31).min(raw.len()));
            }
            let mut v: Vec<f32> = raw[lo..hi].iter().flatten().copied().filter(|v| v.is_finite()).collect();
            v.sort_by(f32::total_cmp);
            v.get(v.len() / 2).copied()
        })
        .collect();
    let speed = smooth
        .into_iter()
        .map(|v| {
            let v = v?;
            match sport {
                Sport::Bike => Some(v * 3.6),
                // Slower than 20:00 /km (or 5:00 /100 m) is standing: no point.
                Sport::Swim => (v > 0.33).then(|| 100.0 / v),
                _ => (v > 0.83).then(|| 1000.0 / v),
            }
        })
        .collect();
    Profiles {
        xs: Arc::new(xs),
        by_distance,
        speed: Arc::new(speed),
        hr: Arc::new(track.samples.iter().map(|s| s.hr).collect()),
        alt: Arc::new(track.samples.iter().map(|s| s.alt).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::{Sport, Track, profiles};
    use crate::logic::fit::Sample;

    #[test]
    fn profiles_follow_the_sport() {
        let s: Vec<Sample> = (0..100)
            .map(|i| Sample { t: i as f32, dist: Some(i as f32 * 3.0), speed: Some(if i == 50 { 30.0 } else { 3.0 }), hr: Some(140.0), ..Default::default() })
            .collect();
        let tr = Track { samples: s, laps: vec![] };
        let run = profiles(&tr, Sport::Run);
        assert!(run.by_distance);
        assert!((run.xs[99] - 0.297).abs() < 1e-4);
        // 3 m/s is 5:33 /km; the 30 m/s spike is gone, not softened.
        assert!((run.speed[0].unwrap() - 333.3).abs() < 0.5);
        assert!((run.speed[50].unwrap() - 333.3).abs() < 0.5);
        // A stop of a few samples in a steady run leaves no dip either.
        let dip = Track { samples: (0..100).map(|i| Sample { t: i as f32, dist: Some(i as f32 * 3.0), speed: Some(if (40..46).contains(&i) { 0.9 } else { 3.0 }), ..Default::default() }).collect(), laps: vec![] };
        assert!(profiles(&dip, Sport::Run).speed.iter().all(|v| (v.unwrap() - 333.3).abs() < 0.5));
        let bike = profiles(&tr, Sport::Bike);
        assert!((bike.speed[0].unwrap() - 10.8).abs() < 1e-3);
        // Standing still: no pace rather than an absurd one.
        let still = Track { samples: vec![Sample { speed: Some(0.1), ..Default::default() }], laps: vec![] };
        assert_eq!(profiles(&still, Sport::Run).speed[0], None);
        // No distance at all: minutes on the x axis.
        let indoor = Track { samples: (0..10).map(|i| Sample { t: i as f32 * 60.0, ..Default::default() }).collect(), laps: vec![] };
        let p = profiles(&indoor, Sport::Strength);
        assert!(!p.by_distance);
        assert_eq!(p.xs[9], 9.0);
        assert!(profiles(&Track::default(), Sport::Run).xs.is_empty());
    }
}
