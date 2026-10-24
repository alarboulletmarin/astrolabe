//! The dashboard's figures (pure). Several sports mix here, and their distances do not add up (two
//! kilometres of swimming are not two of cycling): across sports the common measure is time,
//! and distances are told sport by sport. A figure is set against the athlete's own habit (the
//! median week) or the same date a year before, never against a period that is not over.

use crate::logic::activity::{Activity, Metric, Sport, Totals, totals, week_start};
use chrono::{Datelike, Days, NaiveDate};

/// Whether a distance is the measure of this selection: one sport that has one. Across sports, time.
pub fn metric_for(sport: Option<Sport>) -> Metric {
    match sport {
        Some(s) if s.has_distance() => Metric::Distance,
        _ => Metric::Time,
    }
}

pub fn measure(t: &Totals, metric: Metric) -> f64 {
    match metric {
        Metric::Distance => t.distance,
        Metric::Time => t.duration,
    }
}

/// The sports with an outing in `[from, to]`, the most of `metric` first (then the most outings).
pub fn sports_by_weight(acts: &[Activity], from: NaiveDate, to: NaiveDate, metric: Metric) -> Vec<Sport> {
    let mut out: Vec<(Sport, Totals)> = Sport::ALL.iter().map(|s| (*s, totals(acts, from, to, Some(*s)))).filter(|(_, t)| t.count > 0).collect();
    out.sort_by(|a, b| measure(&b.1, metric).total_cmp(&measure(&a.1, metric)).then(b.1.count.cmp(&a.1.count)));
    out.into_iter().map(|(s, _)| s).collect()
}

/// The Monday of each of the last `weeks` weeks, oldest first, the last being the week under way.
pub fn mondays(today: NaiveDate, weeks: usize) -> Vec<NaiveDate> {
    let this = week_start(today);
    (0..weeks).rev().map(|back| this - Days::new(7 * back as u64)).collect()
}

/// Per Monday-to-Sunday week (oldest first, the last being the week under way), the measure of
/// each of `sports`, in their order.
pub fn weekly(acts: &[Activity], today: NaiveDate, weeks: usize, sports: &[Sport], metric: Metric) -> Vec<Vec<f64>> {
    mondays(today, weeks).into_iter().map(|from| sports.iter().map(|s| measure(&totals(acts, from, from + Days::new(6), Some(*s)), metric)).collect()).collect()
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

/// The measure from 1 January to today, and over the same days a year before (a 29 February
/// is set against the 28th).
pub fn year_to_date(acts: &[Activity], today: NaiveDate, sport: Option<Sport>, metric: Metric) -> (f64, f64) {
    let first = |y: i32| NaiveDate::from_ymd_opt(y, 1, 1).unwrap();
    let year = today.year();
    let a_year_ago = today.with_year(year - 1).unwrap_or_else(|| NaiveDate::from_ymd_opt(year - 1, 2, 28).unwrap());
    (measure(&totals(acts, first(year), today, sport), metric), measure(&totals(acts, first(year - 1), a_year_ago, sport), metric))
}

/// Indices of the `n` newest outings of the selection, newest first.
pub fn recent(acts: &[Activity], sport: Option<Sport>, n: usize) -> Vec<usize> {
    let mut out: Vec<usize> = (0..acts.len()).filter(|i| acts[*i].start().is_some() && sport.is_none_or(|s| acts[*i].sport() == s)).collect();
    out.sort_by_key(|i| std::cmp::Reverse((acts[*i].start(), acts[*i].activity_id)));
    out.truncate(n);
    out
}

/// A round step for two to five grid lines up to `top`: the largest of 1, 2 or 5 times a power
/// of ten that fits twice. None when there is nothing to scale.
pub fn step(top: f64) -> Option<f64> {
    if !(top > 0.0) || !top.is_finite() {
        return None;
    }
    let raw = top / 2.0;
    let mag = 10f64.powf(raw.log10().floor());
    [5.0, 2.0, 1.0].into_iter().map(|m| m * mag).find(|s| *s <= raw * 1.0001)
}

/// The grid lines under `top` (in the measure's unit: kilometres or hours), from one step up.
pub fn ticks(top: f64) -> Vec<f64> {
    let Some(s) = step(top) else { return vec![] };
    (1..).map(|i| i as f64 * s).take_while(|v| *v <= top * 1.0001).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::activity::ActivityType;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn act(id: i64, date: NaiveDate, key: &str, km: f64, min: f64) -> Activity {
        Activity {
            activity_id: id,
            activity_name: None,
            start_time_local: format!("{date} 08:00:00"),
            activity_type: ActivityType { type_key: key.into() },
            distance: (km > 0.0).then_some(km * 1000.0),
            duration: Some(min * 60.0),
            elevation_gain: Some(10.0),
            average_hr: None,
            calories: None,
        }
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
    fn weeks_line_up_on_mondays_sport_by_sport() {
        let acts = [
            act(1, d(2026, 10, 19), "running", 10.0, 60.0), // Monday of the week under way
            act(2, d(2026, 10, 25), "running", 5.0, 30.0),  // its Sunday
            act(3, d(2026, 10, 25), "cycling", 40.0, 90.0),
            act(4, d(2026, 10, 18), "running", 1.0, 10.0), // the week before: a Sunday
            act(5, d(2026, 8, 3), "running", 7.0, 40.0),   // exactly 11 weeks back
            act(6, d(2026, 8, 2), "running", 9.0, 50.0),   // just before the 12 weeks
        ];
        for today in [d(2026, 10, 19), d(2026, 10, 22), d(2026, 10, 25)] {
            let m = mondays(today, 12);
            assert_eq!((m.len(), m[0], m[11]), (12, d(2026, 8, 3), d(2026, 10, 19)), "today {today}");
            let w = weekly(&acts, today, 12, &[Sport::Run, Sport::Bike], Metric::Distance);
            assert_eq!(w.len(), 12);
            // The week under way is the whole Monday to Sunday, whatever day today is.
            assert_eq!((w[11].clone(), w[10].clone(), w[0].clone()), (vec![15_000.0, 40_000.0], vec![1_000.0, 0.0], vec![7_000.0, 0.0]));
        }
        assert_eq!(weekly(&acts, d(2026, 10, 22), 1, &[Sport::Bike], Metric::Time), [vec![5400.0]]);
        assert_eq!(weekly(&acts, d(2026, 10, 22), 1, &[], Metric::Time), [Vec::<f64>::new()]);
        assert!(weekly(&acts, d(2026, 10, 22), 0, &[Sport::Run], Metric::Time).is_empty());
        // The heaviest sport first; a sport absent from the range is left out.
        // 90 min each: the sport with more outings first.
        assert_eq!(sports_by_weight(&acts, d(2026, 10, 19), d(2026, 10, 25), Metric::Time), [Sport::Run, Sport::Bike]);
        assert_eq!(sports_by_weight(&acts, d(2026, 10, 19), d(2026, 10, 25), Metric::Distance), [Sport::Bike, Sport::Run]);
        assert_eq!(sports_by_weight(&acts, d(2026, 8, 1), d(2026, 8, 31), Metric::Time), [Sport::Run]);
        assert!(sports_by_weight(&acts, d(2020, 1, 1), d(2020, 1, 2), Metric::Time).is_empty());
    }

    #[test]
    fn a_year_against_the_same_days_of_the_one_before() {
        let acts = [
            act(1, d(2026, 1, 1), "running", 10.0, 60.0),
            act(2, d(2026, 10, 24), "running", 5.0, 30.0),
            act(3, d(2026, 10, 25), "running", 99.0, 30.0), // after today
            act(4, d(2025, 10, 24), "running", 7.0, 40.0),  // a year ago today: counts
            act(5, d(2025, 10, 25), "running", 9.0, 50.0),  // a year ago tomorrow: does not
            act(6, d(2024, 12, 31), "running", 3.0, 50.0),
            act(7, d(2026, 3, 1), "cycling", 30.0, 60.0),
        ];
        assert_eq!(year_to_date(&acts, d(2026, 10, 24), Some(Sport::Run), Metric::Distance), (15_000.0, 7_000.0));
        assert_eq!(year_to_date(&acts, d(2026, 10, 24), None, Metric::Time), ((60.0 + 30.0 + 60.0) * 60.0, 40.0 * 60.0));
        // The first of January: one day against one day.
        assert_eq!(year_to_date(&acts, d(2026, 1, 1), Some(Sport::Run), Metric::Distance), (10_000.0, 0.0));
        // A 29 February looks at the 28th; nothing at all gives nothing.
        let leap = [act(1, d(2023, 2, 28), "running", 4.0, 20.0), act(2, d(2024, 2, 29), "running", 6.0, 30.0)];
        assert_eq!(year_to_date(&leap, d(2024, 2, 29), None, Metric::Distance), (6_000.0, 4_000.0));
        assert_eq!(year_to_date(&[], d(2024, 2, 29), None, Metric::Distance), (0.0, 0.0));
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
    fn the_newest_outings_of_the_selection() {
        let mut acts = vec![
            act(1, d(2026, 10, 20), "running", 10.0, 60.0),
            act(2, d(2026, 10, 22), "cycling", 40.0, 120.0),
            act(3, d(2026, 10, 22), "cycling", 5.0, 20.0),
        ];
        acts.push(Activity { start_time_local: "bad".into(), ..act(4, d(2030, 1, 1), "running", 1.0, 1.0) });
        assert_eq!(recent(&acts, None, 5), [2, 1, 0]);
        assert_eq!(recent(&acts, None, 2), [2, 1]);
        assert_eq!(recent(&acts, Some(Sport::Run), 5), [0]);
        assert!(recent(&acts, Some(Sport::Swim), 5).is_empty());
        assert!(recent(&acts, None, 0).is_empty());
        assert!(recent(&[], None, 5).is_empty());
    }

    #[test]
    fn round_grid_lines() {
        assert_eq!(step(0.0), None);
        assert_eq!(step(-3.0), None);
        assert_eq!(step(f64::NAN), None);
        assert_eq!(step(f64::INFINITY), None);
        assert_eq!(ticks(0.0), Vec::<f64>::new());
        // 30 km: every 10; 31: still 10; 9 h: every 2 h; 19: every 5; 20: every 10.
        assert_eq!(ticks(30.0), [10.0, 20.0, 30.0]);
        assert_eq!(ticks(31.0), [10.0, 20.0, 30.0]);
        assert_eq!(ticks(9.0), [2.0, 4.0, 6.0, 8.0]);
        assert_eq!(ticks(19.0), [5.0, 10.0, 15.0]);
        assert_eq!(ticks(20.0), [10.0, 20.0]);
        assert_eq!(ticks(6.0), [2.0, 4.0, 6.0]);
        assert_eq!(ticks(1.0), [0.5, 1.0]);
        assert_eq!(ticks(100.0), [50.0, 100.0]);
        assert_eq!(ticks(101.0), [50.0, 100.0]);
        // Very small and very big.
        assert_eq!(step(0.3), Some(0.1));
        assert_eq!(ticks(0.3).len(), 3);
        assert_eq!(step(12_000.0), Some(5_000.0));
    }
}
