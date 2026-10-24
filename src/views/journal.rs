//! The journal: every activity in a table that takes the whole width, grouped by week.
//! Columns fold away as the window narrows; a click opens the activity.

use crate::data::{Data, Snapshot};
use crate::logic::activity::{self as act, Sport};
use crate::logic::columns::{self, Column, DATE_W, GAP};
use crate::logic::journal::{Row, fold, name, rows, sports_present};
use crate::logic::map::{View, bbox};
use crate::ui::theme::blend;
use std::{collections::HashMap, time::Instant};
use crate::views::{Page, Width, today};
use crate::ui::format::{fr_day, fr_short, totals_line};
use crate::ui::{button, heading, icon, num, pad, segmented, sport_tag, title};
use gpui_kit::assets::IconName;
use chrono::Days;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{prelude::FluentBuilder, *};

const ROW_H: f32 = 36.;

pub struct JournalView {
    data: Entity<Data>,
    snap: Snapshot,
    width: Width,
    sport: Option<Sport>,
    search: Entity<InputState>,
    rows: Vec<Row>,
    /// The columns shown, as chosen in the menu and kept in the settings.
    columns: Vec<Column>,
    /// The biggest outing of each sport, for the bars.
    maxima: HashMap<Sport, f64>,
    menu: bool,
    /// When a press outside just closed the menu: the same press must not reopen it.
    closed: Option<Instant>,
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
        let columns = columns::parse(data.read(cx).settings.get("journal_columns").map(String::as_str).unwrap_or(""));
        let mut view = JournalView { data, snap, width, sport: None, search, rows: Vec::new(), columns, maxima: HashMap::new(), menu: false, closed: None, selected: None, list: UniformListScrollHandle::new(), _subscriptions: subs };
        view.refresh(cx);
        view
    }

    /// Rebuilds the rows after the data, the filter or the search changed.
    fn refresh(&mut self, cx: &App) {
        let needle = fold(&self.search.read(cx).value());
        self.rows = rows(&self.snap.acts, self.sport, &needle);
        self.maxima = columns::maxima(&self.snap.acts);
    }

    fn toggle_column(&mut self, column: Column, cx: &mut Context<Self>) {
        self.columns = columns::toggle(&self.columns, column);
        let value = columns::encode(&self.columns);
        self.data.update(cx, |d, cx| d.set_setting("journal_columns", value, cx));
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

    /// One line of the table: the date, the name taking what is left, then the columns that fit,
    /// each in its fixed width. `cells` come for the columns of `shown`, in order.
    fn line(&self, date: Div, name: Div, cells: Vec<(Column, Div)>) -> Div {
        let mut line = div().w_full().flex().items_center().gap(px(GAP)).child(date.w(px(DATE_W)).flex_none().whitespace_nowrap()).child(name.flex_1().min_w_0());
        for (c, cell) in cells {
            line = line.child(if c.right() { cell.w(px(c.width())).flex_none().flex().justify_end().whitespace_nowrap() } else { cell.w(px(c.width())).flex_none().flex() });
        }
        line
    }

    /// The cell of a column for an activity.
    fn cell(&self, c: Column, a: &act::Activity, rank: Option<(usize, usize)>) -> Div {
        let t = self.snap.theme;
        match c {
            Column::Sport => sport_tag(a.sport(), t),
            Column::Bar => {
                let share = columns::share(a, &self.maxima);
                div().h(px(6.)).rounded(px(3.)).bg(t.border).child(div().h_full().w(relative(share)).rounded(px(3.)).bg(blend(t.accent, t.bg, 0.7)))
            }
            Column::Trace => {
                let line = self.snap.derived.get(&a.activity_id).map(|d| d.line.clone()).filter(|l| l.len() >= 2);
                let color = t.soft;
                div().h(px(24.)).child(
                    canvas(
                        |_, _, _| {},
                        move |b, _, window, _| {
                            let Some(line) = &line else { return };
                            let (w, h) = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
                            let Some(bb) = bbox(std::iter::once(line.as_slice()), 0.0) else { return };
                            let view = View::fit(bb, w, h, 2.0);
                            let at = |p: &[f32; 2]| {
                                let (x, y) = view.screen(p[0] as f64, p[1] as f64, w, h);
                                point(b.origin.x + px(x as f32), b.origin.y + px(y as f32))
                            };
                            let mut path = PathBuilder::stroke(px(1.2));
                            path.move_to(at(&line[0]));
                            line[1..].iter().for_each(|p| path.line_to(at(p)));
                            if let Ok(p) = path.build() {
                                window.paint_path(p, color);
                            }
                        },
                    )
                    .size_full(),
                )
            }
            _ => {
                let text = num(columns::text(c, a, rank));
                if matches!(c, Column::Distance | Column::Time) { text } else { text.text_color(t.soft) }
            }
        }
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
                let shown = columns::visible(&self.columns, width);
                let cells = shown.iter().map(|c| (*c, self.cell(*c, a, (*c == Column::Rank).then(|| act::rank(&self.snap.acts, a))))).collect();
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
                    .child(self.line(
                        num(format!("{}{time}", a.date().map(fr_short).unwrap_or_default())).text_size(px(12.)).text_color(t.soft),
                        div().truncate().child(name(a)),
                        cells,
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
                        .child(self.sport_filter("journal-sport", cx))
                        .child(self.columns_button(window.viewport_size().height / px(1.) - 230., cx)),
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
                        .child(self.line(
                            div().child("Date"),
                            div().child("Activité"),
                            columns::visible(&self.columns, inner).into_iter().map(|c| (c, div().child(c.label()))).collect(),
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
    /// The "Colonnes" button and, while it is open, the list of columns to show or hide.
    fn columns_button(&mut self, room: f32, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let open = self.menu;
        let rows = columns::ALL.iter().copied().map(|c| {
            let on = self.columns.contains(&c);
            div()
                .id(("column", c as usize))
                .flex()
                .items_start()
                .gap(px(8.))
                .px(px(8.))
                .py(px(5.))
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|d| d.bg(t.panel))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_column(c, cx);
                    cx.notify();
                }))
                .child(div().w(px(14.)).flex_none().mt(px(2.)).child(if on { icon(IconName::Check, t.accent, 14.).into_any_element() } else { div().into_any_element() }))
                .child(div().flex().flex_col().child(div().child(c.label())).child(div().text_size(px(12.)).text_color(t.dim).child(c.hint())))
        });
        div()
            .relative()
            .child(
                div()
                    .id("journal-columns")
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(9.))
                    .py(px(3.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(t.border)
                    .text_size(px(12.))
                    .text_color(t.soft)
                    .cursor_pointer()
                    .when(open, |d| d.bg(t.panel))
                    .hover(|d| d.text_color(t.text))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let just_closed = this.closed.is_some_and(|at| at.elapsed().as_millis() < 250);
                        this.menu = !this.menu && !just_closed;
                        cx.notify();
                    }))
                    .child("Colonnes")
                    .child(icon(IconName::ChevronDown, t.soft, 13.)),
            )
            .when(open, |d| {
                d.child(
                    deferred(
                        div()
                            .id("columns-panel")
                            .absolute()
                            .top(px(30.))
                            .right_0()
                            .w(px(340.))
                            .max_h(px(room.clamp(140., 460.)))
                            .overflow_y_scroll()
                            .p(px(4.))
                            .rounded(px(8.))
                            .bg(t.bg)
                            .shadow_lg()
                            .text_size(px(13.))
                            .flex()
                            .flex_col()
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.menu = false;
                                this.closed = Some(Instant::now());
                                cx.notify();
                            }))
                            .children(rows)
                            .child(div().px(px(8.)).py(px(6.)).text_size(px(12.)).text_color(t.dim).child("Une colonne qui ne tient pas dans la fenêtre se replie : les moins utiles d'abord. Agrandis la fenêtre pour les revoir.")),
                    )
                    .with_priority(1),
                )
            })
    }

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
