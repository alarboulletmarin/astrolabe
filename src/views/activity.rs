//! An activity's page: its figures, its map, its profiles (pace, heart rate, altitude) sharing
//! one pointer, its kilometre splits and laps, and where it stands among the previous ones.

use crate::app::{Home, TrackState, View};
use crate::logic::activity::{self as act, Activity, Metric, Sport, fmt_clock, fmt_km, fmt_m};
use crate::logic::fit::Track;
use crate::logic::journal::name;
use crate::logic::profiles::profiles;
use crate::ui::chart::{Series, line_chart, nearest};
use crate::ui::format::{fr_long, fr_short};
use crate::ui::theme::blend;
use crate::ui::{button, figure, heading, icon, num, page, sport_tag, title};
use crate::views::map::{Which, line_bbox};
use gpui_kit::assets::IconName;
use gpui_kit::{prelude::FluentBuilder, *};
use std::sync::Arc;

fn speed_fmt(v: f32, sport: Sport) -> String {
    match sport {
        Sport::Bike => format!("{v:.1} km/h").replace('.', ","),
        Sport::Swim => format!("{} /100m", fmt_clock(v as f64)),
        _ => format!("{} /km", fmt_clock(v as f64)),
    }
}

impl Home {
    pub fn activity_page(&mut self, id: i64, width: f32, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let Some(a) = self.activity(id).cloned() else {
            return page("activity", width).child(div().text_color(t.soft).child("Cette activité n'existe plus.")).into_any_element();
        };
        let s = a.sport();
        let wide = width >= 1180.0;
        let loaded = self.track.as_ref().filter(|l| l.id == id).map(|l| (l.track.clone(), l.splits.clone()));
        let older = self.index_of(id).and_then(|i| self.acts.get(i + 1)).is_some();
        let newer = self.index_of(id).is_some_and(|i| i > 0);

        let nav = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .id("back")
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_color(t.soft)
                    .cursor_pointer()
                    .hover(|d| d.text_color(t.text))
                    .on_click(cx.listener(|this, _, window, cx| this.set_view(View::Journal, window, cx)))
                    .child(icon(IconName::ChevronLeft, t.soft, 14.))
                    .child("Journal"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .when(older, |d| d.child(button("older", "‹ Plus ancienne", false, t).on_click(cx.listener(|this, _, w, cx| this.step_activity(true, w, cx)))))
                    .when(newer, |d| d.child(button("newer", "Plus récente ›", false, t).on_click(cx.listener(|this, _, w, cx| this.step_activity(false, w, cx))))),
            );

        let (rank, of) = act::rank(&self.acts, &a);
        let rank_text = match (rank, of) {
            (_, 1) => "Ta première dans ce sport.".to_string(),
            (1, n) => format!("Ta plus longue, sur {n}."),
            (r, n) if r == n => format!("Ta plus courte, sur {n}."),
            (r, n) => format!("{r}e plus longue sur {n}."),
        };
        let head = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(title(name(&a)))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(10.))
                    .child(sport_tag(s, t))
                    .child(num(a.start().map(fr_long).unwrap_or_default()).text_size(px(12.)).text_color(t.soft))
                    .child(div().text_color(t.soft).child(rank_text)),
            );

        let mut figures = div().flex().flex_wrap().gap_x(px(40.)).gap_y(px(14.));
        if let Some(d) = a.distance.filter(|d| *d > 0.0) {
            figures = figures.child(figure("Distance", fmt_km(d), t));
        }
        if let Some(d) = a.duration {
            figures = figures.child(figure("Temps", fmt_clock(d), t));
        }
        if let Some(v) = a.speed_label() {
            figures = figures.child(figure(if s == Sport::Bike { "Vitesse" } else { "Allure" }, v, t));
        }
        if let Some(e) = a.elevation_gain.filter(|e| *e >= 1.0) {
            figures = figures.child(figure("Dénivelé", fmt_m(e), t));
        }
        if let Some(hr) = a.average_hr {
            figures = figures.child(figure("FC moyenne", format!("{hr:.0} bpm"), t));
        }
        if let Some(c) = a.calories.filter(|c| *c > 0.0) {
            figures = figures.child(figure("Calories", format!("{} kcal", act::group(c.round() as i64)), t));
        }

        let map = self.activity_map(id, loaded.as_ref().map(|l| l.0.clone()), wide, cx);
        let charts = match &loaded {
            Some((track, _)) => self.profile_charts(track, s, cx).into_any_element(),
            None => div().into_any_element(),
        };
        let side = div()
            .flex()
            .flex_col()
            .gap(px(28.))
            .when_some(loaded.as_ref(), |d, (track, splits)| d.child(self.splits_table(splits, track, s)))
            .child(self.history(&a, cx));

        let body = if wide {
            div()
                .flex()
                .gap(px(32.))
                .child(div().flex_1().min_w_0().flex().flex_col().gap(px(24.)).child(map).child(charts))
                .child(div().w(px(380.)).flex_none().child(side))
        } else {
            div().flex().flex_col().gap(px(24.)).child(map).child(charts).child(side)
        };

        page("activity", width).gap(px(22.)).child(nav).child(head).child(figures).child(body).into_any_element()
    }

    fn activity_map(&mut self, id: i64, track: Option<Arc<Track>>, wide: bool, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let height = if wide { 460. } else { 340. };
        let placeholder = |text: String, retry: bool, cx: &mut Context<Self>| {
            div()
                .h(px(height))
                .rounded(px(8.))
                .bg(t.panel)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .text_color(t.soft)
                .child(text)
                .when(retry, |d| {
                    d.child(button("retry", "Réessayer", false, t).on_click(cx.listener(move |this, _, _, cx| {
                        this.track_state = TrackState::None;
                        this.load_track(id, cx);
                        cx.notify();
                    })))
                })
        };
        let Some(track) = track else {
            return match &self.track_state {
                TrackState::Failed(x, e) if *x == id => placeholder(e.clone(), true, cx),
                _ => placeholder("Lecture de la trace…".into(), false, cx),
            };
        };
        let line: Vec<[f32; 2]> = track.positions().map(|p| [p[0] as f32, p[1] as f32]).collect();
        if line.len() < 2 {
            return placeholder("Pas de position GPS : activité en salle, ou sans GPS.".into(), false, cx);
        }
        let bbox = line_bbox(&line);
        self.fit_map(Which::Activity, id as u64, bbox, false);
        let view = self.act_map.view;
        let bounds = self.act_map.bounds.get();
        let hover = self.act_hover.and_then(|i| track.samples.get(i)).and_then(|s| Some([s.lat?, s.lon?]));
        let (accent, ink, bg) = (t.accent, t.text, t.bg);
        let points = Arc::new(line);
        let overlay = canvas(
            |_, _, _| {},
            move |b, _, window, _| {
                let (w, h) = ((bounds.size.width / px(1.)) as f64, (bounds.size.height / px(1.)) as f64);
                let at = |p: [f32; 2]| {
                    let (x, y) = view.screen(p[0] as f64, p[1] as f64, w, h);
                    point(b.origin.x + px(x as f32), b.origin.y + px(y as f32))
                };
                let mut path = PathBuilder::stroke(px(3.));
                path.move_to(at(points[0]));
                points[1..].iter().for_each(|p| path.line_to(at(*p)));
                if let Ok(p) = path.build() {
                    window.paint_path(p, accent);
                }
                let dot = |p: Point<Pixels>, r: f32, c: Hsla, window: &mut Window| {
                    window.paint_quad(fill(Bounds::new(point(p.x - px(r), p.y - px(r)), size(px(2. * r), px(2. * r))), c).corner_radii(px(r)));
                };
                dot(at(points[0]), 6.0, bg, window);
                dot(at(points[0]), 4.0, ink, window);
                dot(at(*points.last().unwrap()), 6.0, bg, window);
                dot(at(*points.last().unwrap()), 4.0, accent, window);
                if let Some(p) = hover {
                    let c = at([p[0] as f32, p[1] as f32]);
                    dot(c, 8.0, bg, window);
                    dot(c, 6.0, ink, window);
                }
            },
        )
        .absolute()
        .size_full()
        .into_any_element();
        div().h(px(height)).rounded(px(8.)).overflow_hidden().child(self.map_widget(Which::Activity, "act-map", Some(overlay), bbox, true, cx))
    }

    fn profile_charts(&mut self, track: &Track, sport: Sport, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let p = profiles(track, sport);
        let hover = self.act_hover;
        let at = |ys: &Arc<Vec<Option<f32>>>| hover.and_then(|i| ys.get(i).copied().flatten());
        let x_label = match hover.and_then(|i| p.xs.get(i)) {
            Some(x) if p.by_distance => format!("km {:.2}", x).replace('.', ","),
            Some(x) => format!("{:.0} min", x),
            None => String::new(),
        };
        let bounds = self.plot("profiles");
        let xs = p.xs.clone();
        let red = t.hue(0.0).1;
        let rows: Vec<(&'static str, Arc<Vec<Option<f32>>>, Hsla, bool, Box<dyn Fn(f32) -> String>)> = vec![
            (
                if sport == Sport::Bike { "Vitesse" } else { "Allure" },
                p.speed.clone(),
                t.accent,
                sport != Sport::Bike,
                Box::new(move |v| speed_fmt(v, sport)),
            ),
            ("Fréquence cardiaque", p.hr.clone(), red, false, Box::new(|v| format!("{v:.0} bpm"))),
            ("Altitude", p.alt.clone(), t.soft, false, Box::new(|v| format!("{v:.0} m"))),
        ];
        let charts: Vec<Div> = rows
            .into_iter()
            .filter(|(_, ys, ..)| ys.iter().flatten().count() > 1)
            .map(|(label, ys, color, invert, fmt)| {
                let value = at(&ys).map(|v| fmt(v)).unwrap_or_default();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_size(px(12.))
                            .child(div().text_color(t.soft).child(label))
                            .child(num(value).font_weight(FontWeight::BOLD)),
                    )
                    .child(
                        div().h(px(96.)).child(
                            line_chart(Series { xs: p.xs.clone(), ys, line: color, fill: blend(color, t.bg, 0.12), invert }, hover, t.soft).size_full(),
                        ),
                    )
            })
            .collect();
        if charts.is_empty() {
            return div();
        }
        let cell = bounds.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(div().flex().justify_between().child(heading("Profil")).child(num(x_label).text_size(px(12.)).text_color(t.soft)))
            .child(
                div()
                    .id("profile-area")
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(canvas(move |b, _, _| cell.set(b), |_, _, _, _| {}).absolute().size_full())
                    .children(charts)
                    .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                        let b = bounds.get();
                        if b.contains(&e.position) && b.size.width > px(0.) {
                            let f = ((e.position.x - b.origin.x) / b.size.width).clamp(0.0, 1.0);
                            let (x0, x1) = (xs.first().copied().unwrap_or(0.0), xs.last().copied().unwrap_or(0.0));
                            this.act_hover = nearest(&xs, x0 + f * (x1 - x0));
                            cx.notify();
                        }
                    }))
                    .on_hover(cx.listener(|this, on: &bool, _, cx| {
                        if !on {
                            this.act_hover = None;
                            cx.notify();
                        }
                    })),
            )
    }

    fn splits_table(&self, splits: &[crate::logic::fit::Split], track: &Track, sport: Sport) -> Div {
        let t = self.theme;
        let swim = sport == Sport::Swim;
        let fastest = splits.iter().filter(|s| s.metres >= 500.0).map(|s| s.seconds / s.metres).fold(f32::INFINITY, f32::min);
        let slowest = splits.iter().filter(|s| s.metres >= 500.0).map(|s| s.seconds / s.metres).fold(0.0, f32::max);
        let cell = |d: Div, w: f32| d.w(px(w)).flex_none().flex().justify_end();
        let mut table = div()
            .flex()
            .flex_col()
            .child(heading(if swim { "Par 100 m" } else { "Par kilomètre" }).mb(px(10.)))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .pb(px(4.))
                    .text_size(px(12.))
                    .text_color(t.dim)
                    .child(div().w(px(34.)).child("Km"))
                    .child(div().flex_1().child(if sport == Sport::Bike { "Vitesse" } else { "Allure" }))
                    .child(cell(div().child("FC"), 56.))
                    .child(cell(div().child("D+"), 48.)),
            );
        let splits_shown: Vec<crate::logic::fit::Split> = if swim { track.splits(100.0) } else { splits.to_vec() };
        if splits_shown.is_empty() {
            return table.child(div().text_color(t.soft).child("Pas de distance enregistrée."));
        }
        for (i, sp) in splits_shown.iter().enumerate() {
            let per = sp.seconds / sp.metres.max(1.0);
            let label = if swim { speed_fmt(per * 100.0, sport) } else if sport == Sport::Bike { speed_fmt(3.6 / per, sport) } else { speed_fmt(per * 1000.0, sport) };
            // Bar length: the faster, the longer; the fastest full km in the accent.
            let share = if slowest > fastest { 0.35 + 0.65 * (slowest - per) / (slowest - fastest) } else { 1.0 };
            let best = (per - fastest).abs() < 1e-6 && sp.metres >= 500.0;
            let km = if sp.metres < (if swim { 99.0 } else { 990.0 }) { format!("{:.2}", sp.metres / 1000.0).replace('.', ",") } else { (i + 1).to_string() };
            table = table.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(26.))
                    .text_size(px(12.))
                    .child(num(km).w(px(34.)).text_color(t.soft))
                    .child(
                        div()
                            .flex_1()
                            .relative()
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .child(div().absolute().left_0().top(px(3.)).h(px(14.)).rounded(px(3.)).w(relative(share.clamp(0.05, 1.0))).bg(if best { blend(t.accent, t.bg, 0.35) } else { t.panel }))
                            .child(num(label).pl(px(6.)).relative()),
                    )
                    .child(cell(num(sp.hr.map(|h| format!("{h:.0}")).unwrap_or_default()), 56.).text_color(t.soft))
                    .child(cell(num(format!("{:.0}", sp.ascent)), 48.).text_color(t.soft)),
            );
        }
        if track.laps.len() > 1 {
            table = table.child(heading("Tours").mt(px(24.)).mb(px(8.))).children(track.laps.iter().enumerate().map(|(i, l)| {
                let per = l.time / l.dist.max(1.0);
                div()
                    .flex()
                    .gap(px(10.))
                    .h(px(24.))
                    .items_center()
                    .text_size(px(12.))
                    .child(num((i + 1).to_string()).w(px(34.)).text_color(t.soft))
                    .child(num(fmt_km(l.dist as f64)).w(px(70.)))
                    .child(num(fmt_clock(l.time as f64)).w(px(70.)))
                    .child(num(if l.dist > 0.0 { speed_fmt(if sport == Sport::Bike { 3.6 / per } else { per * 1000.0 }, sport) } else { String::new() }).flex_1().text_color(t.soft))
                    .child(cell(num(l.hr.map(|h| format!("{h:.0}")).unwrap_or_default()), 56.).text_color(t.soft))
            }));
        }
        table
    }

    /// The activity among the previous ones of its sport: one bar each, a click opens it.
    fn history(&self, a: &Activity, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let s = a.sport();
        let metric = if a.distance.is_some_and(|d| d > 0.0) { Metric::Distance } else { Metric::Time };
        let start = self.index_of(a.activity_id).unwrap_or(0);
        let mut past: Vec<&Activity> = self.acts[start..].iter().filter(|x| x.sport() == s).take(24).collect();
        past.reverse();
        let top = past.iter().map(|x| metric.of(x)).fold(1.0, f64::max);
        let readout = match self.act_hover_history(&past) {
            Some(x) => format!("{} · {}", x.date().map(fr_short).unwrap_or_default(), metric.fmt(metric.of(x))),
            None => format!("{} · {}", past.len(), metric.label().to_lowercase()),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().flex().justify_between().child(heading("Avant celle-ci")).child(num(readout).text_size(px(12.)).text_color(t.soft)))
            .child(div().h(px(80.)).flex().items_end().gap(px(3.)).children(past.iter().enumerate().map(|(i, x)| {
                let id = x.activity_id;
                let this_one = id == a.activity_id;
                let h = (metric.of(x) / top * 80.0).max(2.0) as f32;
                div()
                    .id(("hist", i))
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_end()
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, on: &bool, _, cx| {
                        this.hist_hover = if *on { Some(id) } else { this.hist_hover.filter(|h| *h != id) };
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| this.open(id, window, cx)))
                    .child(div().w_full().h(px(h)).rounded_t(px(2.)).bg(if this_one {
                        t.accent
                    } else if self.hist_hover == Some(id) {
                        t.soft
                    } else {
                        t.mark()
                    }))
            })))
    }

    fn act_hover_history<'a>(&self, past: &[&'a Activity]) -> Option<&'a Activity> {
        self.hist_hover.and_then(|id| past.iter().find(|a| a.activity_id == id).copied())
    }
}
