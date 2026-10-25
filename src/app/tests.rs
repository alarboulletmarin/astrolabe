//! Tests that drive the whole window on GPUI's test platform (no screen): clicks and keys go in,
//! the page shown comes out. The data is made up and never touches the disk nor the key store.

use super::{Shell, bind_keys};
use crate::data::Data;
use crate::logic::activity::ActivityType;
use crate::ui::theme::{Theme, init_fonts};
use crate::views::Page;
use gpui::{AppContext as _, Entity, TestAppContext};

fn outing(id: i64, day: u32, key: &str) -> crate::logic::activity::Activity {
    crate::logic::activity::Activity {
        activity_id: id,
        activity_name: Some(format!("Sortie {id}")),
        start_time_local: format!("2026-10-{day:02} 08:00:00"),
        activity_type: ActivityType { type_key: key.into() },
        distance: Some(10_000.0),
        duration: Some(3000.0),
        elevation_gain: None,
        average_hr: None,
        calories: None,
    }
}

/// A window on `n` outings, one a day over the last days of October 2026, the newest first.
fn start(cx: &mut TestAppContext, n: i64) -> (Entity<Shell>, &mut gpui::VisualTestContext) {
    cx.update(gpui_kit::init);
    cx.update(bind_keys);
    cx.update(init_fonts);
    let acts: Vec<_> = (0..n).map(|i| outing(1000 - i, (25 - i % 25) as u32, if i % 2 == 0 { "running" } else { "cycling" })).collect();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        let data = cx.new(|_| Data::with_activities(Theme::for_appearance(window.appearance()), acts));
        Shell::with_data(data, window, cx)
    });
    cx.run_until_parked();
    (shell, cx)
}

/// A click on the top edge of `selector`, where a tall element (an unfolded row) has its own line.
fn click_top(cx: &mut gpui::VisualTestContext, selector: &'static str) {
    let b = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is not on screen"));
    cx.simulate_click(b.origin + gpui::point(gpui::px(20.), gpui::px(14.)), gpui::Modifiers::none());
    cx.run_until_parked();
}

fn page(shell: &Entity<Shell>, cx: &mut gpui::VisualTestContext) -> Page {
    shell.read_with(cx, |s, _| s.page)
}

fn click(cx: &mut gpui::VisualTestContext, selector: &'static str) {
    let at = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is not on screen")).center();
    cx.simulate_click(at, gpui::Modifiers::none());
    cx.run_until_parked();
}

/// The rail opens each page; Alt + ← and Alt + → walk back and forth through them.
#[gpui::test]
fn the_rail_opens_pages_and_the_keys_walk_through_them(cx: &mut TestAppContext) {
    let (shell, cx) = start(cx, 5);
    assert_eq!(page(&shell, cx), Page::Dashboard);
    for (rail, expected) in [("rail-journal", Page::Journal), ("rail-calendar", Page::Calendar), ("rail-stats", Page::Stats), ("rail-records", Page::Records), ("rail-map", Page::Heatmap), ("rail-settings", Page::Settings)] {
        click(cx, rail);
        assert_eq!(page(&shell, cx), expected, "{rail}");
    }
    // A click on the page already shown moves nothing: one step back is still the one before.
    click(cx, "rail-settings");
    cx.simulate_keystrokes("alt-left");
    assert_eq!(page(&shell, cx), Page::Heatmap);
    cx.simulate_keystrokes("alt-left alt-left");
    assert_eq!(page(&shell, cx), Page::Stats);
    cx.simulate_keystrokes("alt-right");
    assert_eq!(page(&shell, cx), Page::Records);
    // Back past the first page, and forward past the last: nothing breaks, nothing moves.
    cx.simulate_keystrokes("alt-left alt-left alt-left alt-left alt-left alt-left alt-left");
    assert_eq!(page(&shell, cx), Page::Dashboard);
    cx.simulate_keystrokes("alt-right alt-right alt-right alt-right alt-right alt-right alt-right alt-right");
    assert_eq!(page(&shell, cx), Page::Settings);
}

/// A click on a journal row opens the outing; Esc and the arrows then behave.
#[gpui::test]
fn a_journal_row_opens_the_outing(cx: &mut TestAppContext) {
    let (shell, cx) = start(cx, 5);
    click(cx, "rail-journal");
    click(cx, "row-1000");
    assert_eq!(page(&shell, cx), Page::Activity(1000));
    // The newest has nothing newer; an older one opens in place of it.
    cx.simulate_keystrokes("right");
    assert_eq!(page(&shell, cx), Page::Activity(1000));
    cx.simulate_keystrokes("left");
    assert_eq!(page(&shell, cx), Page::Activity(999));
    // Moving from one outing to another leaves one history entry: Esc returns to the journal.
    cx.simulate_keystrokes("escape");
    assert_eq!(page(&shell, cx), Page::Journal);
}

/// Every page draws with no outing at all and with a great many.
#[gpui::test]
fn every_page_draws_empty_and_full(cx: &mut TestAppContext) {
    for n in [0, 1, 1500] {
        let (shell, cx) = start(cx, n);
        for rail in ["rail-journal", "rail-calendar", "rail-stats", "rail-records", "rail-map", "rail-settings", "rail-dashboard"] {
            click(cx, rail);
        }
        assert_eq!(page(&shell, cx), Page::Dashboard, "{n} outings");
    }
}

/// A record row opens on its ten best, a click on one opens its outing, and a second click on the row closes it.
#[gpui::test]
fn a_record_row_unfolds_its_ranking(cx: &mut TestAppContext) {
    let (shell, cx) = start(cx, 30);
    // Tall enough for the row to be laid out (what is below the window is not drawn).
    cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1600.)));
    click(cx, "rail-records");
    assert!(cx.debug_bounds("rank-Course sur route-5-0").is_none(), "closed at first");
    click(cx, "rec-Course sur route-5");
    let ranks = (0..12).filter(|k| cx.debug_bounds(Box::leak(format!("rank-Course sur route-5-{k}").into_boxed_str())).is_some()).count();
    assert_eq!(ranks, 10, "ten best, though there are fifteen runs");
    // The row's own line closes it again.
    click_top(cx, "rec-Course sur route-5");
    assert!(cx.debug_bounds("rank-Course sur route-5-0").is_none());
    // A click on a ranked outing opens it (and does not close nor open anything else).
    click_top(cx, "rec-Course sur route-5");
    click(cx, "rank-Course sur route-5-3");
    assert!(matches!(page(&shell, cx), Page::Activity(_)));
}
