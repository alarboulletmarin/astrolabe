//! The journal's columns: which exist, which are shown, and which fit the width (pure). The
//! choice is kept as a word per column, so a setting from another version only loses what it
//! no longer knows.

use crate::logic::activity::{Activity, Sport, fmt_clock, fmt_m, group};
use std::collections::HashMap;

/// A column the user can show or hide. The date and the name are always there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Column {
    Sport,
    Distance,
    Time,
    Pace,
    Elevation,
    Hr,
    Calories,
    Climb,
    Rank,
    Bar,
    Trace,
}

/// Every column, in the order they are shown.
pub const ALL: [Column; 11] = [
    Column::Sport,
    Column::Distance,
    Column::Time,
    Column::Pace,
    Column::Elevation,
    Column::Hr,
    Column::Calories,
    Column::Climb,
    Column::Rank,
    Column::Bar,
    Column::Trace,
];

/// Room kept for the date and, at the least, the name; the gap between two columns.
pub const DATE_W: f32 = 152.0;
pub const NAME_MIN: f32 = 190.0;
pub const GAP: f32 = 16.0;

impl Column {
    pub fn key(self) -> &'static str {
        match self {
            Column::Sport => "sport",
            Column::Distance => "distance",
            Column::Time => "time",
            Column::Pace => "pace",
            Column::Elevation => "elevation",
            Column::Hr => "hr",
            Column::Calories => "calories",
            Column::Climb => "climb",
            Column::Rank => "rank",
            Column::Bar => "bar",
            Column::Trace => "trace",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Column::Sport => "Sport",
            Column::Distance => "Distance",
            Column::Time => "Temps",
            Column::Pace => "Allure",
            Column::Elevation => "D+",
            Column::Hr => "FC",
            Column::Calories => "Calories",
            Column::Climb => "D+ / km",
            Column::Rank => "Rang",
            Column::Bar => "Ampleur",
            Column::Trace => "Tracé",
        }
    }

    /// One line in the menu: what the column says.
    pub fn hint(self) -> &'static str {
        match self {
            Column::Sport => "le sport",
            Column::Distance => "la distance",
            Column::Time => "la durée",
            Column::Pace => "allure, vitesse ou temps aux 100 m selon le sport",
            Column::Elevation => "le dénivelé positif",
            Column::Hr => "la fréquence cardiaque moyenne",
            Column::Calories => "les calories dépensées",
            Column::Climb => "mètres de dénivelé par kilomètre : à quel point la sortie grimpe",
            Column::Rank => "la place parmi les sorties du même sport, par distance",
            Column::Bar => "une barre : la sortie face à la plus longue du même sport",
            Column::Trace => "la forme du parcours (une fois le fichier de l'activité lu)",
        }
    }

    pub fn width(self) -> f32 {
        match self {
            Column::Sport => 110.0,
            Column::Distance => 86.0,
            Column::Time => 78.0,
            Column::Pace => 96.0,
            Column::Elevation => 72.0,
            Column::Hr => 78.0,
            Column::Calories => 92.0,
            Column::Climb => 86.0,
            Column::Rank => 96.0,
            Column::Bar => 90.0,
            Column::Trace => 64.0,
        }
    }

    /// Lower stays longer when the window narrows.
    fn priority(self) -> u8 {
        match self {
            Column::Distance => 0,
            Column::Time => 1,
            Column::Pace => 2,
            Column::Sport => 3,
            Column::Hr => 4,
            Column::Elevation => 5,
            Column::Bar => 6,
            Column::Trace => 7,
            Column::Climb => 8,
            Column::Calories => 9,
            Column::Rank => 10,
        }
    }

    /// Numbers sit against the right edge of their column; the rest to the left.
    pub fn right(self) -> bool {
        !matches!(self, Column::Sport | Column::Bar | Column::Trace)
    }
}

/// What a new install shows.
pub fn default() -> Vec<Column> {
    vec![Column::Sport, Column::Distance, Column::Time, Column::Pace, Column::Elevation, Column::Hr, Column::Bar, Column::Trace]
}

/// The setting as columns. Nothing saved (empty) gives the defaults; "-" is a deliberate empty
/// choice; unknown words and repeats are dropped, and the order is always the canonical one.
pub fn parse(setting: &str) -> Vec<Column> {
    let setting = setting.trim();
    if setting.is_empty() {
        return default();
    }
    ALL.iter().copied().filter(|c| setting.split(',').any(|w| w.trim() == c.key())).collect()
}

pub fn encode(columns: &[Column]) -> String {
    if columns.is_empty() { "-".into() } else { columns.iter().map(|c| c.key()).collect::<Vec<_>>().join(",") }
}

/// `columns` with `column` shown if it was hidden, hidden if it was shown.
pub fn toggle(columns: &[Column], column: Column) -> Vec<Column> {
    ALL.iter().copied().filter(|c| (columns.contains(c)) != (*c == column)).collect()
}

/// The chosen columns that fit `width`, the least important dropping first.
pub fn visible(columns: &[Column], width: f32) -> Vec<Column> {
    let mut by_priority: Vec<Column> = columns.to_vec();
    by_priority.sort_by_key(|c| c.priority());
    let mut room = width - DATE_W - NAME_MIN - GAP;
    let mut fit = Vec::new();
    for c in by_priority {
        if room >= c.width() + GAP {
            room -= c.width() + GAP;
            fit.push(c);
        }
    }
    ALL.iter().copied().filter(|c| fit.contains(c)).collect()
}

/// "1er", "2e", "12e".
fn ordinal(n: usize) -> String {
    if n == 1 { "1er".into() } else { format!("{n}e") }
}

/// The cell of a text column, empty when the activity has nothing to say. `rank` is the place and the
/// number of outings of the sport, asked for only by the column that needs it.
pub fn text(column: Column, a: &Activity, rank: Option<(usize, usize)>) -> String {
    match column {
        Column::Distance => a.distance.filter(|d| *d > 0.0).map(crate::logic::activity::fmt_km).unwrap_or_default(),
        Column::Time => a.duration.filter(|d| *d > 0.0).map(fmt_clock).unwrap_or_default(),
        Column::Pace => a.speed_label().unwrap_or_default(),
        Column::Elevation => a.elevation_gain.filter(|e| *e >= 1.0).map(fmt_m).unwrap_or_default(),
        Column::Hr => a.average_hr.filter(|h| *h > 0.0).map(|h| format!("{h:.0} bpm")).unwrap_or_default(),
        Column::Calories => a.calories.filter(|c| *c >= 1.0).map(|c| format!("{} kcal", group(c.round() as i64))).unwrap_or_default(),
        Column::Climb => match (a.elevation_gain, a.distance) {
            (Some(e), Some(d)) if e >= 1.0 && d >= 1000.0 => format!("{:.0} m/km", e / (d / 1000.0)),
            _ => String::new(),
        },
        Column::Rank => rank.map(|(r, n)| format!("{} sur {}", ordinal(r), group(n as i64))).unwrap_or_default(),
        Column::Sport | Column::Bar | Column::Trace => String::new(),
    }
}

/// What the bar compares: distance where a sport has one, time otherwise.
fn size(a: &Activity) -> f64 {
    if a.sport().has_distance() { a.distance } else { a.duration }.unwrap_or(0.0).max(0.0)
}

/// The biggest outing of each sport.
pub fn maxima(acts: &[Activity]) -> HashMap<Sport, f64> {
    let mut m: HashMap<Sport, f64> = HashMap::new();
    for a in acts {
        let e = m.entry(a.sport()).or_insert(0.0);
        *e = e.max(size(a));
    }
    m
}

/// Share (0 to 1) of the sport's biggest outing.
pub fn share(a: &Activity, maxima: &HashMap<Sport, f64>) -> f32 {
    let max = maxima.get(&a.sport()).copied().unwrap_or(0.0);
    if max <= 0.0 { 0.0 } else { (size(a) / max).clamp(0.0, 1.0) as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::activity::ActivityType;

    fn act(key: &str, km: Option<f64>, min: Option<f64>, climb: Option<f64>, hr: Option<f64>, cal: Option<f64>) -> Activity {
        Activity {
            activity_id: 1,
            activity_name: None,
            start_time_local: "2026-10-20 08:00:00".into(),
            activity_type: ActivityType { type_key: key.into() },
            distance: km.map(|k| k * 1000.0),
            duration: min.map(|m| m * 60.0),
            elevation_gain: climb,
            average_hr: hr,
            calories: cal,
        }
    }

    #[test]
    fn the_choice_survives_a_round_trip_and_bad_settings() {
        for cols in [default(), vec![], ALL.to_vec(), vec![Column::Rank], vec![Column::Hr, Column::Distance]] {
            // The order is always the canonical one, whatever was asked.
            let canon: Vec<Column> = ALL.iter().copied().filter(|c| cols.contains(c)).collect();
            assert_eq!(parse(&encode(&cols)), canon);
        }
        // Nothing saved, a first launch: the defaults. A deliberate empty choice is kept.
        assert_eq!(parse(""), default());
        assert_eq!(parse("   "), default());
        assert_eq!(parse("-"), vec![]);
        // Unknown words (a later version's column), repeats, spaces, upper case.
        assert_eq!(parse("distance, time ,distance,heartbeat,Sport"), vec![Column::Distance, Column::Time]);
        assert_eq!(parse("heartbeat"), vec![]);
        assert_eq!(parse("pace\ndistance"), vec![]);
        assert_eq!(encode(&[]), "-");
        assert_eq!(ALL.len(), ALL.iter().map(|c| c.key()).collect::<std::collections::HashSet<_>>().len());
    }

    #[test]
    fn toggling_adds_or_removes_one_column() {
        let d = default();
        let without = toggle(&d, Column::Pace);
        assert_eq!(without.len(), d.len() - 1);
        assert!(!without.contains(&Column::Pace));
        assert_eq!(toggle(&without, Column::Pace), d);
        assert_eq!(toggle(&[], Column::Rank), vec![Column::Rank]);
        assert_eq!(toggle(&[Column::Rank], Column::Rank), vec![]);
        // Added out of order, shown in order.
        assert_eq!(toggle(&[Column::Trace], Column::Sport), vec![Column::Sport, Column::Trace]);
    }

    #[test]
    fn columns_fall_away_by_importance_as_the_width_shrinks() {
        let all = ALL.to_vec();
        // Wide enough for everything.
        let total: f32 = ALL.iter().map(|c| c.width() + GAP).sum::<f32>() + DATE_W + NAME_MIN + GAP;
        assert_eq!(visible(&all, total), ALL.to_vec());
        assert_eq!(visible(&all, 4000.0), ALL.to_vec());
        // One pixel short: the least important one goes, nothing else.
        let short = visible(&all, total - 1.0);
        assert_eq!(short.len(), ALL.len() - 1);
        assert!(!short.contains(&Column::Rank));
        // Narrow: what stays is the most important, in display order, and never exceeds the room.
        for w in [0.0, 200.0, 360.0, 500.0, 700.0, 900.0, 1100.0, 1400.0] {
            let v = visible(&default(), w);
            let used: f32 = v.iter().map(|c| c.width() + GAP).sum();
            assert!(used <= (w - DATE_W - NAME_MIN - GAP).max(0.0), "width {w}");
            let canon: Vec<Column> = ALL.iter().copied().filter(|c| v.contains(c)).collect();
            assert_eq!(v, canon);
        }
        assert!(visible(&default(), 300.0).is_empty());
        assert_eq!(visible(&[Column::Rank], 300.0), vec![]);
        // A wide column that does not fit lets a narrower, less important one in.
        assert!(visible(&[Column::Sport, Column::Trace], DATE_W + NAME_MIN + GAP + 64.0 + GAP).contains(&Column::Trace));
        assert!(visible(&[], 2000.0).is_empty());
    }

    #[test]
    fn cells_say_nothing_rather_than_something_wrong() {
        let run = act("running", Some(10.0), Some(60.0), Some(250.0), Some(152.4), Some(1945.0));
        assert_eq!(text(Column::Distance, &run, None), "10,0\u{a0}km");
        assert_eq!(text(Column::Time, &run, None), "1:00:00");
        assert_eq!(text(Column::Pace, &run, None), "6:00 /km");
        assert_eq!(text(Column::Elevation, &run, None), "250\u{a0}m");
        assert_eq!(text(Column::Hr, &run, None), "152 bpm");
        assert_eq!(text(Column::Calories, &run, None), "1\u{a0}945 kcal");
        assert_eq!(text(Column::Climb, &run, None), "25 m/km");
        assert_eq!(text(Column::Rank, &run, Some((1, 186))), "1er sur 186");
        assert_eq!(text(Column::Rank, &run, Some((2, 12))), "2e sur 12");
        assert_eq!(text(Column::Rank, &run, Some((11, 1941))), "11e sur 1\u{a0}941");
        assert_eq!(text(Column::Rank, &run, None), "");
        assert_eq!(text(Column::Sport, &run, None), "");
        // Missing, zero or too small to mean anything: empty, not "0".
        let none = act("running", None, None, None, None, None);
        let zero = act("running", Some(0.0), Some(0.0), Some(0.0), Some(0.0), Some(0.0));
        for a in [&none, &zero] {
            for c in [Column::Distance, Column::Time, Column::Pace, Column::Elevation, Column::Hr, Column::Calories, Column::Climb] {
                assert_eq!(text(c, a, None), "", "{c:?}");
            }
        }
        // Steepness needs a kilometre to speak of: 5 m of climb over 200 m is not "25 m/km".
        assert_eq!(text(Column::Climb, &act("running", Some(0.2), Some(2.0), Some(5.0), None, None), None), "");
        assert_eq!(text(Column::Climb, &act("running", Some(1.0), Some(6.0), Some(5.0), None, None), None), "5 m/km");
        // Swimming: pace per 100 m; strength: no distance, so none of its columns.
        assert_eq!(text(Column::Pace, &act("lap_swimming", Some(1.0), Some(20.0), None, None, None), None), "2:00 /100m");
        assert_eq!(text(Column::Pace, &act("strength_training", None, Some(40.0), None, None, None), None), "");
    }

    #[test]
    fn the_bar_compares_with_the_biggest_of_the_sport() {
        let acts = [
            act("running", Some(10.0), Some(60.0), None, None, None),
            act("running", Some(40.0), Some(240.0), None, None, None),
            act("cycling", Some(100.0), Some(200.0), None, None, None),
            act("strength_training", None, Some(30.0), None, None, None),
            act("strength_training", None, Some(60.0), None, None, None),
            act("yoga", None, None, None, None, None),
        ];
        let m = maxima(&acts);
        assert_eq!(share(&acts[0], &m), 0.25);
        assert_eq!(share(&acts[1], &m), 1.0);
        // Another sport has its own scale: 100 km of cycling does not shrink a run.
        assert_eq!(share(&acts[2], &m), 1.0);
        // Without a distance, time is the measure; with nothing at all, an empty bar and no division by zero.
        assert_eq!(share(&acts[3], &m), 0.5);
        assert_eq!(share(&acts[5], &m), 0.0);
        assert_eq!(share(&acts[0], &HashMap::new()), 0.0);
        assert!(maxima(&[]).is_empty());
    }
}
