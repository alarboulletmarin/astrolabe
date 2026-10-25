//! The heat map: every track at once, darker where more activities passed. Filter by sport and
//! year; a click on the map lists the activities that went through that spot.

use crate::data::{Data, Snapshot};
use crate::logic::activity::Sport;
use crate::logic::journal::{name, sports_present};
use crate::logic::map;
use crate::ui::format::fr_short;
use crate::ui::theme::blend;
use crate::ui::{heading, icon, num, render_image, segmented, sport_tag};
use crate::views::Page;
use crate::views::map::{MapState, map_widget};
use chrono::Datelike;
use gpui_kit::assets::IconName;
use gpui_kit::{prelude::FluentBuilder, *};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Default)]
struct Heat {
    /// The raster and the view it was drawn for.
    image: Option<(Arc<RenderImage>, map::View, (f64, f64))>,
    busy: bool,
    drawn: Option<u64>,
}

/// The map opens on the points within this many metres of where most of them are.
const HOME_RADIUS: f64 = 50_000.0;

pub struct HeatmapView {
    data: Entity<Data>,
    snap: Snapshot,
    map: MapState,
    /// Empty: every sport.
    sports: Vec<Sport>,
    year: Option<i32>,
    heat: Heat,
    /// The spot clicked, and the activities through it.
    pick: Option<(Point<Pixels>, Vec<i64>)>,
    _subscription: Subscription,
}

impl EventEmitter<Page> for HeatmapView {}

impl HeatmapView {
    pub fn new(data: Entity<Data>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&data, |this, data, cx| {
            this.snap = data.read(cx).snapshot();
            cx.notify();
        });
        HeatmapView {
            snap: data.read(cx).snapshot(),
            data,
            map: MapState::new(),
            sports: vec![],
            year: None,
            heat: Heat::default(),
            pick: None,
            _subscription: sub,
        }
    }

    /// Lines of the heat map, as filtered.
    fn ids(&self) -> Vec<i64> {
        self.snap
            .acts
            .iter()
            .filter(|a| self.sports.is_empty() || self.sports.contains(&a.sport()))
            .filter(|a| self.year.is_none_or(|y| a.date().is_some_and(|d| d.year() == y)))
            .filter(|a| self.snap.derived.get(&a.activity_id).is_some_and(|d| !d.line.is_empty()))
            .map(|a| a.activity_id)
            .collect()
    }

    /// Redraws the heat raster when what it shows changed; one drawing at a time, the latest wins.
    fn ensure_heat(&mut self, size: (f64, f64), scale: f32, cx: &mut Context<Self>) {
        let ids = self.ids();
        let view = self.map.view;
        let theme = self.snap.theme;
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (view.cx.to_bits(), view.cy.to_bits(), view.z.to_bits(), size.0.to_bits(), size.1.to_bits(), theme.dark).hash(&mut h);
            ids.hash(&mut h);
            h.finish()
        };
        if self.heat.drawn == Some(key) || self.heat.busy || size.0 < 8.0 || size.1 < 8.0 {
            return;
        }
        self.heat.busy = true;
        let derived = self.snap.derived.clone();
        let (w, h) = ((size.0 * scale as f64) as usize, (size.1 * scale as f64) as usize);
        let scaled = map::View { cx: view.cx * scale as f64, cy: view.cy * scale as f64, z: view.z + (scale as f64).log2() };
        let accent = theme.accent.to_rgb();
        let a = [(accent.r * 255.0) as u8, (accent.g * 255.0) as u8, (accent.b * 255.0) as u8];
        // Light theme: pale where one passage, deep where the most; dark theme: the reverse reads.
        let shade = |k: f32| a.map(|c| (c as f32 * k).min(255.0) as u8);
        let lift = |k: f32| a.map(|c| (c as f32 + (255.0 - c as f32) * k) as u8);
        let (light, deep) = if theme.dark { (shade(0.55), lift(0.55)) } else { (lift(0.55), shade(0.45)) };
        cx.spawn(async move |this, cx| {
            let img = cx
                .background_executor()
                .spawn(async move {
                    let lines: Vec<&[[f32; 2]]> = ids.iter().filter_map(|id| derived.get(id)).map(|d| d.line.as_slice()).collect();
                    let counts = map::heat(&lines, &scaled, w, h);
                    render_image(w as u32, h as u32, map::colorize(&counts, light, deep)).map(Arc::new)
                })
                .await;
            this.update(cx, |h, cx| {
                h.heat.busy = false;
                h.heat.drawn = Some(key);
                if let Some(img) = img {
                    h.heat.image = Some((img, view, size));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for HeatmapView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.snap.theme;
        let ids = self.ids();
        let subject = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (&self.sports, self.year).hash(&mut h);
            h.finish()
        };
        let bbox = map::home_bbox(ids.iter().filter_map(|id| self.snap.derived.get(id)).map(|d| d.line.as_slice()), HOME_RADIUS, 0.01);
        self.map.fit(subject, bbox, false);

        let b = self.map.bounds.get();
        let (w, h) = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
        // A click (not a drag) on the map: the activities through that spot.
        if let Some((sx, sy)) = self.map.click.take() {
            let lines: Vec<(i64, &[[f32; 2]])> = ids.iter().filter_map(|id| self.snap.derived.get(id).map(|d| (*id, d.line.as_slice()))).collect();
            let found = map::near(&lines, &self.map.view, w, h, sx, sy, 6.0);
            self.pick = Some((point(px(sx as f32), px(sy as f32)), found));
        }
        self.ensure_heat((w, h), window.scale_factor(), cx);

        // The raster drawn for an earlier view follows the map until the new one is ready.
        let view = self.map.view;
        let overlay = self.heat.image.as_ref().map(|(raster, v0, s0)| {
            let k = 2f64.powf(view.z - v0.z);
            let left = (v0.cx - s0.0 / 2.0) * k - (view.cx - w / 2.0);
            let top = (v0.cy - s0.1 / 2.0) * k - (view.cy - h / 2.0);
            img(raster.clone())
                .absolute()
                .left(px(left as f32))
                .top(px(top as f32))
                .w(px((s0.0 * k) as f32))
                .h(px((s0.1 * k) as f32))
                .into_any_element()
        });
        let pick_dot = self.pick.as_ref().map(|(p, _)| {
            div().absolute().left(p.x - px(5.)).top(p.y - px(5.)).size(px(10.)).rounded_full().bg(t.text).into_any_element()
        });
        let overlay = div().absolute().size_full().children(overlay).children(pick_dot).into_any_element();

        div()
            .size_full()
            .relative()
            .child(map_widget(&self.map, |v: &mut HeatmapView| &mut v.map, &self.data, "heat-map", Some(overlay), bbox, false, cx))
            .child(self.panel(&ids, cx))
    }
}

impl HeatmapView {
    fn panel(&mut self, ids: &[i64], cx: &mut Context<Self>) -> Stateful<Div> {
        let t = self.snap.theme;
        let present = sports_present(&self.snap.acts);
        let chosen = self.sports.clone();
        let mut chips: Vec<(SharedString, bool)> = vec![("Tous".into(), chosen.is_empty())];
        chips.extend(present.iter().map(|s| (s.label().into(), chosen.contains(s))));
        let sports = segmented("heat-sports", chips, t, cx, move |this: &mut HeatmapView, i, _, _| {
            let list = &mut this.sports;
            if i == 0 {
                list.clear();
            } else if let Some(p) = list.iter().position(|x| *x == present[i - 1]) {
                list.remove(p);
            } else {
                list.push(present[i - 1]);
            }
            this.pick = None;
        });
        let mut years: Vec<i32> = self.snap.acts.iter().filter_map(|a| a.date()).map(|d| d.year()).collect();
        years.sort_unstable_by(|a, b| b.cmp(a));
        years.dedup();
        let mut ys: Vec<(SharedString, bool)> = vec![("Toujours".into(), self.year.is_none())];
        ys.extend(years.iter().map(|y| (y.to_string().into(), self.year == Some(*y))));
        let year_list = years.clone();
        let year = segmented("heat-year", ys, t, cx, move |this: &mut HeatmapView, i, _, _| {
            this.year = if i == 0 { None } else { Some(year_list[i - 1]) };
            this.pick = None;
        });

        let km: f64 = ids.iter().filter_map(|id| self.snap.activity(*id)).filter_map(|a| a.distance).sum();
        let ramp_light = blend(t.accent, t.bg, if t.dark { 0.45 } else { 0.4 });
        let ramp_deep = if t.dark { blend(t.text, t.accent, 0.35) } else { blend(t.text, t.accent, 0.45) };
        let waiting = self.snap.acts.iter().filter(|a| !self.snap.derived.contains_key(&a.activity_id)).count();

        let picks = self.pick.as_ref().map(|(_, found)| found.clone());
        div()
            .id("heat-panel")
            .absolute()
            .top(px(14.0f32.max(crate::ui::top() - 14.0)))
            .left(px(14.))
            .w(px(330.))
            .max_h(relative(0.9))
            .overflow_y_scroll()
            .bg(blend(t.bg, t.panel, 0.94))
            .rounded(px(8.))
            .shadow_md()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            // The panel is not part of the map: no drag or zoom through it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(heading("Carte de chaleur"))
            .child(div().text_size(px(12.)).text_color(t.soft).child(if ids.is_empty() {
                "Aucune trace GPS ici pour l'instant : elles arrivent avec la synchronisation.".to_string()
            } else {
                format!("{} activités tracées · {}", crate::logic::activity::group(ids.len() as i64), crate::logic::activity::fmt_km(km))
            }))
            .child(div().flex().flex_col().gap(px(5.)).child(div().text_size(px(12.)).text_color(t.soft).child("Sports")).child(sports))
            .child(div().flex().flex_col().gap(px(5.)).child(div().text_size(px(12.)).text_color(t.soft).child("Année")).child(year))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().h(px(8.)).rounded(px(4.)).bg(linear_gradient(90., linear_color_stop(ramp_light, 0.), linear_color_stop(ramp_deep, 1.))))
                    .child(div().flex().justify_between().text_size(px(11.)).text_color(t.soft).child("un passage").child("le plus de passages")),
            )
            .when(waiting > 0, |d| {
                d.child(div().text_size(px(12.)).text_color(t.dim).child(format!("{} trace{} encore à télécharger.", waiting, if waiting > 1 { "s" } else { "" })))
            })
            .when_some(picks, |d, found| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .pt(px(8.))
                        .border_t_1()
                        .border_color(t.border)
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .items_center()
                                .child(heading(if found.is_empty() { "Rien ici".to_string() } else { format!("{} passage{} ici", found.len(), if found.len() > 1 { "s" } else { "" }) }).text_size(px(13.)))
                                .child(
                                    div()
                                        .id("pick-close")
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.pick = None;
                                            cx.notify();
                                        }))
                                        .child(icon(IconName::X, t.soft, 14.)),
                                ),
                        )
                        .children(found.iter().take(30).enumerate().filter_map(|(i, id)| {
                            let a = self.snap.activity(*id)?;
                            let id = *id;
                            Some(
                                div()
                                    .id(("pick", i))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .h(px(30.))
                                    .px(px(6.))
                                    .mx(px(-6.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .hover(|d| d.bg(t.panel))
                                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(Page::Activity(id))))
                                    .child(num(a.date().map(fr_short).unwrap_or_default()).w(px(100.)).flex_none().text_size(px(12.)).text_color(t.soft))
                                    .child(sport_tag(a.sport(), t))
                                    .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).child(name(a))),
                            )
                        })),
                )
            })
    }
}

