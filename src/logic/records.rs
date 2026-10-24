//! Records: the boards (which sport, which rows) and their podiums.

use crate::logic::activity::{Activity, Sport, fmt_clock, fmt_km, fmt_m};
use crate::logic::fit::Derived;
use chrono::{Datelike, NaiveDate};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Fastest time over this many metres, anywhere inside an activity.
    Effort(f64),
    Longest,
    MostClimb,
    LongestTime,
}

pub struct Board {
    pub title: &'static str,
    pub sport: Sport,
    /// Leaves out treadmill, indoor and virtual runs, whose distances are estimates.
    pub outdoor: bool,
    pub rows: &'static [(&'static str, Kind)],
}

pub const BOARDS: [Board; 4] = [
    Board {
        title: "Course sur route",
        sport: Sport::Run,
        outdoor: true,
        rows: &[
            ("1 km", Kind::Effort(1000.0)),
            ("5 km", Kind::Effort(5000.0)),
            ("10 km", Kind::Effort(10_000.0)),
            ("Semi-marathon", Kind::Effort(21_097.5)),
            ("Marathon", Kind::Effort(42_195.0)),
            ("Plus longue course", Kind::Longest),
        ],
    },
    Board {
        title: "Trail",
        sport: Sport::Trail,
        outdoor: false,
        rows: &[
            ("Plus longue distance", Kind::Longest),
            ("Plus gros dénivelé", Kind::MostClimb),
            ("Plus long effort", Kind::LongestTime),
            ("10 km", Kind::Effort(10_000.0)),
            ("Semi-marathon", Kind::Effort(21_097.5)),
            ("Marathon", Kind::Effort(42_195.0)),
            ("50 km", Kind::Effort(50_000.0)),
            ("100 km", Kind::Effort(100_000.0)),
        ],
    },
    Board {
        title: "Vélo",
        sport: Sport::Bike,
        outdoor: false,
        rows: &[
            ("20 km", Kind::Effort(20_000.0)),
            ("40 km", Kind::Effort(40_000.0)),
            ("100 km", Kind::Effort(100_000.0)),
            ("Plus longue sortie", Kind::Longest),
            ("Plus gros dénivelé", Kind::MostClimb),
        ],
    },
    Board {
        title: "Natation",
        sport: Sport::Swim,
        outdoor: false,
        rows: &[
            ("400 m", Kind::Effort(400.0)),
            ("1 000 m", Kind::Effort(1000.0)),
            ("1 500 m", Kind::Effort(1500.0)),
            ("Plus longue nage", Kind::Longest),
        ],
    },
];

pub fn outdoor(a: &Activity) -> bool {
    let k = &a.activity_type.type_key;
    !(k.contains("treadmill") || k.contains("indoor") || k.contains("virtual"))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub id: i64,
    /// Seconds for an effort or a duration, metres for a distance or a climb.
    pub value: f64,
    pub date: NaiveDate,
}

/// The `n` best of a board's row, best first.
pub fn podium(acts: &[Activity], derived: &HashMap<i64, Derived>, board: &Board, kind: Kind, year: Option<i32>, n: usize) -> Vec<Entry> {
    let mut out: Vec<Entry> = acts
        .iter()
        .filter(|a| a.sport() == board.sport && (!board.outdoor || outdoor(a)))
        .filter_map(|a| {
            let date = a.date()?;
            if year.is_some_and(|y| date.year() != y) {
                return None;
            }
            let value = match kind {
                Kind::Effort(m) => derived.get(&a.activity_id)?.efforts.iter().find(|e| (e.metres as f64 - m).abs() < 0.5)?.seconds as f64,
                Kind::Longest => a.distance?,
                Kind::MostClimb => a.elevation_gain?,
                Kind::LongestTime => a.duration?,
            };
            (value > 0.0).then_some(Entry { id: a.activity_id, value, date })
        })
        .collect();
    match kind {
        Kind::Effort(_) => out.sort_by(|a, b| a.value.total_cmp(&b.value)),
        _ => out.sort_by(|a, b| b.value.total_cmp(&a.value)),
    }
    out.truncate(n);
    out
}

/// The value as it reads: a time and its pace for an effort.
pub fn show(kind: Kind, sport: Sport, v: f64) -> (String, String) {
    match kind {
        Kind::Effort(m) => {
            let pace = match sport {
                Sport::Bike => format!("{:.1} km/h", m / v * 3.6).replace('.', ","),
                Sport::Swim => format!("{} /100m", fmt_clock(v / m * 100.0)),
                _ => format!("{} /km", fmt_clock(v / m * 1000.0)),
            };
            (fmt_clock(v), pace)
        }
        Kind::Longest => (fmt_km(v), String::new()),
        Kind::MostClimb => (fmt_m(v), String::new()),
        Kind::LongestTime => (fmt_clock(v), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::{BOARDS, Kind, outdoor, podium, show};
    use crate::logic::activity::{Activity, Sport};
    use crate::logic::fit::{Derived, Effort};
    use std::collections::HashMap;

    fn a(id: i64, t: &str, key: &str, dist: f64, dur: f64, climb: f64) -> Activity {
        serde_json::from_value(serde_json::json!({
            "activityId": id, "startTimeLocal": t, "activityType": {"typeKey": key},
            "distance": dist, "duration": dur, "elevationGain": climb
        }))
        .unwrap()
    }

    #[test]
    fn podiums_by_kind_year_and_surface() {
        let acts = vec![
            a(1, "2026-03-01 07:00:00", "running", 10_000.0, 2700.0, 50.0),
            a(2, "2025-03-01 07:00:00", "running", 12_000.0, 3300.0, 80.0),
            a(3, "2026-04-01 07:00:00", "treadmill_running", 10_000.0, 2000.0, 0.0),
            a(4, "2026-05-01 07:00:00", "trail_running", 30_000.0, 14_000.0, 1800.0),
        ];
        let eff = |s: f32| Derived { line: vec![], efforts: vec![Effort { metres: 5000.0, seconds: s }] };
        let derived: HashMap<i64, Derived> = [(1, eff(1290.0)), (2, eff(1250.0)), (3, eff(900.0)), (4, eff(1700.0))].into();
        let road = &BOARDS[0];
        let five = podium(&acts, &derived, road, Kind::Effort(5000.0), None, 3);
        // The treadmill doesn't count; trail neither; fastest first.
        assert_eq!(five.iter().map(|e| e.id).collect::<Vec<_>>(), [2, 1]);
        assert_eq!(podium(&acts, &derived, road, Kind::Effort(5000.0), Some(2026), 3).iter().map(|e| e.id).collect::<Vec<_>>(), [1]);
        assert!(podium(&acts, &derived, road, Kind::Effort(5000.0), Some(1999), 3).is_empty());
        assert!(podium(&acts, &derived, road, Kind::Effort(42_195.0), None, 3).is_empty());
        assert_eq!(podium(&acts, &derived, road, Kind::Longest, None, 1)[0].id, 2);
        assert!(podium(&acts, &HashMap::new(), road, Kind::Effort(5000.0), None, 3).is_empty(), "no tracks yet");
        let trail = &BOARDS[1];
        assert_eq!(podium(&acts, &derived, trail, Kind::MostClimb, None, 3)[0].id, 4);
        assert!(!outdoor(&acts[2]) && outdoor(&acts[0]));
        assert_eq!(show(Kind::Effort(5000.0), Sport::Run, 1250.0), ("20:50".into(), "4:10 /km".into()));
        assert_eq!(show(Kind::Effort(40_000.0), Sport::Bike, 3600.0).1, "40,0 km/h");
        assert_eq!(show(Kind::Effort(400.0), Sport::Swim, 400.0).1, "1:40 /100m");
    }
}
