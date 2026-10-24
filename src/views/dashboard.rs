//! The home page: where things stand right now. The week, the last 30 days or the month, twelve
//! weeks at a glance, the last outing, the split by sport and the latest records. Sports mix, so
//! across all of them the common measure is time; distances are told sport by sport.

use crate::data::{Data, Snapshot};
use crate::logic::activity::{self as act, Metric, Sport, Totals, fmt_duration, fmt_km, fmt_m, group, speed};
use crate::logic::dashboard::{self as dash, Change, Period, Span};
use crate::logic::journal::{name, sports_present};
use crate::logic::map::{View, bbox};
use crate::logic::records::{self, Broken};
use crate::ui::format::{fr_day, fr_long, fr_short};
use crate::ui::theme::blend;
use crate::ui::{figure, heading, num, page, segmented, sport_tag, title};
use crate::views::{Page, Width, today};
use chrono::Days;
use gpui_kit::{prelude::FluentBuilder, *};

/// How many weeks the chart shows.
const WEEKS: usize = 12;
/// Height of the bars, in pixels.
const BARS: f32 = 132.0;

pub struct DashboardView {
    snap: Snapshot,
    width: Width,
    sport: Option<Sport>,
    window: Period,
    _subscription: Subscription,
}

impl EventEmitter<Page> for DashboardView {}

impl DashboardView {
    pub fn new(data: Entity<Data>, width: Width, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&data, |this, data, cx| {
            this.snap = data.read(cx).snapshot();
            cx.notify();
        });
        DashboardView { snap: data.read(cx).snapshot(), width, sport: None, window: Period::Rolling, _subscription: sub }
    }

    fn sport_filter(&self, cx: &mut Context<Self>) -> Div {
        let present = sports_present(&self.snap.acts);
        let mut items = vec![("Tout".into(), self.sport.is_none())];
        items.extend(present.iter().map(|s| (s.label().into(), self.sport == Some(*s))));
        segmented("dash-sport", items, self.snap.theme, cx, move |this: &mut DashboardView, i, _, _| {
            this.sport = if i == 0 { None } else { Some(present[i - 1]) };
        })
    }

    fn window_filter(&self, cx: &mut Context<Self>) -> Div {
        let items = vec![("30 jours".into(), self.window == Period::Rolling), ("Ce mois-ci".into(), self.window == Period::Month)];
        segmented("dash-window", items, self.snap.theme, cx, |this: &mut DashboardView, i, _, _| {
            this.window = if i == 0 { Period::Rolling } else { Period::Month };
        })
    }
}

/// "4 sorties · 38,2 km · 4h12 · 410 m D+ · 6:12 /km" for one sport; across sports, no distance:
/// it is told per sport on the line below.
fn week_line(t: &Totals, sport: Option<Sport>) -> String {
    let mut parts = vec![format!("{} sortie{}", group(t.count as i64), if t.count > 1 { "s" } else { "" })];
    if sport.is_some_and(Sport::has_distance) && t.distance > 0.0 {
        parts.push(fmt_km(t.distance));
    }
    if t.duration > 0.0 {
        parts.push(fmt_duration(t.duration));
    }
    if t.elevation >= 1.0 {
        parts.push(format!("{} D+", fmt_m(t.elevation)));
    }
    if let Some(s) = sport.filter(|s| s.has_distance()) {
        parts.extend(speed(s, t.distance, t.duration));
    }
    parts.join(" · ")
}

/// "Course 38,2 km · Vélo 92 km": the distances, sport by sport, of the sports that have one.
fn distances_line(per: &[(Sport, Totals)]) -> String {
    per.iter().filter(|(s, t)| s.has_distance() && t.distance > 0.0).map(|(s, t)| format!("{} {}", s.label(), fmt_km(t.distance))).collect::<Vec<_>>().join(" · ")
}

impl Render for DashboardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = self.width.get(window);
        let t = self.snap.theme;
        let today = today();
        let head = div()
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
                    .child(title("Aujourd'hui"))
                    .child(div().text_color(t.soft).child(fr_day(today, today))),
            )
            .when(!self.snap.acts.is_empty(), |d| d.child(self.sport_filter(cx)));
        if self.snap.acts.is_empty() {
            return page("dashboard", width)
                .track_scroll(self.width.scroll())
                .child(head)
                .child(div().text_color(t.soft).child("Aucune activité pour l'instant : la synchronisation (en bas du rail) les rapporte depuis Garmin."))
                .into_any_element();
        }

        let acts: &[act::Activity] = &self.snap.acts;
        let sport = self.sport;
        let metric = dash::metric_for(sport);

        // The week so far, against the same days of the week before.
        let monday = act::week_start(today);
        let week = Span { from: monday, to: today };
        let last_week = Span { from: monday - Days::new(7), to: today - Days::new(7) };
        let (w_now, w_before) = (dash::summarize(acts, week, sport).totals, dash::summarize(acts, last_week, sport).totals);
        let per_week = dash::by_sport(acts, week);
        let week_block = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(heading("Cette semaine"))
            .child(if w_now.count == 0 {
                div().text_color(t.soft).child("Rien encore.")
            } else {
                div().text_color(t.text).child(week_line(&w_now, sport))
            })
            .when(sport.is_none() && !distances_line(&per_week).is_empty(), |d| d.child(div().text_color(t.soft).child(distances_line(&per_week))))
            .when(w_now.count + w_before.count > 0, |d| {
                let (a, b) = if metric == Metric::Distance { (w_now.distance, w_before.distance) } else { (w_now.duration, w_before.duration) };
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .text_size(px(12.))
                        .text_color(t.dim)
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_x(px(14.))
                                .child(format!("{} {}", metric.label().to_lowercase(), dash::change(a, b).label()))
                                .child(format!("sorties {}", dash::change(w_now.count as f64, w_before.count as f64).label())),
                        )
                        .child("par rapport aux mêmes jours de la semaine d'avant"),
                )
            });

        // The key figures over the chosen window.
        let (now, before) = dash::spans(self.window, today);
        let (s_now, s_before) = (dash::summarize(acts, now, sport), dash::summarize(acts, before, sport));
        let streak = {
            let mine: Vec<act::Activity> = acts.iter().filter(|a| sport.is_none_or(|s| a.sport() == s)).cloned().collect();
            act::streaks(&mine, today).days
        };
        let (m_now, m_before) = if metric == Metric::Distance { (s_now.totals.distance, s_before.totals.distance) } else { (s_now.totals.duration, s_before.totals.duration) };
        let kpi = |label: &'static str, value: String, change: Option<Change>| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(figure(label, value, t))
                .child(div().h(px(16.)).text_size(px(12.)).text_color(t.dim).child(change.filter(|c| *c != Change::Nothing).map(Change::label).unwrap_or_default()))
        };
        let kpis = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(div().flex().flex_wrap().items_baseline().gap_x(px(10.)).child(heading(if self.window == Period::Rolling { "Les 30 derniers jours" } else { "Ce mois-ci" })).child(
                        div().text_size(px(12.)).text_color(t.dim).child(format!("contre {} – {}", fr_day(before.from, today), fr_day(before.to, today))),
                    ))
                    .child(self.window_filter(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_x(px(44.))
                    .gap_y(px(14.))
                    .child(kpi("Série en cours", if streak == 0 { "—".into() } else { format!("{streak} jour{}", if streak > 1 { "s" } else { "" }) }, None))
                    .child(kpi(
                        metric.label(),
                        if m_now > 0.0 { metric.fmt(m_now) } else { "—".into() },
                        Some(dash::change(m_now, m_before)),
                    ))
                    .child(kpi("Sorties", group(s_now.totals.count as i64), Some(dash::change(s_now.totals.count as f64, s_before.totals.count as f64))))
                    .child(kpi(
                        "FC moyenne",
                        s_now.hr.map_or("—".into(), |h| format!("{:.0} bpm", h)),
                        match (s_now.hr, s_before.hr) {
                            (Some(a), Some(b)) => Some(dash::change(a, b)),
                            _ => None,
                        },
                    )),
            );

        // Twelve weeks.
        let weeks = dash::weekly(acts, today, WEEKS, sport, metric);
        let median = dash::median(&weeks[..WEEKS - 1]).filter(|m| *m > 0.0);
        let top = weeks.iter().copied().fold(0.0_f64, f64::max);
        let chart = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_baseline()
                    .justify_between()
                    .gap_x(px(10.))
                    .child(heading(format!("{} par semaine", metric.label())))
                    .child(div().text_size(px(12.)).text_color(t.dim).child(match median {
                        Some(m) => format!("médiane {} (pointillés)", metric.fmt(m)),
                        None => String::new(),
                    })),
            )
            .child(if top <= 0.0 {
                div().h(px(BARS)).text_color(t.soft).child(format!("Rien sur ces {WEEKS} semaines.")).into_any_element()
            } else {
                let monday = act::week_start(today);
                div()
                    .relative()
                    .h(px(BARS))
                    .flex()
                    .items_end()
                    .gap(px(5.))
                    .children(weeks.iter().copied().enumerate().map(|(i, v)| {
                        let current = i == WEEKS - 1;
                        let h = if v > 0.0 { ((v / top) as f32 * BARS).max(3.0) } else { 1.0 };
                        let from = monday - Days::new(7 * (WEEKS - 1 - i) as u64);
                        div()
                            .id(("dash-week", i))
                            .flex_1()
                            .h(px(h))
                            .rounded_t(px(2.))
                            .bg(if v <= 0.0 { t.border } else if current { t.accent } else { blend(t.accent, t.bg, 0.4) })
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(format!("semaine du {} : {}", fr_day(from, today), if v > 0.0 { metric.fmt(v) } else { "rien".into() })).build(window, cx)
                            })
                    }))
                    .when_some(median, |d, m| {
                        d.child(
                            // Dashes, clipped to the width: no dashed border to lean on.
                            div()
                                .absolute()
                                .left(px(0.))
                                .right(px(0.))
                                .bottom(px((m / top) as f32 * BARS))
                                .h(px(1.))
                                .flex()
                                .gap(px(4.))
                                .overflow_hidden()
                                .children((0..200).map(|_| div().w(px(5.)).h(px(1.)).flex_none().bg(t.soft))),
                        )
                    })
                    .into_any_element()
            })
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(11.))
                    .text_color(t.dim)
                    .child(format!("semaine du {}", fr_day(monday - Days::new(7 * (WEEKS as u64 - 1)), today)))
                    .child("cette semaine"),
            );

        // The last outing, with its trace.
        let last = dash::latest(acts, sport).map(|a| {
            let id = a.activity_id;
            let line = self.snap.derived.get(&id).map(|d| d.line.clone()).filter(|l| l.len() >= 2);
            let accent = t.accent;
            let trace = canvas(
                |_, _, _| {},
                move |b, _, window, _| {
                    let Some(line) = &line else { return };
                    let (w, h) = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
                    let Some(bb) = bbox(std::iter::once(line.as_slice()), 0.0) else { return };
                    let view = View::fit(bb, w, h, 14.0);
                    let at = |p: &[f32; 2]| {
                        let (x, y) = view.screen(p[0] as f64, p[1] as f64, w, h);
                        point(b.origin.x + px(x as f32), b.origin.y + px(y as f32))
                    };
                    let mut path = PathBuilder::stroke(px(2.));
                    path.move_to(at(&line[0]));
                    line[1..].iter().for_each(|p| path.line_to(at(p)));
                    if let Ok(p) = path.build() {
                        window.paint_path(p, accent);
                    }
                },
            )
            .size_full();
            let mut facts = vec![];
            if let Some(d) = a.distance.filter(|d| *d > 0.0 && a.sport().has_distance()) {
                facts.push(("Distance", fmt_km(d)));
            }
            if let Some(d) = a.duration {
                facts.push(("Temps", fmt_duration(d)));
            }
            if let Some(s) = a.speed_label().filter(|_| a.sport().has_distance()) {
                facts.push(("Allure", s));
            }
            if let Some(h) = a.average_hr.filter(|h| *h > 0.0) {
                facts.push(("FC", format!("{h:.0} bpm")));
            }
            div().child(div()
                .id("dash-last")
                .flex()
                .flex_col()
                .gap(px(8.))
                .cursor_pointer()
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(Page::Activity(id))))
                .child(heading("La dernière sortie"))
                .child(div().flex().items_center().gap(px(10.)).child(sport_tag(a.sport(), t)).child(div().font_weight(FontWeight::BOLD).child(name(a))))
                .child(div().text_size(px(12.)).text_color(t.soft).child(a.start().map(fr_long).unwrap_or_default()))
                .child(div().flex().flex_wrap().gap_x(px(22.)).children(facts.into_iter().map(|(l, v)| {
                    div().flex().flex_col().child(div().text_size(px(11.)).text_color(t.dim).child(l)).child(num(v))
                })))
                .child(div().h(px(150.)).rounded(px(8.)).bg(t.panel).child(trace)))
        });

        // Time by sport: only when sports mix.
        let per = dash::by_sport(acts, now);
        let total_time: f64 = per.iter().map(|(_, t)| t.duration).sum();
        let split = (sport.is_none() && total_time > 0.0).then(|| {
            let shade = |i: usize| blend(t.accent, t.bg, (0.9 - 0.17 * i as f32).max(0.18));
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(heading("Répartition du temps"))
                .child(div().flex().h(px(10.)).rounded(px(5.)).overflow_hidden().gap(px(2.)).children(per.iter().enumerate().filter(|(_, (_, s))| s.duration > 0.0).map(|(i, (_, s))| {
                    div().h_full().bg(shade(i)).w(relative((s.duration / total_time) as f32))
                })))
                .children(per.iter().enumerate().map(|(i, (s, tot))| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(div().size(px(8.)).rounded(px(2.)).bg(shade(i)).flex_none())
                        .child(div().w(px(96.)).flex_none().child(s.label()))
                        .child(num(fmt_duration(tot.duration)).w(px(64.)).flex_none())
                        .child(num(format!("{:.0} %", tot.duration / total_time * 100.0)).w(px(44.)).flex_none().text_color(t.soft))
                        .child(div().text_color(t.soft).child(if s.has_distance() && tot.distance > 0.0 { fmt_km(tot.distance) } else { String::new() }))
                }))
        });

        // Records beaten lately.
        let beaten: Vec<Broken> = records::broken(acts, &self.snap.derived, sport, 3);
        let records = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(heading("Records battus récemment"))
            .child(if beaten.is_empty() {
                div().text_color(t.soft).child("Aucun record battu pour l'instant.").into_any_element()
            } else {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(beaten.into_iter().map(|b| {
                        let id = b.entry.id;
                        let (now, _) = records::show(b.kind, b.sport, b.entry.value);
                        let (was, _) = records::show(b.kind, b.sport, b.previous);
                        div()
                            .id(("dash-record", id as usize ^ b.row.len()))
                            .flex()
                            .flex_col()
                            .cursor_pointer()
                            .hover(|d| d.bg(t.panel))
                            .rounded(px(6.))
                            .px(px(6.))
                            .py(px(3.))
                            .on_click(cx.listener(move |_, _, _, cx| cx.emit(Page::Activity(id))))
                            .child(div().flex().items_baseline().gap(px(10.)).child(div().font_weight(FontWeight::BOLD).child(b.row)).child(num(now)).child(div().text_size(px(12.)).text_color(t.dim).child(format!("avait {was}"))))
                            .child(div().text_size(px(12.)).text_color(t.soft).child(format!("{} · {}", b.board, fr_short(b.entry.date))))
                    }))
                    .into_any_element()
            });

        let wide = width - 2.0 * crate::ui::pad(width) >= 860.0;
        let pair = |left: Div, right: Option<Div>| {
            div()
                .flex()
                .when(!wide, |d| d.flex_col())
                .gap(px(40.))
                .child(left.flex_1().min_w_0())
                .when_some(right, |d, r| d.child(r.flex_1().min_w_0()))
        };
        page("dashboard", width)
            .track_scroll(self.width.scroll())
            .gap(px(32.))
            .child(head)
            .child(week_block)
            .child(kpis)
            .child(pair(chart, last))
            .child(pair(records, split))
            .into_any_element()
    }
}
