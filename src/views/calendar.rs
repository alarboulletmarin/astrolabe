//! The calendar: the outings in the cells of the days, by week, month or year. A cell shows what
//! was done that day, an outing at a time; a year shows how busy each day was.

use crate::data::{Data, Snapshot};
use crate::logic::activity::{Activity, Sport, fmt_duration, group};
use crate::logic::calendar::{self as cal, Mode};
use crate::logic::columns::{self, Column};
use crate::logic::journal::{name, sports_present};
use crate::ui::format::{calendar_title, month_name};
use crate::ui::theme::blend;
use crate::ui::{button, icon, num, page, pad, quantity, segmented, sport_icon, sport_tag, title};
use crate::views::{Page, Width, today};
use chrono::{Datelike, NaiveDate};
use gpui_kit::assets::IconName;
use gpui_kit::{prelude::FluentBuilder, *};

/// The least height of a day in the month; the weeks share the height of the window beyond it.
const CELL_H: f32 = 112.0;
/// Height around the month's weeks: margins, title and controls, the days' names.
const MONTH_CHROME: f32 = 210.0;
/// How many outings a day cell lists before saying "+ n".
const CHIPS: usize = 3;
/// Width of the weekly totals beside a month, where there is room for it.
const TOTALS_W: f32 = 92.0;
const DAYS: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];

pub struct CalendarView {
    snap: Snapshot,
    width: Width,
    mode: Mode,
    /// Any day of what is shown: the week, the month or the year holding it.
    anchor: NaiveDate,
    sport: Option<Sport>,
    _subscription: Subscription,
}

impl EventEmitter<Page> for CalendarView {}

impl CalendarView {
    pub fn new(data: Entity<Data>, width: Width, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&data, |this, data, cx| {
            this.snap = data.read(cx).snapshot();
            cx.notify();
        });
        CalendarView { snap: data.read(cx).snapshot(), width, mode: Mode::Month, anchor: today(), sport: None, _subscription: sub }
    }

    pub fn step(&mut self, n: i32, cx: &mut Context<Self>) {
        self.anchor = cal::shift(self.mode, self.anchor, n);
        cx.notify();
    }

    pub fn go_today(&mut self, cx: &mut Context<Self>) {
        self.anchor = today();
        cx.notify();
    }

    fn controls(&self, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let arrow = |id: &'static str, name: IconName, n: i32, cx: &mut Context<Self>| {
            div()
                .id(id)
                .p(px(5.))
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|d| d.bg(t.panel))
                .on_click(cx.listener(move |this, _, _, cx| this.step(n, cx)))
                .child(icon(name, t.soft, 16.))
        };
        let modes = [("Semaine", Mode::Week), ("Mois", Mode::Month), ("Année", Mode::Year)];
        let mode_filter = segmented("cal-mode", modes.iter().map(|(l, m)| ((*l).into(), self.mode == *m)).collect(), t, cx, move |this: &mut CalendarView, i, _, _| {
            this.mode = modes[i].1;
        });
        let present = sports_present(&self.snap.acts);
        let mut items = vec![("Tout".into(), self.sport.is_none())];
        items.extend(present.iter().map(|s| (s.label().into(), self.sport == Some(*s))));
        let sports = segmented("cal-sport", items, t, cx, move |this: &mut CalendarView, i, _, _| {
            this.sport = if i == 0 { None } else { Some(present[i - 1]) };
        });
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(title(calendar_title(self.mode, self.anchor)))
                    .child(div().flex().items_center().ml(px(8.)).child(arrow("cal-prev", IconName::ChevronLeft, -1, cx)).child(arrow("cal-next", IconName::ChevronRight, 1, cx)))
                    .child(button("cal-today", "Aujourd'hui", false, t).on_click(cx.listener(|this, _, _, cx| this.go_today(cx)))),
            )
            .child(div().flex().flex_wrap().items_center().gap(px(10.)).child(mode_filter).child(sports))
    }

    /// What a chip says about an outing: its distance, or its time where there is no distance.
    fn chip_text(a: &Activity) -> String {
        let d = columns::text(Column::Distance, a, None);
        if a.sport().has_distance() && !d.is_empty() { d } else { a.duration.filter(|d| *d > 0.0).map(fmt_duration).unwrap_or_default() }
    }

    fn month(&self, days: &std::collections::HashMap<NaiveDate, Vec<usize>>, inner: f32, height: f32, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let today = today();
        let totals = inner >= 760.0;
        let cell_w = (inner - if totals { TOTALS_W } else { 0.0 }) / 7.0;
        let compact = cell_w < 74.0;
        let weeks = cal::month_weeks(self.anchor);
        // The weeks fill the window rather than leave its lower half empty.
        let cell_h = ((height - MONTH_CHROME) / weeks.len() as f32).max(CELL_H);
        let head = div()
            .flex()
            .children(DAYS.map(|d| div().flex_1().min_w_0().px(px(5.)).text_size(px(11.)).text_color(t.dim).child(d)))
            .when(totals, |d| d.child(div().w(px(TOTALS_W)).flex_none().text_size(px(11.)).text_color(t.dim).pl(px(8.)).child("Semaine")));
        let rows = weeks.into_iter().map(|week| {
            let (mut count, mut secs) = (0usize, 0.0);
            let cells = week.map(|day| {
                let list = days.get(&day).map(Vec::as_slice).unwrap_or(&[]);
                for i in list {
                    count += 1;
                    secs += self.snap.acts[*i].duration.unwrap_or(0.0);
                }
                let out = day.month() != self.anchor.month();
                let is_today = day == today;
                div()
                    .id(("day", day.num_days_from_ce() as usize))
                    .flex_1()
                    .min_w_0()
                    .h(px(cell_h))
                    .p(px(5.))
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .border_t_1()
                    .border_color(t.border)
                    .overflow_hidden()
                    .child(
                        div().flex().child(
                            div()
                                .px(px(5.))
                                .rounded(px(9.))
                                .text_size(px(12.))
                                .when(is_today, |d| d.bg(t.accent).text_color(t.bg).font_weight(FontWeight::BOLD))
                                .when(!is_today, |d| d.text_color(if out { t.dim } else { t.soft }))
                                .child(day.day().to_string()),
                        ),
                    )
                    .child(self.chips(list, out, compact, cx))
            });
            div()
                .flex()
                .children(cells)
                .when(totals, |d| {
                    d.child(
                        div().w(px(TOTALS_W)).flex_none().h(px(cell_h)).border_t_1().border_color(t.border).pl(px(8.)).pt(px(7.)).text_size(px(12.)).text_color(t.soft).when(count > 0, |d| {
                            d.child(quantity(&fmt_duration(secs), t).text_color(t.text)).child(div().text_color(t.dim).child(format!("{} sortie{}", group(count as i64), if count > 1 { "s" } else { "" })))
                        }),
                    )
                })
        });
        div().flex().flex_col().child(head).children(rows)
    }

    /// The outings of a day as chips: a sport icon and a figure. A narrow cell keeps the icons only.
    fn chips(&self, list: &[usize], out: bool, compact: bool, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let acts = &self.snap.acts;
        let shown = if compact { list.len().min(CHIPS + 1) } else { list.len().min(CHIPS) };
        let chip = |i: usize, cx: &mut Context<Self>| {
            let a = &acts[i];
            let id = a.activity_id;
            div()
                .id(("chip", id as usize))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .px(px(4.))
                .py(px(1.))
                .rounded(px(4.))
                .bg(blend(t.accent, t.bg, if out { 0.05 } else { 0.1 }))
                .text_size(px(11.))
                .text_color(if out { t.dim } else { t.text })
                .cursor_pointer()
                .hover(|d| d.bg(blend(t.accent, t.bg, 0.22)))
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(Page::Activity(id))))
                .child(icon(sport_icon(a.sport()), if out { t.dim } else { t.soft }, 12.))
                .when(!compact, |d| d.child(div().min_w_0().truncate().child(Self::chip_text(a))))
        };
        let more = list.len() - shown;
        let mut col = div().flex().gap(px(3.));
        col = if compact { col.flex_wrap() } else { col.flex_col() };
        col.children(list[..shown].iter().map(|i| chip(*i, cx))).when(more > 0, |d| d.child(div().text_size(px(11.)).text_color(t.dim).child(format!("+ {more}"))))
    }

    fn week(&self, days: &std::collections::HashMap<NaiveDate, Vec<usize>>, inner: f32, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let today = today();
        let stacked = inner < 700.0;
        let columns = cal::week_days(self.anchor).map(|day| {
            let list = days.get(&day).map(Vec::as_slice).unwrap_or(&[]);
            let is_today = day == today;
            let cards = list.iter().map(|i| {
                let a = &self.snap.acts[*i];
                let id = a.activity_id;
                let facts: Vec<String> =
                    [Column::Distance, Column::Time, Column::Pace, Column::Elevation, Column::Hr].iter().map(|c| columns::text(*c, a, None)).filter(|s| !s.is_empty()).collect();
                div()
                    .id(("card", id as usize))
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .p(px(8.))
                    .rounded(px(6.))
                    .bg(blend(t.accent, t.bg, 0.1))
                    .cursor_pointer()
                    .hover(|d| d.bg(blend(t.accent, t.bg, 0.2)))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(Page::Activity(id))))
                    .child(sport_tag(a.sport(), t))
                    .child(div().font_weight(FontWeight::BOLD).truncate().child(name(a)))
                    .child(div().flex().flex_wrap().gap_x(px(10.)).children(facts.into_iter().map(|f| num(f).text_size(px(12.)).text_color(t.soft))))
            });
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(8.))
                .when(!stacked, |d| d.min_h(px(260.)))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(6.))
                        .pb(px(6.))
                        .border_b_1()
                        .border_color(t.border)
                        .child(div().text_size(px(12.)).text_color(t.dim).child(DAYS[day.weekday().num_days_from_monday() as usize]))
                        .child(
                            div()
                                .px(px(6.))
                                .rounded(px(9.))
                                .when(is_today, |d| d.bg(t.accent).text_color(t.bg).font_weight(FontWeight::BOLD))
                                .when(!is_today, |d| d.text_color(t.text))
                                .child(day.day().to_string()),
                        ),
                )
                .children(cards)
                .when(list.is_empty() && stacked, |d| d.child(div().text_size(px(12.)).text_color(t.dim).child("rien")))
        });
        div().flex().gap(px(14.)).when(stacked, |d| d.flex_col()).children(columns)
    }

    fn year(&self, days: &std::collections::HashMap<NaiveDate, Vec<usize>>, inner: f32, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let today = today();
        let year = self.anchor.year();
        let seconds = |d: &NaiveDate| days.get(d).map_or(0.0, |l| l.iter().map(|i| self.snap.acts[*i].duration.unwrap_or(0.0)).sum::<f64>());
        let busiest = days.keys().filter(|d| d.year() == year).map(seconds).fold(0.0_f64, f64::max);
        let (count, secs) = days.iter().filter(|(d, _)| d.year() == year).fold((0usize, 0.0), |(n, s), (d, l)| (n + l.len(), s + seconds(d)));
        let cols = if inner >= 940.0 { 4 } else if inner >= 640.0 { 3 } else if inner >= 400.0 { 2 } else { 1 };
        let gap = 24.0;
        let box_w = (inner - gap * (cols as f32 - 1.0)) / cols as f32;
        let day_size = ((box_w - 6.0 * 3.0) / 7.0).clamp(8.0, 20.0);
        let months = (1..=12u32).map(|m| {
            let first = NaiveDate::from_ymd_opt(year, m, 1).unwrap();
            let weeks = cal::month_weeks(first);
            let grid = weeks.into_iter().map(|week| {
                div().flex().gap(px(3.)).children(week.map(|day| {
                    let slot = div().size(px(day_size)).flex_none();
                    if day.month() != m {
                        return slot.into_any_element();
                    }
                    let v = seconds(&day);
                    let shade = match cal::level(v, busiest) {
                        0 => blend(t.border, t.bg, 0.7),
                        l => blend(t.accent, t.bg, 0.25 * l as f32),
                    };
                    slot.id(("ycell", day.num_days_from_ce() as usize))
                        .rounded(px(2.))
                        .bg(shade)
                        .when(day == today, |d| d.border_1().border_color(t.text))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.anchor = day;
                            this.mode = Mode::Month;
                            cx.notify();
                        }))
                        .into_any_element()
                }))
            });
            div()
                .w(px(box_w))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .id(("ymonth", m as usize))
                        .text_size(px(13.))
                        .font_weight(FontWeight::BOLD)
                        .cursor_pointer()
                        .hover(|d| d.text_color(t.accent))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.anchor = first;
                            this.mode = Mode::Month;
                            cx.notify();
                        }))
                        .child(month_name(m - 1)),
                )
                .children(grid)
        });
        div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(div().text_color(t.soft).child(if count == 0 {
                "Rien cette année-là.".to_string()
            } else {
                format!("{} sortie{} · {}", group(count as i64), if count > 1 { "s" } else { "" }, fmt_duration(secs))
            }))
            .child(div().flex().flex_wrap().gap(px(gap)).gap_y(px(28.)).children(months))
    }
}

impl Render for CalendarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = self.width.get(window);
        let inner = width - 2.0 * pad(width);
        let days = cal::by_day(&self.snap.acts, self.sport);
        let body = match self.mode {
            Mode::Month => self.month(&days, inner, window.viewport_size().height / px(1.), cx),
            Mode::Week => self.week(&days, inner, cx),
            Mode::Year => self.year(&days, inner, cx),
        };
        page("calendar", width).track_scroll(self.width.scroll()).child(self.controls(cx)).child(body).into_any_element()
    }
}
