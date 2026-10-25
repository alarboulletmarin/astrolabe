//! Form: what the outings say of load, fatigue and freshness, from their heart rate. A statement
//! of figures with how they are worked out: no advice, no verdict.

use crate::data::{Data, Snapshot};
use crate::lang::{group, pct, plural, tr, trf};
use crate::logic::form::{self as model, Day, FATIGUE_DAYS, FORM_DAYS, Findings, Form, NEAR, Reading, TREND_DAYS};
use crate::ui::chart::{Series, line_chart, nearest};
use crate::ui::format::day_label;
use crate::ui::theme::blend;
use crate::ui::{Plots, heading, num, pad, page, segmented, title};
use crate::views::dashboard::stat;
use crate::views::{Page, Width, today, tracks_progress};
use chrono::{Days, NaiveDate};
use gpui_kit::{prelude::FluentBuilder, *};
use std::{collections::HashMap, sync::Arc};

/// Shorter series draw no curve: a week is the least that says anything.
const MIN_DAYS: usize = 7;
/// Height of the two charts, in pixels.
const LEVELS: f32 = 150.0;
const FRESHNESS: f32 = 120.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Period {
    Months3,
    Months6,
    Year,
    All,
}

const PERIODS: [Period; 4] = [Period::Months3, Period::Months6, Period::Year, Period::All];

impl Period {
    fn label(self) -> &'static str {
        match self {
            Period::Months3 => tr("3 months", "3 mois"),
            Period::Months6 => tr("6 months", "6 mois"),
            Period::Year => tr("1 year", "1 an"),
            Period::All => tr("All", "Tout"),
        }
    }

    fn since(self, today: NaiveDate) -> Option<NaiveDate> {
        let days = match self {
            Period::Months3 => 91,
            Period::Months6 => 182,
            Period::Year => 365,
            Period::All => return None,
        };
        today.checked_sub_days(Days::new(days))
    }
}

/// The series worked out from what the activities hold; kept while neither changes.
struct Computed {
    acts: Arc<Vec<crate::logic::activity::Activity>>,
    derived: Arc<HashMap<i64, crate::logic::fit::Derived>>,
    on: NaiveDate,
    form: Form,
}

/// What the charts draw for a period, worked out once: a hover repaints the page at every move.
struct Drawn {
    period: Period,
    dates: Vec<NaiveDate>,
    xs: Arc<Vec<f32>>,
    atl: Arc<Vec<Option<f32>>>,
    ctl: Arc<Vec<Option<f32>>>,
    tsb: Arc<Vec<Option<f32>>>,
    zero: Arc<Vec<Option<f32>>>,
    levels: Option<(f32, f32)>,
    freshness: Option<(f32, f32)>,
    findings: Option<Findings>,
}

impl Drawn {
    fn new(form: &Form, period: Period, today: NaiveDate) -> Self {
        let since = period.since(today);
        let shown = form.shown(since);
        let col = |f: fn(&Day) -> f64| Arc::new(shown.iter().map(|d| Some(f(d) as f32)).collect::<Vec<_>>());
        let (atl, ctl, tsb) = (col(|d| d.atl), col(|d| d.ctl), col(|d| d.tsb));
        let extent = |ys: &[&Arc<Vec<Option<f32>>>]| ys.iter().flat_map(|y| y.iter().flatten().copied()).fold((0.0f32, 0.0f32), |(lo, hi), v| (lo.min(v), hi.max(v)));
        // One scale for both lines of the first chart, and the zero kept in the second.
        let (_, top) = extent(&[&atl, &ctl]);
        let (lo, hi) = extent(&[&tsb]);
        Drawn {
            period,
            dates: shown.iter().map(|d| d.date).collect(),
            xs: Arc::new((0..shown.len()).map(|i| i as f32).collect()),
            zero: Arc::new(vec![Some(0.0); shown.len()]),
            levels: Some((0.0, top)),
            freshness: Some((lo, hi)),
            atl,
            ctl,
            tsb,
            findings: model::findings(form, since),
        }
    }
}

pub struct FormView {
    data: Entity<Data>,
    snap: Snapshot,
    width: Width,
    period: Period,
    /// The day under the pointer, as an index in the days shown.
    hover: Option<usize>,
    computed: Option<Computed>,
    drawn: Option<Drawn>,
    plots: Plots,
    _subscription: Subscription,
}

impl EventEmitter<Page> for FormView {}

impl FormView {
    pub fn new(data: Entity<Data>, width: Width, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&data, |this, data, cx| {
            this.snap = data.read(cx).snapshot();
            cx.notify();
        });
        FormView { snap: data.read(cx).snapshot(), data, width, period: Period::Months6, hover: None, computed: None, drawn: None, plots: Plots::default(), _subscription: sub }
    }

    /// Works the series out again only when the activities, their tracks or the day changed.
    fn refresh(&mut self, today: NaiveDate) {
        let fresh = self.computed.as_ref().is_some_and(|c| Arc::ptr_eq(&c.acts, &self.snap.acts) && Arc::ptr_eq(&c.derived, &self.snap.derived) && c.on == today);
        if !fresh {
            let form = model::form(&self.snap.acts, &self.snap.derived, today);
            self.computed = Some(Computed { acts: self.snap.acts.clone(), derived: self.snap.derived.clone(), on: today, form });
            self.drawn = None;
        }
        if self.drawn.as_ref().is_none_or(|d| d.period != self.period) {
            let form = &self.computed.as_ref().expect("computed above").form;
            self.drawn = Some(Drawn::new(form, self.period, today));
        }
    }
}

/// "+12", "−99", "0": the sign of a freshness, never "-0".
fn signed(v: f64) -> String {
    let r = v.round();
    match r {
        r if r == 0.0 || !r.is_finite() => "0".into(),
        r if r < 0.0 => format!("−{}", group(-r as i64)),
        r => format!("+{}", group(r as i64)),
    }
}

fn points(v: f64) -> String {
    group(v.round() as i64)
}

/// "1 point", "513 points" ("0 point" in French).
fn pts(v: f64) -> String {
    format!("{} {}", points(v), plural(v.round().max(0.0) as usize, "point", "points"))
}

fn percent(v: f64) -> String {
    format!("{}{}", group(v.abs().round() as i64), pct())
}

/// A square of a line's color, its name, and its value under the pointer.
fn legend(color: Hsla, name: &'static str, value: String, t: crate::ui::theme::Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .child(div().size(px(8.)).flex_none().rounded(px(2.)).bg(color))
        .child(div().text_color(t.soft).child(name))
        .child(num(value).font_weight(FontWeight::BOLD))
}

/// The statements, one sentence each, in the order of the figures. No advice among them.
fn sentences(f: &Findings, since_label: &str) -> Vec<String> {
    let mut out = Vec::new();
    let near = (NEAR * 100.0) as i64;
    out.push(match (f.reading, f.ratio) {
        (Some(Reading::Above), Some(r)) => trf(
            format!("Fatigue (over {FATIGUE_DAYS:.0} days) is {} above form (over {FORM_DAYS:.0} days).", percent((r - 1.0) * 100.0)),
            format!("La fatigue (sur {FATIGUE_DAYS:.0} jours) dépasse la forme (sur {FORM_DAYS:.0} jours) de {}.", percent((r - 1.0) * 100.0)),
        ),
        (Some(Reading::Below), Some(r)) => trf(
            format!("Fatigue (over {FATIGUE_DAYS:.0} days) is {} below form (over {FORM_DAYS:.0} days).", percent((1.0 - r) * 100.0)),
            format!("La fatigue (sur {FATIGUE_DAYS:.0} jours) est inférieure à la forme (sur {FORM_DAYS:.0} jours) de {}.", percent((1.0 - r) * 100.0)),
        ),
        (Some(Reading::Near), _) => trf(
            format!("Fatigue (over {FATIGUE_DAYS:.0} days) is within {near}% of form (over {FORM_DAYS:.0} days)."),
            format!("La fatigue (sur {FATIGUE_DAYS:.0} jours) est à moins de {near} % de la forme (sur {FORM_DAYS:.0} jours)."),
        ),
        _ => tr("The form is nil: no ratio between fatigue and form.", "La forme est nulle : pas de rapport entre fatigue et forme.").into(),
    });
    out.push(match f.form_change {
        Some(c) if c.round() == 0.0 => trf(
            format!("Over {TREND_DAYS} days, form has not changed ({}).", pts(f.today.ctl)),
            format!("Sur {TREND_DAYS} jours, la forme n'a pas changé ({}).", pts(f.today.ctl)),
        ),
        Some(c) => {
            let (from, n) = (points(f.today.ctl - c), points(c.abs()));
            // The verb is chosen by `tr`; the sentence of the other language is dropped by `trf`.
            let verb = if c > 0.0 { tr("risen by", "augmenté de") } else { tr("fallen by", "diminué de") };
            trf(
                format!("Over {TREND_DAYS} days, form has {verb} {n} {} (from {from} to {}).", if n == "1" { "point" } else { "points" }, points(f.today.ctl)),
                format!("Sur {TREND_DAYS} jours, la forme a {verb} {n} point{} (de {from} à {}).", if n == "1" { "" } else { "s" }, points(f.today.ctl)),
            )
        }
        None => trf(format!("Less than {} days of data: no change of form over {TREND_DAYS} days.", TREND_DAYS + 1), format!("Moins de {} jours de données : pas d'évolution de la forme sur {TREND_DAYS} jours.", TREND_DAYS + 1)),
    });
    let w = f.weeks;
    let against = match w.change() {
        Some(c) => format!("{}{}", if c < 0.0 { "−" } else { "+" }, percent(c)),
        None => tr("no load last week", "pas de charge la semaine précédente").into(),
    };
    out.push(trf(
        format!("This week ({} {} elapsed): {} of load, against {} the week before ({against}).", w.elapsed, plural(w.elapsed as usize, "day", "days"), pts(w.this), points(w.last)),
        format!("Cette semaine ({} {} écoulé{}) : {} de charge, contre {} la semaine précédente ({against}).", w.elapsed, plural(w.elapsed as usize, "jour", "jours"), if w.elapsed > 1 { "s" } else { "" }, pts(w.this), points(w.last)),
    ));
    let today = crate::views::today();
    out.push(trf(
        format!("{since_label}: highest freshness {} on {}, highest fatigue {} on {}.", signed(f.freshest.tsb), day_label(f.freshest.date, today), points(f.most_tired.atl), day_label(f.most_tired.date, today)),
        format!("{since_label} : fraîcheur la plus haute {} le {}, fatigue la plus haute {} le {}.", signed(f.freshest.tsb), day_label(f.freshest.date, today), points(f.most_tired.atl), day_label(f.most_tired.date, today)),
    ));
    out
}

impl Render for FormView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = self.width.get(window);
        let t = self.snap.theme;
        let today = today();
        self.refresh(today);
        let bounds = self.plots.get("form");
        let (form, drawn) = (&self.computed.as_ref().expect("refreshed").form, self.drawn.as_ref().expect("refreshed"));
        let items = PERIODS.map(|p| (p.label().into(), p == self.period)).to_vec();
        let filter = segmented("form-period", items, t, cx, |this: &mut FormView, i, _, _| {
            this.period = PERIODS[i];
            this.hover = None;
        });
        let head = div()
            .flex()
            .flex_wrap()
            .items_end()
            .justify_between()
            .gap(px(14.))
            .child(
                div().flex().flex_col().gap(px(4.)).max_w(px(width - 2.0 * pad(width))).child(title(tr("Form", "Forme"))).child(div().text_color(t.soft).child(tr(
                    "What the heart rate of the outings says of load, fatigue and freshness: figures, not advice.",
                    "Ce que la fréquence cardiaque des sorties dit de la charge, de la fatigue et de la fraîcheur : des chiffres, pas des conseils.",
                ))),
            )
            .when(!form.days.is_empty(), |d| d.child(filter));
        let base = page("form", width).track_scroll(self.width.scroll()).child(head);
        let waiting = self.snap.derived.len() < self.snap.acts.len();
        let Some(found) = drawn.findings.filter(|_| !form.days.is_empty()) else {
            // Nothing to work out: say why.
            let why = if self.snap.acts.is_empty() {
                tr("No outing yet.", "Pas encore de sortie.")
            } else if form.hr_max.is_none() {
                tr(
                    "No maximum heart rate is known yet: the tracks are still being read, or none holds a heart rate.",
                    "Aucune fréquence cardiaque maximale n'est connue : les traces sont en cours de lecture, ou aucune ne contient de fréquence cardiaque.",
                )
            } else {
                tr("No outing with a heart rate yet: no load can be worked out.", "Aucune sortie avec fréquence cardiaque : aucune charge ne peut être calculée.")
            };
            return base
                .child(div().id("form-empty").debug_selector(|| "form-empty".into()).max_w(px(640.)).text_color(t.soft).child(why))
                .when(waiting && !self.snap.acts.is_empty(), |d| d.child(div().max_w(px(560.)).child(tracks_progress(&self.data, cx))))
                .when(!self.snap.acts.is_empty(), |d| d.child(self.source(form, t)));
        };

        let figures = div()
            .id("form-figures")
            .debug_selector(|| "form-figures".into())
            .flex()
            .flex_wrap()
            .gap(px(28.))
            .child(stat(tr("Freshness today", "Fraîcheur du jour").into(), signed(found.today.tsb), [tr("form minus fatigue, end of day", "forme moins fatigue, en fin de journée").into(), String::new()], t))
            .child(stat(
                trf(format!("Fatigue, {FATIGUE_DAYS:.0} days"), format!("Fatigue, {FATIGUE_DAYS:.0} jours")),
                format!("{} {}", points(found.today.atl), tr("pts", "pts")),
                [tr("load averaged over about a week", "charge moyennée sur environ une semaine").into(), String::new()],
                t,
            ))
            .child(stat(
                trf(format!("Form, {FORM_DAYS:.0} days"), format!("Forme, {FORM_DAYS:.0} jours")),
                format!("{} {}", points(found.today.ctl), tr("pts", "pts")),
                [tr("load averaged over about six weeks", "charge moyennée sur environ six semaines").into(), String::new()],
                t,
            ));

        let since_label = match drawn.period {
            Period::All => tr("Since the start", "Depuis le début").to_string(),
            p => trf(format!("Over {}", p.label()), format!("Sur {}", p.label())),
        };
        let statements = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .max_w(px(760.))
            .when(!form.settled(), |d| {
                d.child(div().text_color(t.soft).child(trf(
                    format!("Less than {FORM_DAYS:.0} days of data: the form over {FORM_DAYS:.0} days is not yet established."),
                    format!("Moins de {FORM_DAYS:.0} jours de données : la forme sur {FORM_DAYS:.0} jours n'est pas encore établie."),
                )))
            })
            .children(sentences(&found, &since_label).into_iter().map(|s| div().child(s)));

        let charts = if drawn.dates.len() < MIN_DAYS {
            div().text_color(t.soft).child(trf(
                format!("Less than {MIN_DAYS} days in this period: no curve."),
                format!("Moins de {MIN_DAYS} jours sur cette période : pas de courbe."),
            )).into_any_element()
        } else {
            self.charts(drawn, bounds, cx).into_any_element()
        };
        base.child(figures).child(statements).child(charts).child(self.source(form, t))
    }
}

impl FormView {
    /// The two charts on one time axis, with the pointer shared between them.
    fn charts(&self, d: &Drawn, bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = self.snap.theme;
        let hover = self.hover.filter(|i| *i < d.dates.len());
        let at = |ys: &Arc<Vec<Option<f32>>>| hover.and_then(|i| ys.get(i).copied().flatten()).map(|v| v as f64);
        let label = hover.map(|i| day_label(d.dates[i], today())).unwrap_or_default();
        let (first, last) = (d.dates.first().copied(), d.dates.last().copied());
        let fatigue_color = t.soft;
        let invisible = hsla(0., 0., 0., 0.);
        let series = |ys: &Arc<Vec<Option<f32>>>, scale, line, fill| Series { xs: d.xs.clone(), ys: ys.clone(), scale, line, fill, invert: false };
        let chart = |s: Series, h: f32| div().h(px(h)).w_full().child(line_chart(s, hover, t.soft).size_full());
        let layer = |s: Series, hover| div().absolute().top_0().left_0().size_full().child(line_chart(s, hover, t.soft).size_full());

        let levels = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().flex().justify_between().child(heading(tr("Form and fatigue", "Forme et fatigue"))).child(num(label).text_size(px(12.)).text_color(t.soft)))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_x(px(18.))
                    .child(legend(t.accent, tr("Form", "Forme"), at(&d.ctl).map(points).unwrap_or_default(), t))
                    .child(legend(fatigue_color, tr("Fatigue", "Fatigue"), at(&d.atl).map(points).unwrap_or_default(), t)),
            )
            .child(
                div()
                    .relative()
                    .h(px(LEVELS))
                    .w_full()
                    .child(chart(series(&d.ctl, d.levels, t.accent, blend(t.accent, t.bg, 0.12)), LEVELS))
                    .child(layer(series(&d.atl, d.levels, fatigue_color, invisible), hover)),
            );
        let fresh = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(heading(tr("Freshness", "Fraîcheur")))
            .child(div().flex().child(legend(t.text, tr("Form minus fatigue", "Forme moins fatigue"), at(&d.tsb).map(signed).unwrap_or_default(), t)))
            .child(
                div()
                    .relative()
                    .h(px(FRESHNESS))
                    .w_full()
                    .child(chart(series(&d.tsb, d.freshness, t.text, invisible), FRESHNESS))
                    // The zero, drawn on the same scale.
                    .child(layer(series(&d.zero, d.freshness, t.dim, invisible), None)),
            );
        let (xs, cell) = (d.xs.clone(), bounds.clone());
        div()
            .id("form-area")
            .relative()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(canvas(move |b, _, _| cell.set(b), |_, _, _, _| {}).absolute().size_full())
            .child(levels)
            .child(fresh)
            .child(div().flex().justify_between().text_size(px(12.)).text_color(t.soft).child(num(first.map(|f| day_label(f, today())).unwrap_or_default())).child(num(last.map(|l| day_label(l, today())).unwrap_or_default())))
            .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                let b = bounds.get();
                if b.contains(&e.position) && b.size.width > px(0.) {
                    let f = ((e.position.x - b.origin.x) / b.size.width).clamp(0.0, 1.0);
                    let top = xs.last().copied().unwrap_or(0.0);
                    this.hover = nearest(&xs, f * top);
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|this, on: &bool, _, cx| {
                if !on {
                    this.hover = None;
                    cx.notify();
                }
            }))
    }

    /// How the figures are worked out, and what they leave out.
    fn source(&self, form: &Form, t: crate::ui::theme::Theme) -> Div {
        let max = match form.hr_max {
            Some(m) => trf(
                format!("Maximum heart rate estimated at {m:.0} bpm: the second highest value held over any outing, so that one stray reading of the sensor does not set it."),
                format!("Fréquence cardiaque maximale estimée à {m:.0} bpm : la deuxième valeur la plus haute relevée sur l'ensemble des sorties, pour qu'un pic isolé du capteur ne la fixe pas."),
            ),
            None => tr("Maximum heart rate: not known yet.", "Fréquence cardiaque maximale : pas encore connue.").into(),
        };
        let kinds = trf(
            format!(
                "{} {} counted{}{}.",
                group(form.counted as i64),
                plural(form.counted, "outing", "outings"),
                if form.estimated > 0 { format!(", of which {} estimated from the average heart rate alone", group(form.estimated as i64)) } else { String::new() },
                if form.skipped > 0 { format!("; {} {} without heart rate, left out", group(form.skipped as i64), plural(form.skipped, "outing", "outings")) } else { String::new() },
            ),
            format!(
                "{} {} comptée{}{}{}.",
                group(form.counted as i64),
                plural(form.counted, "sortie", "sorties"),
                if form.counted > 1 { "s" } else { "" },
                if form.estimated > 0 { format!(", dont {} estimée{} par la fréquence cardiaque moyenne seule", group(form.estimated as i64), if form.estimated > 1 { "s" } else { "" }) } else { String::new() },
                if form.skipped > 0 { format!(" ; {} {} sans fréquence cardiaque, non comptée{}", group(form.skipped as i64), plural(form.skipped, "sortie", "sorties"), if form.skipped > 1 { "s" } else { "" }) } else { String::new() },
            ),
        );
        let text = [
            tr(
                "Each outing is given points of load: the minutes spent in each zone of heart rate, weighted from 1 (50-60% of the maximum) to 5 (90% and above); under 50%, nothing. The points of a day are added up, rest days count for zero.",
                "Chaque sortie reçoit des points de charge : les minutes passées dans chaque zone de fréquence cardiaque, pondérées de 1 (50-60 % du maximum) à 5 (90 % et plus) ; sous 50 %, rien. Les points d'un jour s'additionnent, un jour de repos vaut zéro.",
            ).to_string(),
            trf(
                format!("Fatigue is the daily load averaged over about {FATIGUE_DAYS:.0} days, form over about {FORM_DAYS:.0} days (each day weighs a little less than the one after it). Freshness is form minus fatigue, both at the end of the same day."),
                format!("La fatigue est la charge quotidienne moyennée sur environ {FATIGUE_DAYS:.0} jours, la forme sur environ {FORM_DAYS:.0} jours (chaque jour pèse un peu moins que le suivant). La fraîcheur est la forme moins la fatigue, toutes deux en fin de journée."),
            ),
            max,
            kinds,
            tr(
                "An estimate from heart rate alone: neither power nor pace is used, and a heart rate shaped by heat, sleep or illness is read like any other.",
                "Une estimation à partir de la seule fréquence cardiaque : ni la puissance ni l'allure ne sont utilisées, et une fréquence cardiaque modifiée par la chaleur, le sommeil ou la maladie est lue comme les autres.",
            ).to_string(),
        ]
        .join(" ");
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_color(t.soft)
            .child(heading(tr("Where these figures come from", "D'où viennent ces chiffres")).text_color(t.text))
            .child(text)
    }
}

#[cfg(test)]
mod tests {
    use super::{Drawn, Form, Period, signed};
    use crate::logic::form::Day;
    use chrono::{Days, NaiveDate};

    fn series(n: usize, today: NaiveDate) -> Form {
        let days = (0..n).map(|i| Day { date: today - Days::new((n - 1 - i) as u64), load: 50.0, atl: 40.0 + i as f64, ctl: 30.0, tsb: -10.0 - i as f64 }).collect();
        Form { days, ..Form::default() }
    }

    #[test]
    fn every_period_cuts_the_series_and_keeps_the_scales_sound() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
        for n in [0, 1, 6, 7, 91, 92, 400] {
            let form = series(n, today);
            for (period, shown) in [(Period::Months3, 92), (Period::Months6, 183), (Period::Year, 366), (Period::All, usize::MAX)] {
                let d = Drawn::new(&form, period, today);
                assert_eq!(d.dates.len(), n.min(shown), "{n} days over {}", period.label());
                assert!(d.xs.len() == d.dates.len() && d.atl.len() == d.dates.len() && d.zero.len() == d.dates.len());
                // The scales hold the zero and are finite, even with nothing to draw.
                for (lo, hi) in [d.levels, d.freshness].into_iter().flatten() {
                    assert!(lo.is_finite() && hi.is_finite() && lo <= 0.0 && hi >= 0.0);
                }
                assert_eq!(d.findings.is_some(), n > 0);
            }
        }
    }

    #[test]
    fn signs_never_show_a_negative_zero() {
        let cases = [(0.0, "0"), (-0.4, "0"), (0.4, "0"), (0.5, "+1"), (-0.5, "−1"), (-99.0, "−99"), (99.0, "+99"), (-12345.6, "−12\u{a0}346"), (1e6, "+1\u{a0}000\u{a0}000"), (f64::NAN, "0")];
        for (v, text) in cases {
            assert_eq!(signed(v), text, "{v}");
        }
    }
}
