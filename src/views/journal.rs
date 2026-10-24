//! The journal: every activity in a table that takes the whole width, grouped by week.
//! Columns fold away as the window narrows; a click opens the activity.

use crate::data::{Data, Snapshot};
use crate::logic::activity::{self as act, Sport, fmt_clock, fmt_km, fmt_m};
use crate::logic::journal::{Row, fold, name, rows, sports_present};
use crate::views::{Page, Width, today};
use crate::ui::format::{fr_day, fr_short, totals_line};
use crate::ui::{button, heading, num, pad, segmented, sport_tag, title};
use chrono::Days;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{prelude::FluentBuilder, *};

const ROW_H: f32 = 36.;

/// Which optional columns fit: (sport, pace, elevation, heart rate).
fn columns_for(width: f32) -> (bool, bool, bool, bool) {
    (width >= 700.0, width >= 860.0, width >= 1000.0, width >= 1140.0)
}

pub struct JournalView {
    data: Entity<Data>,
    snap: Snapshot,
    width: Width,
    sport: Option<Sport>,
    search: Entity<InputState>,
    rows: Vec<Row>,
    pub selected: Option<i64>,
    list: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Page> for JournalView {}

impl JournalView {
    pub fn new(data: Entity<Data>, width: Width, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Chercher une activité"));
        let subs = vec![
            cx.observe(&data, |this, data, cx| {
                this.snap = data.read(cx).snapshot();
                this.refresh(cx);
                cx.notify();
            }),
            cx.subscribe(&search, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.refresh(cx);
                    cx.notify();
                }
            }),
        ];
        let snap = data.read(cx).snapshot();
        let mut view = JournalView { data, snap, width, sport: None, search, rows: Vec::new(), selected: None, list: UniformListScrollHandle::new(), _subscriptions: subs };
        view.refresh(cx);
        view
    }

    /// Rebuilds the rows after the data, the filter or the search changed.
    fn refresh(&mut self, cx: &App) {
        let needle = fold(&self.search.read(cx).value());
        self.rows = rows(&self.snap.acts, self.sport, &needle);
    }

    fn set_sport(&mut self, sport: Option<Sport>, cx: &mut Context<Self>) {
        self.sport = sport;
        self.refresh(cx);
    }

    /// Selects an activity and brings its row into sight.
    pub fn select(&mut self, id: i64) {
        self.selected = Some(id);
        if let Some(row) = self.snap.index_of(id).and_then(|ix| self.rows.iter().position(|r| *r == Row::Act(ix))) {
            self.list.scroll_to_item(row, ScrollStrategy::Center);
        }
    }

    pub fn select_step(&mut self, step: isize, cx: &mut Context<Self>) {
        let listed: Vec<usize> = self.rows.iter().filter_map(|r| if let Row::Act(i) = r { Some(*i) } else { None }).collect();
        let at = self.selected.and_then(|id| listed.iter().position(|i| self.snap.acts[*i].activity_id == id));
        let next = match at {
            Some(p) => p.saturating_add_signed(step).min(listed.len().saturating_sub(1)),
            None => 0,
        };
        if let Some(i) = listed.get(next) {
            self.select(self.snap.acts[*i].activity_id);
            cx.notify();
        }
    }

    fn sport_filter(&self, id: &'static str, cx: &mut Context<Self>) -> Div {
        let present = sports_present(&self.snap.acts);
        let mut items = vec![("Tout".into(), self.sport.is_none())];
        items.extend(present.iter().map(|s| (s.label().into(), self.sport == Some(*s))));
        segmented(id, items, self.snap.theme, cx, move |this: &mut JournalView, i, _, cx| {
            this.set_sport(if i == 0 { None } else { Some(present[i - 1]) }, cx);
        })
    }

    /// One table row: date, sport, name stretch; numbers right-aligned in fixed columns.
    fn cells(&self, width: f32, cells: [Div; 8]) -> Div {
        let (sport_on, pace_on, elev_on, hr_on) = columns_for(width);
        let [date, sport, name, dist, time, pace, elev, hr] = cells;
        let right = |d: Div, w: f32| d.w(px(w)).flex_none().flex().justify_end().whitespace_nowrap();
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(date.w(px(152.)).flex_none().whitespace_nowrap())
            .when(sport_on, |d| d.child(sport.w(px(110.)).flex_none()))
            .child(name.flex_1().min_w_0())
            .child(right(dist, 86.))
            .child(right(time, 78.))
            .when(pace_on, |d| d.child(right(pace, 96.)))
            .when(elev_on, |d| d.child(right(elev, 72.)))
            .when(hr_on, |d| d.child(right(hr, 72.)))
    }

    fn row(&self, ix: usize, row: Row, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let t = self.snap.theme;
        match row {
            Row::Week(monday) => {
                let w = act::totals(&self.snap.acts, monday, monday + Days::new(6), self.sport);
                div()
                    .id(("row", ix))
                    .w_full()
                    .h(px(ROW_H))
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(px(12.))
                    .pb(px(6.))
                    .px(px(10.))
                    .border_b_1()
                    .border_color(t.border)
                    .text_size(px(12.))
                    .child(div().font_weight(FontWeight::BOLD).whitespace_nowrap().child(format!("Semaine du {}", fr_day(monday, today()))))
                    .child(num(totals_line(&w)).text_color(t.soft).truncate())
                    .into_any_element()
            }
            Row::Act(i) => {
                let a = &self.snap.acts[i];
                let on = self.selected == Some(a.activity_id);
                let id = a.activity_id;
                let time = a.start().map(|d| d.format(" · %H:%M").to_string()).unwrap_or_default();
                let opt = |v: Option<String>| num(v.unwrap_or_default());
                div()
                    .id(("row", ix))
                    .w_full()
                    .h(px(ROW_H))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(t.selection))
                    .when(!on, |d| d.hover(|d| d.bg(t.panel)))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(Page::Activity(id))))
                    .child(self.cells(
                        width,
                        [
                            num(format!("{}{time}", a.date().map(fr_short).unwrap_or_default())).text_size(px(12.)).text_color(t.soft),
                            div().flex().child(sport_tag(a.sport(), t)),
                            div().truncate().child(name(a)),
                            opt(a.distance.filter(|d| *d > 0.0).map(fmt_km)),
                            opt(a.duration.map(fmt_clock)),
                            opt(a.speed_label()).text_color(t.soft),
                            opt(a.elevation_gain.filter(|e| *e >= 1.0).map(fmt_m)).text_color(t.soft),
                            opt(a.average_hr.map(|h| format!("{h:.0} bpm"))).text_color(t.soft),
                        ],
                    ))
                    .into_any_element()
            }
        }
    }

}

impl Render for JournalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = self.width.get(window);
        let t = self.snap.theme;
        let p = pad(width);
        let today = today();
        let week = act::totals(&self.snap.acts, act::week_start(today), today, None);
        let streak = act::streaks(&self.snap.acts, today);
        let inner = width - 2.0 * p;

        let header = div()
            .flex()
            .flex_wrap()
            .items_end()
            .justify_between()
            .gap(px(14.))
            .px(px(p))
            .pt(px(crate::ui::top()))
            .pb(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(title("Journal"))
                    .child(div().text_color(t.soft).child(if week.count == 0 {
                        "Rien encore cette semaine.".to_string()
                    } else {
                        format!("Cette semaine : {}", totals_line(&week))
                    }))
                    .when(streak.weeks > 0, |d| {
                        let s = |n: u32| if n > 1 { "s" } else { "" };
                        let days = if streak.days > 0 { format!(" · {} jour{} d'affilée", streak.days, s(streak.days)) } else { String::new() };
                        d.child(div().text_color(t.soft).child(format!(
                            "Série : {} semaine{} d'affilée{days} · record {} jour{}",
                            streak.weeks,
                            s(streak.weeks),
                            streak.longest_days,
                            s(streak.longest_days)
                        )))
                    }),
            )
            .when(!self.snap.acts.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(10.))
                        .child(div().w(px(220.)).child(Input::new(&self.search).cleanable(true)))
                        .child(self.sport_filter("journal-sport", cx)),
                )
            });

        let body = if self.snap.acts.is_empty() {
            self.journal_empty(p, cx).into_any_element()
        } else if self.rows.is_empty() {
            div().px(px(p + 10.)).py(px(20.)).text_color(t.soft).child("Aucune activité ne correspond.").into_any_element()
        } else {
            let count = self.rows.len();
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .px(px(p - 10.))
                .child(
                    div()
                        .px(px(10.))
                        .pb(px(4.))
                        .text_size(px(12.))
                        .text_color(t.dim)
                        .child(self.cells(
                            inner,
                            ["Date", "Sport", "Activité", "Distance", "Temps", "Allure", "D+", "FC"].map(|h| div().child(h)),
                        )),
                )
                .child(
                    uniform_list(
                        "journal-rows",
                        count,
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            range.map(|ix| this.row(ix, this.rows[ix], inner, cx)).collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.list)
                    .flex_1()
                    .pb(px(36.)),
                )
                .into_any_element()
        };

        div().size_full().flex().flex_col().child(header).child(body)
    }

}

impl JournalView {
    fn journal_empty(&self, p: f32, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let no_key = self.data.read(cx).key.is_none() && self.data.read(cx).key_loaded;
        div()
            .px(px(p))
            .py(px(12.))
            .max_w(px(620.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(heading("Rien ici pour l'instant."))
            .child(div().text_color(t.soft).child(if no_key {
                "Tes activités arrivent de Garmin Connect. Ajoute ta clé API personnelle dans les réglages : la première synchronisation récupère tout ton historique."
            } else {
                "La synchronisation n'a encore rien trouvé. Une activité enregistrée sur ta montre apparaîtra ici après la prochaine."
            }))
            .when(no_key, |d| {
                d.child(
                    div().flex().child(
                        button("empty-settings", "Ajouter ma clé", true, t)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Page::Settings))),
                    ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's glob brings its own `test` macro.
    use super::columns_for;

    #[test]
    fn columns_fold_as_the_width_shrinks() {
        assert_eq!(columns_for(1400.0), (true, true, true, true));
        assert_eq!(columns_for(900.0), (true, true, false, false));
        assert_eq!(columns_for(500.0), (false, false, false, false));
    }
}
