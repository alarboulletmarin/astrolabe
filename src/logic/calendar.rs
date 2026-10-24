//! The calendar's days, weeks and months (pure). Weeks run Monday to Sunday.

use crate::logic::activity::{Activity, Sport, week_start};
use chrono::{Datelike, Days, Months, NaiveDate};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Week,
    Month,
    Year,
}

/// The seven days of the week that holds `d`, Monday first.
pub fn week_days(d: NaiveDate) -> [NaiveDate; 7] {
    let monday = week_start(d);
    std::array::from_fn(|i| monday + Days::new(i as u64))
}

/// The weeks that cover the month of `d`: the days of the neighbouring months that fill the first
/// and last weeks are in them too (four to six weeks).
pub fn month_weeks(d: NaiveDate) -> Vec<[NaiveDate; 7]> {
    let first = d.with_day(1).unwrap();
    let last = first + Months::new(1) - Days::new(1);
    let mut weeks = Vec::new();
    let mut monday = week_start(first);
    while monday <= last {
        weeks.push(week_days(monday));
        monday = monday + Days::new(7);
    }
    weeks
}

/// `anchor` moved by `n` weeks, months or years (negative: back). A day that does not exist in the
/// month reached (the 31st, a 29 February) falls on its last day.
pub fn shift(mode: Mode, anchor: NaiveDate, n: i32) -> NaiveDate {
    let by = |months: u32| if n >= 0 { anchor.checked_add_months(Months::new(months)) } else { anchor.checked_sub_months(Months::new(months)) };
    match mode {
        Mode::Week => anchor.checked_add_signed(chrono::Duration::days(7 * n as i64)),
        Mode::Month => by(n.unsigned_abs()),
        Mode::Year => by(12 * n.unsigned_abs()),
    }
    .unwrap_or(anchor)
}

/// Indices into `acts` of each day's outings, earliest first; `sport` leaves out the others.
pub fn by_day(acts: &[Activity], sport: Option<Sport>) -> HashMap<NaiveDate, Vec<usize>> {
    let mut days: HashMap<NaiveDate, Vec<usize>> = HashMap::new();
    for (i, a) in acts.iter().enumerate() {
        if let (Some(d), true) = (a.date(), sport.is_none_or(|s| a.sport() == s)) {
            days.entry(d).or_default().push(i);
        }
    }
    for list in days.values_mut() {
        list.sort_by_key(|i| (acts[*i].start(), acts[*i].activity_id));
    }
    days
}

/// 0 for a rest day, then 1 to 4 for how much of the busiest day this one is (never 0 when
/// something was done, never above 4).
pub fn level(value: f64, max: f64) -> u8 {
    if value <= 0.0 || max <= 0.0 {
        return 0;
    }
    // Computed wide: a huge ratio must not wrap before it is capped.
    (1 + (value / max * 4.0).floor() as u32).min(4) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::activity::ActivityType;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn a_month_is_four_to_six_whole_weeks() {
        // Thursday the 1st: from the Monday before.
        let w = month_weeks(d(2026, 10, 15));
        assert_eq!((w.len(), w[0][0], w[4][6]), (5, d(2026, 9, 28), d(2026, 11, 1)));
        // February starting on a Monday and 28 days long: exactly four weeks, no stray day.
        let w = month_weeks(d(2027, 2, 10));
        assert_eq!((w.len(), w[0][0], w[3][6]), (4, d(2027, 2, 1), d(2027, 2, 28)));
        // Leap February.
        let w = month_weeks(d(2024, 2, 29));
        assert_eq!((w.len(), w[0][0], w[4][6]), (5, d(2024, 1, 29), d(2024, 3, 3)));
        // Six weeks: a 31-day month starting on a Saturday.
        let w = month_weeks(d(2026, 8, 1));
        assert_eq!((w.len(), w[0][0], w[5][6]), (6, d(2026, 7, 27), d(2026, 9, 6)));
        // Across a year, either way; the first and the last day of the month are always in.
        assert_eq!(month_weeks(d(2026, 1, 31))[0][0], d(2025, 12, 29));
        assert_eq!(month_weeks(d(2026, 12, 1)).last().unwrap()[6], d(2027, 1, 3));
        for (y, m) in [(2023, 1), (2023, 2), (2024, 2), (2025, 6), (2026, 12), (2100, 2)] {
            let first = d(y, m, 1);
            let w = month_weeks(first);
            let days: Vec<_> = w.iter().flatten().copied().collect();
            assert!(days.contains(&first) && days.contains(&(first + Months::new(1) - Days::new(1))));
            assert!((4..=6).contains(&w.len()));
            // Consecutive days, whole Monday to Sunday weeks.
            assert!(days.windows(2).all(|p| p[1] - p[0] == chrono::Duration::days(1)));
            assert!(w.iter().all(|wk| wk[0].weekday() == chrono::Weekday::Mon));
        }
    }

    #[test]
    fn a_week_from_any_day() {
        for day in 19..=25 {
            let w = week_days(d(2026, 10, day));
            assert_eq!((w[0], w[6]), (d(2026, 10, 19), d(2026, 10, 25)));
        }
        assert_eq!(week_days(d(2026, 1, 1))[0], d(2025, 12, 29));
    }

    #[test]
    fn moving_never_lands_on_a_day_that_does_not_exist() {
        assert_eq!(shift(Mode::Month, d(2026, 1, 31), 1), d(2026, 2, 28));
        assert_eq!(shift(Mode::Month, d(2026, 3, 31), -1), d(2026, 2, 28));
        assert_eq!(shift(Mode::Month, d(2024, 1, 31), 1), d(2024, 2, 29));
        assert_eq!(shift(Mode::Month, d(2026, 12, 15), 1), d(2027, 1, 15));
        assert_eq!(shift(Mode::Month, d(2026, 1, 15), -1), d(2025, 12, 15));
        assert_eq!(shift(Mode::Month, d(2026, 5, 10), 0), d(2026, 5, 10));
        assert_eq!(shift(Mode::Year, d(2024, 2, 29), 1), d(2025, 2, 28));
        assert_eq!(shift(Mode::Year, d(2024, 2, 29), -4), d(2020, 2, 29));
        assert_eq!(shift(Mode::Week, d(2026, 12, 28), 1), d(2027, 1, 4));
        assert_eq!(shift(Mode::Week, d(2026, 1, 1), -1), d(2025, 12, 25));
        // Back and forth by a month from the 31st does not come home (it cannot), but never panics.
        assert_eq!(shift(Mode::Month, shift(Mode::Month, d(2026, 1, 31), 1), -1), d(2026, 1, 28));
        // Far past the calendar's end: stays where it is.
        assert_eq!(shift(Mode::Year, NaiveDate::MAX, 1), NaiveDate::MAX);
    }

    fn act(id: i64, at: &str, key: &str) -> Activity {
        Activity {
            activity_id: id,
            activity_name: None,
            start_time_local: at.into(),
            activity_type: ActivityType { type_key: key.into() },
            distance: None,
            duration: Some(1800.0),
            elevation_gain: None,
            average_hr: None,
            calories: None,
        }
    }

    #[test]
    fn outings_are_filed_by_day_earliest_first() {
        let acts = [
            act(1, "2026-10-20 18:00:00", "running"),
            act(2, "2026-10-20 07:00:00", "cycling"),
            act(3, "2026-10-21 07:00:00", "running"),
            act(4, "not a date", "running"),
            act(5, "2026-10-20 07:00:00", "running"),
        ];
        let all = by_day(&acts, None);
        assert_eq!(all[&d(2026, 10, 20)], [1, 4, 0]);
        assert_eq!(all[&d(2026, 10, 21)], [2]);
        assert_eq!(all.len(), 2, "the broken date is left out");
        let runs = by_day(&acts, Some(Sport::Run));
        assert_eq!(runs[&d(2026, 10, 20)], [4, 0]);
        assert!(by_day(&acts, Some(Sport::Swim)).is_empty());
        assert!(by_day(&[], None).is_empty());
    }

    #[test]
    fn heat_levels_cover_zero_to_four() {
        assert_eq!(level(0.0, 100.0), 0);
        assert_eq!(level(-5.0, 100.0), 0);
        assert_eq!(level(10.0, 0.0), 0);
        // Something was done: never invisible.
        assert_eq!(level(0.001, 100.0), 1);
        assert_eq!(level(24.9, 100.0), 1);
        assert_eq!(level(25.0, 100.0), 2);
        assert_eq!(level(50.0, 100.0), 3);
        assert_eq!(level(75.0, 100.0), 4);
        assert_eq!(level(100.0, 100.0), 4);
        assert_eq!(level(1e9, 100.0), 4);
    }
}
