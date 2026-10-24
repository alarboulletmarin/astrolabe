//! The heat map: every track at once, darker where more activities passed. Filter by sport and
//! year; a click on the map lists the activities that went through that spot.

use crate::app::Home;
use crate::logic::journal::{name, sports_present};
use crate::logic::map;
use crate::ui::format::{fr_short};
use crate::ui::theme::blend;
use crate::ui::{heading, icon, num, segmented, sport_tag};
use crate::views::map::Which;
use chrono::Datelike;
use gpui_kit::assets::IconName;
use gpui_kit::{prelude::FluentBuilder, *};
use std::hash::{Hash, Hasher};

impl Home {
    pub fn heatmap(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        let ids = self.heat_ids();
        let subject = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (&self.heat_sports, self.heat_year).hash(&mut h);
            h.finish()
        };
        let bbox = map::bbox(ids.iter().filter_map(|id| self.derived.get(id)).map(|d| d.line.as_slice()), 0.02);
        self.fit_map(Which::Heat, subject, bbox, false);

        let b = self.heat_map.bounds.get();
        let (w, h) = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
        // A click (not a drag) on the map: the activities through that spot.
        if let Some((Which::Heat, (sx, sy))) = self.map_click.take() {
            let lines: Vec<(i64, &[[f32; 2]])> = ids.iter().filter_map(|id| self.derived.get(id).map(|d| (*id, d.line.as_slice()))).collect();
            let found = map::near(&lines, &self.heat_map.view, w, h, sx, sy, 6.0);
            self.heat_pick = Some((point(px(sx as f32), px(sy as f32)), found));
        }
        self.ensure_heat((w, h), window.scale_factor(), cx);

        // The raster drawn for an earlier view follows the map until the new one is ready.
        let view = self.heat_map.view;
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
        let pick_dot = self.heat_pick.as_ref().map(|(p, _)| {
            div().absolute().left(p.x - px(5.)).top(p.y - px(5.)).size(px(10.)).rounded_full().bg(t.text).into_any_element()
        });
        let overlay = div().absolute().size_full().children(overlay).children(pick_dot).into_any_element();

        div()
            .size_full()
            .relative()
            .child(self.map_widget(Which::Heat, "heat-map", Some(overlay), bbox, false, cx))
            .child(self.heat_panel(&ids, cx))
    }

    fn heat_panel(&mut self, ids: &[i64], cx: &mut Context<Self>) -> Stateful<Div> {
        let t = self.theme;
        let present = sports_present(&self.acts);
        let chosen = self.heat_sports.clone();
        let mut chips: Vec<(SharedString, bool)> = vec![("Tous".into(), chosen.is_empty())];
        chips.extend(present.iter().map(|s| (s.label().into(), chosen.contains(s))));
        let sports = segmented("heat-sports", chips, t, cx, move |this: &mut Home, i, _, _| {
            let list = &mut this.heat_sports;
            if i == 0 {
                list.clear();
            } else if let Some(p) = list.iter().position(|x| *x == present[i - 1]) {
                list.remove(p);
            } else {
                list.push(present[i - 1]);
            }
            this.heat_pick = None;
        });
        let mut years: Vec<i32> = self.acts.iter().filter_map(|a| a.date()).map(|d| d.year()).collect();
        years.sort_unstable_by(|a, b| b.cmp(a));
        years.dedup();
        let mut ys: Vec<(SharedString, bool)> = vec![("Toujours".into(), self.heat_year.is_none())];
        ys.extend(years.iter().map(|y| (y.to_string().into(), self.heat_year == Some(*y))));
        let year_list = years.clone();
        let year = segmented("heat-year", ys, t, cx, move |this: &mut Home, i, _, _| {
            this.heat_year = if i == 0 { None } else { Some(year_list[i - 1]) };
            this.heat_pick = None;
        });

        let km: f64 = ids.iter().filter_map(|id| self.activity(*id)).filter_map(|a| a.distance).sum();
        let ramp_light = blend(t.accent, t.bg, if t.dark { 0.45 } else { 0.4 });
        let ramp_deep = if t.dark { blend(t.text, t.accent, 0.35) } else { blend(t.text, t.accent, 0.45) };
        let waiting = self.acts.iter().filter(|a| !self.derived.contains_key(&a.activity_id)).count();

        let picks = self.heat_pick.as_ref().map(|(_, found)| found.clone());
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
                                            this.heat_pick = None;
                                            cx.notify();
                                        }))
                                        .child(icon(IconName::X, t.soft, 14.)),
                                ),
                        )
                        .children(found.iter().take(30).enumerate().filter_map(|(i, id)| {
                            let a = self.activity(*id)?;
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
                                    .on_click(cx.listener(move |this, _, window, cx| this.open(id, window, cx)))
                                    .child(num(a.date().map(fr_short).unwrap_or_default()).w(px(100.)).flex_none().text_size(px(12.)).text_color(t.soft))
                                    .child(sport_tag(a.sport(), t))
                                    .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).child(name(a))),
                            )
                        })),
                )
            })
    }
}

