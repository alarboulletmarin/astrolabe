//! Form: what the outings say of load, fatigue and freshness, from heart rate (Banister's
//! impulse-response model, read the way a "performance management chart" does). A statement of
//! figures, never advice. Pure.
//!
//! - The load of an outing is worked out in points by zones (Edwards): minutes spent in each zone
//!   of the maximum heart rate, times the zone's weight.
//! - Fatigue (ATL) and form (CTL) are exponential moving averages of the daily load, over 7 and 42
//!   days; freshness (TSB) is form minus fatigue, both taken at the end of the same day.

use super::activity::Activity;
use super::fit::{BAND, Derived};
use chrono::{Datelike, Days, NaiveDate};
use std::collections::HashMap;

/// Time constants of the two averages, in days.
pub const FATIGUE_DAYS: f64 = 7.0;
pub const FORM_DAYS: f64 = 42.0;
/// Fatigue within this share of the form is said to be about equal to it.
pub const NEAR: f64 = 0.05;
/// Days over which the change of form is told.
pub const TREND_DAYS: usize = 28;

// ponytail: Edwards' scale is fixed (no personal zones), and the maximum heart rate is estimated
// from the outings, with no setting to enter a measured one nor a resting heart rate.

/// Weight of a zone, from the share of the maximum heart rate: under 50 %: 0, then 1 to 5 by
/// steps of 10 % (50-60: 1, 60-70: 2, 70-80: 3, 80-90: 4, from 90: 5).
pub fn zone_weight(share: f64) -> f64 {
    match share {
        s if s >= 0.9 => 5.0,
        s if s >= 0.8 => 4.0,
        s if s >= 0.7 => 3.0,
        s if s >= 0.6 => 2.0,
        s if s >= 0.5 => 1.0,
        _ => 0.0,
    }
}

/// The second highest of the maxima held by the outings (the highest if there is only one): a
/// sensor's isolated spike does not set the maximum. `None` without any heart rate.
pub fn estimate_hr_max(derived: &HashMap<i64, Derived>) -> Option<f32> {
    let mut maxima: Vec<f32> = derived.values().map(|d| d.max_hr).filter(|m| m.is_finite() && *m > 0.0).collect();
    maxima.sort_by(|a, b| b.total_cmp(a));
    maxima.get(1).or(maxima.first()).copied()
}

/// How the load of an outing was known.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Load {
    /// From the time spent in each band of heart rate.
    Bands(f64),
    /// From the average heart rate and the duration alone.
    Average(f64),
}

/// The load of an outing, in points; `None` without heart rate (nothing is made up).
pub fn load(act: &Activity, derived: Option<&Derived>, hr_max: f64) -> Option<Load> {
    if !(hr_max.is_finite() && hr_max > 0.0) {
        return None;
    }
    if let Some(bands) = derived.map(|d| &d.hr_bands).filter(|b| !b.is_empty()) {
        let points = bands.iter().enumerate().map(|(i, secs)| {
            let middle = (i as f64 + 0.5) * BAND as f64;
            f64::from(*secs).max(0.0) / 60.0 * zone_weight(middle / hr_max)
        });
        return Some(Load::Bands(points.sum()));
    }
    let (hr, secs) = (act.average_hr.filter(|h| h.is_finite() && *h > 0.0)?, act.duration.filter(|d| d.is_finite() && *d > 0.0)?);
    Some(Load::Average(secs / 60.0 * zone_weight(hr / hr_max)))
}

/// One day, at its end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Day {
    pub date: NaiveDate,
    pub load: f64,
    /// Fatigue: average of the load over about 7 days.
    pub atl: f64,
    /// Form: average of the load over about 42 days.
    pub ctl: f64,
    /// Freshness: form minus fatigue.
    pub tsb: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Form {
    /// Every day from the first outing counted to today, rest days included. Empty without load.
    pub days: Vec<Day>,
    pub hr_max: Option<f32>,
    /// Outings with a load, those known by their average heart rate included.
    pub counted: usize,
    /// Of the counted, those known by their average heart rate alone.
    pub estimated: usize,
    /// Outings without heart rate, left out (also all of them when no maximum is known).
    pub skipped: usize,
}

impl Form {
    /// The form needs about 42 days to settle: before, it is the average of a short history.
    pub fn settled(&self) -> bool {
        self.days.len() >= FORM_DAYS as usize
    }

    /// The days from `since` on (all of them for `None`).
    pub fn shown(&self, since: Option<NaiveDate>) -> &[Day] {
        let from = since.map_or(0, |s| self.days.partition_point(|d| d.date < s));
        &self.days[from..]
    }
}

/// The daily series up to `today`. An outing dated after it, or without a date, is ignored.
pub fn form(acts: &[Activity], derived: &HashMap<i64, Derived>, today: NaiveDate) -> Form {
    let hr_max = estimate_hr_max(derived);
    let mut out = Form { hr_max, ..Form::default() };
    let mut by_day: HashMap<NaiveDate, f64> = HashMap::new();
    for a in acts {
        let Some(date) = a.date().filter(|d| *d <= today) else { continue };
        match hr_max.and_then(|m| load(a, derived.get(&a.activity_id), f64::from(m))) {
            Some(l) => {
                let (Load::Bands(p) | Load::Average(p)) = l;
                *by_day.entry(date).or_default() += p;
                out.counted += 1;
                out.estimated += matches!(l, Load::Average(_)) as usize;
            }
            None => out.skipped += 1,
        }
    }
    let Some(first) = by_day.keys().min().copied() else { return out };
    let (kf, kc) = (1.0 - (-1.0 / FATIGUE_DAYS).exp(), 1.0 - (-1.0 / FORM_DAYS).exp());
    let (mut atl, mut ctl) = (0.0, 0.0);
    let mut date = first;
    while date <= today {
        let load = by_day.get(&date).copied().unwrap_or(0.0);
        atl += (load - atl) * kf;
        ctl += (load - ctl) * kc;
        out.days.push(Day { date, load, atl, ctl, tsb: ctl - atl });
        let Some(next) = date.checked_add_days(Days::new(1)) else { break };
        date = next;
    }
    out
}

/// Fatigue against form.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reading {
    Above,
    Below,
    Near,
}

/// This week against the one before: the loads summed, the days of the week elapsed so far.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weeks {
    pub this: f64,
    pub last: f64,
    pub elapsed: u32,
}

impl Weeks {
    /// Change of this week from the last, in percent; `None` when last week had no load.
    pub fn change(&self) -> Option<f64> {
        (self.last > 0.0).then(|| (self.this / self.last - 1.0) * 100.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Findings {
    pub today: Day,
    /// Fatigue over form, `None` when the form is nil.
    pub ratio: Option<f64>,
    pub reading: Option<Reading>,
    /// Form today minus form 28 days ago; `None` with less than 29 days of series.
    pub form_change: Option<f64>,
    pub weeks: Weeks,
    /// Over the days shown: the highest freshness and the highest fatigue.
    pub freshest: Day,
    pub most_tired: Day,
}

/// What the figures state; `None` without a day. `since`: first day of the period shown, which
/// bounds the extremes only.
pub fn findings(form: &Form, since: Option<NaiveDate>) -> Option<Findings> {
    let today = *form.days.last()?;
    let ratio = (today.ctl > 0.0).then(|| today.atl / today.ctl);
    let reading = ratio.map(|r| if r > 1.0 + NEAR { Reading::Above } else if r < 1.0 - NEAR { Reading::Below } else { Reading::Near });
    let form_change = form.days.len().checked_sub(TREND_DAYS + 1).map(|i| today.ctl - form.days[i].ctl);
    let shown = form.shown(since);
    let by = |key: fn(&Day) -> f64| shown.iter().copied().fold(None, |best: Option<Day>, d| if best.is_none_or(|b| key(&d) > key(&b)) { Some(d) } else { best });
    let (freshest, most_tired) = (by(|d| d.tsb)?, by(|d| d.atl)?);
    let elapsed = today.date.weekday().num_days_from_monday() + 1;
    let monday = today.date.checked_sub_days(Days::new(u64::from(elapsed - 1)))?;
    let sum = |from: NaiveDate, to: NaiveDate| form.days.iter().filter(|d| d.date >= from && d.date <= to).map(|d| d.load).sum::<f64>();
    let last_monday = monday.checked_sub_days(Days::new(7))?;
    let weeks = Weeks { this: sum(monday, today.date), last: sum(last_monday, monday.pred_opt()?), elapsed };
    Some(Findings { today, ratio, reading, form_change, weeks, freshest, most_tired })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::activity::ActivityType;

    fn day(n: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap() + Days::new(u64::from(n))
    }

    fn act(id: i64, date: NaiveDate, avg_hr: Option<f64>, duration: Option<f64>) -> Activity {
        Activity {
            activity_id: id,
            activity_name: None,
            start_time_local: format!("{date} 08:00:00"),
            activity_type: ActivityType { type_key: "running".into() },
            distance: None,
            duration,
            elevation_gain: None,
            average_hr: avg_hr,
            calories: None,
        }
    }

    /// An outing whose heart rate bands give `minutes` in the zone of weight `w` for a maximum of 200
    /// (the band starting at 100 bpm, 50 %-52.5 %, middle 51.25 %: weight 1).
    fn banded(id: i64, minutes: f32, max: f32) -> (Derived, i64) {
        let mut bands = vec![0.0; 21];
        bands[20] = minutes * 60.0; // 100-105 bpm
        (Derived { hr_bands: bands, max_hr: max, ..Default::default() }, id)
    }

    fn with_max(acts: &[(Activity, Derived)]) -> (Vec<Activity>, HashMap<i64, Derived>) {
        (acts.iter().map(|(a, _)| a.clone()).collect(), acts.iter().map(|(a, d)| (a.activity_id, d.clone())).collect())
    }

    #[test]
    fn zones_at_their_edges() {
        let cases = [(0.0, 0.0), (0.499, 0.0), (0.50, 1.0), (0.599, 1.0), (0.60, 2.0), (0.699, 2.0), (0.70, 3.0), (0.799, 3.0), (0.80, 4.0), (0.899, 4.0), (0.90, 5.0), (1.0, 5.0), (1.1, 5.0)];
        for (share, weight) in cases {
            assert_eq!(zone_weight(share), weight, "{share}");
        }
        assert_eq!(zone_weight(f64::NAN), 0.0);
    }

    #[test]
    fn the_maximum_is_the_second_highest() {
        let d = |v: &[f32]| v.iter().enumerate().map(|(i, m)| (i as i64, Derived { max_hr: *m, ..Default::default() })).collect::<HashMap<_, _>>();
        assert_eq!(estimate_hr_max(&d(&[])), None);
        assert_eq!(estimate_hr_max(&d(&[0.0, 0.0])), None);
        assert_eq!(estimate_hr_max(&d(&[190.0])), Some(190.0));
        assert_eq!(estimate_hr_max(&d(&[190.0, 180.0])), Some(180.0));
        // The isolated spike (225) is ignored; outings without heart rate (0) do not count.
        assert_eq!(estimate_hr_max(&d(&[214.0, 225.0, 0.0, 216.0])), Some(216.0));
        assert_eq!(estimate_hr_max(&d(&[216.0, 225.0, 214.0, 216.0])), Some(216.0));
    }

    #[test]
    fn a_load_from_bands_from_the_average_or_none() {
        // 60 min in a band of weight 1 (middle 102.5 of 200 = 51 %).
        let (d, _) = banded(1, 60.0, 200.0);
        let a = act(1, day(0), None, None);
        assert_eq!(load(&a, Some(&d), 200.0), Some(Load::Bands(60.0)));
        // Same band, a maximum of 100: 102 % of it, weight 5.
        assert_eq!(load(&a, Some(&d), 100.0), Some(Load::Bands(300.0)));
        // Average heart rate alone: 30 min at 150 of 200 (75 %, weight 3).
        let b = act(2, day(0), Some(150.0), Some(1800.0));
        assert_eq!(load(&b, None, 200.0), Some(Load::Average(90.0)));
        assert_eq!(load(&b, Some(&Derived::default()), 200.0), Some(Load::Average(90.0)));
        // Nothing to go by.
        assert_eq!(load(&act(3, day(0), None, Some(1800.0)), None, 200.0), None);
        assert_eq!(load(&act(3, day(0), Some(150.0), None), None, 200.0), None);
        assert_eq!(load(&act(3, day(0), Some(0.0), Some(1800.0)), None, 200.0), None);
        // No maximum: no load at all, and no division by zero.
        assert_eq!(load(&b, None, 0.0), None);
        assert_eq!(load(&b, None, f64::NAN), None);
        // Easy outing: 0 points, but a load all the same.
        assert_eq!(load(&act(4, day(0), Some(80.0), Some(3600.0)), None, 200.0), Some(Load::Average(0.0)));
    }

    #[test]
    fn nothing_to_count() {
        let f = form(&[], &HashMap::new(), day(10));
        assert!(f.days.is_empty() && f.hr_max.is_none() && !f.settled());
        assert_eq!((f.counted, f.estimated, f.skipped), (0, 0, 0));
        assert_eq!(findings(&f, None), None);
        // Outings, but no maximum anywhere: no load, all left out, nothing divides by zero.
        let acts = vec![act(1, day(0), Some(150.0), Some(1800.0))];
        let f = form(&acts, &HashMap::new(), day(10));
        assert!(f.days.is_empty());
        assert_eq!((f.counted, f.skipped, f.hr_max), (0, 1, None));
    }

    #[test]
    fn an_outing_without_heart_rate_is_left_out_and_counted() {
        let (a, d) = (act(1, day(0), None, Some(3600.0)), banded(1, 60.0, 200.0).0);
        let b = act(2, day(1), None, Some(3600.0));
        let (acts, derived) = with_max(&[(a, d)]);
        let mut acts = acts;
        acts.push(b);
        let f = form(&acts, &derived, day(3));
        assert_eq!((f.counted, f.estimated, f.skipped), (1, 0, 1));
        assert_eq!(f.days.len(), 4);
        assert_eq!(f.days[1].load, 0.0);
    }

    #[test]
    fn the_average_heart_rate_alone_is_an_estimate() {
        let (a, d) = (act(1, day(0), None, None), banded(1, 60.0, 200.0).0);
        let b = act(2, day(2), Some(150.0), Some(1800.0));
        let (mut acts, derived) = with_max(&[(a, d)]);
        acts.push(b);
        let f = form(&acts, &derived, day(2));
        assert_eq!((f.counted, f.estimated, f.skipped), (2, 1, 0));
        assert_eq!(f.days[2].load, 90.0);
        assert_eq!(f.days[0].load, 60.0);
    }

    #[test]
    fn one_impulse_decays_by_the_formula() {
        let (a, d) = (act(1, day(0), None, None), banded(1, 100.0, 200.0).0);
        let (acts, derived) = with_max(&[(a, d)]);
        let f = form(&acts, &derived, day(60));
        assert_eq!(f.days.len(), 61);
        let (kf, kc) = (1.0 - (-1.0f64 / 7.0).exp(), 1.0 - (-1.0f64 / 42.0).exp());
        assert!((f.days[0].atl - 100.0 * kf).abs() < 1e-9 && (f.days[0].ctl - 100.0 * kc).abs() < 1e-9);
        // Rest days after the last outing: each multiplies by e^(-1/tau).
        for n in [1usize, 7, 30, 60] {
            assert!((f.days[n].atl - 100.0 * kf * (-(n as f64) / 7.0).exp()).abs() < 1e-9, "atl {n}");
            assert!((f.days[n].ctl - 100.0 * kc * (-(n as f64) / 42.0).exp()).abs() < 1e-9, "ctl {n}");
            assert!((f.days[n].tsb - (f.days[n].ctl - f.days[n].atl)).abs() < 1e-12);
        }
        // The day of the outing, fatigue is above form: freshness is negative; later it turns positive.
        assert!(f.days[0].tsb < 0.0 && f.days[60].tsb > 0.0);
        assert!(f.settled());
    }

    #[test]
    fn a_constant_load_settles_on_itself() {
        // 60 min a day at weight 1 = 60 points a day, for 400 days.
        let list: Vec<(Activity, Derived)> = (0..400).map(|i| (act(i, day(i as u32), None, None), banded(i, 60.0, 200.0).0)).collect();
        let (acts, derived) = with_max(&list);
        let f = form(&acts, &derived, day(399));
        let last = f.days.last().unwrap();
        assert!((last.atl - 60.0).abs() < 0.6 && (last.ctl - 60.0).abs() < 0.6, "{last:?}");
        assert!(last.tsb.abs() < 0.6);
        let found = findings(&f, None).unwrap();
        assert_eq!(found.reading, Some(Reading::Near));
        assert!(found.form_change.unwrap().abs() < 0.1);
    }

    #[test]
    fn same_day_adds_up_and_the_future_is_ignored() {
        let (a, d1) = (act(1, day(0), None, None), banded(1, 10.0, 200.0).0);
        let (b, d2) = (act(2, day(0), None, None), banded(2, 20.0, 200.0).0);
        let (c, d3) = (act(3, day(5), None, None), banded(3, 999.0, 200.0).0);
        let (acts, derived) = with_max(&[(a, d1), (b, d2), (c, d3)]);
        let f = form(&acts, &derived, day(2));
        assert_eq!(f.days.len(), 3);
        assert_eq!(f.days[0].load, 30.0);
        // The future outing is neither counted nor left out.
        assert_eq!((f.counted, f.skipped), (2, 0));
        // Today before the first outing: no day at all.
        assert!(form(&acts, &derived, day(0) - Days::new(3)).days.is_empty());
    }

    #[test]
    fn a_huge_load_stays_finite() {
        let (a, d) = (act(1, day(0), None, None), banded(1, 1.0e6 / 5.0, 100.0).0);
        let (acts, derived) = with_max(&[(a, d)]);
        let f = form(&acts, &derived, day(10));
        assert!(f.days.iter().all(|d| d.atl.is_finite() && d.ctl.is_finite() && d.tsb.is_finite()));
        assert_eq!(f.days[0].load, 1.0e6);
    }

    #[test]
    fn what_is_found() {
        // Day 0 Monday 2026-06-01? Pick a known Monday: 2026-10-19 is a Monday.
        let monday = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
        let list: Vec<(Activity, Derived)> = (0..45).map(|i| (act(i, monday - Days::new(44 - i as u64), None, None), banded(i, if i < 30 { 30.0 } else { 90.0 }, 200.0).0)).collect();
        let (acts, derived) = with_max(&list);
        let f = form(&acts, &derived, monday);
        let found = findings(&f, None).unwrap();
        assert_eq!(found.today.date, monday);
        assert_eq!(found.weeks.elapsed, 1);
        assert_eq!(found.weeks.this, 90.0);
        assert_eq!(found.weeks.last, 7.0 * 90.0);
        assert!((found.weeks.change().unwrap() + 100.0 * (1.0 - 1.0 / 7.0)).abs() < 1e-9);
        // The load went up three-fold: fatigue is above form, and form has risen.
        assert_eq!(found.reading, Some(Reading::Above));
        assert!(found.form_change.unwrap() > 0.0);
        assert!(found.ratio.unwrap() > 1.05);
        // Extremes bounded by the period shown.
        let since = monday - Days::new(2);
        let short = findings(&f, Some(since)).unwrap();
        assert!(short.freshest.date >= since && short.most_tired.date >= since);
        // Less than 29 days: no change of form told.
        let tiny = form(&acts, &derived, monday - Days::new(43));
        assert_eq!(tiny.days.len(), 2);
        assert_eq!(findings(&tiny, None).unwrap().form_change, None);
        // The reading's thresholds.
        assert_eq!(findings(&Form { days: vec![Day { date: monday, load: 0.0, atl: 0.0, ctl: 0.0, tsb: 0.0 }], ..Default::default() }, None).unwrap().ratio, None);
    }

    #[test]
    fn the_period_shown_cuts_the_series() {
        let list: Vec<(Activity, Derived)> = (0..10).map(|i| (act(i, day(i as u32), None, None), banded(i, 10.0, 200.0).0)).collect();
        let (acts, derived) = with_max(&list);
        let f = form(&acts, &derived, day(9));
        assert_eq!(f.shown(None).len(), 10);
        assert_eq!(f.shown(Some(day(7))).len(), 3);
        assert_eq!(f.shown(Some(day(100))).len(), 0);
        assert_eq!(f.shown(Some(day(0) - Days::new(100))).len(), 10);
        assert_eq!(findings(&f, Some(day(100))), None);
    }
}
