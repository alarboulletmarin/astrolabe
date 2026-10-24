//! The dashboard's figures (pure). Several sports mix here, and their distances do not add up (two
//! kilometres of swimming are not two of cycling): across sports the common measure is time,
//! and distances are told sport by sport.

use crate::logic::activity::{Activity, Metric, Sport, Totals, totals, week_start};
use chrono::{Datelike, Days, NaiveDate};

/// The period the key figures cover.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Period {
    /// The last 30 days, today included.
    Rolling,
    /// The calendar month so far.
    Month,
}

/// An inclusive range of days.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

/// The window ending today, and the one before it to compare with: the 30 days before, or the
/// same number of days into the previous month (never past its end).
pub fn spans(window: Period, today: NaiveDate) -> (Span, Span) {
    match window {
        Period::Rolling => (
            Span { from: today - Days::new(29), to: today },
            Span { from: today - Days::new(59), to: today - Days::new(30) },
        ),
        Period::Month => {
            let first = today.with_day(1).unwrap();
            let last_before = first - Days::new(1);
            let prev_first = last_before.with_day(1).unwrap();
            let prev_to = (prev_first + Days::new(today.day() as u64 - 1)).min(last_before);
            (Span { from: first, to: today }, Span { from: prev_first, to: prev_to })
        }
    }
}

/// Whether a distance is the measure of this selection: one sport that has one. Across sports, time.
pub fn metric_for(sport: Option<Sport>) -> Metric {
    match sport {
        Some(s) if s.has_distance() => Metric::Distance,
        _ => Metric::Time,
    }
}

pub struct Summary {
    pub totals: Totals,
    /// Average heart rate, each outing weighted by its duration; none when no outing has one.
    pub hr: Option<f64>,
}

pub fn summarize(acts: &[Activity], span: Span, sport: Option<Sport>) -> Summary {
    let (mut beats, mut secs) = (0.0, 0.0);
    for a in acts.iter().filter(|a| a.date().is_some_and(|d| d >= span.from && d <= span.to) && sport.is_none_or(|s| a.sport() == s)) {
        if let (Some(hr), Some(t)) = (a.average_hr.filter(|h| *h > 0.0), a.duration.filter(|t| *t > 0.0)) {
            beats += hr * t;
            secs += t;
        }
    }
    Summary { totals: totals(acts, span.from, span.to, sport), hr: (secs > 0.0).then(|| beats / secs) }
}

/// Totals per sport over `span`, the most time first; sports without an outing are left out.
pub fn by_sport(acts: &[Activity], span: Span) -> Vec<(Sport, Totals)> {
    let mut out: Vec<(Sport, Totals)> = Sport::ALL.iter().map(|s| (*s, totals(acts, span.from, span.to, Some(*s)))).filter(|(_, t)| t.count > 0).collect();
    out.sort_by(|a, b| b.1.duration.total_cmp(&a.1.duration).then(b.1.count.cmp(&a.1.count)));
    out
}

/// How a figure moved from the previous period: said, not judged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    /// Nothing in either period.
    Nothing,
    /// Something now, nothing before: no percentage to give.
    New,
    Same,
    Up(u32),
    Down(u32),
}

pub fn change(now: f64, before: f64) -> Change {
    if before <= 0.0 {
        return if now > 0.0 { Change::New } else { Change::Nothing };
    }
    let pct = (now - before) / before * 100.0;
    match pct.abs().round() as u32 {
        0 => Change::Same,
        n if pct > 0.0 => Change::Up(n),
        n => Change::Down(n),
    }
}

impl Change {
    pub fn label(self) -> String {
        match self {
            Change::Nothing => "—".into(),
            Change::New => "nouveau".into(),
            Change::Same => "= stable".into(),
            Change::Up(n) => format!("▲ {} %", crate::logic::activity::group(n as i64)),
            Change::Down(n) => format!("▼ {} %", crate::logic::activity::group(n as i64)),
        }
    }
}

/// The measure per Monday-to-Sunday week, oldest first, the last being the week under way.
pub fn weekly(acts: &[Activity], today: NaiveDate, weeks: usize, sport: Option<Sport>, metric: Metric) -> Vec<f64> {
    let this = week_start(today);
    (0..weeks)
        .rev()
        .map(|back| {
            let from = this - Days::new(7 * back as u64);
            let t = totals(acts, from, from + Days::new(6), sport);
            match metric {
                Metric::Distance => t.distance,
                Metric::Time => t.duration,
            }
        })
        .collect()
}

/// The middle value; the mean of the two middle ones when even; none for nothing.
pub fn median(values: &[f64]) -> Option<f64> {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    match v.len() {
        0 => None,
        n if n % 2 == 1 => Some(v[n / 2]),
        n => Some((v[n / 2 - 1] + v[n / 2]) / 2.0),
    }
}

/// The newest outing of the selection.
pub fn latest(acts: &[Activity], sport: Option<Sport>) -> Option<&Activity> {
    acts.iter().filter(|a| sport.is_none_or(|s| a.sport() == s)).filter_map(|a| Some((a.start()?, a))).max_by_key(|(at, a)| (*at, a.activity_id)).map(|(_, a)| a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::activity::ActivityType;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn act(id: i64, date: NaiveDate, key: &str, km: f64, min: f64, hr: Option<f64>) -> Activity {
        Activity {
            activity_id: id,
            activity_name: None,
            start_time_local: format!("{date} 08:00:00"),
            activity_type: ActivityType { type_key: key.into() },
            distance: (km > 0.0).then_some(km * 1000.0),
            duration: Some(min * 60.0),
            elevation_gain: Some(10.0),
            average_hr: hr,
            calories: None,
        }
    }

    #[test]
    fn windows_and_what_they_compare_with() {
        // 30 days, today included; the 30 before.
        let (now, before) = spans(Period::Rolling, d(2026, 10, 24));
        assert_eq!((now.from, now.to), (d(2026, 9, 25), d(2026, 10, 24)));
        assert_eq!((before.from, before.to), (d(2026, 8, 26), d(2026, 9, 24)));
        // Across a year, and a leap year's 29 February.
        assert_eq!(spans(Period::Rolling, d(2026, 1, 10)).0.from, d(2025, 12, 12));
        assert_eq!(spans(Period::Rolling, d(2024, 3, 1)).0.from, d(2024, 2, 1));
        assert_eq!(spans(Period::Rolling, d(2023, 3, 1)).0.from, d(2023, 1, 31));
        // The month so far, against as many days of the month before.
        let (now, before) = spans(Period::Month, d(2026, 10, 24));
        assert_eq!((now.from, now.to, before.from, before.to), (d(2026, 10, 1), d(2026, 10, 24), d(2026, 9, 1), d(2026, 9, 24)));
        // First day: one day against one day. January looks at December of the year before.
        let (now, before) = spans(Period::Month, d(2026, 1, 1));
        assert_eq!((now.from, now.to, before.from, before.to), (d(2026, 1, 1), d(2026, 1, 1), d(2025, 12, 1), d(2025, 12, 1)));
        // The 31st against a shorter month: it stops at that month's end, never spills into this one.
        let (_, before) = spans(Period::Month, d(2026, 3, 31));
        assert_eq!((before.from, before.to), (d(2026, 2, 1), d(2026, 2, 28)));
        let (_, before) = spans(Period::Month, d(2024, 3, 31));
        assert_eq!(before.to, d(2024, 2, 29));
        let (_, before) = spans(Period::Month, d(2026, 5, 31));
        assert_eq!((before.from, before.to), (d(2026, 4, 1), d(2026, 4, 30)));
    }

    #[test]
    fn the_measure_follows_the_selection() {
        assert_eq!(metric_for(None), Metric::Time);
        assert_eq!(metric_for(Some(Sport::Run)), Metric::Distance);
        assert_eq!(metric_for(Some(Sport::Swim)), Metric::Distance);
        assert_eq!(metric_for(Some(Sport::Strength)), Metric::Time);
        assert_eq!(metric_for(Some(Sport::Other)), Metric::Time);
    }

    #[test]
    fn sports_stay_apart_and_hr_is_weighted_by_time() {
        let acts = [
            act(1, d(2026, 10, 20), "running", 10.0, 60.0, Some(150.0)),
            act(2, d(2026, 10, 21), "cycling", 40.0, 120.0, Some(120.0)),
            act(3, d(2026, 10, 22), "lap_swimming", 2.0, 40.0, None),
            act(4, d(2026, 10, 23), "strength_training", 0.0, 30.0, Some(100.0)),
            act(5, d(2026, 9, 1), "running", 99.0, 600.0, Some(200.0)),
        ];
        let span = Span { from: d(2026, 10, 1), to: d(2026, 10, 24) };
        let all = summarize(&acts, span, None);
        assert_eq!((all.totals.count, all.totals.duration), (4, (60. + 120. + 40. + 30.) * 60.));
        // (150·60 + 120·120 + 100·30) / (60 + 120 + 30) minutes: the swim has no heart rate.
        assert!((all.hr.unwrap() - (150. * 60. + 120. * 120. + 100. * 30.) / 210.).abs() < 1e-9);
        let run = summarize(&acts, span, Some(Sport::Run));
        assert_eq!((run.totals.count, run.totals.distance, run.hr), (1, 10_000.0, Some(150.0)));
        assert_eq!(summarize(&acts, span, Some(Sport::Swim)).hr, None);
        assert_eq!(summarize(&acts, span, Some(Sport::Hike)).totals.count, 0);
        // Per sport, the most time first, none for a sport that did not move.
        let per: Vec<_> = by_sport(&acts, span).iter().map(|(s, t)| (*s, t.count)).collect();
        assert_eq!(per, [(Sport::Bike, 1), (Sport::Run, 1), (Sport::Swim, 1), (Sport::Strength, 1)]);
        assert!(by_sport(&acts, Span { from: d(2020, 1, 1), to: d(2020, 1, 2) }).is_empty());
        assert_eq!(summarize(&[], span, None).hr, None);
    }

    #[test]
    fn says_how_a_figure_moved() {
        assert_eq!(change(0.0, 0.0), Change::Nothing);
        assert_eq!(change(5.0, 0.0), Change::New);
        assert_eq!(change(0.0, 5.0), Change::Down(100));
        assert_eq!(change(112.0, 100.0), Change::Up(12));
        assert_eq!(change(92.0, 100.0), Change::Down(8));
        assert_eq!(change(100.0, 100.0), Change::Same);
        // Under half a percent is stable, not "0 %".
        assert_eq!(change(100.4, 100.0), Change::Same);
        assert_eq!(change(100.6, 100.0), Change::Up(1));
        assert_eq!(change(3_000.0, 100.0), Change::Up(2_900));
        assert_eq!(Change::Up(12).label(), "▲ 12 %");
        assert_eq!(Change::Down(8).label(), "▼ 8 %");
        assert_eq!(Change::Up(2_900).label(), "▲ 2\u{a0}900 %");
        assert_eq!(Change::New.label(), "nouveau");
        assert_eq!(Change::Nothing.label(), "—");
    }

    #[test]
    fn weeks_line_up_on_mondays_whatever_the_day() {
        let acts = [
            act(1, d(2026, 10, 19), "running", 10.0, 60.0, None), // Monday of the week under way
            act(2, d(2026, 10, 25), "running", 5.0, 30.0, None),  // its Sunday
            act(3, d(2026, 10, 18), "running", 1.0, 10.0, None),  // the week before: a Sunday
            act(4, d(2026, 8, 3), "running", 7.0, 40.0, None),    // exactly 11 weeks back
            act(5, d(2026, 8, 2), "running", 9.0, 50.0, None),    // just before the 12 weeks
        ];
        for today in [d(2026, 10, 19), d(2026, 10, 22), d(2026, 10, 25)] {
            let w = weekly(&acts, today, 12, None, Metric::Distance);
            assert_eq!(w.len(), 12);
            let km: Vec<f64> = w.iter().map(|m| m / 1000.0).collect();
            // The week under way is the whole Monday to Sunday, whatever day today is.
            assert_eq!((km[11], km[10], km[0]), (15.0, 1.0, 7.0), "today {today}");
        }
        // One sport only, time as the measure, and no weeks at all.
        assert_eq!(weekly(&acts, d(2026, 10, 22), 1, Some(Sport::Run), Metric::Time), [5400.0]);
        assert_eq!(weekly(&acts, d(2026, 10, 22), 1, Some(Sport::Bike), Metric::Time), [0.0]);
        assert!(weekly(&acts, d(2026, 10, 22), 0, None, Metric::Time).is_empty());
    }

    #[test]
    fn median_of_nothing_one_two_many() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[4.0]), Some(4.0));
        assert_eq!(median(&[4.0, 8.0]), Some(6.0));
        assert_eq!(median(&[9.0, 1.0, 5.0]), Some(5.0));
        assert_eq!(median(&[0.0, 0.0, 0.0, 10.0]), Some(0.0));
        assert_eq!(median(&[3.0, 1.0, 2.0, 4.0]), Some(2.5));
    }

    #[test]
    fn the_latest_outing_is_the_newest_of_the_selection() {
        let acts = [
            act(1, d(2026, 10, 20), "running", 10.0, 60.0, None),
            act(2, d(2026, 10, 22), "cycling", 40.0, 120.0, None),
            act(3, d(2026, 10, 22), "cycling", 5.0, 20.0, None),
        ];
        assert_eq!(latest(&acts, None).map(|a| a.activity_id), Some(3));
        assert_eq!(latest(&acts, Some(Sport::Run)).map(|a| a.activity_id), Some(1));
        assert_eq!(latest(&acts, Some(Sport::Swim)).map(|a| a.activity_id), None);
        assert!(latest(&[], None).is_none());
    }
}
