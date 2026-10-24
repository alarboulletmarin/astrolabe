//! One view per file, each an entity with its own state. A view reads the shared `Data`, and
//! asks for another page by emitting it (`cx.emit(Page::…)`): it never sees the window.

pub mod activity;
pub mod heatmap;
pub mod journal;
pub mod map;
pub mod records;
pub mod settings;
pub mod stats;

use crate::data::Data;
use chrono::{Local, NaiveDate};
use gpui_kit::{prelude::FluentBuilder, *};
use std::{cell::Cell, rc::Rc};

/// What the window shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Page {
    Journal,
    Activity(i64),
    Stats,
    Records,
    Heatmap,
    Settings,
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// What the window shares with the views: the width they really get, measured at the last frame
/// (its frame and the rail make the viewport a poor guess; before the first frame, the viewport
/// minus the rail), and the scroll of the page shown, which the window saves and restores.
#[derive(Clone, Default)]
pub struct Width(pub Rc<Cell<f32>>, ScrollHandle);

impl Width {
    pub fn scroll(&self) -> &ScrollHandle {
        &self.1
    }

    pub fn get(&self, window: &Window) -> f32 {
        let measured = self.0.get();
        if measured > 0.0 { measured } else { window.viewport_size().width / px(1.) - 46.0 }
    }
}

/// Where the track downloads stand, live: count, bar, the activity in progress, failures, and
/// the button to resume or stop.
pub fn tracks_progress(data: &Entity<Data>, cx: &App) -> Div {
    let d = data.read(cx);
    let t = d.theme;
    let f = &d.fetch;
    let (read, total) = (d.derived.len().min(d.acts.len()), d.acts.len());
    let share = if total > 0 { read as f32 / total as f32 } else { 0.0 };
    let has_key = d.key.is_some();
    let g = |n: usize| crate::logic::activity::group(n as i64);
    let (stop, resume) = (data.clone(), data.clone());
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .justify_between()
                .child(div().font_weight(FontWeight::BOLD).child(format!("{} / {} traces lues", g(read), g(total))))
                .child(crate::ui::num(format!("{:.0} %", share * 100.0)).text_color(t.soft)),
        )
        .child(div().h(px(6.)).w_full().rounded(px(3.)).bg(t.border).child(div().h_full().rounded(px(3.)).bg(t.accent).w(relative(share))))
        .when_some(f.current.as_ref().filter(|_| f.running), |d, name| {
            d.child(div().text_size(px(12.)).text_color(t.soft).child(format!(
                "Téléchargement {} / {} de cette série : « {name} »",
                g((f.done + 1).min(f.total)),
                g(f.total)
            )))
        })
        .when(!f.running && read < total && f.stopped.is_none() && has_key, |d| {
            d.child(div().text_size(px(12.)).text_color(t.soft).child(format!("{} encore à télécharger.", g(total - read))))
        })
        .when(!has_key && read < total, |d| {
            d.child(div().text_size(px(12.)).text_color(t.soft).child("Ajoute ta clé API pour télécharger les traces."))
        })
        .when(f.failed > 0, |d| d.child(div().text_size(px(12.)).text_color(t.soft).child(format!("{} échec{} dans cette série", f.failed, if f.failed > 1 { "s" } else { "" }))))
        .when_some(f.error.clone(), |d, e| d.child(div().text_size(px(12.)).text_color(t.bad()).child(e)))
        .when_some(f.stopped.clone(), |d, e| d.child(div().text_size(px(12.)).text_color(t.bad()).child(e)))
        .child(div().flex().gap(px(8.)).when(f.running, |d| {
            d.child(
                crate::ui::button("fetch-stop", if f.cancel { "Arrêt après ce fichier…" } else { "Arrêter" }, false, t)
                    .on_click(move |_, _, cx| stop.update(cx, |d, cx| d.stop_fetch(cx))),
            )
        }).when(!f.running && read < total && has_key, |d| {
            d.child(
                crate::ui::button("fetch-go", "Reprendre le téléchargement", true, t)
                    .on_click(move |_, _, cx| resume.update(cx, |d, cx| d.fetch_tracks(cx))),
            )
        }))
}
