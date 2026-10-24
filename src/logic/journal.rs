//! The journal's rows: activities grouped by week, filtered by sport and by a forgiving search.

use crate::logic::activity::{self as act, Activity, Sport};
use chrono::NaiveDate;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row {
    /// A week header, its Monday.
    Week(NaiveDate),
    /// An activity, index in `Home::acts`.
    Act(usize),
}

/// Lowercase without accents, for a search that forgives "eclair" for "Éclair".
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ÿ' => 'y',
            c => c,
        })
        .collect()
}

/// `acts` come newest first; one header before each week's first activity.
/// `needle` (folded) keeps the activities whose name or sport holds it.
pub fn rows(acts: &[Activity], sport: Option<Sport>, needle: &str) -> Vec<Row> {
    let needle = needle.trim();
    let mut out = Vec::new();
    let mut week = None;
    for (i, a) in acts.iter().enumerate() {
        if !act::matches(a, sport) || (!needle.is_empty() && !fold(&format!("{} {}", name(a), a.sport().label())).contains(needle)) {
            continue;
        }
        let w = a.date().map(act::week_start);
        if let Some(monday) = w.filter(|_| w != week) {
            out.push(Row::Week(monday));
            week = w;
        }
        out.push(Row::Act(i));
    }
    out
}

pub fn name(a: &Activity) -> String {
    a.activity_name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| a.sport().label().into())
}

/// The sport a device writes after the place in the names it gives ("Nice Course à pied"), longest
/// first so that "Natation en eau libre" goes before "Natation".
const ADDED: [&str; 21] = [
    "Natation en eau libre",
    "Natation en piscine",
    "Open Water Swimming",
    "Vélo d'intérieur",
    "Course sur tapis",
    "Stand up paddle",
    "Marche à pied",
    "Pool Swimming",
    "Trail Running",
    "Course à pied",
    "Musculation",
    "Randonnée",
    "Cyclisme",
    "Natation",
    "Strength",
    "Running",
    "Walking",
    "Cycling",
    "Hiking",
    "Cardio",
    "Trail",
];

/// The name without the sport a device added after the place ("Nice Course à pied" → "Nice"):
/// the row shows the sport's icon. A name of one's own, or a sport alone, is kept whole.
pub fn short_name(a: &Activity) -> String {
    let full = name(a);
    ADDED
        .iter()
        .find_map(|sport| full.strip_suffix(sport)?.strip_suffix(' ').filter(|place| !place.trim().is_empty() && !place.ends_with('-')).map(str::to_string))
        .unwrap_or(full)
}

/// The sports present in the data, for the filters.
pub fn sports_present(acts: &[Activity]) -> Vec<Sport> {
    Sport::ALL.into_iter().filter(|s| acts.iter().any(|a| a.sport() == *s)).collect()
}

#[cfg(test)]
mod tests {
    use super::{Activity, NaiveDate, Row, Sport, fold, name, rows, short_name};

    fn a(id: i64, t: &str, key: &str, n: &str) -> Activity {
        serde_json::from_value(serde_json::json!({
            "activityId": id, "startTimeLocal": t, "activityType": {"typeKey": key}, "activityName": n
        }))
        .unwrap()
    }

    #[test]
    fn rows_group_filter_and_search() {
        assert!(rows(&[], None, "").is_empty());
        let acts = vec![
            a(5, "2026-10-26 07:00:00", "running", "Footing"),
            a(4, "2026-10-25 07:00:00", "road_biking", "Col de l'Écharpe"),
            a(3, "2026-10-19 07:00:00", "trail_running", "Trail des Monts"),
            a(2, "bad date", "running", ""),
            a(1, "2026-09-01 07:00:00", "running", "Footing"),
        ];
        let d = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(
            rows(&acts, None, ""),
            [Row::Week(d("2026-10-26")), Row::Act(0), Row::Week(d("2026-10-19")), Row::Act(1), Row::Act(2), Row::Act(3), Row::Week(d("2026-08-31")), Row::Act(4)]
        );
        assert_eq!(rows(&acts, Some(Sport::Trail), ""), [Row::Week(d("2026-10-19")), Row::Act(2)]);
        assert!(rows(&acts, Some(Sport::Swim), "").is_empty());
        // Accents and case forgiven; the sport's name matches too; spaces around ignored.
        assert_eq!(rows(&acts, None, &fold(" echarpe ")), [Row::Week(d("2026-10-19")), Row::Act(1)]);
        assert_eq!(rows(&acts, None, "velo"), [Row::Week(d("2026-10-19")), Row::Act(1)]);
        assert!(rows(&acts, None, "zzz").is_empty());
        assert_eq!(name(&acts[3]), "Course");
        // The sport a device added goes; a name of one's own stays.
        let short = |n: &str| short_name(&a(1, "2026-10-26 07:00:00", "running", n));
        assert_eq!(short("Nice Course à pied"), "Nice");
        assert_eq!(short("Saint-Jean-Cap-Ferrat Trail"), "Saint-Jean-Cap-Ferrat");
        assert_eq!(short("Alpes-Maritimes Natation en eau libre"), "Alpes-Maritimes");
        assert_eq!(short("Alpes-Maritimes Stand up paddle"), "Alpes-Maritimes");
        assert_eq!(short("Lyon Trail Running"), "Lyon");
        assert_eq!(short("Nice - Footing 40min"), "Nice - Footing 40min");
        assert_eq!(short("Sortie longue Course à pied du dimanche"), "Sortie longue Course à pied du dimanche");
        assert_eq!(short("Course à pied"), "Course à pied");
        assert_eq!(short(" Course à pied"), " Course à pied");
        assert_eq!(short("Valbonne - Course à pied"), "Valbonne - Course à pied");
        assert_eq!(short("NiceCourse à pied"), "NiceCourse à pied");
        assert_eq!(short(""), "Course");
        assert_eq!(fold("ÉCLAIR Çà"), "eclair ca");
    }
}
