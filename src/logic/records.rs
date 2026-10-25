//! Records: the boards (which sport, which rows) and their podiums.

use crate::logic::activity::{Activity, Sport, fmt_clock, fmt_km, fmt_m};
use crate::logic::fit::Derived;
use chrono::{Datelike, NaiveDate, Weekday};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Fastest time over this many metres, anywhere inside an activity.
    Effort(f64),
    Longest,
    MostClimb,
    LongestTime,
    /// Highest heart rate held a few seconds, in bpm.
    MaxHr,
    /// The week (Monday to Sunday) with the most kilometres, seconds or metres of climb.
    WeekDistance,
    WeekTime,
    WeekClimb,
}

impl Kind {
    pub fn is_week(self) -> bool {
        matches!(self, Kind::WeekDistance | Kind::WeekTime | Kind::WeekClimb)
    }

    /// Whether the smaller the value, the better: a time over a distance.
    pub fn is_time_over_distance(self) -> bool {
        matches!(self, Kind::Effort(_))
    }
}

#[derive(Clone, Copy)]
pub struct Board {
    pub title: &'static str,
    /// `None`: every sport together.
    pub sport: Option<Sport>,
    /// Leaves out treadmill, indoor and virtual runs, whose distances are estimates.
    pub outdoor: bool,
    pub rows: &'static [(&'static str, Kind)],
}

pub const BOARDS: [Board; 5] = [
    Board {
        title: "Course sur route",
        sport: Some(Sport::Run),
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
        sport: Some(Sport::Trail),
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
        sport: Some(Sport::Bike),
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
        sport: Some(Sport::Swim),
        outdoor: false,
        rows: &[
            ("400 m", Kind::Effort(400.0)),
            ("1 000 m", Kind::Effort(1000.0)),
            ("1 500 m", Kind::Effort(1500.0)),
            ("Plus longue nage", Kind::Longest),
        ],
    },
    Board {
        title: "En tout genre",
        sport: None,
        outdoor: false,
        rows: &[
            ("Plus longue sortie", Kind::Longest),
            ("Plus longue durée", Kind::LongestTime),
            ("Plus gros dénivelé", Kind::MostClimb),
            ("FC maximale", Kind::MaxHr),
            ("Semaine la plus longue", Kind::WeekDistance),
            ("Semaine la plus chargée", Kind::WeekTime),
            ("Semaine la plus haute", Kind::WeekClimb),
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
    let of_board = acts.iter().filter(|a| board.sport.is_none_or(|s| a.sport() == s) && (!board.outdoor || outdoor(a)));
    let dated = of_board.filter_map(|a| Some((a, a.date()?))).filter(|(_, date)| year.is_none_or(|y| date.year() == y));
    let mut out: Vec<Entry> = if kind.is_week() {
        weeks(dated, kind)
    } else {
        dated
            .filter_map(|(a, date)| {
                let value = match kind {
                    Kind::Effort(m) => derived.get(&a.activity_id)?.efforts.iter().find(|e| (e.metres as f64 - m).abs() < 0.5)?.seconds as f64,
                    Kind::Longest => a.distance?,
                    Kind::MostClimb => a.elevation_gain?,
                    Kind::LongestTime => a.duration?,
                    Kind::MaxHr => derived.get(&a.activity_id)?.max_hr as f64,
                    _ => return None,
                };
                (value > 0.0).then_some(Entry { id: a.activity_id, value, date })
            })
            .collect()
    };
    if kind.is_time_over_distance() {
        out.sort_by(|a, b| a.value.total_cmp(&b.value));
    } else {
        out.sort_by(|a, b| b.value.total_cmp(&a.value));
    }
    out.truncate(n);
    out
}

/// One entry per week: the sum, dated by the Monday, standing for the week's longest outing (what a
/// click opens). ponytail: a week cut by the year filter counts only its days in that year.
fn weeks<'a>(acts: impl Iterator<Item = (&'a Activity, NaiveDate)>, kind: Kind) -> Vec<Entry> {
    let mut by_week: HashMap<NaiveDate, (f64, f64, i64)> = HashMap::new();
    for (a, date) in acts {
        let monday = date.week(Weekday::Mon).first_day();
        let value = match kind {
            Kind::WeekDistance => a.distance,
            Kind::WeekTime => a.duration,
            _ => a.elevation_gain,
        }
        .unwrap_or(0.0);
        let longest = a.duration.unwrap_or(0.0);
        let slot = by_week.entry(monday).or_insert((0.0, -1.0, a.activity_id));
        slot.0 += value;
        if longest > slot.1 {
            (slot.1, slot.2) = (longest, a.activity_id);
        }
    }
    by_week.into_iter().filter(|(_, (v, ..))| *v > 0.0).map(|(date, (value, _, id))| Entry { id, value, date }).collect()
}

/// A record that beat the one before it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Broken {
    pub board: &'static str,
    pub row: &'static str,
    pub sport: Sport,
    pub kind: Kind,
    pub entry: Entry,
    /// The record it beat, in the same unit as `entry.value`.
    pub previous: f64,
}

/// The `n` most recent times a record was beaten, over every row of the boards (of one sport, or
/// of all). The first outing in a row only sets a mark: there is nothing to beat yet; a tie does not
/// beat it either.
pub fn broken(acts: &[Activity], derived: &HashMap<i64, Derived>, sport: Option<Sport>, n: usize) -> Vec<Broken> {
    let mut out = Vec::new();
    // The board of every sport together has none of its own to name: a record there is not a sport's.
    for (board, board_sport) in BOARDS.iter().filter_map(|b| Some((b, b.sport?))).filter(|(_, s)| sport.is_none_or(|x| x == *s)) {
        for (row, kind) in board.rows {
            let mut all = podium(acts, derived, board, *kind, None, usize::MAX);
            all.sort_by_key(|e| (e.date, e.id));
            let mut best: Option<f64> = None;
            for entry in all {
                let better = |prev: f64| if kind.is_time_over_distance() { entry.value < prev } else { entry.value > prev };
                match best {
                    Some(prev) if better(prev) => {
                        out.push(Broken { board: board.title, row, sport: board_sport, kind: *kind, entry, previous: prev });
                        best = Some(entry.value);
                    }
                    None => best = Some(entry.value),
                    _ => {}
                }
            }
        }
    }
    out.sort_by_key(|b| std::cmp::Reverse((b.entry.date, b.entry.id)));
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
        Kind::LongestTime | Kind::WeekTime => (fmt_clock(v), String::new()),
        Kind::MaxHr => (format!("{v:.0} bpm"), String::new()),
        Kind::WeekDistance => (fmt_km(v), String::new()),
        Kind::WeekClimb => (fmt_m(v), String::new()),
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
    fn records_beaten_newest_first() {
        use super::broken;
        let acts = vec![
            a(1, "2026-01-01 07:00:00", "running", 10_000.0, 3000.0, 0.0),
            a(2, "2026-02-01 07:00:00", "running", 8_000.0, 2400.0, 0.0),
            a(3, "2026-03-01 07:00:00", "running", 12_000.0, 3000.0, 0.0),
            a(4, "2026-04-01 07:00:00", "running", 12_000.0, 3000.0, 0.0),
            a(5, "2026-05-01 07:00:00", "cycling", 50_000.0, 7200.0, 300.0),
            a(6, "2026-06-01 07:00:00", "cycling", 60_000.0, 7200.0, 300.0),
        ];
        let eff = |s: f32| Derived { efforts: vec![Effort { metres: 5000.0, seconds: s }], ..Default::default() };
        let derived: HashMap<i64, Derived> = [(1, eff(1500.0)), (2, eff(1300.0)), (3, eff(1300.0)), (4, eff(1250.0))].into();
        let all = broken(&acts, &derived, None, 10);
        let line: Vec<_> = all.iter().map(|b| (b.row, b.entry.id, b.previous)).collect();
        // Newest first. Outing 1 only set the marks; 3 tied the 5 km (not beaten); 4 beat it, and
        // beat the longest run (12 km in 3 and 4: 4 ties 3, so only 3 beat 10 km).
        assert_eq!(line, [("Plus longue sortie", 6, 50_000.0), ("5 km", 4, 1300.0), ("Plus longue course", 3, 10_000.0), ("5 km", 2, 1500.0)]);
        // One sport only, and a cap.
        assert_eq!(broken(&acts, &derived, Some(Sport::Bike), 10).len(), 1);
        assert_eq!(broken(&acts, &derived, Some(Sport::Swim), 10), []);
        assert_eq!(broken(&acts, &derived, None, 2).len(), 2);
        assert!(broken(&acts, &derived, None, 0).is_empty());
        assert!(broken(&[], &HashMap::new(), None, 5).is_empty());
        // No tracks yet: distances still count, efforts cannot.
        assert!(broken(&acts, &HashMap::new(), Some(Sport::Run), 5).iter().all(|b| matches!(b.kind, Kind::Longest)));
    }

    #[test]
    fn podiums_by_kind_year_and_surface() {
        let acts = vec![
            a(1, "2026-03-01 07:00:00", "running", 10_000.0, 2700.0, 50.0),
            a(2, "2025-03-01 07:00:00", "running", 12_000.0, 3300.0, 80.0),
            a(3, "2026-04-01 07:00:00", "treadmill_running", 10_000.0, 2000.0, 0.0),
            a(4, "2026-05-01 07:00:00", "trail_running", 30_000.0, 14_000.0, 1800.0),
        ];
        let eff = |s: f32| Derived { efforts: vec![Effort { metres: 5000.0, seconds: s }], ..Default::default() };
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

    #[test]
    fn records_of_every_sport_together() {
        use super::broken;
        let all = BOARDS.iter().find(|b| b.sport.is_none()).unwrap();
        // Sunday 4 Oct and Monday 5 Oct are two weeks; 6 + 8 Oct make one (Mon 5 to Sun 11).
        let acts = vec![
            a(1, "2026-10-04 23:59:00", "running", 30_000.0, 9000.0, 100.0),
            a(2, "2026-10-05 00:00:00", "cycling", 20_000.0, 3000.0, 50.0),
            a(3, "2026-10-06 08:00:00", "cycling", 25_000.0, 4000.0, 60.0),
            a(4, "2026-10-08 08:00:00", "walking", 5_000.0, 3600.0, 10.0),
            a(5, "2027-01-01 08:00:00", "running", 1_000.0, 300.0, 0.0),
        ];
        let km = podium(&acts, &HashMap::new(), all, Kind::WeekDistance, None, 10);
        // Week of 5 Oct: 50 km, its longest outing (by time) is 3; week of 28 Sep: 30 km; 2026-12-28: 1 km.
        assert_eq!(km.iter().map(|e| (e.id, e.value, e.date.to_string())).collect::<Vec<_>>(), [(3, 50_000.0, "2026-10-05".into()), (1, 30_000.0, "2026-09-28".into()), (5, 1_000.0, "2026-12-28".into())]);
        assert_eq!(podium(&acts, &HashMap::new(), all, Kind::WeekTime, None, 1)[0].value, 10_600.0);
        assert_eq!(podium(&acts, &HashMap::new(), all, Kind::WeekClimb, None, 1)[0].value, 120.0);
        // The year filter keeps whole outings: 2027 has one week of one outing.
        assert_eq!(podium(&acts, &HashMap::new(), all, Kind::WeekDistance, Some(2027), 10).len(), 1);
        // Narrowed to one sport, the board counts that sport alone: the 30 km run, not the rides.
        let runs = super::Board { sport: Some(Sport::Run), ..*all };
        assert_eq!(podium(&acts, &HashMap::new(), &runs, Kind::WeekDistance, None, 1)[0].value, 30_000.0);
        assert_eq!(podium(&acts, &HashMap::new(), &runs, Kind::Longest, None, 10).len(), 2);
        let rides = super::Board { sport: Some(Sport::Bike), ..*all };
        assert_eq!(podium(&acts, &HashMap::new(), &rides, Kind::WeekDistance, None, 1)[0].value, 45_000.0);
        assert!(podium(&acts, &HashMap::new(), &super::Board { sport: Some(Sport::Swim), ..*all }, Kind::Longest, None, 10).is_empty());
        // Every sport competes in the plain rows.
        assert_eq!(podium(&acts, &HashMap::new(), all, Kind::Longest, None, 1)[0].id, 1);
        // A week with nothing but zeros leaves no entry; no outing, no entry.
        assert!(podium(&[a(9, "2026-10-05 08:00:00", "yoga", 0.0, 0.0, 0.0)], &HashMap::new(), all, Kind::WeekDistance, None, 3).is_empty());
        assert!(podium(&[], &HashMap::new(), all, Kind::WeekTime, None, 3).is_empty());
        // The highest heart rate comes from the tracks; no track, no entry.
        let hr = |v: f32| Derived { max_hr: v, ..Default::default() };
        let derived: HashMap<i64, Derived> = [(1, hr(171.0)), (2, hr(188.0)), (3, hr(0.0))].into();
        assert_eq!(podium(&acts, &derived, all, Kind::MaxHr, None, 10).iter().map(|e| e.id).collect::<Vec<_>>(), [2, 1]);
        assert!(podium(&acts, &HashMap::new(), all, Kind::MaxHr, None, 10).is_empty());
        assert_eq!(show(Kind::MaxHr, Sport::Run, 188.4).0, "188 bpm");
        assert_eq!(show(Kind::WeekTime, Sport::Run, 36_000.0).0, show(Kind::LongestTime, Sport::Run, 36_000.0).0);
        // Not a sport's record: nothing to say in the list of records beaten.
        assert!(broken(&acts, &HashMap::new(), None, 20).iter().all(|b| b.board != "En tout genre"));
    }
}
