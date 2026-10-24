//! Statistics, set to the day: a measure, a grouping, any range, any sports, bars or a running
//! total, compared with the period before or the year before. A click on a group lists it.

use crate::data::{Data, Snapshot};
use crate::logic::activity::{self as act, Activity, Group, Measure, Point, Query, Sport, fmt_clock, fmt_duration, fmt_km, fmt_m, week_start};
use crate::logic::journal::{name, sports_present};
use crate::ui::format::{fr_day, fr_short, month_short, totals_line};
use crate::ui::theme::blend;
use crate::ui::{Plots, heading, icon, num, pad, page, segmented, sport_tag, title};
use crate::views::{Page, Width, today};
use gpui_kit::assets::IconName;
use chrono::{Datelike, Days, NaiveDate};
use gpui_kit::component::date_picker::{DatePicker, DatePickerEvent, DatePickerState};
use gpui_kit::{prelude::FluentBuilder, *};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq)]
enum Chart {
    Bars,
    Cumul,
}

#[derive(Clone, Copy, PartialEq)]
enum Compare {
    None,
    Previous,
    Year,
}

/// The menus of the stats sentence.
#[derive(Clone, Copy, PartialEq)]
enum Menu {
    Measure,
    Group,
    Period,
    Sports,
    Compare,
}

/// The presets of the stats range.
fn presets(today: NaiveDate) -> [(&'static str, NaiveDate, NaiveDate); 6] {
    let months = |n: u32| today.checked_sub_months(chrono::Months::new(n)).unwrap() + Days::new(1);
    [
        ("Les 4 dernières semaines", week_start(today) - Days::new(21), today),
        ("Les 3 derniers mois", months(3), today),
        ("Les 6 derniers mois", months(6), today),
        ("Les 12 derniers mois", months(12), today),
        ("Cette année", today.with_ordinal(1).unwrap(), today),
        ("Depuis le début", NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(), today),
    ]
}

const GUTTER: f32 = 64.;
const PLOT_H: f32 = 280.;

/// "12 oct.", "oct. 2026", "2026": a group as its axis reads it.
pub fn group_label(g: Group, d: NaiveDate) -> String {
    match g {
        Group::Day | Group::Week => format!("{} {}", d.day(), month_short(d.month0())),
        Group::Month => format!("{} {}", month_short(d.month0()), d.year()),
        Group::Year => d.year().to_string(),
    }
}

/// A period as one reads it: "2026" for a calendar year (or its start, up to today),
/// otherwise both dates with their years.
pub fn period_label(from: NaiveDate, to: NaiveDate) -> String {
    if from.ordinal() == 1 && from.year() == to.year() {
        from.year().to_string()
    } else {
        format!("{} → {}", fr_day(from, NaiveDate::MIN), fr_day(to, NaiveDate::MIN))
    }
}

fn cumul_title(m: Measure) -> String {
    match m {
        Measure::Time => "Temps cumulé jour après jour".into(),
        Measure::Distance => "Distance cumulée jour après jour".into(),
        Measure::Elevation => "Dénivelé cumulé jour après jour".into(),
        Measure::Count => "Séances cumulées jour après jour".into(),
        m => format!("{}, moyenne depuis le début de la période", m.label()),
    }
}

/// What one activity weighs in the measure, for the lists.
fn own_value(m: Measure, a: &Activity, bike: bool) -> String {
    match m {
        Measure::Time => a.duration.map(fmt_duration).unwrap_or_default(),
        Measure::Distance => a.distance.filter(|d| *d > 0.0).map(fmt_km).unwrap_or_default(),
        Measure::Elevation => a.elevation_gain.map(fmt_m).unwrap_or_default(),
        Measure::Count => a.duration.map(fmt_clock).unwrap_or_default(),
        Measure::Pace => match (a.distance, a.duration) {
            (Some(d), Some(t)) if d > 0.0 => Measure::Pace.fmt(t / d * 1000.0, bike),
            _ => String::new(),
        },
        Measure::Hr => a.average_hr.map(|h| format!("{h:.0} bpm")).unwrap_or_default(),
    }
}

/// Round steps for the y axis: 4 lines at most, on 1, 2, 2.5 or 5 times a power of ten.
pub fn ticks(max: f64) -> Vec<f64> {
    if max <= 0.0 || !max.is_finite() {
        return vec![];
    }
    let raw = max / 4.0;
    let p = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|k| k * p).find(|s| *s >= raw).unwrap();
    (1..).map(|i| i as f64 * step).take_while(|v| *v <= max * 1.0001).collect()
}

pub struct StatsView {
    snap: Snapshot,
    width: Width,
    query: Query,
    chart: Chart,
    compare: Compare,
    /// Index of the preset in use, `None` once the range was set by hand.
    preset: Option<usize>,
    hover: Option<usize>,
    pick: Option<usize>,
    cal_hover: Option<usize>,
    range: Entity<DatePickerState>,
    /// Dates picked by hand: the sentence shows the date picker instead of the period word.
    custom: bool,
    menu: Option<Menu>,
    /// Which menu a press outside just closed, and when.
    closed: Option<(Menu, Instant)>,
    plots: Plots,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Page> for StatsView {}

impl StatsView {
    pub fn new(data: Entity<Data>, width: Width, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let today = today();
        let range = cx.new(|cx| {
            let mut s = DatePickerState::range(window, cx);
            s.set_date((today.with_ordinal(1).unwrap(), today), window, cx);
            s
        });
        let subs = vec![
            cx.observe(&data, |this, data, cx| {
                this.snap = data.read(cx).snapshot();
                cx.notify();
            }),
            cx.subscribe(&range, |this, _, e: &DatePickerEvent, cx| {
                let DatePickerEvent::Change(v) = e;
                if let gpui_kit::component::calendar::Date::Range(Some(a), Some(b)) = v.date() {
                    this.query.from = a.min(b);
                    this.query.to = a.max(b);
                    this.preset = None;
                    this.pick = None;
                    cx.notify();
                }
            }),
        ];
        StatsView {
            snap: data.read(cx).snapshot(),
            width,
            query: Query { measure: Measure::Distance, group: Group::Week, from: today.with_ordinal(1).unwrap(), to: today, sports: vec![] },
            chart: Chart::Bars,
            compare: Compare::None,
            preset: Some(4),
            hover: None,
            pick: None,
            cal_hover: None,
            range,
            custom: false,
            menu: None,
            closed: None,
            plots: Plots::default(),
            _subscriptions: subs,
        }
    }

    fn bike_only(&self) -> bool {
        self.query.sports == [Sport::Bike]
    }

    fn compared(&self) -> Option<Query> {
        match self.compare {
            Compare::None => None,
            Compare::Previous => Some(self.query.shifted(false)),
            Compare::Year => Some(self.query.shifted(true)),
        }
    }

    fn set_range(&mut self, from: NaiveDate, to: NaiveDate, preset: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        // "Tout" starts at the first activity, not in 2000.
        let first = self.snap.acts.iter().filter_map(|a| a.date()).min().unwrap_or(from);
        let from = from.max(first.min(to));
        self.query.from = from;
        self.query.to = to;
        self.preset = preset;
        self.pick = None;
        self.range.update(cx, |s, cx| s.set_date((from, to), window, cx));
        // A long range by day is a wall of slivers: pick a sensible grouping.
        let days = (to - from).num_days();
        let g = &mut self.query.group;
        if days > 800 && matches!(g, Group::Day | Group::Week) {
            *g = Group::Month;
        } else if days > 120 && *g == Group::Day {
            *g = Group::Week;
        }
    }

}

impl Render for StatsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = self.width.get(window);
        let t = self.snap.theme;
        if self.snap.acts.is_empty() {
            return page("stats", width).track_scroll(self.width.scroll())
                .child(title("Statistiques"))
                .child(div().text_color(t.soft).child("Les graphiques se dessinent dès la première synchronisation."))
                .into_any_element();
        }
        let q = self.query.clone();
        let mut tot = act::Totals::default();
        for a in self.snap.acts.iter().filter(|a| q.keeps(a)) {
            tot.count += 1;
            tot.distance += a.distance.unwrap_or(0.0);
            tot.duration += a.duration.unwrap_or(0.0);
            tot.elevation += a.elevation_gain.unwrap_or(0.0);
        }
        let chart_tabs = segmented(
            "chart",
            vec![("Barres".into(), self.chart == Chart::Bars), ("Cumul".into(), self.chart == Chart::Cumul)],
            t,
            cx,
            |this: &mut StatsView, i, _, _| {
                this.chart = if i == 0 { Chart::Bars } else { Chart::Cumul };
                this.pick = None;
            },
        );
        let header = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().flex().flex_wrap().items_center().justify_between().gap(px(12.)).child(title("Statistiques")).child(chart_tabs))
            .child(self.sentence(cx))
            .child(div().text_color(t.soft).child(if tot.count == 0 { "Aucune activité sur cette période.".into() } else { totals_line(&tot) }));

        // The plots' width: known before their first frame, so labels and cells fit at once.
        let inner = width - 2.0 * pad(width);
        let chart = match self.chart {
            Chart::Bars => self.bars(inner - GUTTER, cx).into_any_element(),
            Chart::Cumul => self.cumul(inner - GUTTER, cx).into_any_element(),
        };
        let two = inner >= 1050.0;
        let below = div()
            .grid()
            .grid_cols(if two { 2 } else { 1 })
            .gap_x(px(48.))
            .gap_y(px(32.))
            .child(self.selection(cx))
            .child(self.by_sport());

        page("stats", width).track_scroll(self.width.scroll()).gap(px(36.)).child(header).child(chart).child(below).child(self.calendar(inner, cx)).into_any_element()
    }

}

impl StatsView {
    /// The settings as one sentence: "Distance par semaine · cette année · tous les sports ·
    /// sans comparaison". Each word opens its menu; the rest stays out of sight.
    fn sentence(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let q = self.query.clone();
        let today = today();
        let period = match self.preset {
            Some(i) => presets(today)[i].0.to_lowercase(),
            None => format!("du {} au {}", fr_day(q.from, NaiveDate::MIN), fr_day(q.to, NaiveDate::MIN)),
        };
        let sports = match q.sports.as_slice() {
            [] => "tous les sports".to_string(),
            [s] => s.label().to_lowercase(),
            [a, b] => format!("{} et {}", a.label().to_lowercase(), b.label().to_lowercase()),
            more => format!("{} sports", more.len()),
        };
        let compare = match self.compare {
            Compare::None => "sans comparaison",
            Compare::Previous => "comparé à la période d'avant",
            Compare::Year => "comparé à un an avant",
        };
        let dot = || div().text_color(t.dim).child("·");
        let step = |id: &'static str, name: IconName, back: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .p(px(3.))
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|d| d.bg(t.panel))
                .on_click(cx.listener(move |this, _, window, cx| {
                    let q = &this.query;
                    let s = if back { q.shifted(false) } else { Query { from: q.to + Days::new(1), to: q.to + Days::new(q.days() as u64), ..q.clone() } };
                    this.set_range(s.from, s.to, None, window, cx);
                }))
                .child(icon(name, t.soft, 14.))
        };
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .text_size(px(16.))
            .child(self.word("tok-measure", Menu::Measure, q.measure.label().into(), cx))
            .child(self.word("tok-group", Menu::Group, format!("par {}", q.group.label().to_lowercase()), cx))
            .child(dot())
            .child(step("prev-range", IconName::ChevronLeft, true, cx))
            .child(if self.custom {
                div().w(px(260.)).text_size(px(13.)).child(DatePicker::new(&self.range).number_of_months(2)).into_any_element()
            } else {
                self.word("tok-period", Menu::Period, period, cx).into_any_element()
            })
            .child(step("next-range", IconName::ChevronRight, false, cx))
            .child(dot())
            .child(self.word("tok-sports", Menu::Sports, sports, cx))
            .child(dot())
            .child(self.word("tok-compare", Menu::Compare, compare.into(), cx))
    }

    /// A word of the sentence: bold, a small chevron; its menu floats under it while open.
    fn word(&mut self, id: &'static str, menu: Menu, label: String, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let open = self.menu == Some(menu);
        div()
            .relative()
            .child(
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(6.))
                    .py(px(2.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .font_weight(FontWeight::BOLD)
                    .when(open, |d| d.bg(t.panel))
                    .hover(|d| d.bg(t.panel))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // The press that closed this menu (outside it) must not reopen it.
                        let just_closed = this.closed.is_some_and(|(m, at)| m == menu && at.elapsed().as_millis() < 250);
                        this.menu = if this.menu == Some(menu) || just_closed { None } else { Some(menu) };
                        cx.notify();
                    }))
                    .child(label)
                    .child(icon(IconName::ChevronDown, t.soft, 13.)),
            )
            .when(open, |d| d.child(deferred(self.menu_panel(menu, cx)).with_priority(1)))
    }

    fn menu_panel(&mut self, menu: Menu, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = self.snap.theme;
        let q = self.query.clone();
        let today = today();
        // (label, chosen, action); `keep`: the menu stays open (several sports at once).
        let mut rows: Vec<(String, bool, Box<dyn Fn(&mut StatsView, &mut Window, &mut Context<StatsView>)>)> = Vec::new();
        let mut keep = false;
        match menu {
            Menu::Measure => {
                for m in Measure::ALL {
                    rows.push((m.label().into(), m == q.measure, Box::new(move |h, _, _| h.query.measure = m)));
                }
            }
            Menu::Group => {
                for g in Group::ALL {
                    rows.push((format!("par {}", g.label().to_lowercase()), g == q.group, Box::new(move |h, _, _| {
                        h.query.group = g;
                        h.pick = None;
                    })));
                }
            }
            Menu::Period => {
                for (i, (label, from, to)) in presets(today).into_iter().enumerate() {
                    rows.push((label.into(), self.preset == Some(i) && !self.custom, Box::new(move |h, w, cx| {
                        h.custom = false;
                        h.set_range(from, to, Some(i), w, cx);
                    })));
                }
                rows.push(("Choisir les dates…".into(), self.custom, Box::new(|h, _, _| h.custom = true)));
            }
            Menu::Sports => {
                keep = true;
                rows.push(("Tous les sports".into(), q.sports.is_empty(), Box::new(|h, _, _| h.query.sports.clear())));
                for s in sports_present(&self.snap.acts) {
                    rows.push((s.label().into(), q.sports.contains(&s), Box::new(move |h, _, _| {
                        let list = &mut h.query.sports;
                        match list.iter().position(|x| *x == s) {
                            Some(p) => {
                                list.remove(p);
                            }
                            None => list.push(s),
                        }
                        h.pick = None;
                    })));
                }
            }
            Menu::Compare => {
                for (label, c) in [("Sans comparaison", Compare::None), ("Avec la période d'avant", Compare::Previous), ("Avec la même période un an avant", Compare::Year)] {
                    rows.push((label.into(), self.compare == c, Box::new(move |h, _, _| h.compare = c)));
                }
            }
        }
        let rows: Vec<Stateful<Div>> = rows
            .into_iter()
            .enumerate()
            .map(|(i, (label, on, act))| {
                div()
                    .id(("menu", i))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.panel))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        act(this, window, cx);
                        if !keep {
                            this.menu = None;
                        }
                        cx.notify();
                    }))
                    .child(div().w(px(14.)).child(if on { icon(IconName::Check, t.accent, 14.).into_any_element() } else { div().into_any_element() }))
                    .child(label)
            })
            .collect();
        div()
            .id("menu-panel")
            .absolute()
            .top(px(30.))
            .left_0()
            .min_w(px(220.))
            .p(px(4.))
            .rounded(px(8.))
            .bg(t.bg)
            .shadow_lg()
            .text_size(px(13.))
            .font_weight(FontWeight::NORMAL)
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                this.menu = None;
                this.closed = Some((menu, Instant::now()));
                cx.notify();
            }))
            .children(rows)
    }

    /// Y axis labels, right-aligned in the gutter, and the plot beside them.
    fn plot_frame(&self, floor: f64, max: f64, measure: Measure, plot: AnyElement) -> Div {
        let t = self.snap.theme;
        let bike = self.bike_only();
        let labels = ticks(max).into_iter().filter(|v| *v >= floor).map(|v| {
            let top = PLOT_H * (1.0 - ((v - floor) / (max - floor)) as f32) - 8.0;
            // Axis steps are round: whole kilometres read better there.
            let text = if measure == Measure::Distance { format!("{}\u{a0}km", act::group((v / 1000.0).round() as i64)) } else { measure.fmt(v, bike) };
            num(text).absolute().right(px(10.)).top(px(top)).text_size(px(11.)).text_color(t.dim).whitespace_nowrap()
        });
        div()
            .flex()
            .h(px(PLOT_H))
            .child(div().w(px(GUTTER)).flex_none().relative().h_full().children(labels))
            .child(div().flex_1().min_w_0().h_full().relative().child(plot))
    }

    fn bars(&mut self, plot_w: f32, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let q = self.query.clone();
        let bike = self.bike_only();
        let cur = q.series(&self.snap.acts);
        let prev: Vec<Point> = self.compared().map(|c| c.series(&self.snap.acts)).unwrap_or_default();
        let sums = q.measure.sums();
        let vals = |s: &[Point]| s.iter().map(|p| p.value).collect::<Vec<_>>();
        let (cv, pv) = (vals(&cur), vals(&prev));
        let all = || cv.iter().chain(pv.iter()).flatten().copied();
        let peak = all().fold(0.0f64, f64::max);
        // Averages (pace, heart rate) read better from a floor than from zero.
        let floor = if sums { 0.0 } else { (all().fold(f64::INFINITY, f64::min) * 0.95).min(peak) };
        let floor = if floor.is_finite() { floor } else { 0.0 };
        let max = if peak > 0.0 { (peak * 1.08).max(floor + 1.0) } else { 1.0 };
        let n = cur.len();
        let hover = self.hover;
        let pick = self.pick;
        let shown = hover.or(pick).or(n.checked_sub(1));
        let today = today();
        let readout = shown.and_then(|i| cur.get(i)).map_or(String::new(), |p| {
            let mut s = format!(
                "{} · {} · {} séance{}",
                if p.from == p.to { fr_day(p.from, today) } else { format!("{} → {}", fr_day(p.from, today), fr_day(p.to, today)) },
                p.value.map(|v| q.measure.fmt(v, bike)).unwrap_or("—".into()),
                p.count,
                if p.count > 1 { "s" } else { "" }
            );
            if let Some(Some(v)) = shown.and_then(|i| pv.get(i)) {
                s += &format!(" · avant : {}", q.measure.fmt(*v, bike));
            }
            s
        });

        let (cur_c, hi_c, prev_c, grid) = (blend(t.accent, t.bg, 0.55), t.accent, t.mark(), t.border);
        let compare = !pv.is_empty();
        let bounds = self.plots.get("bars");
        let cell = bounds.clone();
        let (cva, pva) = (Arc::new(cv), Arc::new(pv));
        let line = !sums;
        let plot = canvas(
            move |b, _, _| cell.set(b),
            move |b, _, window, _| {
                let w = b.size.width / px(1.);
                let h = b.size.height / px(1.);
                let bottom = b.origin.y + b.size.height;
                let y = |v: f64| b.origin.y + px(h * (1.0 - ((v - floor) / (max - floor)) as f32));
                let slot = w / n.max(1) as f32;
                for v in ticks(max).into_iter().filter(|v| *v >= floor) {
                    window.paint_quad(fill(Bounds::new(point(b.origin.x, y(v)), size(b.size.width, px(1.))), grid));
                }
                if line {
                    for (vals, color, width) in [(&pva, prev_c, 1.5), (&cva, hi_c, 2.0)] {
                        let mut p = PathBuilder::stroke(px(width));
                        let mut started = false;
                        for (i, v) in vals.iter().enumerate() {
                            if let Some(v) = v {
                                let pt = point(b.origin.x + px(slot * (i as f32 + 0.5)), y(*v));
                                if started { p.line_to(pt) } else { p.move_to(pt) }
                                started = true;
                                let r = if Some(i) == hover || Some(i) == pick { 4.5 } else { 3.0 };
                                window.paint_quad(fill(Bounds::new(point(pt.x - px(r), pt.y - px(r)), size(px(2. * r), px(2. * r))), color).corner_radii(px(r)));
                            }
                        }
                        if let Ok(p) = p.build() {
                            window.paint_path(p, color);
                        }
                    }
                } else {
                    let gap = (slot * 0.25).clamp(1.0, 12.0);
                    let bw = if compare { (slot - gap) / 2.0 } else { slot - gap };
                    for i in 0..n {
                        let x = b.origin.x + px(slot * i as f32 + gap / 2.0);
                        if let Some(Some(v)) = pva.get(i).filter(|_| compare) {
                            let top = y(*v);
                            window.paint_quad(fill(Bounds::new(point(x, top), size(px(bw), bottom - top)), prev_c).corner_radii(px(2.)));
                        }
                        if let Some(Some(v)) = cva.get(i).filter(|v| v.is_some_and(|v| v > 0.0)) {
                            let top = y(*v).min(bottom - px(1.));
                            let x = if compare { x + px(bw) } else { x };
                            let c = if Some(i) == hover || Some(i) == pick { hi_c } else { cur_c };
                            window.paint_quad(fill(Bounds::new(point(x, top), size(px(bw), bottom - top)), c).corner_radii(px(2.)));
                        }
                    }
                }
            },
        )
        .size_full();

        let area = div()
            .id("bars-area")
            .size_full()
            .cursor_pointer()
            .child(plot)
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                let b = bounds.get();
                if b.contains(&e.position) && n > 0 {
                    let f = ((e.position.x - b.origin.x) / b.size.width).clamp(0.0, 0.9999);
                    this.hover = Some((f * n as f32) as usize);
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|this, on: &bool, _, cx| {
                if !on {
                    this.hover = None;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(|this, _, _, cx| {
                this.pick = this.hover;
                cx.notify();
            }));

        // X labels: as many as fit, evenly.
        let every = ((n as f32 * 76.0 / plot_w.max(1.0)).ceil() as usize).max(1);
        // A label is never cut by the right edge: the last ones that wouldn't fit are left out.
        let xlabels = cur.iter().enumerate().filter(|(i, _)| i % every == 0 && plot_w * *i as f32 / n.max(1) as f32 + 64.0 <= plot_w).map(|(i, p)| {
            num(group_label(q.group, p.from))
                .absolute()
                .left(px(plot_w * i as f32 / n.max(1) as f32))
                .text_size(px(11.))
                .text_color(t.dim)
                .whitespace_nowrap()
        });

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_between()
                    .gap(px(10.))
                    .child(heading(format!("{} par {}", q.measure.label(), q.group.label().to_lowercase())))
                    .child(num(readout).text_size(px(12.)).text_color(t.soft)),
            )
            .child(self.plot_frame(floor, max, q.measure, area.into_any_element()))
            .child(div().pl(px(GUTTER)).child(div().relative().h(px(16.)).children(xlabels)))
            .when(compare, |d| d.child(self.legend(cur_c, prev_c)))
    }

    fn legend(&self, cur: Hsla, prev: Hsla) -> Div {
        let t = self.snap.theme;
        let q = &self.query;
        let c = self.compared().unwrap();
        let key = |color: Hsla, label: String| div().flex().items_center().gap(px(6.)).child(div().size(px(10.)).rounded(px(2.)).bg(color)).child(label);
        div()
            .pl(px(GUTTER))
            .flex()
            .gap(px(18.))
            .text_size(px(12.))
            .text_color(t.soft)
            .child(key(cur, period_label(q.from, q.to)))
            .child(key(prev, period_label(c.from, c.to)))
    }

    fn cumul(&mut self, pw: f32, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let q = self.query.clone();
        let bike = self.bike_only();
        let cur = q.running(&self.snap.acts);
        let prev = self.compared().map(|c| c.running(&self.snap.acts)).unwrap_or_default();
        let n = cur.len().max(prev.len());
        let all = || cur.iter().chain(prev.iter()).flatten().copied();
        let peak = all().fold(0.0f64, f64::max);
        let floor = if q.measure.sums() { 0.0 } else { (all().fold(f64::INFINITY, f64::min) * 0.95).min(peak) };
        let floor = if floor.is_finite() { floor } else { 0.0 };
        let max = if peak > 0.0 { (peak * 1.1).max(floor + 1.0) } else { 1.0 };
        let at = self.hover.or(self.pick).unwrap_or(cur.len().saturating_sub(1));
        let today = today();
        let date = q.from + Days::new(at as u64);
        let now = cur.get(at).copied().flatten();
        let then = prev.get(at).copied().flatten();
        let readout = {
            let mut s = format!("au {} : {}", fr_day(date, today), now.map(|v| q.measure.fmt(v, bike)).unwrap_or("—".into()));
            if let (Some(v), Some(c)) = (then, self.compared()) {
                s += &format!(" · {} : {}", period_label(c.from, c.to), q.measure.fmt(v, bike));
                if let Some(c) = now.filter(|_| v > 0.0 && q.measure.sums()) {
                    s += &format!(" ({:+.0} %)", (c / v - 1.0) * 100.0);
                }
            }
            s
        };
        let (cur_c, prev_c, guide, grid) = (t.accent, t.soft, t.dim, t.border);
        let hover = self.hover;
        let bounds = self.plots.get("cumul");
        let cell = bounds.clone();
        let (ca, pa) = (Arc::new(cur.clone()), Arc::new(prev.clone()));
        let plot = canvas(
            move |b, _, _| cell.set(b),
            move |b, _, window, _| {
                let w = b.size.width / px(1.);
                let h = b.size.height / px(1.);
                let x = |i: usize| b.origin.x + px(w * i as f32 / (n.max(2) - 1) as f32);
                let y = |v: f64| b.origin.y + px(h * (1.0 - ((v - floor) / (max - floor)) as f32));
                for v in ticks(max).into_iter().filter(|v| *v >= floor) {
                    window.paint_quad(fill(Bounds::new(point(b.origin.x, y(v)), size(b.size.width, px(1.))), grid));
                }
                // No fill: two lines that never hide each other.
                for (vals, color, width) in [(&pa, prev_c, 1.5), (&ca, cur_c, 2.5)] {
                    let mut p = PathBuilder::stroke(px(width));
                    let mut started = false;
                    for (i, v) in vals.iter().enumerate() {
                        if let Some(v) = v {
                            if started { p.line_to(point(x(i), y(*v))) } else { p.move_to(point(x(i), y(*v))) }
                            started = true;
                        }
                    }
                    if let Ok(p) = p.build() {
                        window.paint_path(p, color);
                    }
                }
                if let Some(i) = hover {
                    window.paint_quad(fill(Bounds::new(point(x(i), b.origin.y), size(px(1.), b.size.height)), guide));
                    for (vals, color) in [(&pa, prev_c), (&ca, cur_c)] {
                        if let Some(Some(v)) = vals.get(i) {
                            let c = point(x(i) - px(4.), y(*v) - px(4.));
                            window.paint_quad(fill(Bounds::new(c, size(px(8.), px(8.))), color).corner_radii(px(4.)));
                        }
                    }
                }
            },
        )
        .size_full();

        // Each line named at its end, where the eye lands.
        let ph = PLOT_H;
        let end_label = |vals: &[Option<f64>], label: String, color: Hsla| {
            let (i, v) = vals.iter().enumerate().rev().find_map(|(i, v)| v.map(|v| (i, v)))?;
            let left = pw * i as f32 / (n.max(2) - 1) as f32;
            let top = ph * (1.0 - ((v - floor) / (max - floor)) as f32) - 18.0;
            Some(
                num(format!("{label} {}", q.measure.fmt(v, bike)))
                    .absolute()
                    .right(px((pw - left).max(0.0)))
                    .top(px(top.clamp(0.0, (ph - 16.0).max(0.0))))
                    .pr(px(6.))
                    .text_size(px(11.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(color)
                    .whitespace_nowrap(),
            )
        };
        let c = self.compared();
        let labels: Vec<Div> = [
            end_label(&cur, period_label(q.from, q.to), cur_c),
            c.as_ref().and_then(|c| end_label(&prev, period_label(c.from, c.to), prev_c)),
        ]
        .into_iter()
        .flatten()
        .collect();

        let area = div()
            .id("cumul-area")
            .size_full()
            .relative()
            .cursor_pointer()
            .child(plot)
            .children(labels)
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                let b = bounds.get();
                if b.contains(&e.position) && n > 1 {
                    let f = ((e.position.x - b.origin.x) / b.size.width).clamp(0.0, 1.0);
                    this.hover = Some((f * (n - 1) as f32).round() as usize);
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|this, on: &bool, _, cx| {
                if !on {
                    this.hover = None;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(|this, _, _, cx| {
                this.pick = this.hover;
                cx.notify();
            }));

        let every = ((n as f32 * 80.0 / pw.max(1.0)).ceil() as usize).max(1);
        let xlabels = (0..n).filter(|i| i % every == 0 && pw * *i as f32 / (n.max(2) - 1) as f32 + 52.0 <= pw).map(|i| {
            let d = q.from + Days::new(i as u64);
            num(format!("{} {}", d.day(), month_short(d.month0())))
                .absolute()
                .left(px(pw * i as f32 / (n.max(2) - 1) as f32))
                .text_size(px(11.))
                .text_color(t.dim)
                .whitespace_nowrap()
        });

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_between()
                    .gap(px(10.))
                    .child(heading(cumul_title(q.measure)))
                    .child(num(readout).text_size(px(12.)).text_color(t.soft)),
            )
            .child(self.plot_frame(floor, max, q.measure, area.into_any_element()))
            .child(div().pl(px(GUTTER)).child(div().relative().h(px(16.)).children(xlabels)))
    }

    /// The activities of the group clicked (the last one by default).
    fn selection(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let q = self.query.clone();
        let bike = self.bike_only();
        let (from, to) = match self.chart {
            Chart::Bars => {
                let s = q.series(&self.snap.acts);
                match self.pick.or(s.len().checked_sub(1)).and_then(|i| s.get(i)) {
                    Some(p) => (p.from, p.to),
                    None => (q.from, q.to),
                }
            }
            Chart::Cumul => match self.pick {
                Some(i) => (q.from + Days::new(i as u64), q.from + Days::new(i as u64)),
                None => (q.from, q.to),
            },
        };
        let sub = Query { from, to, ..q.clone() };
        let list: Vec<Activity> = self.snap.acts.iter().filter(|a| sub.keeps(a)).cloned().collect();
        let today = today();
        let title_text = if from == to { fr_day(from, today) } else { format!("{} → {}", fr_day(from, today), fr_day(to, today)) };
        div()
            .flex()
            .flex_col()
            .child(heading(title_text).mb(px(2.)))
            .child(div().text_size(px(12.)).text_color(t.soft).mb(px(8.)).child(if list.is_empty() {
                "Aucune activité.".to_string()
            } else {
                format!("{} activité{} · un clic sur le graphique change la sélection", list.len(), if list.len() > 1 { "s" } else { "" })
            }))
            .children(list.iter().take(40).enumerate().map(|(i, a)| {
                let id = a.activity_id;
                div()
                    .id(("sel", i))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(32.))
                    .px(px(8.))
                    .mx(px(-8.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.panel))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(Page::Activity(id))))
                    .child(num(a.date().map(fr_short).unwrap_or_default()).w(px(96.)).flex_none().text_size(px(12.)).text_color(t.soft))
                    .child(div().flex().w(px(110.)).flex_none().child(sport_tag(a.sport(), t)))
                    .child(div().flex_1().min_w_0().truncate().child(name(a)))
                    .child(num(own_value(q.measure, a, bike)).font_weight(FontWeight::BOLD))
            }))
            .when(list.len() > 40, |d| d.child(div().pt(px(6.)).text_size(px(12.)).text_color(t.soft).child(format!("… et {} autres", list.len() - 40))))
    }

    /// The range sport by sport, with the change against the compared period.
    fn by_sport(&self) -> Div {
        let t = self.snap.theme;
        let q = self.query.clone();
        let cmp = self.compared();
        let sports: Vec<Sport> = if q.sports.is_empty() { sports_present(&self.snap.acts) } else { q.sports.clone() };
        let rows: Vec<(Sport, Point, Option<Point>)> = sports
            .into_iter()
            .map(|s| {
                let one = Query { sports: vec![s], ..q.clone() };
                let before = cmp.as_ref().map(|c| Query { sports: vec![s], ..c.clone() }.total(&self.snap.acts));
                (s, one.total(&self.snap.acts), before)
            })
            .filter(|(_, p, b)| p.count > 0 || b.is_some_and(|b| b.count > 0))
            .collect();
        let top = rows.iter().filter_map(|r| r.1.value).fold(0.0f64, f64::max).max(1e-9);
        let cell = |d: Div, w: f32| d.w(px(w)).flex_none().flex().justify_end();
        div()
            .flex()
            .flex_col()
            .child(heading("Par sport").mb(px(10.)))
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .pb(px(4.))
                    .text_size(px(12.))
                    .text_color(t.dim)
                    .child(div().w(px(110.)).child("Sport"))
                    .child(div().flex_1().child(q.measure.label()))
                    .child(cell(div().child("Séances"), 64.))
                    .when(cmp.is_some(), |d| d.child(cell(div().child("Évolution"), 80.))),
            )
            .when(rows.is_empty(), |d| d.child(div().text_color(t.soft).child("Rien sur cette période.")))
            .children(rows.into_iter().map(|(s, p, b)| {
                let bike = s == Sport::Bike;
                let share = if q.measure.sums() { p.value.map_or(0.0, |v| v / top) } else { 1.0 };
                let delta = match (p.value, b.and_then(|b| b.value)) {
                    (Some(now), Some(then)) if then > 0.0 && p.count > 0 => {
                        let d = (now / then - 1.0) * 100.0;
                        // A faster pace is a smaller number: say it as progress.
                        let d = if q.measure == Measure::Pace { -d } else { d };
                        format!("{d:+.0} %")
                    }
                    _ => "—".into(),
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .h(px(34.))
                    .child(div().w(px(110.)).flex_none().flex().child(sport_tag(s, t)))
                    .child(
                        div()
                            .flex_1()
                            .relative()
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .child(div().absolute().left_0().top(px(3.)).h(px(16.)).rounded(px(3.)).w(relative((share as f32).clamp(0.02, 1.0))).bg(t.panel))
                            .child(num(p.value.filter(|_| p.count > 0).map(|v| q.measure.fmt(v, bike)).unwrap_or("—".into())).relative().pl(px(6.)).font_weight(FontWeight::BOLD)),
                    )
                    .child(cell(num(p.count.to_string()), 64.).text_color(t.soft))
                    .when(cmp.is_some(), |d| d.child(cell(num(delta), 80.).text_color(t.soft)))
            }))
    }

    /// Day by day, as many recent weeks as the width holds (up to a year): the cells keep a size
    /// one can point at, the grid always fits.
    fn calendar(&mut self, w: f32, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let today = today();
        let q = self.query.clone();
        let (label_w, gap) = (34.0, 3.0);
        let weeks = (((w - label_w + gap) / (15.0 + gap)) as usize).clamp(6, 53);
        let cell = ((w - label_w - (weeks as f32 - 1.0) * gap) / weeks as f32).clamp(9.0, 22.0);
        let first = act::week_start(today) - Days::new((weeks as u64 - 1) * 7);
        let days = Query { group: Group::Day, from: first, to: today, ..q.clone() }.series(&self.snap.acts);
        let vals: Vec<f64> = days.iter().map(|p| if p.count > 0 { p.value.unwrap_or(0.0).max(1e-6) } else { 0.0 }).collect();
        let mut sorted: Vec<f64> = vals.iter().copied().filter(|v| *v > 0.0).collect();
        sorted.sort_by(f64::total_cmp);
        let p90 = sorted.get(sorted.len() * 9 / 10).copied().unwrap_or(1.0).max(1e-9);
        let (mut longest, mut run) = (0, 0);
        for v in &vals {
            run = if *v > 0.0 { run + 1 } else { 0 };
            longest = longest.max(run);
        }
        let hover = self.cal_hover;
        let summary = match hover.and_then(|i| days.get(i)) {
            Some(p) if p.count > 0 => format!("{} · {}", fr_day(p.from, today), q.measure.fmt(p.value.unwrap_or(0.0), self.bike_only())),
            Some(p) => format!("{} · repos", fr_day(p.from, today)),
            None => format!("{} jours actifs · plus longue série : {} jour{}", sorted.len(), longest, if longest > 1 { "s" } else { "" }),
        };
        let shade = move |v: f64, accent: Hsla, bg: Hsla, rest: Hsla| if v > 0.0 { blend(accent, bg, (0.3 + 0.7 * (v / p90).min(1.0)) as f32) } else { rest };
        let (rest, accent, bg, ink) = (t.border, t.accent, t.bg, t.text);
        let vals = Arc::new(vals);
        let n = vals.len();
        let bounds = self.plots.get("calendar");
        let cell_b = bounds.clone();
        let plot = canvas(
            move |b, _, _| cell_b.set(b),
            move |b, _, window, _| {
                for (i, v) in vals.iter().enumerate() {
                    let (col, row) = (i / 7, i % 7);
                    let o = point(b.origin.x + px(col as f32 * (cell + gap)), b.origin.y + px(row as f32 * (cell + gap)));
                    let c = if hover == Some(i) { ink } else { shade(*v, accent, bg, rest) };
                    window.paint_quad(fill(Bounds::new(o, size(px(cell), px(cell))), c).corner_radii(px(2.)));
                }
            },
        )
        .w(px(weeks as f32 * (cell + gap) - gap))
        .h(px(7.0 * cell + 6.0 * gap));
        let weekdays = div().w(px(label_w)).flex_none().relative().children([(0usize, "lun"), (2, "mer"), (4, "ven")].map(|(row, l)| {
            div().absolute().top(px(row as f32 * (cell + gap) + cell / 2.0 - 7.0)).text_size(px(11.)).text_color(t.dim).child(l)
        }));
        let months = (0..weeks).filter_map(|col| {
            let d = first + Days::new(col as u64 * 7);
            (d.day() <= 7 && col as f32 * (cell + gap) + 34.0 <= w - label_w).then(|| num(month_short(d.month0())).absolute().left(px(label_w + col as f32 * (cell + gap))).text_size(px(11.)).text_color(t.dim))
        });
        let legend = div().flex().items_center().gap(px(3.)).text_size(px(11.)).text_color(t.dim).child("moins").children(
            [0.0, 0.3, 0.6, 1.0].map(|l| div().size(px(10.)).rounded(px(2.)).bg(shade(l * p90, accent, bg, rest))),
        ).child("plus");
        let hit = bounds.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .justify_between()
                    .gap(px(10.))
                    .child(div().flex().flex_col().gap(px(3.)).child(heading(format!("Régularité · {} semaines", weeks))).child(num(summary).text_size(px(12.)).text_color(t.soft)))
                    .child(legend),
            )
            .child(div().relative().h(px(14.)).children(months))
            .child(
                div().flex().child(weekdays).child(
                    div()
                        .id("calendar")
                        .cursor_pointer()
                        .child(plot)
                        .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                            let b = hit.get();
                            let (x, y) = ((e.position.x - b.origin.x) / px(1.), (e.position.y - b.origin.y) / px(1.));
                            let (col, row) = ((x / (cell + gap)) as usize, (y / (cell + gap)) as usize);
                            let i = col * 7 + row;
                            this.cal_hover = (x >= 0.0 && y >= 0.0 && row < 7 && i < n).then_some(i);
                            cx.notify();
                        }))
                        .on_hover(cx.listener(|this, on: &bool, _, cx| {
                            if !on {
                                this.cal_hover = None;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            // A day becomes the range, its activities listed above.
                            if let Some(i) = this.cal_hover {
                                let d = first + Days::new(i as u64);
                                this.query.group = Group::Day;
                                this.custom = true;
                                this.set_range(d, d, None, window, cx);
                            }
                        })),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{group_label, period_label, ticks};
    use crate::logic::activity::Group;
    use chrono::NaiveDate;

    #[test]
    fn axis() {
        assert!(ticks(0.0).is_empty());
        assert!(ticks(f64::NAN).is_empty());
        assert_eq!(ticks(100.0), [25.0, 50.0, 75.0, 100.0]);
        assert_eq!(ticks(7.0), [2.0, 4.0, 6.0]);
        assert_eq!(ticks(1234.0), [500.0, 1000.0]);
        let d = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        assert_eq!(group_label(Group::Week, d), "5 oct.");
        assert_eq!(group_label(Group::Month, d), "oct. 2026");
        assert_eq!(group_label(Group::Year, d), "2026");
        let day = |y, m, dd| NaiveDate::from_ymd_opt(y, m, dd).unwrap();
        assert_eq!(period_label(day(2025, 1, 1), day(2025, 12, 31)), "2025");
        assert_eq!(period_label(day(2026, 1, 1), day(2026, 10, 24)), "2026");
        assert_eq!(period_label(day(2025, 3, 10), day(2025, 12, 31)), "10 mars 2025 → 31 décembre 2025");
    }
}
