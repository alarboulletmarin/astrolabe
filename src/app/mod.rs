//! The window: the rail, the page shown, the status line and the window buttons. The pages are
//! views of their own (`crate::views`); the window only says which one shows, and passes the keys.

use crate::data::{Data, DataEvent, Status};
use crate::lang::tr;
use crate::logic::history::History;
use crate::ui::icon;
use crate::ui::theme::{MONO, SANS, Theme};
use crate::views::activity::ActivityView;
use crate::views::calendar::CalendarView;
use crate::views::dashboard::DashboardView;
use crate::views::heatmap::HeatmapView;
use crate::views::journal::JournalView;
use crate::views::records::RecordsView;
use crate::views::settings::SettingsView;
use crate::views::stats::StatsView;
use crate::views::{Page, Width};
use gpui_kit::assets::IconName;
use gpui_kit::component::{Theme as KitTheme, ThemeMode, tooltip::Tooltip};
use gpui_kit::{prelude::FluentBuilder, *};
use std::time::Duration;

/// Height of the strip at the top of the window that moves it; the pages start lower.
const HEADER: f32 = 36.0;

actions!(astrolabe, [SelectNext, SelectPrev, OpenSelected, Back, GoBack, GoForward, Older, Newer, CalPrev, CalNext, CalToday]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some("Journal")),
        KeyBinding::new("up", SelectPrev, Some("Journal")),
        KeyBinding::new("enter", OpenSelected, Some("Journal")),
        KeyBinding::new("escape", Back, Some("Activity")),
        KeyBinding::new(if cfg!(target_os = "macos") { "cmd-[" } else { "alt-left" }, GoBack, None),
        KeyBinding::new(if cfg!(target_os = "macos") { "cmd-]" } else { "alt-right" }, GoForward, None),
        KeyBinding::new("left", Older, Some("Activity")),
        KeyBinding::new("right", Newer, Some("Activity")),
        KeyBinding::new("left", CalPrev, Some("Calendar")),
        KeyBinding::new("right", CalNext, Some("Calendar")),
        KeyBinding::new("t", CalToday, Some("Calendar")),
    ]);
}

pub struct Shell {
    data: Entity<Data>,
    page: Page,
    history: History<Page>,
    dashboard: Entity<DashboardView>,
    journal: Entity<JournalView>,
    calendar: Entity<CalendarView>,
    activity: Entity<ActivityView>,
    stats: Entity<StatsView>,
    records: Entity<RecordsView>,
    heatmap: Entity<HeatmapView>,
    settings: Entity<SettingsView>,
    focus: FocusHandle,
    width: Width,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme = Theme::for_appearance(window.appearance());
        KitTheme::change(if theme.dark { ThemeMode::Dark } else { ThemeMode::Light }, Some(window), cx);
        let data = cx.new(|cx| Data::new(theme, cx));
        Self::with_data(data, window, cx)
    }

    fn with_data(data: Entity<Data>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let width = Width::default();
        let dashboard = cx.new(|cx| DashboardView::new(data.clone(), width.clone(), cx));
        let journal = cx.new(|cx| JournalView::new(data.clone(), width.clone(), window, cx));
        let calendar = cx.new(|cx| CalendarView::new(data.clone(), width.clone(), cx));
        let activity = cx.new(|cx| ActivityView::new(data.clone(), width.clone(), cx));
        let stats = cx.new(|cx| StatsView::new(data.clone(), width.clone(), window, cx));
        let records = cx.new(|cx| RecordsView::new(data.clone(), width.clone(), cx));
        let heatmap = cx.new(|cx| HeatmapView::new(data.clone(), cx));
        let settings = cx.new(|cx| SettingsView::new(data.clone(), width.clone(), window, cx));
        let subs = vec![
            cx.observe_window_appearance(window, |this, window, cx| {
                let theme = Theme::for_appearance(window.appearance());
                KitTheme::change(if theme.dark { ThemeMode::Dark } else { ThemeMode::Light }, Some(window), cx);
                this.data.update(cx, |d, cx| d.set_theme(theme, cx));
            }),
            cx.observe(&data, |_, _, cx| cx.notify()),
            cx.subscribe_in(&data, window, |this, _, e: &DataEvent, window, cx| match e {
                DataEvent::NeedKey => this.go(Page::Settings, window, cx),
                DataEvent::KeyReady => this.go(Page::Dashboard, window, cx),
            }),
            cx.subscribe_in(&dashboard, window, Self::follow),
            cx.subscribe_in(&journal, window, Self::follow),
            cx.subscribe_in(&calendar, window, Self::follow),
            cx.subscribe_in(&activity, window, Self::follow),
            cx.subscribe_in(&stats, window, Self::follow),
            cx.subscribe_in(&records, window, Self::follow),
            cx.subscribe_in(&heatmap, window, Self::follow),
        ];
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Shell { data, page: Page::Dashboard, history: History::new(Page::Dashboard), dashboard, journal, calendar, activity, stats, records, heatmap, settings, focus, width, _subscriptions: subs }
    }

    /// Shows `page` and makes it the newest history entry. Moving from one activity to another
    /// replaces the entry instead: "back" returns to where one came from, not through each step.
    pub fn go(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.mark_scroll();
        if let (Page::Activity(_), Page::Activity(_)) = (self.page, page) {
            self.history.replace(page);
        } else if !self.history.visit(page) {
            // Already there (a click on the current page in the rail): nothing moves.
            window.focus(&self.focus, cx);
            return;
        }
        self.show(page, 0.0, window, cx);
    }

    /// Back or forward in the pages shown, at the scroll they were left at.
    pub fn travel(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.mark_scroll();
        let entry = if forward { self.history.forward() } else { self.history.back() };
        if let Some(entry) = entry {
            self.show(entry.page, entry.scroll, window, cx);
        }
    }

    /// The pages share one scroll handle, so it is saved before leaving and restored after.
    fn mark_scroll(&mut self) {
        self.history.mark(-self.width.scroll().offset().y / px(1.));
    }

    fn show(&mut self, page: Page, scroll: f32, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        if let Page::Activity(id) = page {
            self.journal.update(cx, |j, _| j.select(id));
            self.activity.update(cx, |a, cx| a.show(id, cx));
        }
        self.width.scroll().set_offset(point(px(0.), px(-scroll)));
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// A view asked for a page.
    fn follow<V>(&mut self, _: &Entity<V>, page: &Page, window: &mut Window, cx: &mut Context<Self>) {
        self.go(*page, window, cx);
    }

    fn rail(&self, cx: &mut Context<Self>) -> Div {
        let d = self.data.read(cx);
        let t = d.theme;
        let current = match self.page {
            Page::Activity(_) => Page::Journal,
            p => p,
        };
        let item = |id: &'static str, view: Page, name: IconName, tip: &'static str, cx: &mut Context<Self>| {
            let on = current == view;
            div()
                .id(id)
                .debug_selector(|| id.into())
                .size(px(30.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(on, |d| d.bg(t.selection))
                .hover(|d| d.bg(t.border))
                .tooltip_show_delay(crate::ui::TIP_DELAY).tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                .on_click(cx.listener(move |this, _, window, cx| this.go(view, window, cx)))
                .child(icon(name, if on { t.accent } else { t.soft }, 16.))
        };
        let syncing = d.syncing();
        // Idle, the icon says how the last synchronisation went: a tick, or red.
        let outcome = d.sync_outcome().filter(|_| !syncing);
        let spin = match outcome {
            Some(true) => icon(IconName::Check, t.accent, 15.),
            Some(false) => icon(IconName::RefreshCw, t.bad(), 15.),
            None => icon(IconName::RefreshCw, if syncing { t.accent } else if d.key.is_some() { t.soft } else { t.dim }, 15.),
        };
        let spin = if syncing {
            spin.with_animation("sync-spin", Animation::new(Duration::from_millis(1100)).repeat(), |i, d| {
                i.transform(Transformation::rotate(percentage(d)))
            })
            .into_any_element()
        } else {
            spin.into_any_element()
        };
        let tip: SharedString = match (d.sync_line(), outcome, &d.status) {
            (Some(line), ..) => line,
            (_, Some(false), Status::Failed(e)) => format!("{e} · {}", tr("click to retry", "cliquer pour réessayer")),
            (_, Some(false), _) => tr("Synchronisation incomplete · click to resume", "Synchronisation incomplète · cliquer pour reprendre").into(),
            (_, Some(true), _) => d.note.clone().unwrap_or_else(|| tr("Synchronisation complete", "Synchronisation terminée").into()),
            _ => tr("Synchronise with Garmin", "Synchroniser avec Garmin").into(),
        }
        .into();
        let percent = d.sync_share().map(|f| format!("{:.0}%", f * 100.0));
        div()
            .w(px(46.))
            .flex_none()
            .h_full()
            .bg(t.panel)
            .border_r_1()
            .border_color(t.border)
            .flex()
            .flex_col()
            .items_center()
            .py(px(12.))
            .gap(px(6.))
            .child(crate::ui::drag_window(div().id("rail-logo").w_full().flex().justify_center().pb(px(10.)).child(crate::ui::logo::logo(30., t.text, t.accent))))
            .child(self.travel_buttons(cx))
            .child(item("rail-dashboard", Page::Dashboard, IconName::LayoutDashboard, tr("Today", "Aujourd'hui"), cx))
            .child(item("rail-journal", Page::Journal, IconName::NotebookText, "Journal", cx))
            .child(item("rail-calendar", Page::Calendar, IconName::Calendar, tr("Calendar", "Calendrier"), cx))
            .child(item("rail-stats", Page::Stats, IconName::ChartColumn, tr("Statistics", "Statistiques"), cx))
            .child(item("rail-records", Page::Records, IconName::Trophy, "Records", cx))
            .child(item("rail-map", Page::Heatmap, IconName::Map, tr("Heatmap", "Carte de chaleur"), cx))
            // The empty part of the rail moves the window, as in Bref.
            .child(crate::ui::drag_window(div().id("rail-drag").flex_1().w_full()))
            .child(
                div()
                    .id("rail-sync")
                    .w(px(34.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(3.))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.border))
                    .tooltip_show_delay(crate::ui::TIP_DELAY).tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    // While it runs, a click shows where it is; otherwise it starts one.
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.data.read(cx).syncing() {
                            this.go(Page::Settings, window, cx);
                        } else {
                            this.data.update(cx, |d, cx| d.sync(cx));
                        }
                    }))
                    .child(spin)
                    .when_some(percent, |d, p| d.child(div().font_family(MONO).text_size(px(9.)).text_color(t.accent).child(p))),
            )
            .child(item("rail-settings", Page::Settings, IconName::Settings, tr("Settings", "Réglages"), cx))
    }

    /// Back and forward, one above the other; dimmed when there is nowhere to go.
    fn travel_buttons(&self, cx: &mut Context<Self>) -> Div {
        let t = self.data.read(cx).theme;
        let button = |id: &'static str, name: IconName, tip: &'static str, on: bool, forward: bool, cx: &mut Context<Self>| {
            div()
                .id(id)
                .size(px(30.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .tooltip_show_delay(crate::ui::TIP_DELAY).tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                .when(on, |d| d.cursor_pointer().hover(|d| d.bg(t.border)).on_click(cx.listener(move |this, _, window, cx| this.travel(forward, window, cx))))
                .child(icon(name, if on { t.soft } else { t.dim }, 16.))
        };
        let (back, forward) = (self.history.can_back(), self.history.can_forward());
        div()
            .flex()
            .flex_col()
            .items_center()
            .pb(px(6.))
            .child(button("rail-back", IconName::ChevronLeft, tr("Back", "Précédent"), back, false, cx))
            .child(button("rail-forward", IconName::ChevronRight, tr("Forward", "Suivant"), forward, true, cx))
    }

    /// Bottom-right, like Bref's word count: what the app is doing, errors in red. A click shows
    /// the synchronisation in the settings.
    fn status_line(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let d = self.data.read(cx);
        let t = d.theme;
        let outings = crate::logic::activity::outings(d.acts.len());
        let (text, color) = if let Status::Failed(e) = &d.status {
            (e.clone(), t.bad())
        } else if let Some(line) = d.sync_line() {
            (format!("{} {line}", "Synchronisation"), t.soft)
        } else if let Status::Busy(s) = &d.status {
            (s.clone(), t.soft)
        } else if let Some(why) = &d.fetch.stopped {
            (format!("{outings} · {} : {why}", tr("synchronisation interrupted", "synchronisation interrompue")), t.bad())
        } else {
            (format!("{outings}{}", d.note.as_ref().map(|x| format!(" · {x}")).unwrap_or_default()), t.dim)
        };
        let share = d.sync_share();
        div()
            .id("status")
            .absolute()
            .bottom(px(6.))
            .right(px(14.))
            .max_w(px(620.))
            .px(px(6.))
            .py(px(1.))
            .rounded(px(6.))
            .bg(t.bg)
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(color)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| {
                this.data.update(cx, |d, cx| d.dismiss(cx));
                this.go(Page::Settings, window, cx);
            }))
            .when_some(share, |d, f| {
                d.child(div().w(px(70.)).h(px(4.)).rounded(px(2.)).bg(t.border).child(div().h_full().rounded(px(2.)).bg(t.accent).w(relative(f))))
            })
            .child(div().min_w_0().child(text))
    }

    /// Minimise, maximise, close: a floating pill at the top right, which also moves the window.
    fn window_controls(&self, window: &Window, cx: &App) -> Div {
        let t = self.data.read(cx).theme;
        let controls = window.window_controls();
        let button = |id: &'static str, name: IconName| {
            div()
                .id(id)
                .size(px(24.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|d| d.bg(t.border))
                .child(icon(name, t.soft, 14.))
        };
        crate::ui::drag_window(
            div()
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .flex()
                .items_center()
                .gap(px(2.))
                .p(px(3.))
                .rounded(px(10.))
                .bg(t.panel)
                .border_1()
                .border_color(t.border)
                .shadow(vec![BoxShadow { color: hsla(0., 0., 0., 0.18), offset: point(px(0.), px(1.)), blur_radius: px(6.), spread_radius: px(0.), inset: false }])
                .occlude(),
        )
        .when(controls.minimize, |d| d.child(button("win-min", IconName::Minus).on_click(|_, window, _| window.minimize_window())))
        .when(controls.maximize, |d| {
            let name = if window.is_maximized() { IconName::Copy } else { IconName::Square };
            d.child(button("win-max", name).on_click(|_, window, _| window.zoom_window()))
        })
        .child(button("win-close", IconName::X).on_click(|_, window, _| window.remove_window()))
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.data.read(cx).theme;
        // On Linux gpui-kit draws the frame (shadow, border, resize edges) and the app adds the
        // window buttons. Elsewhere the system does it all.
        let client = matches!(window.window_decorations(), Decorations::Client { .. });
        crate::ui::set_top(if client { 52.0 } else { 28.0 });
        let body = match self.page {
            Page::Dashboard => self.dashboard.clone().into_any_element(),
            Page::Journal => self.journal.clone().into_any_element(),
            Page::Calendar => self.calendar.clone().into_any_element(),
            Page::Activity(_) => self.activity.clone().into_any_element(),
            Page::Stats => self.stats.clone().into_any_element(),
            Page::Records => self.records.clone().into_any_element(),
            Page::Heatmap => self.heatmap.clone().into_any_element(),
            Page::Settings => self.settings.clone().into_any_element(),
        };
        div()
            .size_full()
            .relative()
            .flex()
            .overflow_hidden()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(SANS)
            .text_size(px(13.))
            .track_focus(&self.focus)
            .key_context(match self.page {
                Page::Journal => "Journal",
                Page::Activity(_) => "Activity",
                Page::Calendar => "Calendar",
                _ => "Home",
            })
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.journal.update(cx, |j, cx| j.select_step(1, cx))))
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.journal.update(cx, |j, cx| j.select_step(-1, cx))))
            .on_action(cx.listener(|this, _: &OpenSelected, window, cx| {
                if let Some(id) = this.journal.read(cx).selected {
                    this.go(Page::Activity(id), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Back, window, cx| {
                if this.history.can_back() {
                    this.travel(false, window, cx);
                } else {
                    this.go(Page::Journal, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &GoBack, window, cx| this.travel(false, window, cx)))
            .on_action(cx.listener(|this, _: &GoForward, window, cx| this.travel(true, window, cx)))
            // The mouse back and forward buttons.
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Back), cx.listener(|this, _, window, cx| this.travel(false, window, cx)))
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Forward), cx.listener(|this, _, window, cx| this.travel(true, window, cx)))
            .on_action(cx.listener(|this, _: &CalPrev, _, cx| this.calendar.update(cx, |c, cx| c.step(-1, cx))))
            .on_action(cx.listener(|this, _: &CalNext, _, cx| this.calendar.update(cx, |c, cx| c.step(1, cx))))
            .on_action(cx.listener(|this, _: &CalToday, _, cx| this.calendar.update(cx, |c, cx| c.go_today(cx))))
            .on_action(cx.listener(|this, _: &Older, _, cx| this.activity.update(cx, |a, cx| a.step(true, cx))))
            .on_action(cx.listener(|this, _: &Newer, _, cx| this.activity.update(cx, |a, cx| a.step(false, cx))))
            .child(self.rail(cx))
            .child({
                let cell = self.width.0.clone();
                div()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .relative()
                    .child(
                        canvas(
                            move |b, window, _| {
                                let w = b.size.width / px(1.);
                                // A new width lays the views out again at the next frame. (`refresh` does
                                // nothing while drawing: a frame has to be asked for.)
                                if (cell.get() - w).abs() > 0.5 {
                                    cell.set(w);
                                    window.request_animation_frame();
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(body)
                    // The top of the window, anywhere, moves it (the buttons float above).
                    .when(client, |d| d.child(crate::ui::drag_window(div().id("header-drag").absolute().top(px(0.)).left(px(0.)).right(px(0.)).h(px(HEADER)).occlude())))
            })
            .child(self.status_line(cx))
            .when(client, |d| d.child(self.window_controls(window, cx)))
            // Alt + drag, anywhere: moves the window.
            .when(client, |d| {
                d.capture_any_mouse_down(|e, window, cx| {
                    if e.button == MouseButton::Left && e.modifiers.alt {
                        window.start_window_move();
                        cx.stop_propagation();
                    }
                })
            })
    }
}

#[cfg(test)]
mod tests;
