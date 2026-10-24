//! The map widget shared by the activity page and the heat map: tiles in the theme's colors,
//! drag to move, wheel to zoom around the pointer, +/− and refit buttons, attribution.

use crate::data::Data;
use crate::logic::map::{self, View};
use crate::ui::icon;
use gpui_kit::assets::IconName;
use gpui_kit::{prelude::FluentBuilder, *};
use std::{cell::Cell, rc::Rc, time::Instant};

/// Space left around what a map is fitted to: enough to clear the controls (bottom left) and the
/// attribution (bottom right).
const FIT_PAD: f64 = 52.0;

/// A map's state, kept by the view that shows it: what it shows, where it is on screen, the drag
/// in progress, and what the pointer just did.
pub struct MapState {
    pub view: View,
    pub bounds: Rc<Cell<Bounds<Pixels>>>,
    pub drag: Option<(Point<Pixels>, View)>,
    /// What the view was last fitted to; a new subject fits again.
    pub fitted: Option<u64>,
    /// The size and view of the last fit: the same subject fits again if the map was resized
    /// and not moved since, so it keeps framing what it shows.
    fit_at: Option<(Size<Pixels>, View)>,
    /// A click (a press and release without moving), in map pixels, for the view to read.
    pub click: Option<(f64, f64)>,
    /// When the wheel last tried to zoom without Ctrl: the hint shows a moment.
    pub hint: Option<Instant>,
}

impl MapState {
    pub fn new() -> Self {
        MapState { view: View::default(), bounds: Rc::new(Cell::new(Bounds::default())), drag: None, fitted: None, fit_at: None, click: None, hint: None }
    }

    /// Fits the map to `bbox` when `subject` changed (or on demand), or when the map was resized
    /// without being moved, once its size is known.
    pub fn fit(&mut self, subject: u64, bbox: Option<[f64; 4]>, force: bool) {
        let b = self.bounds.get();
        let resized = self.fit_at.is_some_and(|(size, view)| size != b.size && view == self.view);
        if b.size.width < px(10.) || (!force && self.fitted == Some(subject) && !resized) {
            return;
        }
        // Nothing to fit yet (tracks still loading): try again at the next frame.
        if let Some(bbox) = bbox {
            self.view = View::fit(bbox, (b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64, FIT_PAD);
            self.fitted = Some(subject);
            self.fit_at = Some((b.size, self.view));
        }
    }
}

/// The map `m` of the view `V` (`state` reaches it again for the gestures): tiles, then
/// `overlay` (drawn in screen coordinates), then the controls.
#[allow(clippy::too_many_arguments)]
pub fn map_widget<V: 'static>(
    m: &MapState,
    state: fn(&mut V) -> &mut MapState,
    data: &Entity<Data>,
    id: &'static str,
    overlay: Option<AnyElement>,
    refit: Option<[f64; 4]>,
    // In a page that scrolls, the wheel scrolls the page; Ctrl + wheel zooms.
    wheel_needs_ctrl: bool,
    cx: &mut Context<V>,
) -> Stateful<Div> {
    let (view, bounds, dragging, hint) = (m.view, m.bounds.clone(), m.drag.is_some(), m.hint);
    let t = data.read(cx).theme;
    let b = bounds.get();
    let (w, h) = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
    let mut tiles = view.tiles(w.max(1.0), h.max(1.0));
    // Centre first: it's what one looks at while the rest arrives.
    tiles.sort_by(|a, c| {
        let d = |x: &(u8, u32, u32, f64, f64, f64)| ((x.3 + x.5 / 2.0 - w / 2.0).powi(2) + (x.4 + x.5 / 2.0 - h / 2.0).powi(2)) as i64;
        d(a).cmp(&d(c))
    });
    let imgs: Vec<AnyElement> = tiles
        .into_iter()
        .filter_map(|(z, x, y, left, top, size)| {
            let image = data.update(cx, |d, cx| d.tile((z, x, y), cx))?;
            // Half a pixel more on each side hides the seams between scaled tiles.
            Some(
                img(image)
                    .absolute()
                    .left(px(left as f32 - 0.5))
                    .top(px(top as f32 - 0.5))
                    .w(px(size as f32 + 1.0))
                    .h(px(size as f32 + 1.0))
                    .into_any_element(),
            )
        })
        .collect();

    let control = |id: &'static str, name: IconName| {
        div()
            .id(id)
            .size(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .bg(t.bg)
            .shadow_sm()
            .cursor_pointer()
            .hover(|d| d.bg(t.panel))
            .child(icon(name, t.soft, 15.))
    };
    let zoom = move |dz: f64| {
        move |this: &mut V, _: &ClickEvent, _: &mut Window, cx: &mut Context<V>| {
            let m = state(this);
            let b = m.bounds.get();
            let (w, h) = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
            m.view.zoom_at(dz, w / 2.0, h / 2.0, w, h);
            cx.notify();
        }
    };

    let cell = bounds.clone();
    div()
        .id(id)
        .relative()
        .overflow_hidden()
        .size_full()
        .bg(t.panel)
        .cursor(if dragging { CursorStyle::ClosedHand } else { CursorStyle::OpenHand })
        .child(
            canvas(
                move |b, window, _| {
                    // A new size lays the map out again (tiles, fit) at the next frame.
                    if cell.get().size != b.size {
                        window.request_animation_frame();
                    }
                    cell.set(b)
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .children(imgs)
        .children(overlay)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                let m = state(this);
                m.drag = Some((e.position, m.view));
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
            let m = state(this);
            if let Some((start, from)) = m.drag {
                if e.pressed_button != Some(MouseButton::Left) {
                    m.drag = None;
                    return;
                }
                let mut v = from;
                v.pan(((e.position.x - start.x) / px(1.)) as f64, ((e.position.y - start.y) / px(1.)) as f64);
                m.view = v;
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseUpEvent, _, cx| {
                let m = state(this);
                let b = m.bounds.get();
                if let Some((start, _)) = m.drag.take() {
                    let moved = ((e.position.x - start.x) / px(1.)).abs() + ((e.position.y - start.y) / px(1.)).abs();
                    if moved < 4.0 {
                        m.click = Some((((e.position.x - b.origin.x) / px(1.)) as f64, ((e.position.y - b.origin.y) / px(1.)) as f64));
                    }
                }
                cx.notify();
            }),
        )
        .on_scroll_wheel(cx.listener(move |this, e: &ScrollWheelEvent, _, cx| {
            if wheel_needs_ctrl && !(e.modifiers.control || e.modifiers.platform) {
                // Let the page scroll; say once how to zoom.
                state(this).hint = Some(Instant::now());
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(std::time::Duration::from_millis(1600)).await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                })
                .detach();
                cx.notify();
                return;
            }
            cx.stop_propagation();
            let m = state(this);
            let b = m.bounds.get();
            let dy = e.delta.pixel_delta(px(20.)).y / px(1.);
            let (w, h) = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
            let (sx, sy) = (((e.position.x - b.origin.x) / px(1.)) as f64, ((e.position.y - b.origin.y) / px(1.)) as f64);
            m.view.zoom_at(dy as f64 / 240.0, sx, sy, w, h);
            cx.notify();
        }))
        .child(
            div()
                .absolute()
                .left(px(10.))
                .bottom(px(10.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(control("map-in", IconName::Plus).on_click(cx.listener(zoom(1.0))))
                .child(control("map-out", IconName::Minus).on_click(cx.listener(zoom(-1.0))))
                .when_some(refit, |d, bbox| {
                    d.child(control("map-fit", IconName::Maximize).on_click(cx.listener(move |this, _, _, cx| {
                        state(this).fit(0, Some(bbox), true);
                        cx.notify();
                    })))
                }),
        )
        .when(wheel_needs_ctrl && hint.is_some_and(|t| t.elapsed().as_millis() < 1500), |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(12.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(div().px(px(10.)).py(px(4.)).rounded(px(6.)).bg(t.text.opacity(0.75)).text_color(t.bg).text_size(px(12.)).child("Ctrl + molette pour zoomer")),
            )
        })
        .child(
            div()
                .id("osm")
                .absolute()
                .right(px(0.))
                .bottom(px(0.))
                .px(px(6.))
                .py(px(2.))
                .bg(crate::ui::theme::blend(t.bg, t.panel, 0.8))
                .text_size(px(11.))
                .text_color(t.soft)
                .cursor_pointer()
                .on_click(|_, _, cx| cx.open_url("https://www.openstreetmap.org/copyright"))
                .child("© les contributeurs d'OpenStreetMap"),
        )
}

/// Bounding box of one line, untrimmed.
pub fn line_bbox(line: &[[f32; 2]]) -> Option<[f64; 4]> {
    map::bbox(std::iter::once(line), 0.0)
}
