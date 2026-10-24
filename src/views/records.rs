//! Records: the best efforts over the classic distances (from the tracks, like Strava's), the
//! longest outings, the biggest climbs. Road running and trail apart; a podium for each.

use crate::data::{Data, Snapshot};
use crate::logic::journal::name;
use crate::logic::records::{BOARDS, Board, podium, show};
use crate::ui::format::fr_day;
use crate::ui::{heading, num, pad, segmented, title};
use crate::views::{Page, Width, today, tracks_progress};
use chrono::Datelike;
use gpui_kit::{prelude::FluentBuilder, *};

pub struct RecordsView {
    data: Entity<Data>,
    snap: Snapshot,
    width: Width,
    /// `None`: all years.
    year: Option<i32>,
    _subscription: Subscription,
}

impl EventEmitter<Page> for RecordsView {}

impl RecordsView {
    pub fn new(data: Entity<Data>, width: Width, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&data, |this, data, cx| {
            this.snap = data.read(cx).snapshot();
            cx.notify();
        });
        RecordsView { snap: data.read(cx).snapshot(), data, width, year: None, _subscription: sub }
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

        let boards: Vec<&Board> = BOARDS.iter().filter(|b| self.snap.acts.iter().any(|a| a.sport() == b.sport)).collect();
        let cols = if width - 2.0 * p < 900.0 { 1 } else if width - 2.0 * p < 1450.0 { 2 } else { 3 };
        let waiting = self.snap.acts.iter().filter(|a| boards.iter().any(|b| b.sport == a.sport()) && !self.snap.derived.contains_key(&a.activity_id)).count();

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
                        .child(tracks_progress(&self.data, cx)),
                )
            })
            .when(boards.is_empty(), |d| d.child(div().text_color(t.soft).child("Pas encore d'activité.")))
            .child(grid)
    }

}

impl RecordsView {
    fn board(&self, board: &'static Board, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let mut col = div().flex().flex_col().child(heading(board.title).mb(px(8.)));
        for (i, (label, kind)) in board.rows.iter().enumerate() {
            let top = podium(&self.snap.acts, &self.snap.derived, board, *kind, self.year, 3);
            let today = today();
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
                    d.cursor_pointer().hover(|d| d.bg(t.panel)).on_click(cx.listener(move |_, _, _, cx| cx.emit(Page::Activity(e.id))))
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
                                    .child(format!("{} · {}", self.snap.activity(e.id).map(name).unwrap_or_default(), fr_day(e.date, today))),
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
                                .on_click(cx.listener(move |_, _, _, cx| cx.emit(Page::Activity(id))))
                                .child(num(format!("{}e {} · {}", k + 2, show(*kind, board.sport, e.value).0, fr_day(e.date, today))))
                        },
                    )))
                });
            col = col.child(row);
        }
        col
    }
}
