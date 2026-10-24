//! The home page: where things stand. The week against the usual week, the run of active weeks,
//! the year against the one before, six months week by week, a year day by day, and the latest
//! outings. Sports mix, so across all of them the common measure is time; one sport with a
//! distance is told in kilometres.

use crate::data::{Data, Snapshot};
use crate::logic::activity::{self as act, Metric, Sport, fmt_duration, fmt_km, group};
use crate::logic::calendar as cal;
use crate::logic::dashboard as dash;
use crate::logic::journal::{name, sports_present};
use crate::logic::records::{self, Broken};
use crate::ui::format::{fr_day, fr_short, fr_weekday, month_short, split_unit};
use crate::ui::theme::{Theme, blend};
use crate::ui::{heading, icon, num, page, segmented, sport_icon, title};
use crate::views::{Page, Width, today};
use chrono::{Datelike, Days, NaiveDate};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{prelude::FluentBuilder, *};

/// Weeks in the chart, weeks of the regularity grid at most, outings listed.
const WEEKS: usize = 26;
const HEAT_WEEKS: usize = 53;
const RECENT: usize = 5;
/// Height of the bars, width of the scale on their left, in pixels.
const BARS: f32 = 150.0;
const AXIS: f32 = 52.0;
/// A record shows here while it is this fresh, in days.
const FRESH: u64 = 14;

pub struct DashboardView {
    snap: Snapshot,
    width: Width,
    sport: Option<Sport>,
    _subscription: Subscription,
}

impl EventEmitter<Page> for DashboardView {}

impl DashboardView {
    pub fn new(data: Entity<Data>, width: Width, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&data, |this, data, cx| {
            this.snap = data.read(cx).snapshot();
            cx.notify();
        });
        DashboardView { snap: data.read(cx).snapshot(), width, sport: None, _subscription: sub }
    }

    /// The sports of the last year (and the one chosen, whatever it is).
    fn sport_filter(&self, today: NaiveDate, cx: &mut Context<Self>) -> Div {
        let since = dash::mondays(today, HEAT_WEEKS)[0];
        let lately: Vec<act::Activity> = self.snap.acts.iter().filter(|a| a.date().is_some_and(|d| d >= since) || Some(a.sport()) == self.sport).cloned().collect();
        let present = sports_present(&lately);
        let mut items = vec![("Tout".into(), self.sport.is_none())];
        items.extend(present.iter().map(|s| (s.label().into(), self.sport == Some(*s))));
        segmented("dash-sport", items, self.snap.theme, cx, move |this: &mut DashboardView, i, _, _| {
            this.sport = if i == 0 { None } else { Some(present[i - 1]) };
        })
    }
}

/// A dashed horizontal line: dashes clipped to the width (a dashed border is not drawn on a line
/// with no height).
fn dashes(color: Hsla) -> Div {
    div().h(px(1.)).flex().gap(px(4.)).overflow_hidden().children((0..400).map(move |_| div().w(px(5.)).h(px(1.)).flex_none().bg(color)))
}

/// A key figure: its label, the value in big mono with its unit in small sans, and its notes.
fn stat(label: String, value: String, notes: [String; 2], t: Theme) -> Div {
    let (n, unit) = split_unit(&value);
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .flex_1()
        .min_w(px(200.))
        .child(div().text_size(px(12.)).text_color(t.soft).child(label))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(5.))
                .child(num(n).text_size(px(30.)).font_weight(FontWeight::BOLD))
                .when(!unit.is_empty(), |d| d.child(div().text_size(px(14.)).text_color(t.soft).child(unit))),
        )
        .children(notes.into_iter().filter(|n| !n.is_empty()).map(|n| div().text_size(px(12.)).text_color(t.dim).child(n)))
}

/// A grid line's label: "10 km", "2,5 km", "5h", "30min".
fn tick_label(metric: Metric, v: f64) -> String {
    match metric {
        Metric::Distance if v.fract() == 0.0 => format!("{}\u{a0}km", group(v as i64)),
        Metric::Distance => format!("{:.1}\u{a0}km", v).replace('.', ","),
        Metric::Time if v.fract() == 0.0 => format!("{}h", group(v as i64)),
        Metric::Time => fmt_duration(v * 3600.0),
    }
}

/// "1 sortie", "3 sorties".
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{}\u{a0}{}", group(n as i64), if n > 1 { many } else { one })
}

/// A list row: the outing's day, its sport and name, then two figures. Lined up with the heading
/// above: the hover background reaches out past the text.
fn outing_row(id: (&'static str, usize), date: Option<String>, sport: Sport, label: String, figures: [String; 2], t: Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(12.))
        .mx(px(-8.))
        .px(px(8.))
        .py(px(5.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|d| d.bg(t.panel))
        .when_some(date, |d, date| d.child(num(date).w(px(96.)).flex_none().text_size(px(12.)).text_color(t.soft)))
        .child(icon(sport_icon(sport), t.soft, 14.))
        .child(div().flex_1().min_w_0().truncate().child(label))
        .children(figures.into_iter().map(|f| num(f).flex_none().min_w(px(64.)).flex().justify_end()))
}

impl Render for DashboardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = self.width.get(window);
        let inner = width - 2.0 * crate::ui::pad(width);
        let t = self.snap.theme;
        let today = today();
        let head = div()
            .flex()
            .flex_wrap()
            .items_end()
            .justify_between()
            .gap(px(14.))
            .child(div().flex().flex_col().gap(px(4.)).child(title("Aujourd'hui")).child(div().text_color(t.soft).child(fr_weekday(today))))
            .when(!self.snap.acts.is_empty(), |d| d.child(self.sport_filter(today, cx)));
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

        // Six months, week by week, sport by sport (the heaviest at the bottom).
        let mondays = dash::mondays(today, WEEKS);
        let sports = match sport {
            Some(s) => vec![s],
            None => dash::sports_by_weight(acts, mondays[0], today, metric),
        };
        let weeks = dash::weekly(acts, today, WEEKS, &sports, metric);
        let totals: Vec<f64> = weeks.iter().map(|w| w.iter().sum()).collect();
        let median = dash::median(&totals[..WEEKS - 1]).filter(|m| *m > 0.0);
        let shade = |i: usize| blend(t.accent, t.bg, if sports.len() == 1 { 0.7 } else { [0.85, 0.6, 0.4, 0.26, 0.15][i.min(4)] });

        // The three figures.
        let outings = act::totals(acts, mondays[WEEKS - 1], today, sport).count;
        let left = 6 - today.weekday().num_days_from_monday() as usize;
        let mine: Vec<act::Activity> = acts.iter().filter(|a| act::matches(a, sport)).cloned().collect();
        let streak = act::streaks(&mine, today);
        let (ytd, ytd_before) = dash::year_to_date(acts, today, sport, metric);
        let figures = div()
            .flex()
            .flex_wrap()
            .gap_x(px(56.))
            .gap_y(px(20.))
            .child(stat(
                "Cette semaine".into(),
                metric.fmt(totals[WEEKS - 1]),
                [
                    format!("{} · {}", count(outings, "sortie", "sorties"), if left == 0 { "dernier jour".into() } else { format!("encore {}", count(left, "jour", "jours")) }),
                    median.map(|m| format!("semaine médiane : {}", metric.fmt(m))).unwrap_or_default(),
                ],
                t,
            ))
            .child(stat(
                "Série".into(),
                count(streak.weeks as usize, "semaine", "semaines"),
                [
                    "au moins une sortie chaque semaine".into(),
                    if streak.longest_weeks > streak.weeks {
                        format!("record : {}", count(streak.longest_weeks as usize, "semaine", "semaines"))
                    } else if streak.weeks > 1 {
                        "la plus longue à ce jour".into()
                    } else {
                        String::new()
                    },
                ],
                t,
            ))
            .child(stat(
                format!("En {}", today.year()),
                metric.fmt(ytd),
                [
                    if ytd_before > 0.0 { format!("{} à la même date : {}", today.year() - 1, metric.fmt(ytd_before)) } else { format!("rien en {} à la même date", today.year() - 1) },
                    String::new(),
                ],
                t,
            ));

        let chart = {
            let top = totals.iter().copied().fold(0.0_f64, f64::max).max(median.unwrap_or(0.0));
            let unit = if metric == Metric::Distance { 1000.0 } else { 3600.0 };
            let scale = top * 1.06;
            let y = move |v: f64| (v / scale) as f32 * BARS;
            let body = if top <= 0.0 {
                div().h(px(BARS)).text_color(t.soft).child(format!("Rien sur ces {WEEKS} semaines.")).into_any_element()
            } else {
                let grid = dash::ticks(top / unit);
                let bars = weeks.iter().enumerate().map(|(i, parts)| {
                    let total = totals[i];
                    let current = i == WEEKS - 1;
                    let lines: Vec<(String, String)> = sports.iter().zip(parts).filter(|(_, v)| **v > 0.0).map(|(s, v)| (s.label().to_string(), metric.fmt(*v))).collect();
                    let head = format!(
                        "{} {} : {}",
                        if current { "Cette semaine, du" } else { "Semaine du" },
                        fr_day(mondays[i], today),
                        if total > 0.0 { metric.fmt(total) } else { "rien".into() }
                    );
                    let slot = div().id(("dash-week", i)).flex_1().min_w_0().h_full().flex().flex_col().justify_end().tooltip_show_delay(crate::ui::TIP_DELAY).tooltip(move |window, cx| {
                        let (head, lines) = (head.clone(), lines.clone());
                        Tooltip::element(move |_, _| {
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(div().font_weight(FontWeight::BOLD).child(head.clone()))
                                .when(lines.len() > 1, |d| d.children(lines.iter().map(|(s, v)| div().flex().justify_between().gap(px(16.)).child(s.clone()).child(num(v.clone())))))
                        })
                        .build(window, cx)
                    });
                    if total <= 0.0 {
                        slot.child(div().h(px(1.)).bg(t.border))
                    } else if current {
                        // Not over yet: an outline, so that it does not read as a fall.
                        slot.child(div().h(px(y(total).max(3.0))).rounded_t(px(2.)).border_1().border_dashed().border_color(t.accent).bg(blend(t.accent, t.bg, 0.1)))
                    } else {
                        slot.children(parts.iter().enumerate().rev().filter(|(_, v)| **v > 0.0).map(|(k, v)| div().h(px(y(*v).max(1.0))).bg(shade(k))))
                    }
                });
                // A month's name under the week that holds its first day.
                let months = mondays.iter().map(|m| {
                    let first = (0..7).map(|k| *m + Days::new(k)).find(|d| d.day() == 1);
                    div().flex_1().min_w_0().whitespace_nowrap().children(first.map(|d| month_short(d.month0())))
                });
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .h(px(BARS))
                            .child(
                                div()
                                    .relative()
                                    .w(px(AXIS))
                                    .h_full()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(t.dim)
                                    .children(grid.iter().map(|v| div().absolute().left(px(0.)).bottom(px(y(v * unit) - 7.0)).whitespace_nowrap().child(tick_label(metric, *v)))),
                            )
                            .child(
                                div()
                                    .relative()
                                    .flex_1()
                                    .h_full()
                                    .children(grid.iter().map(|v| div().absolute().left(px(0.)).right(px(0.)).bottom(px(y(v * unit))).h(px(1.)).bg(blend(t.border, t.bg, 0.6))))
                                    .child(div().absolute().left(px(0.)).right(px(0.)).top(px(0.)).bottom(px(0.)).flex().items_end().gap(px(4.)).children(bars))
                                    .when_some(median, |d, m| d.child(dashes(t.text).absolute().left(px(0.)).right(px(0.)).bottom(px(y(m))))),
                            ),
                    )
                    .child(div().flex().pl(px(AXIS)).gap(px(4.)).text_size(px(11.)).text_color(t.dim).children(months))
                    .into_any_element()
            };
            let key = |swatch: Div, label: String| div().flex().items_center().gap(px(6.)).child(swatch).child(label);
            let legend = div()
                .flex()
                .flex_wrap()
                .gap_x(px(16.))
                .gap_y(px(4.))
                .pl(px(AXIS))
                .text_size(px(12.))
                .text_color(t.soft)
                .when(sports.len() > 1, |d| d.children(sports.iter().enumerate().map(|(i, s)| key(div().size(px(8.)).rounded(px(2.)).bg(shade(i)), s.label().into()))))
                .when(top > 0.0, |d| d.child(key(div().size(px(8.)).rounded(px(2.)).border_1().border_dashed().border_color(t.accent), "semaine en cours".into())))
                .when_some(median, |d, m| d.child(key(dashes(t.text).w(px(14.)), format!("médiane : {}", metric.fmt(m)))));
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(div().flex().flex_wrap().items_baseline().gap_x(px(10.)).child(heading(format!("{} par semaine", metric.label()))).child(div().text_size(px(12.)).text_color(t.dim).child(format!("{WEEKS} semaines"))))
                .child(body)
                .child(legend)
        };

        // Up to a year, day by day: how much time each day held. As many weeks as fit.
        let regularity = {
            let gap = 3.0;
            let names_w = 30.0;
            let room = inner - names_w;
            let n = (((room + gap) / (8.0 + gap)).floor() as usize).clamp(4, HEAT_WEEKS);
            // The width is filled: the cells widen, their height stops at 18 px. Whole pixels: the
            // cells are snapped to them, and fractions would add up past the width.
            let cell_w = ((room + gap) / n as f32 - gap).max(8.0).floor();
            let cell = cell_w.min(18.0);
            let days = cal::by_day(acts, sport);
            let seconds = |d: &NaiveDate| days.get(d).map_or(0.0, |l| l.iter().map(|i| acts[*i].duration.unwrap_or(0.0).max(0.0)).sum::<f64>());
            let mondays = dash::mondays(today, n);
            let shown: Vec<NaiveDate> = mondays.iter().flat_map(|m| (0..7).map(move |k| *m + Days::new(k))).filter(|d| *d <= today).collect();
            let busiest = shown.iter().map(seconds).fold(0.0_f64, f64::max);
            let active = shown.iter().filter(|d| days.contains_key(d)).count();
            let fill = |level: u8| if level == 0 { blend(t.border, t.bg, 0.7) } else { blend(t.accent, t.bg, 0.25 * level as f32) };
            let columns = mondays.iter().enumerate().map(|(w, monday)| {
                div().flex().flex_col().gap(px(gap)).children((0..7u64).map(|k| {
                    let day = *monday + Days::new(k);
                    let slot = div().w(px(cell_w)).h(px(cell)).flex_none();
                    if day > today {
                        return slot.into_any_element();
                    }
                    let list = days.get(&day).cloned().unwrap_or_default();
                    let secs = seconds(&day);
                    let tip = if list.is_empty() { format!("{} : repos", fr_short(day)) } else { format!("{} : {} · {}", fr_short(day), count(list.len(), "sortie", "sorties"), fmt_duration(secs)) };
                    let first = list.first().map(|i| acts[*i].activity_id);
                    slot.id(("dash-day", w * 7 + k as usize))
                        .rounded(px(2.))
                        .bg(fill(cal::level(secs, busiest)))
                        .when(day == today, |d| d.border_1().border_color(t.text))
                        .tooltip_show_delay(crate::ui::TIP_DELAY).tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                        .when_some(first, |d, id| d.cursor_pointer().on_click(cx.listener(move |_, _, _, cx| cx.emit(Page::Activity(id)))))
                        .into_any_element()
                }))
            });
            let months = mondays.iter().map(|m| {
                let first = (0..7).map(|k| *m + Days::new(k)).find(|d| d.day() == 1);
                div().w(px(cell_w)).flex_none().whitespace_nowrap().children(first.map(|d| month_short(d.month0())))
            });
            let names = ["lun", "", "mer", "", "ven", "", ""];
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_baseline()
                        .gap_x(px(10.))
                        .child(heading("Régularité"))
                        .child(div().text_size(px(12.)).text_color(t.dim).child(format!("{} sur les {n} dernières semaines", count(active, "jour actif", "jours actifs")))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .text_size(px(11.))
                        .text_color(t.dim)
                        .child(div().flex().pl(px(names_w)).gap(px(gap)).children(months))
                        .child(
                            div()
                                .flex()
                                .gap(px(gap))
                                .child(div().w(px(names_w - gap)).flex_none().flex().flex_col().gap(px(gap)).children(names.map(|n| div().h(px(cell)).flex().items_center().child(n))))
                                .children(columns),
                        )
                        .child(div().flex().items_center().gap(px(4.)).pl(px(names_w)).pt(px(4.)).child("repos").children((0..=4u8).map(|l| div().size(px(10.)).rounded(px(2.)).bg(fill(l)))).child("plus de temps")),
                )
        };

        // The latest outings, and the records of the last two weeks. Narrow, the day gives way to the name.
        let narrow = inner < 480.0;
        let recent = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div().flex().items_baseline().justify_between().pb(px(6.)).child(heading("Dernières sorties")).child(
                    div()
                        .id("dash-journal")
                        .text_size(px(12.))
                        .text_color(t.soft)
                        .cursor_pointer()
                        .hover(|d| d.text_color(t.accent))
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(Page::Journal)))
                        .child("tout le journal"),
                ),
            )
            .children(dash::recent(acts, sport, RECENT).into_iter().map(|i| {
                let a = &acts[i];
                let id = a.activity_id;
                let distance = a.distance.filter(|d| *d > 0.0 && a.sport().has_distance()).map(fmt_km).unwrap_or_default();
                let time = a.duration.filter(|d| *d > 0.0).map(fmt_duration).unwrap_or_default();
                outing_row(("dash-recent", i), a.date().map(fr_short).filter(|_| !narrow), a.sport(), name(a), [distance, time], t).on_click(cx.listener(move |_, _, _, cx| cx.emit(Page::Activity(id))))
            }));
        let since = today - Days::new(FRESH - 1);
        let fresh: Vec<Broken> = records::broken(acts, &self.snap.derived, sport, 5).into_iter().filter(|b| b.entry.date >= since).collect();
        let records = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(div().pb(px(6.)).child(heading("Records de ces deux semaines")))
            .when(fresh.is_empty(), |d| d.child(div().text_color(t.soft).child("Aucun record battu ces deux dernières semaines.")))
            .children(fresh.into_iter().enumerate().map(|(k, b)| {
                let id = b.entry.id;
                let (now, _) = records::show(b.kind, b.sport, b.entry.value);
                let (was, _) = records::show(b.kind, b.sport, b.previous);
                outing_row(("dash-record", k), Some(fr_short(b.entry.date)).filter(|_| !narrow), b.sport, format!("{} · {}", b.row, b.board), [now, format!("avant {was}")], t)
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(Page::Activity(id))))
            }));
        let bottom = div().flex().when(inner < 860.0, |d| d.flex_col()).gap(px(40.)).child(recent.flex_1().min_w_0()).child(records.flex_1().min_w_0());

        page("dashboard", width)
            .track_scroll(self.width.scroll())
            .gap(px(36.))
            .child(head)
            .child(figures)
            .child(chart)
            .child(regularity)
            .child(bottom)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Metric, count, tick_label};

    #[test]
    fn counts_and_grid_labels() {
        assert_eq!(count(0, "semaine", "semaines"), "0\u{a0}semaine");
        assert_eq!(count(1, "jour actif", "jours actifs"), "1\u{a0}jour actif");
        assert_eq!(count(1_200, "jour actif", "jours actifs"), "1\u{a0}200\u{a0}jours actifs");
        assert_eq!(tick_label(Metric::Distance, 10.0), "10\u{a0}km");
        assert_eq!(tick_label(Metric::Distance, 2.5), "2,5\u{a0}km");
        assert_eq!(tick_label(Metric::Distance, 1000.0), "1\u{a0}000\u{a0}km");
        assert_eq!(tick_label(Metric::Time, 5.0), "5h");
        assert_eq!(tick_label(Metric::Time, 0.5), "30min");
    }
}
