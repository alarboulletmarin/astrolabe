//! Shared pieces, drawn once so every view speaks the same language.

pub mod chart;
pub mod format;
pub mod logo;
pub mod theme;

use crate::logic::activity::Sport;
use crate::ui::theme::{MONO, Theme, blend};
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::{prelude::FluentBuilder, *};

/// The bundled icon catalog, plus the window buttons' own icons (1.2 px strokes on a 16 px grid,
/// the same as Bref's).
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        let shapes = match path {
            "window-minimize.svg" => r#"<path d="M4 8H12"/>"#,
            "window-maximize.svg" => r#"<path d="M4.5 4.5H11.5V11.5H4.5Z"/>"#,
            "window-restore.svg" => r#"<path d="M3.5 6.5H9.5V12.5H3.5Z"/><path d="M10 8.5H12.5V3.5H7.5V6"/>"#,
            "window-close.svg" | "close.svg" => r#"<path d="M4.5 4.5L11.5 11.5M11.5 4.5L4.5 11.5"/>"#,
            // An arrow that turns back, and the other way.
            "undo.svg" => r#"<path d="M6 3.5L3.5 6L6 8.5"/><path d="M3.5 6H9.75A2.75 2.75 0 0 1 9.75 11.5H6.5"/>"#,
            "redo.svg" => r#"<path d="M10 3.5L12.5 6L10 8.5"/><path d="M12.5 6H6.25A2.75 2.75 0 0 0 6.25 11.5H9.5"/>"#,
            // Two arrows round a circle: other loops.
            "again.svg" => r#"<path d="M12.6 6.5A4.8 4.8 0 0 0 3.9 5.4M3.5 2.8V5.6H6.3M3.4 9.5A4.8 4.8 0 0 0 12.1 10.6M12.5 13.2V10.4H9.7"/>"#,
            // Two arrows, one each way: the other way round.
            "reverse.svg" => r#"<path d="M3 5.5H12.5M10 3L12.5 5.5L10 8"/><path d="M13 10.5H3.5M6 8L3.5 10.5L6 13"/>"#,
            "export.svg" => r#"<path d="M8 2.5V10M5 7L8 10L11 7M3 13H13"/>"#,
            _ => return gpui_kit::assets::AllAssets.load(path),
        };
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="black" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">{shapes}</svg>"#
        );
        Ok(Some(svg.into_bytes().into()))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_kit::assets::AllAssets.list(path)
    }
}

/// A round button with one icon, as in Bref: `over` is its hover (red for what warns); `None`,
/// a button that can do nothing now, greyed.
pub fn icon_button(id: impl Into<ElementId>, path: &'static str, over: Option<Hsla>, t: Theme) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .when_some(over, |d, over| d.cursor_pointer().hover(|s| s.bg(over)).active(|s| s.bg(t.pressed())))
        .when(over.is_none(), |d| d.opacity(0.35))
        // Keeps the press from what is under it (the map, a window handle): a drag there would
        // take the pointer, and the release (the click) never comes back.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(svg().path(path).size(px(16.)).flex_none().text_color(t.text))
}

/// How long the pointer rests on something before its tooltip shows: gpui waits 500 ms, which
/// reads as a tooltip that does not come.
pub const TIP_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

pub fn icon(name: IconName, color: Hsla, size: f32) -> Icon {
    Icon::new(name).size(px(size)).text_color(color)
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

/// A figure in mono and its unit after it, smaller and in `dim`, so units line up without the
/// wide spaces of a mono font.
pub fn quantity(text: &str, t: Theme) -> Div {
    let (n, unit) = format::split_unit(text);
    div().flex().items_baseline().gap(px(3.)).whitespace_nowrap().child(num(n)).when(!unit.is_empty(), |d| d.child(div().text_size(px(11.)).text_color(t.dim).child(unit)))
}

pub fn heading(text: impl Into<SharedString>) -> Div {
    div().text_size(px(15.)).font_weight(FontWeight::BOLD).child(text.into())
}

/// A label in `soft` over a big mono value, its unit after it in small sans.
pub fn figure(label: &'static str, value: String, t: Theme) -> Div {
    let (n, unit) = format::split_unit(&value);
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().text_size(px(12.)).text_color(t.soft).child(label))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(4.))
                .child(num(n).text_size(px(22.)).font_weight(FontWeight::BOLD))
                .when(!unit.is_empty(), |d| d.child(div().text_size(px(13.)).text_color(t.soft).child(unit))),
        )
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
                .debug_selector(move || format!("{id}-{i}"))
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
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, primary: bool, t: Theme) -> Stateful<Div> {
    button_frame(id, primary, t).child(label.into())
}

/// `button` with an icon before its label.
pub fn icon_label_button(id: impl Into<ElementId>, path: &'static str, label: impl Into<SharedString>, t: Theme) -> Stateful<Div> {
    button_frame(id, false, t)
        .pl(px(9.))
        .flex()
        .items_center()
        .gap(px(6.))
        .child(svg().path(path).size(px(14.)).flex_none().text_color(t.soft))
        .child(label.into())
}

fn button_frame(id: impl Into<ElementId>, primary: bool, t: Theme) -> Stateful<Div> {
    let id = id.into();
    // Found by its id in the window tests.
    let selector = id.to_string();
    div()
        .debug_selector(move || selector)
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
