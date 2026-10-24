use chrono::{Datelike, Days, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// One entry of `/activitylist-service/activities/search/activities`.
/// Garmin omits fields that don't apply (no distance for strength, no HR without a strap).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub activity_id: i64,
    #[serde(default)]
    pub activity_name: Option<String>,
    pub start_time_local: String,
    pub activity_type: ActivityType,
    #[serde(default)]
    pub distance: Option<f64>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub elevation_gain: Option<f64>,
    #[serde(default, rename = "averageHR")]
    pub average_hr: Option<f64>,
    #[serde(default)]
    pub calories: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityType {
    pub type_key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sport {
    Run,
    Trail,
    Bike,
    Swim,
    Walk,
    Hike,
    Multisport,
    Strength,
    Other,
}

impl Sport {
    pub const ALL: [Sport; 9] = [
        Sport::Run,
        Sport::Trail,
        Sport::Bike,
        Sport::Swim,
        Sport::Walk,
        Sport::Hike,
        Sport::Multisport,
        Sport::Strength,
        Sport::Other,
    ];

    /// Garmin has ~100 type keys (trail_running, gravel_cycling, open_water_swimming…).
    pub fn from_type_key(key: &str) -> Sport {
        let has = |s: &str| key.contains(s);
        if has("multi") || has("triathlon") || has("duathlon") {
            Sport::Multisport
        } else if has("trail") || has("ultra") {
            Sport::Trail
        } else if has("run") {
            Sport::Run
        } else if has("cycl") || has("bik") || has("ride") {
            Sport::Bike
        } else if has("swim") {
            Sport::Swim
        } else if has("hik") || has("mountaineering") {
            Sport::Hike
        } else if has("walk") {
            Sport::Walk
        } else if has("strength") || has("fitness") || has("cardio") {
            Sport::Strength
        } else {
            Sport::Other
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sport::Run => "Course",
            Sport::Trail => "Trail",
            Sport::Bike => "Vélo",
            Sport::Swim => "Natation",
            Sport::Walk => "Marche",
            Sport::Hike => "Randonnée",
            Sport::Multisport => "Triathlon",
            Sport::Strength => "Renfo",
            Sport::Other => "Autre",
        }
    }
}

impl Activity {
    pub fn sport(&self) -> Sport {
        Sport::from_type_key(&self.activity_type.type_key)
    }

    pub fn start(&self) -> Option<NaiveDateTime> {
        NaiveDateTime::parse_from_str(&self.start_time_local, "%Y-%m-%d %H:%M:%S").ok()
    }

    pub fn date(&self) -> Option<NaiveDate> {
        self.start().map(|d| d.date())
    }

    /// Pace for foot sports, speed for bike, pace per 100 m for swimming.
    pub fn speed_label(&self) -> Option<String> {
        let (d, t) = (self.distance?, self.duration?);
        if d <= 0.0 || t <= 0.0 {
            return None;
        }
        Some(match self.sport() {
            Sport::Bike => format!("{:.1}\u{a0}km/h", d / t * 3.6).replace('.', ","),
            Sport::Swim => format!("{} /100m", fmt_clock(t / d * 100.0)),
            _ => format!("{} /km", fmt_clock(t / d * 1000.0)),
        })
    }
}

/// 3725 s -> "1:02:05", 305 s -> "5:05".
pub fn fmt_clock(seconds: f64) -> String {
    let s = seconds.round() as i64;
    let (h, m, s) = (s / 3600, s % 3600 / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// 1941 -> "1 941" (no-break space, French grouping).
pub fn group(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('\u{a0}');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// 3725 s -> "1h02", 1500 s -> "25min", 1 000 000 s -> "278h": totals stay short.
pub fn fmt_duration(seconds: f64) -> String {
    let m = (seconds / 60.0).round() as i64;
    if m >= 100 * 60 {
        format!("{}h", group(m / 60))
    } else if m >= 60 {
        format!("{}h{:02}", m / 60, m % 60)
    } else {
        format!("{m}min")
    }
}

/// "8,0 km", then whole kilometres from 100 km: "1 941 km".
pub fn fmt_km(meters: f64) -> String {
    let km = meters / 1000.0;
    if km >= 99.95 {
        format!("{}\u{a0}km", group(km.round() as i64))
    } else {
        format!("{:.1}\u{a0}km", km).replace('.', ",")
    }
}

pub fn fmt_m(meters: f64) -> String {
    format!("{}\u{a0}m", group(meters.round() as i64))
}

/// What a chart measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    Time,
    Distance,
}

impl Metric {
    pub fn label(self) -> &'static str {
        match self {
            Metric::Time => "Temps",
            Metric::Distance => "Distance",
        }
    }

    pub fn of(self, a: &Activity) -> f64 {
        match self {
            Metric::Time => a.duration,
            Metric::Distance => a.distance,
        }
        .unwrap_or(0.0)
        .max(0.0)
    }

    pub fn fmt(self, v: f64) -> String {
        match self {
            Metric::Time => fmt_duration(v),
            Metric::Distance => fmt_km(v),
        }
    }
}

pub fn matches(a: &Activity, sport: Option<Sport>) -> bool {
    sport.is_none_or(|s| a.sport() == s)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Totals {
    pub count: usize,
    pub distance: f64,
    pub duration: f64,
    pub elevation: f64,
}

impl Totals {
    fn add(&mut self, a: &Activity) {
        self.count += 1;
        self.distance += a.distance.unwrap_or(0.0);
        self.duration += a.duration.unwrap_or(0.0);
        self.elevation += a.elevation_gain.unwrap_or(0.0);
    }
}

pub fn week_start(d: NaiveDate) -> NaiveDate {
    d - Days::new(d.weekday().num_days_from_monday() as u64)
}

/// Totals over `[from, to]` inclusive, optionally for a single sport.
pub fn totals(acts: &[Activity], from: NaiveDate, to: NaiveDate, sport: Option<Sport>) -> Totals {
    let mut t = Totals::default();
    for a in acts {
        let Some(d) = a.date() else { continue };
        if d >= from && d <= to && sport.is_none_or(|s| a.sport() == s) {
            t.add(a);
        }
    }
    t
}

#[derive(Debug, PartialEq)]
pub struct Streaks {
    /// Consecutive days with an activity, ending today (or yesterday: today isn't over).
    pub days: u32,
    /// Consecutive Mon–Sun weeks with an activity, Strava style; the current week counts once active.
    pub weeks: u32,
    pub longest_days: u32,
}

pub fn streaks(acts: &[Activity], today: NaiveDate) -> Streaks {
    let days: HashSet<NaiveDate> = acts.iter().filter_map(Activity::date).collect();
    let weeks: HashSet<NaiveDate> = days.iter().map(|d| week_start(*d)).collect();

    let run_back = |set: &HashSet<NaiveDate>, mut cur: NaiveDate, step: u64| {
        let mut n = 0;
        while set.contains(&cur) {
            n += 1;
            cur = cur - Days::new(step);
        }
        n
    };

    let yesterday = today - Days::new(1);
    let day_anchor = if days.contains(&today) { today } else { yesterday };
    let this_week = week_start(today);
    let week_anchor = if weeks.contains(&this_week) {
        this_week
    } else {
        this_week - Days::new(7)
    };

    let mut sorted: Vec<_> = days.iter().copied().collect();
    sorted.sort();
    let (mut longest, mut cur) = (0, 0);
    for (i, d) in sorted.iter().enumerate() {
        cur = if i > 0 && *d - sorted[i - 1] == chrono::Duration::days(1) {
            cur + 1
        } else {
            1
        };
        longest = longest.max(cur);
    }

    Streaks {
        days: run_back(&days, day_anchor, 1),
        weeks: run_back(&weeks, week_anchor, 7),
        longest_days: longest,
    }
}

/// 1-based rank of `a` among the activities of its sport, by distance (by time when it has none), and their count.
pub fn rank(acts: &[Activity], a: &Activity) -> (usize, usize) {
    let metric = if a.distance.is_some_and(|d| d > 0.0) { Metric::Distance } else { Metric::Time };
    let v = metric.of(a);
    let same: Vec<_> = acts.iter().filter(|x| x.sport() == a.sport()).collect();
    (same.iter().filter(|x| metric.of(x) > v).count() + 1, same.len())
}

// --- The stats builder: any measure, grouped by day, week, month or year, over any range,
// for any set of sports.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {
    Time,
    Distance,
    Elevation,
    Count,
    /// Total time over total distance: what a group was run at, not a mean of paces.
    Pace,
    /// Average heart rate, weighted by duration.
    Hr,
}

impl Measure {
    pub const ALL: [Measure; 6] = [Measure::Time, Measure::Distance, Measure::Elevation, Measure::Count, Measure::Pace, Measure::Hr];

    pub fn label(self) -> &'static str {
        match self {
            Measure::Time => "Temps",
            Measure::Distance => "Distance",
            Measure::Elevation => "Dénivelé",
            Measure::Count => "Séances",
            Measure::Pace => "Allure",
            Measure::Hr => "FC moyenne",
        }
    }

    /// Sums add up over time (a running total means something); averages don't.
    pub fn sums(self) -> bool {
        matches!(self, Measure::Time | Measure::Distance | Measure::Elevation | Measure::Count)
    }

    /// `bike`: speed in km/h reads better than a pace.
    pub fn fmt(self, v: f64, bike: bool) -> String {
        match self {
            Measure::Time => fmt_duration(v),
            Measure::Distance => fmt_km(v),
            Measure::Elevation => fmt_m(v),
            Measure::Count => group(v.round() as i64),
            Measure::Pace if bike => format!("{:.1}\u{a0}km/h", 3600.0 / v).replace('.', ","),
            Measure::Pace => format!("{} /km", fmt_clock(v)),
            Measure::Hr => format!("{v:.0}\u{a0}bpm"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Day,
    Week,
    Month,
    Year,
}

impl Group {
    pub const ALL: [Group; 4] = [Group::Day, Group::Week, Group::Month, Group::Year];

    pub fn label(self) -> &'static str {
        match self {
            Group::Day => "Jour",
            Group::Week => "Semaine",
            Group::Month => "Mois",
            Group::Year => "Année",
        }
    }

    /// First day of the group holding `d`.
    pub fn start(self, d: NaiveDate) -> NaiveDate {
        match self {
            Group::Day => d,
            Group::Week => week_start(d),
            Group::Month => d.with_day(1).unwrap(),
            Group::Year => d.with_ordinal(1).unwrap(),
        }
    }

    pub fn next(self, d: NaiveDate) -> NaiveDate {
        match self {
            Group::Day => d + Days::new(1),
            Group::Week => d + Days::new(7),
            Group::Month => d.checked_add_months(chrono::Months::new(1)).unwrap(),
            Group::Year => d.checked_add_months(chrono::Months::new(12)).unwrap(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub measure: Measure,
    pub group: Group,
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// Empty: every sport.
    pub sports: Vec<Sport>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// First and last day of the group, clipped to the range.
    pub from: NaiveDate,
    pub to: NaiveDate,
    /// `None`: nothing to average (no activity, or none with a distance for a pace).
    pub value: Option<f64>,
    pub count: usize,
}

#[derive(Default)]
struct Acc {
    count: usize,
    time: f64,
    dist: f64,
    elev: f64,
    paced_time: f64,
    hr_time: f64,
    hr_sum: f64,
}

impl Acc {
    fn add(&mut self, a: &Activity) {
        let t = a.duration.unwrap_or(0.0).max(0.0);
        let d = a.distance.unwrap_or(0.0).max(0.0);
        self.count += 1;
        self.time += t;
        self.dist += d;
        self.elev += a.elevation_gain.unwrap_or(0.0).max(0.0);
        if d > 0.0 {
            self.paced_time += t;
        }
        if let Some(hr) = a.average_hr.filter(|h| *h > 0.0) {
            self.hr_sum += hr * t.max(1.0);
            self.hr_time += t.max(1.0);
        }
    }

    fn value(&self, m: Measure) -> Option<f64> {
        match m {
            Measure::Time => Some(self.time),
            Measure::Distance => Some(self.dist),
            Measure::Elevation => Some(self.elev),
            Measure::Count => Some(self.count as f64),
            Measure::Pace => (self.dist > 0.0).then(|| self.paced_time / self.dist * 1000.0),
            Measure::Hr => (self.hr_time > 0.0).then(|| self.hr_sum / self.hr_time),
        }
    }
}

impl Query {
    pub fn keeps(&self, a: &Activity) -> bool {
        (self.sports.is_empty() || self.sports.contains(&a.sport())) && a.date().is_some_and(|d| d >= self.from && d <= self.to)
    }

    pub fn days(&self) -> i64 {
        (self.to - self.from).num_days() + 1
    }

    /// The same query moved back by its own length (`years`: by whole years instead).
    pub fn shifted(&self, years: bool) -> Query {
        let (from, to) = if years {
            (self.from.checked_sub_months(chrono::Months::new(12)).unwrap(), self.to.checked_sub_months(chrono::Months::new(12)).unwrap())
        } else {
            (self.from - Days::new(self.days() as u64), self.from - Days::new(1))
        };
        Query { from, to, ..self.clone() }
    }

    pub fn total(&self, acts: &[Activity]) -> Point {
        let mut acc = Acc::default();
        acts.iter().filter(|a| self.keeps(a)).for_each(|a| acc.add(a));
        Point { from: self.from, to: self.to, value: acc.value(self.measure).filter(|_| acc.count > 0 || self.measure.sums()), count: acc.count }
    }

    /// One point per group from the range's start to its end.
    pub fn series(&self, acts: &[Activity]) -> Vec<Point> {
        if self.to < self.from {
            return Vec::new();
        }
        let mut starts = Vec::new();
        let mut d = self.group.start(self.from);
        while d <= self.to {
            starts.push(d);
            d = self.group.next(d);
        }
        let mut accs: Vec<Acc> = starts.iter().map(|_| Acc::default()).collect();
        for a in acts.iter().filter(|a| self.keeps(a)) {
            let day = a.date().unwrap();
            let i = starts.partition_point(|s| *s <= day) - 1;
            accs[i].add(a);
        }
        starts
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let end = starts.get(i + 1).map_or(self.to, |n| *n - Days::new(1));
                let acc = &accs[i];
                let value = if self.measure.sums() { acc.value(self.measure) } else { acc.value(self.measure).filter(|_| acc.count > 0) };
                Point { from: (*s).max(self.from), to: end.min(self.to), value, count: acc.count }
            })
            .collect()
    }

    /// Running value day by day over the range: a total for sums, the average so far otherwise.
    pub fn running(&self, acts: &[Activity]) -> Vec<Option<f64>> {
        let n = self.days().max(0) as usize;
        let mut per_day: Vec<Vec<&Activity>> = vec![Vec::new(); n];
        for a in acts.iter().filter(|a| self.keeps(a)) {
            per_day[(a.date().unwrap() - self.from).num_days() as usize].push(a);
        }
        let mut acc = Acc::default();
        per_day
            .into_iter()
            .map(|day| {
                day.into_iter().for_each(|a| acc.add(a));
                acc.value(self.measure).filter(|_| acc.count > 0 || self.measure.sums())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(id: i64, start: &str, key: &str, dist: f64, dur: f64) -> Activity {
        Activity {
            activity_id: id,
            activity_name: None,
            start_time_local: start.into(),
            activity_type: ActivityType { type_key: key.into() },
            distance: Some(dist),
            duration: Some(dur),
            elevation_gain: None,
            average_hr: None,
            calories: None,
        }
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn parses_garmin_json_with_missing_fields() {
        let json = r#"[{"activityId":1,"startTimeLocal":"2026-10-20 07:31:12",
            "activityType":{"typeKey":"strength_training","typeId":13},"duration":1800.5,
            "averageHR":120.0,"unknownField":true}]"#;
        let a: Vec<Activity> = serde_json::from_str(json).unwrap();
        assert_eq!(a[0].sport(), Sport::Strength);
        assert_eq!(a[0].distance, None);
        assert_eq!(a[0].average_hr, Some(120.0));
        assert_eq!(a[0].date(), Some(d("2026-10-20")));
    }

    #[test]
    fn classifies_sports() {
        for (k, s) in [
            ("trail_running", Sport::Trail),
            ("ultra_run", Sport::Trail),
            ("running", Sport::Run),
            ("track_running", Sport::Run),
            ("treadmill_running", Sport::Run),
            ("gravel_cycling", Sport::Bike),
            ("mountain_biking", Sport::Bike),
            ("virtual_ride", Sport::Bike),
            ("open_water_swimming", Sport::Swim),
            ("lap_swimming", Sport::Swim),
            ("hiking", Sport::Hike),
            ("walking", Sport::Walk),
            ("multi_sport", Sport::Multisport),
            ("triathlon", Sport::Multisport),
            ("yoga", Sport::Other),
            ("", Sport::Other),
        ] {
            assert_eq!(Sport::from_type_key(k), s, "{k}");
        }
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_clock(0.0), "0:00");
        assert_eq!(fmt_clock(59.6), "1:00");
        assert_eq!(fmt_clock(305.0), "5:05");
        assert_eq!(fmt_clock(3725.0), "1:02:05");
        assert_eq!(fmt_clock(360_000.0), "100:00:00");
        assert_eq!(fmt_duration(0.0), "0min");
        assert_eq!(fmt_duration(3599.0), "1h00");
        assert_eq!(fmt_duration(3725.0), "1h02");
        assert_eq!(fmt_duration(5999.0 * 60.0), "99h59");
        assert_eq!(fmt_duration(1_004_400.0), "279h");
        assert_eq!(fmt_duration(36_000_000.0), "10\u{a0}000h");
        assert_eq!(fmt_km(0.0), "0,0\u{a0}km");
        assert_eq!(fmt_km(99_949.0), "99,9\u{a0}km");
        assert_eq!(fmt_km(99_950.0), "100\u{a0}km");
        assert_eq!(fmt_km(1_941_400.0), "1\u{a0}941\u{a0}km");
        assert_eq!(fmt_m(58_384.4), "58\u{a0}384\u{a0}m");
        assert_eq!(fmt_m(0.0), "0\u{a0}m");
        for (n, s) in [(0, "0"), (999, "999"), (1000, "1\u{a0}000"), (100_000, "100\u{a0}000"), (-1234, "-1\u{a0}234")] {
            assert_eq!(group(n), s);
        }
        let run = act(1, "2026-10-20 07:00:00", "running", 10_000.0, 3000.0);
        assert_eq!(run.speed_label().as_deref(), Some("5:00 /km"));
        let bike = act(1, "2026-10-20 07:00:00", "road_biking", 30_000.0, 3600.0);
        assert_eq!(bike.speed_label().as_deref(), Some("30,0\u{a0}km/h"));
        let swim = act(1, "2026-10-20 07:00:00", "lap_swimming", 1000.0, 1200.0);
        assert_eq!(swim.speed_label().as_deref(), Some("2:00 /100m"));
        let zero = act(1, "2026-10-20 07:00:00", "running", 0.0, 600.0);
        assert_eq!(zero.speed_label(), None);
    }

    #[test]
    fn totals_filter_range_and_sport() {
        let acts = vec![
            act(1, "2026-10-19 07:00:00", "running", 10_000.0, 3000.0), // Mon
            act(2, "2026-10-25 07:00:00", "road_biking", 40_000.0, 5000.0), // Sun
            act(3, "2026-10-26 07:00:00", "running", 5_000.0, 1500.0), // next Mon
            act(4, "garbage", "running", 5_000.0, 1500.0),
        ];
        let w = week_start(d("2026-10-22"));
        assert_eq!(w, d("2026-10-19"));
        let all = totals(&acts, w, w + Days::new(6), None);
        assert_eq!((all.count, all.distance), (2, 50_000.0));
        let run = totals(&acts, w, w + Days::new(6), Some(Sport::Run));
        assert_eq!((run.count, run.duration), (1, 3000.0));
        assert_eq!(totals(&[], w, w, None), Totals::default());
    }

    #[test]
    fn streaks_edge_cases() {
        let today = d("2026-10-24"); // Saturday
        assert_eq!(streaks(&[], today), Streaks { days: 0, weeks: 0, longest_days: 0 });

        // Yesterday + day before, nothing today: streak still alive.
        let acts = vec![
            act(1, "2026-10-22 07:00:00", "running", 0.0, 0.0),
            act(2, "2026-10-23 07:00:00", "running", 0.0, 0.0),
            act(3, "2026-10-23 18:00:00", "walking", 0.0, 0.0), // same day twice
        ];
        let s = streaks(&acts, today);
        assert_eq!((s.days, s.longest_days, s.weeks), (2, 2, 1));

        // Two days ago only: day streak broken; last week only: week streak still alive.
        let acts = vec![act(1, "2026-10-14 07:00:00", "running", 0.0, 0.0)];
        let s = streaks(&acts, today);
        assert_eq!((s.days, s.weeks), (0, 1));

        // Three consecutive weeks including this one, then a gap.
        let acts = vec![
            act(1, "2026-09-21 07:00:00", "running", 0.0, 0.0), // week of 21/09, then a gap
            act(2, "2026-10-11 07:00:00", "running", 0.0, 0.0),
            act(3, "2026-10-13 07:00:00", "running", 0.0, 0.0),
            act(4, "2026-10-24 07:00:00", "running", 0.0, 0.0),
        ];
        let s = streaks(&acts, today);
        assert_eq!((s.days, s.weeks, s.longest_days), (1, 3, 1));

        // Longest streak crossing a month boundary.
        let acts: Vec<_> = (0..10)
            .map(|i| {
                let day = d("2026-01-27") + Days::new(i);
                act(i as i64, &format!("{day} 07:00:00"), "running", 0.0, 0.0)
            })
            .collect();
        assert_eq!(streaks(&acts, today).longest_days, 10);
    }

    #[test]
    fn rank_among_the_sport() {
        let mut hilly = act(3, "2026-10-03 07:00:00", "hiking", 14_000.0, 20_000.0);
        hilly.elevation_gain = Some(1200.0);
        let acts = vec![
            act(1, "2026-10-01 07:00:00", "running", 21_100.0, 7200.0), // 5:41 /km
            act(2, "2026-10-02 07:00:00", "running", 5_000.0, 1200.0),  // 4:00 /km
            act(4, "2026-10-04 07:00:00", "running", 3_000.0, 600.0),   // faster, but under 5 km
            hilly,
        ];
        assert_eq!(rank(&acts, &acts[0]), (1, 3));
        assert_eq!(rank(&acts, &acts[2]), (3, 3));
        assert_eq!(rank(&acts, &acts[3]), (1, 1));
    }

    #[test]
    fn query_series_groups_and_measures() {
        let mut a1 = act(1, "2026-01-30 07:00:00", "running", 10_000.0, 3000.0);
        a1.average_hr = Some(150.0);
        let mut a2 = act(2, "2026-02-02 07:00:00", "trail_running", 20_000.0, 9000.0);
        a2.average_hr = Some(140.0);
        let a3 = act(3, "2026-02-03 07:00:00", "strength_training", 0.0, 1800.0);
        let acts = vec![a1, a2, a3];
        let q = |measure, group, from: &str, to: &str, sports: Vec<Sport>| Query { measure, group, from: d(from), to: d(to), sports };

        // Months, clipped to the range.
        let m = q(Measure::Distance, Group::Month, "2026-01-15", "2026-02-10", vec![]).series(&acts);
        assert_eq!(m.len(), 2);
        assert_eq!((m[0].from, m[0].to, m[0].value, m[0].count), (d("2026-01-15"), d("2026-01-31"), Some(10_000.0), 1));
        assert_eq!((m[1].from, m[1].to, m[1].value, m[1].count), (d("2026-02-01"), d("2026-02-10"), Some(20_000.0), 2));

        // Pace is total time over total distance, ignoring activities without distance.
        let p = q(Measure::Pace, Group::Year, "2026-01-01", "2026-12-31", vec![]).series(&acts);
        assert_eq!(p.len(), 1);
        assert!((p[0].value.unwrap() - 12_000.0 / 30_000.0 * 1000.0).abs() < 1e-9);
        // Heart rate weighted by duration: (150·3000 + 140·9000) / 12000.
        let h = q(Measure::Hr, Group::Year, "2026-01-01", "2026-12-31", vec![]).total(&acts);
        assert!((h.value.unwrap() - 142.5).abs() < 1e-9);

        // Several sports at once; an empty week has a zero sum but no average.
        let w = q(Measure::Count, Group::Week, "2026-01-26", "2026-02-15", vec![Sport::Run, Sport::Trail]).series(&acts);
        assert_eq!(w.iter().map(|p| p.value).collect::<Vec<_>>(), [Some(1.0), Some(1.0), Some(0.0)]);
        let wp = q(Measure::Pace, Group::Week, "2026-01-26", "2026-02-15", vec![]).series(&acts);
        assert_eq!(wp[2].value, None);

        // Days, an empty or reversed range, years.
        assert_eq!(q(Measure::Time, Group::Day, "2026-02-01", "2026-02-03", vec![]).series(&acts).len(), 3);
        assert!(q(Measure::Time, Group::Day, "2026-02-03", "2026-02-01", vec![]).series(&acts).is_empty());
        assert_eq!(q(Measure::Time, Group::Year, "2024-06-01", "2026-02-01", vec![]).series(&acts).len(), 3);
        assert_eq!(q(Measure::Time, Group::Week, "2026-03-01", "2026-03-31", vec![]).total(&acts).count, 0);
    }

    #[test]
    fn query_shift_and_running() {
        let acts = vec![
            act(1, "2026-01-01 07:00:00", "running", 1000.0, 300.0),
            act(2, "2026-01-03 07:00:00", "running", 2000.0, 600.0),
        ];
        let q = Query { measure: Measure::Distance, group: Group::Day, from: d("2026-01-01"), to: d("2026-01-04"), sports: vec![] };
        assert_eq!(q.running(&acts), [Some(1000.0), Some(1000.0), Some(3000.0), Some(3000.0)]);
        let pace = Query { measure: Measure::Pace, ..q.clone() };
        assert_eq!(pace.running(&acts)[0], Some(300.0));
        assert_eq!(pace.running(&acts)[3], Some(300.0));
        let before = q.shifted(false);
        assert_eq!((before.from, before.to), (d("2025-12-28"), d("2025-12-31")));
        let year = q.shifted(true);
        assert_eq!((year.from, year.to), (d("2025-01-01"), d("2025-01-04")));
        // 29 February shifted by a year lands on the 28th.
        let leap = Query { from: d("2024-02-29"), to: d("2024-02-29"), ..q.clone() }.shifted(true);
        assert_eq!(leap.from, d("2023-02-28"));
        assert_eq!(Measure::Pace.fmt(300.0, false), "5:00 /km");
        assert_eq!(Measure::Pace.fmt(120.0, true), "30,0\u{a0}km/h");
        assert_eq!(Measure::Count.fmt(1234.0, false), "1\u{a0}234");
    }
}
