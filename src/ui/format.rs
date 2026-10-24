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


/// "6 activités · 65,6 km · 10h18 · 1 610 m D+", leaving out what is zero.
pub fn totals_line(t: &crate::logic::activity::Totals) -> String {
    use crate::logic::activity::{fmt_duration, fmt_km, fmt_m, group};
    let mut parts = vec![format!("{} activité{}", group(t.count as i64), if t.count > 1 { "s" } else { "" })];
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
