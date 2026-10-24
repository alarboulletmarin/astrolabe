//! Records: the best efforts over the classic distances (from the tracks, like Strava's), the
//! longest outings, the biggest climbs. Road running and trail apart; a podium for each.

use crate::activity::{Activity, Sport, fmt_clock, fmt_km, fmt_m};
use crate::app::Home;
use crate::fit::Derived;
use crate::journal::name;
use crate::ui::{fr_day, heading, num, pad, segmented, title};
use chrono::{Datelike, NaiveDate};
use gpui_kit::{prelude::FluentBuilder, *};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Fastest time over this many metres, anywhere inside an activity.
    Effort(f64),
    Longest,
    MostClimb,
    LongestTime,
}

pub struct Board {
    pub title: &'static str,
    pub sport: Sport,
    /// Leaves out treadmill, indoor and virtual runs, whose distances are estimates.
    pub outdoor: bool,
    pub rows: &'static [(&'static str, Kind)],
}

pub const BOARDS: [Board; 4] = [
    Board {
        title: "Course sur route",
        sport: Sport::Run,
        outdoor: true,
        rows: &[
            ("1 km", Kind::Effort(1000.0)),
            ("5 km", Kind::Effort(5000.0)),
            ("10 km", Kind::Effort(10_000.0)),
            ("Semi-marathon", Kind::Effort(21_097.5)),
            ("Marathon", Kind::Effort(42_195.0)),
            ("Plus longue course", Kind::Longest),
        ],
    },
    Board {
        title: "Trail",
        sport: Sport::Trail,
        outdoor: false,
        rows: &[
            ("Plus longue distance", Kind::Longest),
            ("Plus gros dénivelé", Kind::MostClimb),
            ("Plus long effort", Kind::LongestTime),
            ("10 km", Kind::Effort(10_000.0)),
            ("Semi-marathon", Kind::Effort(21_097.5)),
            ("Marathon", Kind::Effort(42_195.0)),
            ("50 km", Kind::Effort(50_000.0)),
            ("100 km", Kind::Effort(100_000.0)),
        ],
    },
    Board {
        title: "Vélo",
        sport: Sport::Bike,
        outdoor: false,
        rows: &[
            ("20 km", Kind::Effort(20_000.0)),
            ("40 km", Kind::Effort(40_000.0)),
            ("100 km", Kind::Effort(100_000.0)),
            ("Plus longue sortie", Kind::Longest),
            ("Plus gros dénivelé", Kind::MostClimb),
        ],
    },
    Board {
        title: "Natation",
        sport: Sport::Swim,
        outdoor: false,
        rows: &[
            ("400 m", Kind::Effort(400.0)),
            ("1 000 m", Kind::Effort(1000.0)),
            ("1 500 m", Kind::Effort(1500.0)),
            ("Plus longue nage", Kind::Longest),
        ],
    },
];

pub fn outdoor(a: &Activity) -> bool {
    let k = &a.activity_type.type_key;
    !(k.contains("treadmill") || k.contains("indoor") || k.contains("virtual"))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub id: i64,
    /// Seconds for an effort or a duration, metres for a distance or a climb.
    pub value: f64,
    pub date: NaiveDate,
}

/// The `n` best of a board's row, best first.
pub fn podium(acts: &[Activity], derived: &HashMap<i64, Derived>, board: &Board, kind: Kind, year: Option<i32>, n: usize) -> Vec<Entry> {
    let mut out: Vec<Entry> = acts
        .iter()
        .filter(|a| a.sport() == board.sport && (!board.outdoor || outdoor(a)))
        .filter_map(|a| {
            let date = a.date()?;
            if year.is_some_and(|y| date.year() != y) {
                return None;
            }
            let value = match kind {
                Kind::Effort(m) => derived.get(&a.activity_id)?.efforts.iter().find(|e| (e.metres as f64 - m).abs() < 0.5)?.seconds as f64,
                Kind::Longest => a.distance?,
                Kind::MostClimb => a.elevation_gain?,
                Kind::LongestTime => a.duration?,
            };
            (value > 0.0).then_some(Entry { id: a.activity_id, value, date })
        })
        .collect();
    match kind {
        Kind::Effort(_) => out.sort_by(|a, b| a.value.total_cmp(&b.value)),
        _ => out.sort_by(|a, b| b.value.total_cmp(&a.value)),
    }
    out.truncate(n);
    out
}

/// The value as it reads: a time and its pace for an effort.
pub fn show(kind: Kind, sport: Sport, v: f64) -> (String, String) {
    match kind {
        Kind::Effort(m) => {
            let pace = match sport {
                Sport::Bike => format!("{:.1} km/h", m / v * 3.6).replace('.', ","),
                Sport::Swim => format!("{} /100m", fmt_clock(v / m * 100.0)),
                _ => format!("{} /km", fmt_clock(v / m * 1000.0)),
            };
            (fmt_clock(v), pace)
        }
        Kind::Longest => (fmt_km(v), String::new()),
        Kind::MostClimb => (fmt_m(v), String::new()),
        Kind::LongestTime => (fmt_clock(v), String::new()),
    }
}

impl Home {
    pub fn records(&mut self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let p = pad(width);
        let mut years: Vec<i32> = self.acts.iter().filter_map(|a| a.date()).map(|d| d.year()).collect();
        years.sort_unstable_by(|a, b| b.cmp(a));
        years.dedup();
        let year = self.record_year;
        let mut items = vec![("Toutes les années".into(), year.is_none())];
        items.extend(years.iter().map(|y| (y.to_string().into(), year == Some(*y))));
        let ys = years.clone();
        let filter = segmented("record-year", items, t, cx, move |this: &mut Home, i, _, _| {
            this.record_year = if i == 0 { None } else { Some(ys[i - 1]) };
        });

        let boards: Vec<&Board> = BOARDS.iter().filter(|b| self.acts.iter().any(|a| a.sport() == b.sport)).collect();
        let cols = if width - 2.0 * p < 900.0 { 1 } else if width - 2.0 * p < 1450.0 { 2 } else { 3 };
        let waiting = self.acts.iter().filter(|a| boards.iter().any(|b| b.sport == a.sport()) && !self.derived.contains_key(&a.activity_id)).count();

        let mut grid = div().grid().grid_cols(cols).gap_x(px(48.)).gap_y(px(40.));
        for board in boards.iter() {
            grid = grid.child(self.board(board, cx));
        }

        crate::ui::page("records", width)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .justify_between()
                    .gap(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(title("Records"))
                            .child(div().text_color(t.soft).child("Meilleurs temps n'importe où dans une sortie, comme un chrono pris en route.")),
                    )
                    .child(filter),
            )
            // The distance records come from the tracks: show where their download stands.
            .when(waiting > 0, |d| {
                d.child(
                    div()
                        .max_w(px(560.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(div().text_color(t.soft).child("Les temps sur distance se calculent à partir des traces : la page se complète au fil du téléchargement."))
                        .child(self.tracks_progress(cx)),
                )
            })
            .when(boards.is_empty(), |d| d.child(div().text_color(t.soft).child("Pas encore d'activité.")))
            .child(grid)
    }

    fn board(&self, board: &'static Board, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let mut col = div().flex().flex_col().child(heading(board.title).mb(px(8.)));
        for (i, (label, kind)) in board.rows.iter().enumerate() {
            let top = podium(&self.acts, &self.derived, board, *kind, self.record_year, 3);
            let today = crate::app::today();
            let first = top.first().copied();
            let (value, pace) = first.map_or(("—".into(), String::new()), |e| show(*kind, board.sport, e.value));
            let row = div()
                .id(SharedString::from(format!("rec-{}-{i}", board.title)))
                .flex()
                .flex_col()
                .gap(px(3.))
                .py(px(10.))
                .px(px(10.))
                .mx(px(-10.))
                .rounded(px(6.))
                .border_b_1()
                .border_color(t.border)
                .when_some(first, |d, e| {
                    d.cursor_pointer().hover(|d| d.bg(t.panel)).on_click(cx.listener(move |this, _, window, cx| this.open(e.id, window, cx)))
                })
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(12.))
                        .child(div().w(px(130.)).flex_none().text_color(t.soft).child(*label))
                        .child(num(value).text_size(px(18.)).font_weight(FontWeight::BOLD))
                        .child(num(pace).text_size(px(12.)).text_color(t.soft))
                        .child(div().flex_1())
                        .when_some(first, |d, e| {
                            d.child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(t.soft)
                                    .child(format!("{} · {}", self.activity(e.id).map(name).unwrap_or_default(), fr_day(e.date, today))),
                            )
                        }),
                )
                .when(top.len() > 1, |d| {
                    d.child(div().flex().gap(px(16.)).pl(px(142.)).text_size(px(12.)).text_color(t.dim).children(top.iter().skip(1).enumerate().map(
                        |(k, e)| {
                            let id = e.id;
                            div()
                                .id(SharedString::from(format!("pod-{}-{i}-{k}", board.title)))
                                .cursor_pointer()
                                .hover(|d| d.text_color(t.text))
                                .on_click(cx.listener(move |this, _, window, cx| this.open(id, window, cx)))
                                .child(num(format!("{}e {} · {}", k + 2, show(*kind, board.sport, e.value).0, fr_day(e.date, today))))
                        },
                    )))
                });
            col = col.child(row);
        }
        col
    }
}

#[cfg(test)]
mod tests {
    use super::{BOARDS, Kind, outdoor, podium, show};
    use crate::activity::{Activity, Sport};
    use crate::fit::{Derived, Effort};
    use std::collections::HashMap;

    fn a(id: i64, t: &str, key: &str, dist: f64, dur: f64, climb: f64) -> Activity {
        serde_json::from_value(serde_json::json!({
            "activityId": id, "startTimeLocal": t, "activityType": {"typeKey": key},
            "distance": dist, "duration": dur, "elevationGain": climb
        }))
        .unwrap()
    }

    #[test]
    fn podiums_by_kind_year_and_surface() {
        let acts = vec![
            a(1, "2026-03-01 07:00:00", "running", 10_000.0, 2700.0, 50.0),
            a(2, "2025-03-01 07:00:00", "running", 12_000.0, 3300.0, 80.0),
            a(3, "2026-04-01 07:00:00", "treadmill_running", 10_000.0, 2000.0, 0.0),
            a(4, "2026-05-01 07:00:00", "trail_running", 30_000.0, 14_000.0, 1800.0),
        ];
        let eff = |s: f32| Derived { line: vec![], efforts: vec![Effort { metres: 5000.0, seconds: s }] };
        let derived: HashMap<i64, Derived> = [(1, eff(1290.0)), (2, eff(1250.0)), (3, eff(900.0)), (4, eff(1700.0))].into();
        let road = &BOARDS[0];
        let five = podium(&acts, &derived, road, Kind::Effort(5000.0), None, 3);
        // The treadmill doesn't count; trail neither; fastest first.
        assert_eq!(five.iter().map(|e| e.id).collect::<Vec<_>>(), [2, 1]);
        assert_eq!(podium(&acts, &derived, road, Kind::Effort(5000.0), Some(2026), 3).iter().map(|e| e.id).collect::<Vec<_>>(), [1]);
        assert!(podium(&acts, &derived, road, Kind::Effort(5000.0), Some(1999), 3).is_empty());
        assert!(podium(&acts, &derived, road, Kind::Effort(42_195.0), None, 3).is_empty());
        assert_eq!(podium(&acts, &derived, road, Kind::Longest, None, 1)[0].id, 2);
        assert!(podium(&acts, &HashMap::new(), road, Kind::Effort(5000.0), None, 3).is_empty(), "no tracks yet");
        let trail = &BOARDS[1];
        assert_eq!(podium(&acts, &derived, trail, Kind::MostClimb, None, 3)[0].id, 4);
        assert!(!outdoor(&acts[2]) && outdoor(&acts[0]));
        assert_eq!(show(Kind::Effort(5000.0), Sport::Run, 1250.0), ("20:50".into(), "4:10 /km".into()));
        assert_eq!(show(Kind::Effort(40_000.0), Sport::Bike, 3600.0).1, "40,0 km/h");
        assert_eq!(show(Kind::Effort(400.0), Sport::Swim, 400.0).1, "1:40 /100m");
    }
}
