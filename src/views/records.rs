//! Records: the best efforts over the classic distances (from the tracks, like Strava's), the
//! longest outings, the biggest climbs. Road running and trail apart; a podium for each.

use crate::data::{Data, Snapshot};
use crate::logic::journal::short_name;
use crate::logic::activity::Sport;
use crate::logic::records::{BOARDS, Board, Kind, podium, show};
use crate::ui::format::{fr_day, split_unit};
use crate::ui::{heading, icon, pad, quantity, segmented, title};
use crate::views::{Page, Width, today, tracks_progress};
use chrono::Datelike;
use gpui_kit::assets::IconName;
use gpui_kit::{prelude::FluentBuilder, *};

pub struct RecordsView {
    data: Entity<Data>,
    snap: Snapshot,
    width: Width,
    /// `None`: all years.
    year: Option<i32>,
    /// The row showing its ten best: (board title, row).
    open: Option<(&'static str, usize)>,
    /// The sport of the board of every sport together; `None`: all of them.
    sport: Option<Sport>,
    _subscription: Subscription,
}

impl EventEmitter<Page> for RecordsView {}

impl RecordsView {
    pub fn new(data: Entity<Data>, width: Width, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&data, |this, data, cx| {
            this.snap = data.read(cx).snapshot();
            cx.notify();
        });
        RecordsView { snap: data.read(cx).snapshot(), data, width, year: None, open: None, sport: None, _subscription: sub }
    }
}

impl Render for RecordsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = self.width.get(window);
        let t = self.snap.theme;
        let p = pad(width);
        let mut years: Vec<i32> = self.snap.acts.iter().filter_map(|a| a.date()).map(|d| d.year()).collect();
        years.sort_unstable_by(|a, b| b.cmp(a));
        years.dedup();
        let year = self.year;
        let mut items = vec![("Toutes les années".into(), year.is_none())];
        items.extend(years.iter().map(|y| (y.to_string().into(), year == Some(*y))));
        let ys = years.clone();
        let filter = segmented("record-year", items, t, cx, move |this: &mut RecordsView, i, _, _| {
            this.year = if i == 0 { None } else { Some(ys[i - 1]) };
        });

        let boards: Vec<&Board> = BOARDS.iter().filter(|b| b.sport.map_or(!self.snap.acts.is_empty(), |s| self.snap.acts.iter().any(|a| a.sport() == s))).collect();
        let cols = if width - 2.0 * p < 900.0 { 1 } else if width - 2.0 * p < 1450.0 { 2 } else { 3 };
        let waiting = self.snap.acts.iter().filter(|a| boards.iter().any(|b| b.sport.is_none_or(|s| s == a.sport())) && !self.snap.derived.contains_key(&a.activity_id)).count();

        let mut grid = div().grid().grid_cols(cols).gap_x(px(48.)).gap_y(px(40.));
        for board in boards.iter() {
            grid = grid.child(self.board(board, cx));
        }

        crate::ui::page("records", width).track_scroll(self.width.scroll())
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
                        .child(tracks_progress(&self.data, cx)),
                )
            })
            .when(boards.is_empty(), |d| d.child(div().text_color(t.soft).child("Pas encore de sortie.")))
            .child(grid)
    }

}

/// Width of the name of a row, its chevron included.
const LABEL_W: f32 = 190.;

impl RecordsView {
    /// When a record was set: a day, or the week for a week's record.
    fn when(kind: Kind, date: chrono::NaiveDate) -> String {
        if kind.is_week() { format!("semaine du {}", fr_day(date, today())) } else { fr_day(date, today()) }
    }

    /// Which outing and when; a week's record is no outing's, so only the week.
    fn named(&self, kind: Kind, e: &crate::logic::records::Entry, with_sport: bool) -> String {
        match kind.is_week() {
            true => Self::when(kind, e.date),
            false => format!("{}{} · {}", self.snap.activity(e.id).map(short_name).unwrap_or_default(), self.sport_of(e.id, with_sport), Self::when(kind, e.date)),
        }
    }

    /// " · Vélo" when the board mixes sports and the outing's is not obvious from the board.
    fn sport_of(&self, id: i64, with_sport: bool) -> String {
        self.snap.activity(id).filter(|_| with_sport).map(|a| format!(" · {}", a.sport().label())).unwrap_or_default()
    }

    /// The board of every sport together, narrowed to the sport chosen.
    fn narrowed(&self, board: &Board) -> Board {
        Board { sport: board.sport.or(self.sport), ..*board }
    }

    /// "Tous les sports" and the sports there are outings of.
    fn sport_filter(&self, cx: &mut Context<Self>) -> Div {
        let present: Vec<Sport> = Sport::ALL.into_iter().filter(|s| self.snap.acts.iter().any(|a| a.sport() == *s)).collect();
        let mut items = vec![("Tous les sports".into(), self.sport.is_none())];
        items.extend(present.iter().map(|s| (s.label().into(), self.sport == Some(*s))));
        segmented("record-sport", items, self.snap.theme, cx, move |this: &mut RecordsView, i, _, _| {
            this.sport = if i == 0 { None } else { Some(present[i - 1]) };
            this.open = None;
        })
    }

    fn board(&self, board: &'static Board, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let title = board.title;
        let any_sport = board.sport.is_none();
        // Every sport together: each outing says its own.
        let mixed = any_sport && self.sport.is_none();
        let narrowed = self.narrowed(board);
        let board = &narrowed;
        let pace_sport = board.sport.unwrap_or(Sport::Other);
        let mut col = div().flex().flex_col().child(
            div().flex().flex_wrap().items_center().justify_between().gap(px(10.)).mb(px(8.)).child(heading(title)).when(any_sport, |d| d.child(self.sport_filter(cx))),
        );
        for (i, (label, kind)) in board.rows.iter().enumerate() {
            let open = self.open == Some((title, i));
            let top = podium(&self.snap.acts, &self.snap.derived, board, *kind, self.year, if open { 10 } else { 3 });
            let first = top.first().copied();
            let (value, pace) = first.map_or((String::new(), String::new()), |e| show(*kind, pace_sport, e.value));
            let row = div()
                .id(SharedString::from(format!("rec-{}-{i}", title)))
                .debug_selector(move || format!("rec-{}-{i}", title))
                .flex()
                .flex_col()
                .gap(px(3.))
                .py(px(10.))
                .px(px(10.))
                .mx(px(-10.))
                .rounded(px(6.))
                .relative()
                // The rule under the row lines up with the text; the hover reaches past it.
                .child(div().absolute().bottom_0().left(px(10.)).right(px(10.)).h(px(1.)).bg(t.border))
                .when(first.is_some(), |d| {
                    d.cursor_pointer().hover(|d| d.bg(t.panel)).on_click(cx.listener(move |this, _, _, cx| {
                        this.open = if this.open == Some((title, i)) { None } else { Some((title, i)) };
                        cx.notify();
                    }))
                })
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(12.))
                        .child(
                            div()
                                .w(px(LABEL_W))
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_color(t.soft)
                                .child(div().w(px(12.)).flex_none().when(first.is_some(), |d| d.child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }, t.dim, 12.))))
                                .child(div().min_w_0().truncate().child(*label)),
                        )
                        .when(first.is_none(), |d| d.child(div().text_size(px(13.)).text_color(t.dim).child("pas encore")))
                        .child(quantity(&value, t).text_size(px(18.)).font_weight(FontWeight::BOLD))
                        // Over 1 km the pace is the time itself: said once.
                        .when(split_unit(&pace).0 != value, |d| d.child(quantity(&pace, t).text_size(px(12.)).text_color(t.soft)))
                        .child(div().flex_1())
                        .when_some(first.filter(|_| !open), |d, e| {
                            d.child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(t.soft)
                                    .child(self.named(*kind, &e, mixed)),
                            )
                        }),
                )
                .when(top.len() > 1 && !open, |d| {
                    d.child(div().flex().gap(px(16.)).pl(px(LABEL_W + 12.)).text_size(px(12.)).text_color(t.dim).children(top.iter().skip(1).enumerate().map(
                        |(k, e)| {
                            let id = e.id;
                            div()
                                .id(SharedString::from(format!("pod-{}-{i}-{k}", title)))
                                .cursor_pointer()
                                .hover(|d| d.text_color(t.text))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.stop_propagation();
                                    cx.emit(Page::Activity(id))
                                }))
                                .child(format!("{}e {}{} · {}", k + 2, show(*kind, pace_sport, e.value).0, if kind.is_week() { String::new() } else { self.sport_of(e.id, mixed) }, Self::when(*kind, e.date)))
                        },
                    )))
                })
                .when(open, |d| d.child(self.ranking(board, i, *kind, &top, mixed, cx)));
            col = col.child(row);
        }
        col
    }

    /// The ten best of a row, each opening its outing; for a time over a distance, how far
    /// behind the best.
    fn ranking(&self, board: &Board, row: usize, kind: Kind, top: &[crate::logic::records::Entry], mixed: bool, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let pace_sport = board.sport.unwrap_or(Sport::Other);
        let title = board.title;
        let best = top.first().map_or(0.0, |e| e.value);
        div().flex().flex_col().mt(px(6.)).children(top.iter().enumerate().map(|(k, e)| {
            let id = e.id;
            let (value, pace) = show(kind, pace_sport, e.value);
            // Over 1 km the pace is the time itself: said once.
            let pace = if split_unit(&pace).0 != value { pace } else { String::new() };
            let gap = if kind.is_time_over_distance() && k > 0 { format!("+{}", crate::logic::activity::fmt_clock(e.value - best)) } else { String::new() };
            div()
                .id(SharedString::from(format!("rank-{}-{row}-{k}", title)))
                .debug_selector(move || format!("rank-{}-{row}-{k}", title))
                .flex()
                .items_baseline()
                .gap(px(10.))
                .py(px(4.))
                .px(px(4.))
                .rounded(px(4.))
                .text_size(px(12.))
                .cursor_pointer()
                .hover(|d| d.bg(t.border))
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.emit(Page::Activity(id))
                }))
                .child(quantity(&(k + 1).to_string(), t).w(px(18.)).flex_none().text_color(t.dim))
                .child(quantity(&value, t).w(px(70.)).flex_none().font_weight(if k == 0 { FontWeight::BOLD } else { FontWeight::NORMAL }))
                .child(quantity(&pace, t).w(px(70.)).flex_none().text_color(t.soft))
                .child(quantity(&gap, t).w(px(52.)).flex_none().text_color(t.dim))
                .child(div().min_w_0().flex_1().truncate().text_color(t.soft).child(self.named(kind, e, mixed)))
        }))
    }
}
