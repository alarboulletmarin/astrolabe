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
    // Speed smoothed over ±5 samples: GPS speed is jumpy second to second.
    let raw: Vec<Option<f32>> = track.samples.iter().map(|s| s.speed).collect();
    let smooth: Vec<Option<f32>> = (0..raw.len())
        .map(|i| {
            let w = &raw[i.saturating_sub(5)..(i + 6).min(raw.len())];
            let v: Vec<f32> = w.iter().flatten().copied().collect();
            (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
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
        // 3 m/s is 5:33 /km; the 30 m/s spike is smoothed, not drawn as is.
        assert!((run.speed[0].unwrap() - 333.3).abs() < 0.5);
        assert!(run.speed[50].unwrap() > 100.0);
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
