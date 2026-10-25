//! Dates and totals as the interface writes them, in the language of the system.

use crate::lang::{is_french, tr};

const DAYS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
const DAYS_EN: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const DAYS_SHORT: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];
const DAYS_SHORT_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = [
    "janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre",
    "novembre", "décembre",
];
const MONTHS_EN: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const MONTHS_SHORT: [&str; 12] =
    ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];
const MONTHS_SHORT_EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The month's short name: "oct.", "Oct".
pub fn month_short(m0: u32) -> &'static str {
    if is_french() { MONTHS_SHORT[m0 as usize] } else { MONTHS_SHORT_EN[m0 as usize] }
}

/// The month's name, in full: "octobre", "October".
pub fn month_name(m0: u32) -> &'static str {
    if is_french() { MONTHS[m0 as usize] } else { MONTHS_EN[m0 as usize] }
}

/// Monday is 0: "lun.", "Mon".
pub fn weekday_short(i: usize) -> &'static str {
    if is_french() { DAYS_SHORT[i] } else { DAYS_SHORT_EN[i] }
}

fn weekday_name(d: chrono::NaiveDate) -> &'static str {
    use chrono::Datelike;
    let i = d.weekday().num_days_from_monday() as usize;
    if is_french() { DAYS[i] } else { DAYS_EN[i] }
}

/// A day and a month, in the order the language writes them: "24 oct.", "Oct 24".
pub fn day_month_short(day: u32, m0: u32) -> String {
    if is_french() { format!("{day} {}", month_short(m0)) } else { format!("{} {day}", month_short(m0)) }
}

/// "24 octobre", "October 24", with the year when it isn't this one: "24 octobre 2025", "October 24, 2025".
pub fn day_label(d: chrono::NaiveDate, today: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    let (day, month) = (d.day(), month_name(d.month0()));
    match (is_french(), d.year() == today.year()) {
        (true, true) => format!("{day} {month}"),
        (true, false) => format!("{day} {month} {}", d.year()),
        (false, true) => format!("{month} {day}"),
        (false, false) => format!("{month} {day}, {}", d.year()),
    }
}

/// A figure and its unit, to set them apart: "4,0 km" → ("4,0", "km"), "6:46 /km" → ("6:46",
/// "/km"), "27min" → ("27", "min"), "153h" → ("153", "h"). What has no unit after a number stays
/// whole: "4h26", "1er sur 12", "≈ habitude".
pub fn split_unit(s: &str) -> (String, String) {
    if let Some((i, c)) = s.char_indices().rev().find(|(_, c)| *c == ' ' || *c == '\u{a0}') {
        let (n, u) = (&s[..i], &s[i + c.len_utf8()..]);
        if n.ends_with(|c: char| c.is_ascii_digit()) && u.starts_with(|c: char| c.is_alphabetic() || c == '/') {
            return (n.into(), u.into());
        }
    }
    let digits = |n: &str| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '\u{a0}' || c == ',');
    for unit in ["min", "h"] {
        if let Some(n) = s.strip_suffix(unit).filter(|n| digits(n)) {
            return (n.into(), unit.into());
        }
    }
    (s.into(), String::new())
}

/// "samedi 24 octobre", "Saturday, October 24".
pub fn weekday_label(d: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    if is_french() {
        format!("{} {} {}", weekday_name(d), d.day(), month_name(d.month0()))
    } else {
        format!("{}, {} {}", weekday_name(d), month_name(d.month0()), d.day())
    }
}

/// "sam. 24 oct.", "Sat, Oct 24": for tables.
pub fn short_date(d: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    let i = d.weekday().num_days_from_monday() as usize;
    let (wd, dm) = (weekday_short(i), day_month_short(d.day(), d.month0()));
    if is_french() { format!("{wd} {dm}") } else { format!("{wd}, {dm}") }
}

/// "samedi 24 octobre 2026 · 07:31", "Saturday, October 24, 2026 · 07:31".
pub fn long_date_time(dt: chrono::NaiveDateTime) -> String {
    use chrono::{Datelike, Timelike};
    let d = dt.date();
    let (hour, minute) = (dt.hour(), dt.minute());
    if is_french() {
        format!("{} {} {} {} · {hour:02}:{minute:02}", weekday_name(d), d.day(), month_name(d.month0()), d.year())
    } else {
        format!("{}, {} {}, {} · {hour:02}:{minute:02}", weekday_name(d), month_name(d.month0()), d.day(), d.year())
    }
}

/// "6 sorties · 65,6 km · 10h18 · 1 610 m D+" ("6 outings · 65.6 km · 10h18 · 1,610 m gain"), leaving
/// out what is zero.
pub fn totals_line(t: &crate::logic::activity::Totals) -> String {
    use crate::logic::activity::{fmt_duration, fmt_km, fmt_m, outings};
    let mut parts = vec![outings(t.count)];
    if t.distance > 0.0 {
        parts.push(fmt_km(t.distance));
    }
    if t.duration > 0.0 {
        parts.push(fmt_duration(t.duration));
    }
    if t.elevation >= 1.0 {
        parts.push(format!("{} {}", fmt_m(t.elevation), tr("gain", "D+")));
    }
    parts.join(" · ")
}

/// What a calendar page is about: "Octobre 2026", "19 – 25 octobre 2026", "28 sept. – 4 oct. 2026",
/// "29 déc. 2025 – 4 janv. 2026", "2026"; in English "October 2026", "October 19 – 25, 2026",
/// "Sep 28 – Oct 4, 2026", "Dec 29, 2025 – Jan 4, 2026".
pub fn calendar_title(mode: crate::logic::calendar::Mode, anchor: chrono::NaiveDate) -> String {
    use crate::logic::calendar::{Mode, week_days};
    use chrono::Datelike;
    match mode {
        Mode::Year => anchor.year().to_string(),
        Mode::Month => {
            let name = month_name(anchor.month0());
            let mut cs = name.chars();
            format!("{}{} {}", cs.next().unwrap().to_uppercase(), cs.as_str(), anchor.year())
        }
        Mode::Week => {
            let days = week_days(anchor);
            let (a, b) = (days[0], days[6]);
            let (da, db) = (day_month_short(a.day(), a.month0()), day_month_short(b.day(), b.month0()));
            match (is_french(), a.month() == b.month(), a.year() == b.year()) {
                (true, true, _) => format!("{} – {} {} {}", a.day(), b.day(), month_name(b.month0()), b.year()),
                (true, false, true) => format!("{da} – {db} {}", b.year()),
                (true, false, false) => format!("{da} {} – {db} {}", a.year(), b.year()),
                (false, true, _) => format!("{} {} – {}, {}", month_name(b.month0()), a.day(), b.day(), b.year()),
                (false, false, true) => format!("{da} – {db}, {}", b.year()),
                (false, false, false) => format!("{da}, {} – {db}, {}", a.year(), b.year()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn figures_and_their_units() {
        let split = |s: &str| split_unit(s);
        let pair = |n: &str, u: &str| (n.to_string(), u.to_string());
        assert_eq!(split("4,0\u{a0}km"), pair("4,0", "km"));
        assert_eq!(split("1\u{a0}941\u{a0}km"), pair("1\u{a0}941", "km"));
        assert_eq!(split("34\u{a0}semaines"), pair("34", "semaines"));
        assert_eq!(split("6:46 /km"), pair("6:46", "/km"));
        assert_eq!(split("2:00 /100m"), pair("2:00", "/100m"));
        assert_eq!(split("27,4\u{a0}km/h"), pair("27,4", "km/h"));
        assert_eq!(split("152 bpm"), pair("152", "bpm"));
        assert_eq!(split("1\u{a0}945 kcal"), pair("1\u{a0}945", "kcal"));
        assert_eq!(split("\u{2212}12 s/km"), pair("\u{2212}12", "s/km"));
        assert_eq!(split("+1:05 /km"), pair("+1:05", "/km"));
        assert_eq!(split("27min"), pair("27", "min"));
        assert_eq!(split("0min"), pair("0", "min"));
        assert_eq!(split("153h"), pair("153", "h"));
        assert_eq!(split("1\u{a0}020h"), pair("1\u{a0}020", "h"));
        for whole in ["16h18", "1:00:00", "1er sur 186", "≈ habitude", "min", "h", "", "km", " km"] {
            assert_eq!(split(whole), pair(whole, ""), "{whole:?}");
        }
    }
    use crate::logic::calendar::Mode;
    use chrono::NaiveDate;

    #[test]
    fn calendar_titles_read_in_french() {
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
        assert_eq!(calendar_title(Mode::Month, d(2026, 10, 24)), "Octobre 2026");
        assert_eq!(calendar_title(Mode::Month, d(2026, 2, 1)), "Février 2026");
        assert_eq!(calendar_title(Mode::Month, d(2026, 12, 31)), "Décembre 2026");
        assert_eq!(calendar_title(Mode::Year, d(2026, 10, 24)), "2026");
        assert_eq!(calendar_title(Mode::Week, d(2026, 10, 22)), "19 – 25 octobre 2026");
        assert_eq!(calendar_title(Mode::Week, d(2026, 9, 30)), "28 sept. – 4 oct. 2026");
        assert_eq!(calendar_title(Mode::Week, d(2026, 1, 1)), "29 déc. 2025 – 4 janv. 2026");
        assert_eq!(calendar_title(Mode::Week, d(2024, 2, 29)), "26 févr. – 3 mars 2024");
    }

    #[test]
    fn calendar_titles_read_in_english() {
        let _english = crate::lang::force(false);
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
        assert_eq!(calendar_title(Mode::Month, d(2026, 10, 24)), "October 2026");
        assert_eq!(calendar_title(Mode::Month, d(2026, 2, 1)), "February 2026");
        assert_eq!(calendar_title(Mode::Year, d(2026, 10, 24)), "2026");
        assert_eq!(calendar_title(Mode::Week, d(2026, 10, 22)), "October 19 – 25, 2026");
        assert_eq!(calendar_title(Mode::Week, d(2026, 9, 30)), "Sep 28 – Oct 4, 2026");
        // A week across New Year, either side, and a leap day.
        assert_eq!(calendar_title(Mode::Week, d(2026, 1, 1)), "Dec 29, 2025 – Jan 4, 2026");
        assert_eq!(calendar_title(Mode::Week, d(2024, 2, 29)), "Feb 26 – Mar 3, 2024");
    }

    #[test]
    fn days_are_written_in_both_languages() {
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
        let (today, last_year, new_year) = (d(2026, 10, 24), d(2025, 12, 31), d(2026, 1, 1));
        let at = d(2026, 10, 24).and_hms_opt(7, 31, 0).unwrap();
        assert_eq!(day_label(today, today), "24 octobre");
        assert_eq!(day_label(last_year, today), "31 décembre 2025");
        assert_eq!(weekday_label(today), "samedi 24 octobre");
        assert_eq!(short_date(today), "sam. 24 oct.");
        assert_eq!(long_date_time(at), "samedi 24 octobre 2026 · 07:31");
        assert_eq!(day_month_short(1, 0), "1 janv.");
        let _english = crate::lang::force(false);
        assert_eq!(day_label(today, today), "October 24");
        assert_eq!(day_label(last_year, today), "December 31, 2025");
        assert_eq!(day_label(new_year, today), "January 1");
        assert_eq!(weekday_label(today), "Saturday, October 24");
        assert_eq!(short_date(today), "Sat, Oct 24");
        assert_eq!(short_date(d(2026, 1, 5)), "Mon, Jan 5");
        assert_eq!(long_date_time(at), "Saturday, October 24, 2026 · 07:31");
        assert_eq!(day_month_short(1, 0), "Jan 1");
        // Every month and weekday has a name in both languages.
        for m0 in 0..12 {
            assert!(!month_name(m0).is_empty() && !month_short(m0).is_empty());
        }
        for i in 0..7 {
            assert_eq!(weekday_short(i).len(), 3);
        }
    }

    #[test]
    fn totals_and_figures_in_english() {
        use crate::logic::activity::Totals;
        let line = |count, distance, duration, elevation| totals_line(&Totals { count, distance, duration, elevation });
        assert_eq!(line(6, 65_600.0, 37_080.0, 1610.0), "6 sorties · 65,6\u{a0}km · 10h18 · 1\u{a0}610\u{a0}m D+");
        assert_eq!(line(1, 0.0, 0.0, 0.0), "1 sortie");
        let _english = crate::lang::force(false);
        assert_eq!(line(6, 65_600.0, 37_080.0, 1610.0), "6 outings · 65.6\u{a0}km · 10h18 · 1,610\u{a0}m gain");
        assert_eq!(line(0, 0.0, 0.0, 0.0), "0 outings");
        assert_eq!(line(1, 0.0, 0.0, 0.0), "1 outing");
        assert_eq!(line(2, 1_941_400.0, 0.0, 0.0), "2 outings · 1,941\u{a0}km");
        // A figure is told from its unit in English too.
        assert_eq!(split_unit("4.0\u{a0}km"), ("4.0".to_string(), "km".to_string()));
        assert_eq!(split_unit("1,080\u{a0}m"), ("1,080".to_string(), "m".to_string()));
        assert_eq!(split_unit("1,941\u{a0}km"), ("1,941".to_string(), "km".to_string()));
        assert_eq!(split_unit("1,020h"), ("1,020".to_string(), "h".to_string()));
        assert_eq!(split_unit("27.4\u{a0}km/h"), ("27.4".to_string(), "km/h".to_string()));
        for whole in ["1st of 186", "≈ usual", "10h18"] {
            assert_eq!(split_unit(whole), (whole.to_string(), String::new()), "{whole}");
        }
        assert_eq!(split_unit("34\u{a0}weeks"), ("34".to_string(), "weeks".to_string()));
    }
}
