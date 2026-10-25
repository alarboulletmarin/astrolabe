//! Dates and totals as the interface writes them, in French.

const DAYS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
const MONTHS: [&str; 12] = [
    "janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre",
    "novembre", "décembre",
];
const MONTHS_SHORT: [&str; 12] =
    ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];

pub fn month_short(m0: u32) -> &'static str {
    MONTHS_SHORT[m0 as usize]
}

/// "24 octobre", with the year when it isn't this one.
pub fn fr_day(d: chrono::NaiveDate, today: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    let year = if d.year() == today.year() { String::new() } else { format!(" {}", d.year()) };
    format!("{} {}{year}", d.day(), MONTHS[d.month0() as usize])
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
    let digits = |n: &str| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '\u{a0}');
    for unit in ["min", "h"] {
        if let Some(n) = s.strip_suffix(unit).filter(|n| digits(n)) {
            return (n.into(), unit.into());
        }
    }
    (s.into(), String::new())
}

/// "samedi 24 octobre".
pub fn fr_weekday(d: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    format!("{} {} {}", DAYS[d.weekday().num_days_from_monday() as usize], d.day(), MONTHS[d.month0() as usize])
}

/// "sam. 24 oct." for tables.
pub fn fr_short(d: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    let wd = &DAYS[d.weekday().num_days_from_monday() as usize][..3];
    format!("{wd}. {} {}", d.day(), MONTHS_SHORT[d.month0() as usize])
}

/// "samedi 24 octobre 2026 · 07:31".
pub fn fr_long(dt: chrono::NaiveDateTime) -> String {
    use chrono::{Datelike, Timelike};
    let d = dt.date();
    format!(
        "{} {} {} {} · {:02}:{:02}",
        DAYS[d.weekday().num_days_from_monday() as usize],
        d.day(),
        MONTHS[d.month0() as usize],
        d.year(),
        dt.hour(),
        dt.minute()
    )
}


/// "6 sorties · 65,6 km · 10h18 · 1 610 m D+", leaving out what is zero.
pub fn totals_line(t: &crate::logic::activity::Totals) -> String {
    use crate::logic::activity::{fmt_duration, fmt_km, fmt_m, group};
    let mut parts = vec![format!("{} sortie{}", group(t.count as i64), if t.count > 1 { "s" } else { "" })];
    if t.distance > 0.0 {
        parts.push(fmt_km(t.distance));
    }
    if t.duration > 0.0 {
        parts.push(fmt_duration(t.duration));
    }
    if t.elevation >= 1.0 {
        parts.push(format!("{} D+", fmt_m(t.elevation)));
    }
    parts.join(" · ")
}

/// The month's name, in full.
pub fn month_name(m0: u32) -> &'static str {
    MONTHS[m0 as usize]
}

/// What a calendar page is about: "Octobre 2026", "19 – 25 octobre 2026", "28 sept. – 4 oct. 2026",
/// "29 déc. 2025 – 4 janv. 2026", "2026".
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
            if a.month() == b.month() {
                format!("{} – {} {} {}", a.day(), b.day(), month_name(b.month0()), b.year())
            } else if a.year() == b.year() {
                format!("{} {} – {} {} {}", a.day(), month_short(a.month0()), b.day(), month_short(b.month0()), b.year())
            } else {
                format!("{} {} {} – {} {} {}", a.day(), month_short(a.month0()), a.year(), b.day(), month_short(b.month0()), b.year())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{calendar_title, split_unit};

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
}
