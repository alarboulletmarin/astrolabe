//! Shared pieces, drawn once so every view speaks the same language.

pub mod chart;
pub mod format;
pub mod theme;

use crate::logic::activity::Sport;
use crate::ui::theme::{MONO, Theme, blend};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::{prelude::FluentBuilder, *};

pub fn icon(name: IconName, color: Hsla, size: f32) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
}

/// Tinted label (recipe background, legible text), radius 6, 12 px.
pub fn pill(text: impl Into<SharedString>, (bg, fg): (Hsla, Hsla)) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(6.))
        .bg(bg)
        .text_color(fg)
        .text_size(px(12.))
        .child(text.into())
}

pub fn sport_icon(s: Sport) -> IconName {
    match s {
        Sport::Run => IconName::Footprints,
        Sport::Trail => IconName::Mountain,
        Sport::Bike => IconName::Bike,
        Sport::Swim => IconName::WavesHorizontal,
        Sport::Walk => IconName::PersonStanding,
        Sport::Hike => IconName::Backpack,
        Sport::Multisport => IconName::Medal,
        Sport::Strength => IconName::Dumbbell,
        Sport::Other => IconName::Activity,
    }
}

/// The sport as Bref draws things: a thin icon and its name, in `soft`, no colored chip.
pub fn sport_tag(s: Sport, t: Theme) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .text_size(px(12.))
        .text_color(t.soft)
        .child(icon(sport_icon(s), t.soft, 14.))
        .child(s.label())
}

/// Numbers are set in Plex Mono so columns line up.
pub fn num(text: impl Into<SharedString>) -> Div {
    div().font_family(MONO).whitespace_nowrap().child(text.into())
}

pub fn heading(text: impl Into<SharedString>) -> Div {
    div().text_size(px(15.)).font_weight(FontWeight::BOLD).child(text.into())
}

/// A label in `soft` over a big mono value.
pub fn figure(label: &'static str, value: String, t: Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().text_size(px(12.)).text_color(t.soft).child(label))
        .child(num(value).text_size(px(22.)).font_weight(FontWeight::BOLD))
}

/// Tabs on a single line: `Tout · Course · Vélo`. The chosen one in `text` on `panel`.
pub fn segmented<V: 'static>(
    id: &'static str,
    items: Vec<(SharedString, bool)>,
    t: Theme,
    cx: &mut Context<V>,
    on_pick: impl Fn(&mut V, usize, &mut Window, &mut Context<V>) + Clone + 'static,
) -> Div {
    div()
        .flex()
        .flex_wrap()
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(8.))
        .border_1()
        .border_color(t.border)
        .children(items.into_iter().enumerate().map(|(i, (label, on))| {
            let pick = on_pick.clone();
            div()
                .id((id, i))
                .px(px(9.))
                .py(px(2.))
                .rounded(px(6.))
                .text_size(px(12.))
                .cursor_pointer()
                .text_color(if on { t.text } else { t.soft })
                .when(on, |d| d.bg(t.panel).font_weight(FontWeight::BOLD))
                .hover(|d| d.text_color(t.text))
                .on_click(cx.listener(move |this, _, window, cx| {
                    pick(this, i, window, cx);
                    cx.notify();
                }))
                .child(label)
        }))
}

/// Bref's button: 1 px border, radius 6, 13 px. `primary` fills it with the accent.
pub fn button(id: &'static str, label: &'static str, primary: bool, t: Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .px(px(12.))
        .py(px(5.))
        .rounded(px(6.))
        .text_size(px(13.))
        .cursor_pointer()
        .border_1()
        .when(primary, |d| {
            d.bg(t.accent)
                .border_color(t.accent)
                .text_color(t.bg)
                .font_weight(FontWeight::BOLD)
                .hover(|d| d.bg(blend(t.text, t.accent, 0.12)))
        })
        .when(!primary, |d| d.border_color(t.border).text_color(t.text).hover(|d| d.bg(t.panel)))
        .child(label)
}

/// Side padding of a page: the content takes the whole width, with Bref's margins.
pub fn pad(width: f32) -> f32 {
    if width < 700.0 { 18.0 } else if width < 1100.0 { 28.0 } else { 40.0 }
}

pub fn title(text: impl Into<SharedString>) -> Div {
    div().text_size(px(26.)).font_weight(FontWeight::BOLD).child(text.into())
}

thread_local! {
    static TOP: std::cell::Cell<f32> = const { std::cell::Cell::new(28.0) };
}

/// Top margin of a page: more when the app draws its own window buttons in the corner.
pub fn top() -> f32 {
    TOP.with(|t| t.get())
}

pub fn set_top(v: f32) {
    TOP.with(|t| t.set(v));
}

/// A page: whole width, scrolls, Bref's margins.
pub fn page(id: &'static str, width: f32) -> Stateful<Div> {
    let p = pad(width);
    div().id(id).size_full().overflow_y_scroll().px(px(p)).pt(px(top())).pb(px(56.)).flex().flex_col().gap(px(28.))
}

/// Moves the window from this element (double click: maximise; right click: the window menu).
pub fn drag_window<E: InteractiveElement>(element: E) -> E {
    element
        .on_mouse_down(MouseButton::Left, |e, window, _| {
            if e.click_count == 2 {
                window.zoom_window()
            } else {
                window.start_window_move()
            }
        })
        .on_mouse_down(MouseButton::Right, |e, window, _| window.show_window_menu(e.position))
}

/// BGRA pixels into an image GPUI can draw.
pub fn render_image(w: u32, h: u32, bgra: Vec<u8>) -> Option<RenderImage> {
    let buf = image::RgbaImage::from_raw(w, h, bgra)?;
    Some(RenderImage::new(vec![image::Frame::new(buf)]))
}

/// Plot areas, by name, as laid out at the last frame: hover reads positions against them.
#[derive(Default)]
pub struct Plots(std::collections::HashMap<&'static str, std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>>);

impl Plots {
    pub fn get(&mut self, name: &'static str) -> std::rc::Rc<std::cell::Cell<Bounds<Pixels>>> {
        self.0.entry(name).or_default().clone()
    }
}
