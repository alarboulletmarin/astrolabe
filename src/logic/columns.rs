//! The journal's columns: which exist, which are shown, and which fit the width, and what the
//! computed ones say (pure). The choice is kept as a word per column, so a setting from another
//! version only loses what it no longer knows.

use crate::logic::activity::{Activity, Sport, fmt_clock, fmt_m, group};
use crate::logic::fit::BAND;
use chrono::Duration;
use std::collections::HashMap;

/// A column the user can show or hide. The date and the name (with the sport's icon) are always there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Column {
    Distance,
    Time,
    Pace,
    Gap,
    Elevation,
    Climb,
    Hr,
    Calories,
    Rank,
    Zones,
    Profile,
}

/// Every column, in the order they are shown: the figures, then the small charts.
pub const ALL: [Column; 11] = [
    Column::Distance,
    Column::Time,
    Column::Pace,
    Column::Gap,
    Column::Elevation,
    Column::Climb,
    Column::Hr,
    Column::Calories,
    Column::Rank,
    Column::Zones,
    Column::Profile,
];

/// Room kept for the date and, at the least, the name; the gap between two columns.
pub const DATE_W: f32 = 152.0;
pub const NAME_MIN: f32 = 190.0;
pub const GAP: f32 = 16.0;

impl Column {
    pub fn key(self) -> &'static str {
        match self {
            Column::Distance => "distance",
            Column::Time => "time",
            Column::Pace => "pace",
            Column::Gap => "gap",
            Column::Elevation => "elevation",
            Column::Climb => "climb",
            Column::Hr => "hr",
            Column::Calories => "calories",
            Column::Rank => "rank",
            Column::Zones => "zones",
            Column::Profile => "profile",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Column::Distance => "Distance",
            Column::Time => "Temps",
            Column::Pace => "Allure",
            Column::Gap => "Écart",
            Column::Elevation => "D+",
            Column::Climb => "D+ / km",
            Column::Hr => "FC",
            Column::Calories => "Calories",
            Column::Rank => "Rang",
            Column::Zones => "Zones FC",
            Column::Profile => "Profil",
        }
    }

    /// One line in the menu: what the column says.
    pub fn hint(self) -> &'static str {
        match self {
            Column::Distance => "la distance",
            Column::Time => "la durée",
            Column::Pace => "allure, vitesse ou temps aux 100 m selon le sport",
            Column::Gap => "l'allure face à ta médiane sur les sorties du même sport et de distance proche, l'année d'avant",
            Column::Elevation => "le dénivelé positif",
            Column::Climb => "mètres de dénivelé par kilomètre : à quel point la sortie grimpe",
            Column::Hr => "la fréquence cardiaque moyenne",
            Column::Calories => "les calories dépensées",
            Column::Rank => "la place parmi les sorties du même sport, par distance",
            Column::Zones => "le temps passé dans chacune des 5 zones de fréquence cardiaque (une fois le fichier lu)",
            Column::Profile => "l'altitude le long de la sortie (une fois le fichier lu)",
        }
    }

    /// The width it takes; the profile's is its least, as it takes what the row has left.
    pub fn width(self) -> f32 {
        match self {
            Column::Distance => 86.0,
            Column::Time => 78.0,
            Column::Pace => 96.0,
            Column::Gap => 92.0,
            Column::Elevation => 72.0,
            Column::Climb => 86.0,
            Column::Hr => 78.0,
            Column::Calories => 92.0,
            Column::Rank => 96.0,
            Column::Zones => 104.0,
            Column::Profile => 140.0,
        }
    }

    /// Lower stays longer when the window narrows.
    fn priority(self) -> u8 {
        match self {
            Column::Distance => 0,
            Column::Time => 1,
            Column::Pace => 2,
            Column::Gap => 3,
            Column::Profile => 4,
            Column::Hr => 5,
            Column::Zones => 6,
            Column::Elevation => 7,
            Column::Climb => 8,
            Column::Calories => 9,
            Column::Rank => 10,
        }
    }

    /// The small charts; the rest are words and numbers, set against the right edge.
    pub fn chart(self) -> bool {
        matches!(self, Column::Zones | Column::Profile)
    }
}

/// What a new install shows.
pub fn default() -> Vec<Column> {
    vec![Column::Distance, Column::Time, Column::Pace, Column::Gap, Column::Elevation, Column::Hr, Column::Zones, Column::Profile]
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

/// The cell of a figure column, empty when the activity has nothing to say. `rank` is the place
/// and the number of outings of the sport, asked for only by the column that needs it. The
/// computed columns (the gap, the charts) are not told here.
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
        Column::Gap | Column::Zones | Column::Profile => String::new(),
    }
}

// --- The gap to the usual pace.

/// Speed in m/s, when the outing has a distance and a time that mean something.
fn speed(a: &Activity) -> Option<f64> {
    let (d, t) = (a.distance?, a.duration?);
    (a.sport().has_distance() && d > 0.0 && t > 0.0).then(|| d / t)
}

/// The middle value; the mean of the two middle ones when even.
fn median(mut v: Vec<f64>) -> Option<f64> {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(v[n / 2]),
        _ => Some((v[n / 2 - 1] + v[n / 2]) / 2.0),
    }
}

/// How far each outing was from the usual: its pace against the median of the outings of the
/// same sport, of a distance within a quarter of its own, in the year before it (at least three
/// of them). In seconds per km (per 100 m swimming; more is slower), or in km/h on a bike (more
/// is faster). Outings with nothing to compare with are left out.
pub fn gaps(acts: &[Activity]) -> HashMap<i64, f64> {
    let mut by_sport: HashMap<Sport, Vec<(chrono::NaiveDateTime, f64, f64, i64)>> = HashMap::new();
    for a in acts {
        if let (Some(at), Some(v)) = (a.start(), speed(a)) {
            by_sport.entry(a.sport()).or_default().push((at, a.distance.unwrap_or(0.0), v, a.activity_id));
        }
    }
    let mut out = HashMap::new();
    for (sport, mut list) in by_sport {
        // The pace (or the speed on a bike) as it reads, so the median is the one a reader expects.
        let unit = |v: f64| match sport {
            Sport::Bike => v * 3.6,
            Sport::Swim => 100.0 / v,
            _ => 1000.0 / v,
        };
        list.sort_by_key(|x| x.0);
        for (i, (at, dist, v, id)) in list.iter().enumerate() {
            // ponytail: walks back over the year for each outing, O(n * outings a year); an index by distance if years of daily outings make it slow.
            let year_ago = *at - Duration::days(365);
            let like: Vec<f64> = list[..i].iter().rev().take_while(|x| x.0 >= year_ago).filter(|x| x.0 < *at && x.1 >= dist * 0.8 && x.1 <= dist * 1.25).map(|x| unit(x.2)).collect();
            if like.len() < 3 {
                continue;
            }
            out.insert(*id, unit(*v) - median(like).unwrap());
        }
    }
    out
}

/// "−12 s/km" (faster), "+1:05 /km", "+1,3 km/h", "−4 s/100m"; "≈ habitude" within a hair.
pub fn gap_label(sport: Sport, gap: f64) -> String {
    let sign = |v: f64| if v < 0.0 { "\u{2212}" } else { "+" };
    match sport {
        Sport::Bike if gap.abs() < 0.15 => "≈ habitude".into(),
        Sport::Bike => format!("{}{:.1}\u{a0}km/h", sign(gap), gap.abs()).replace('.', ","),
        _ if gap.abs() < 1.5 => "≈ habitude".into(),
        _ => {
            let per = if sport == Sport::Swim { "/100m" } else { "/km" };
            let s = gap.abs().round() as i64;
            if s >= 60 { format!("{}{}:{:02} {per}", sign(gap), s / 60, s % 60) } else { format!("{}{s} s{per}", sign(gap)) }
        }
    }
}

// --- Heart rate zones.

/// The heart rate reached for at least a minute over all the outings, counted down from the top
/// band: a high estimate of the maximum that a strap glitch does not set. None without heart rate.
pub fn observed_max<'a>(all_bands: impl Iterator<Item = &'a [f32]>) -> Option<f32> {
    let mut total: Vec<f32> = Vec::new();
    for bands in all_bands {
        if total.len() < bands.len() {
            total.resize(bands.len(), 0.0);
        }
        total.iter_mut().zip(bands).for_each(|(t, b)| *t += b);
    }
    let mut above = 0.0;
    for (i, secs) in total.iter().enumerate().rev() {
        above += secs;
        if above >= 60.0 {
            return Some((i as f32 + 1.0) * BAND);
        }
    }
    None
}

/// Seconds in each of the five zones, at 50-60-70-80-90 % of `max` (a band goes by its middle;
/// below 50 % is not a zone, above 100 % is the fifth).
pub fn zones(bands: &[f32], max: f32) -> [f32; 5] {
    let mut z = [0.0; 5];
    if max <= 0.0 {
        return z;
    }
    for (i, secs) in bands.iter().enumerate() {
        let share = (i as f32 + 0.5) * BAND / max;
        if share >= 0.5 {
            z[(((share - 0.5) * 10.0) as usize).min(4)] += secs;
        }
    }
    z
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
        // Unknown words (another version's columns), repeats, spaces, upper case.
        assert_eq!(parse("distance, time ,distance,heartbeat,Sport"), vec![Column::Distance, Column::Time]);
        assert_eq!(parse("sport,distance,bar,trace,profile"), vec![Column::Distance, Column::Profile]);
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
        assert_eq!(toggle(&[Column::Profile], Column::Distance), vec![Column::Distance, Column::Profile]);
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
        assert_eq!(visible(&[Column::Zones, Column::Rank], DATE_W + NAME_MIN + GAP + 96.0 + GAP), vec![Column::Rank]);
        assert!(visible(&[], 2000.0).is_empty());
        // Short of room, the profile shows before the zones.
        let v = visible(&default(), 1000.0);
        assert!(v.contains(&Column::Profile) && !v.contains(&Column::Zones), "{v:?}");
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
        assert_eq!(text(Column::Profile, &run, None), "");
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

    /// An outing `days` after 1 January 2026, of `km` in `min`.
    fn outing(id: i64, days: i64, key: &str, km: f64, min: f64) -> Activity {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap().and_hms_opt(8, 0, 0).unwrap() + Duration::days(days);
        Activity { activity_id: id, start_time_local: at.format("%Y-%m-%d %H:%M:%S").to_string(), ..act(key, Some(km), Some(min), None, None, None) }
    }

    #[test]
    fn the_gap_is_to_the_usual_pace_of_like_outings() {
        // Three 10 km runs at 5:00, 5:30 and 6:00 /km: the median is 5:30.
        let mut acts = vec![outing(1, 0, "running", 10.0, 50.0), outing(2, 10, "running", 10.0, 55.0), outing(3, 20, "running", 10.0, 60.0)];
        // Then 10.5 km at 5:20: 10 s/km faster than usual.
        acts.push(outing(4, 30, "running", 10.5, 56.0));
        // A 30 km run has no like outing; a 9 km one counts the four before it.
        acts.push(outing(5, 31, "running", 30.0, 180.0));
        acts.push(outing(6, 32, "running", 9.0, 45.0));
        let g = gaps(&acts);
        assert!((g[&4] - (-10.0)).abs() < 0.01, "{}", g[&4]);
        assert!(!g.contains_key(&1) && !g.contains_key(&3) && !g.contains_key(&5));
        // The four before 6 (5:00, 5:30, 6:00, 5:20 → 5:25), 9 km in 45 min is 5:00: −25 s.
        assert!((g[&6] - (-25.0)).abs() < 0.01, "{}", g[&6]);
        // More than a year back does not count; another sport neither.
        let old = vec![outing(1, 0, "running", 10.0, 50.0), outing(2, 1, "running", 10.0, 50.0), outing(3, 2, "cycling", 10.0, 50.0), outing(4, 400, "running", 10.0, 50.0), outing(5, 401, "running", 10.0, 50.0)];
        assert!(gaps(&old).is_empty());
        // A bike: km/h, faster is more.
        let bike = vec![outing(1, 0, "cycling", 26.0, 60.0), outing(2, 1, "cycling", 26.0, 60.0), outing(3, 2, "cycling", 26.0, 60.0), outing(4, 3, "cycling", 27.0, 60.0)];
        assert!((gaps(&bike)[&4] - 1.0).abs() < 1e-9);
        // Nothing to compare: nothing; no distance or time: left out, never a division by zero.
        assert!(gaps(&[]).is_empty());
        let blank = vec![outing(1, 0, "running", 0.0, 50.0), outing(2, 1, "running", 0.0, 50.0), outing(3, 2, "running", 0.0, 50.0), outing(4, 3, "running", 0.0, 50.0)];
        assert!(gaps(&blank).is_empty());
        let strength: Vec<Activity> = (0..5).map(|i| outing(i, i, "strength_training", 1.0, 30.0)).collect();
        assert!(gaps(&strength).is_empty());
    }

    #[test]
    fn gap_labels() {
        assert_eq!(gap_label(Sport::Run, -10.0), "\u{2212}10 s/km");
        assert_eq!(gap_label(Sport::Run, 8.4), "+8 s/km");
        assert_eq!(gap_label(Sport::Run, 65.0), "+1:05 /km");
        assert_eq!(gap_label(Sport::Run, -59.6), "\u{2212}1:00 /km");
        assert_eq!(gap_label(Sport::Run, 1.0), "≈ habitude");
        assert_eq!(gap_label(Sport::Run, -1.4), "≈ habitude");
        assert_eq!(gap_label(Sport::Swim, -4.0), "\u{2212}4 s/100m");
        assert_eq!(gap_label(Sport::Bike, 1.26), "+1,3\u{a0}km/h");
        assert_eq!(gap_label(Sport::Bike, -0.8), "\u{2212}0,8\u{a0}km/h");
        assert_eq!(gap_label(Sport::Bike, 0.1), "≈ habitude");
    }

    #[test]
    fn heart_rate_zones_against_the_observed_maximum() {
        // Bands of 5 bpm: 30 min at 140-145 (band 28), 2 min at 185-190 (37), 20 s at 220 (44: a glitch).
        let mut a = vec![0.0; 45];
        a[28] = 1800.0;
        a[37] = 120.0;
        a[44] = 20.0;
        let b = vec![0.0, 0.0, 30.0];
        // The glitch alone is under a minute: the maximum is the top of the band reached for one.
        assert_eq!(observed_max([a.as_slice(), b.as_slice()].into_iter()), Some(190.0));
        assert_eq!(observed_max(std::iter::empty()), None);
        assert_eq!(observed_max([[0.0f32; 3].as_slice()].into_iter()), None);
        // Against 190: 142.5 is 75 % (zone 3), 187.5 is 98.7 % (zone 5), 222.5 is past 100 % (zone 5).
        let z = zones(&a, 190.0);
        assert_eq!(z, [0.0, 0.0, 1800.0, 0.0, 140.0]);
        // Below half the maximum, not a zone; exactly 50 % is zone 1, 60 % zone 2.
        let mut low = vec![0.0; 24];
        low[10] = 50.0; // 52.5 bpm
        low[19] = 10.0; // 97.5 bpm: 48.75 % of 200
        low[20] = 7.0; // 102.5: 51.25 %
        low[23] = 3.0; // 117.5: 58.75 %
        assert_eq!(zones(&low, 200.0), [10.0, 0.0, 0.0, 0.0, 0.0]);
        let mut edge = vec![0.0; 25];
        edge[24] = 5.0; // 122.5 bpm of 200 = 61.25 %: zone 2
        assert_eq!(zones(&edge, 200.0), [0.0, 5.0, 0.0, 0.0, 0.0]);
        assert_eq!(zones(&a, 0.0), [0.0; 5]);
        assert_eq!(zones(&[], 190.0), [0.0; 5]);
    }
}
