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
    for (rail, expected) in [("rail-journal", Page::Journal), ("rail-calendar", Page::Calendar), ("rail-stats", Page::Stats), ("rail-form", Page::Form), ("rail-records", Page::Records), ("rail-map", Page::Heatmap), ("rail-settings", Page::Settings)] {
        click(cx, rail);
        assert_eq!(page(&shell, cx), expected, "{rail}");
    }
    // The keys of `bind_keys`: Cmd + [ and ] on macOS.
    let (back, forward) = if cfg!(target_os = "macos") { ("cmd-[", "cmd-]") } else { ("alt-left", "alt-right") };
    let keys = |key: &str, n: usize| vec![key; n].join(" ");
    // A click on the page already shown moves nothing: one step back is still the one before.
    click(cx, "rail-settings");
    cx.simulate_keystrokes(back);
    assert_eq!(page(&shell, cx), Page::Heatmap);
    cx.simulate_keystrokes(&keys(back, 2));
    assert_eq!(page(&shell, cx), Page::Form);
    cx.simulate_keystrokes(forward);
    assert_eq!(page(&shell, cx), Page::Records);
    // Back past the first page, and forward past the last: nothing breaks, nothing moves.
    cx.simulate_keystrokes(&keys(back, 7));
    assert_eq!(page(&shell, cx), Page::Dashboard);
    cx.simulate_keystrokes(&keys(forward, 8));
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

/// The columns menu takes its own clicks: the row under it neither opens nor lights up.
#[gpui::test]
fn the_columns_menu_keeps_its_clicks_from_the_rows_below(cx: &mut TestAppContext) {
    let (shell, cx) = start(cx, 40);
    click(cx, "rail-journal");
    click(cx, "journal-columns");
    // Each column the menu shows, on and off again: all of them lie over rows.
    let panel = cx.debug_bounds("columns-panel").expect("the menu is open");
    let mut clicked = 0;
    for c in 0..crate::logic::columns::ALL.len() {
        let name: &'static str = Box::leak(format!("column-{c}").into_boxed_str());
        if !cx.debug_bounds(name).is_some_and(|b| panel.contains(&b.center())) {
            continue;
        }
        for _ in 0..2 {
            click(cx, name);
            assert_eq!(page(&shell, cx), Page::Journal, "column {c}");
        }
        clicked += 1;
    }
    assert!(clicked >= 5, "{clicked} columns in sight");
    assert_eq!(page(&shell, cx), Page::Journal);
    assert_eq!(shell.read_with(cx, |s, cx| s.journal.read(cx).selected), None);
}

/// Every page draws with no outing at all and with a great many.
#[gpui::test]
fn every_page_draws_empty_and_full(cx: &mut TestAppContext) {
    for n in [0, 1, 1500] {
        let (shell, cx) = start(cx, n);
        for rail in ["rail-journal", "rail-calendar", "rail-stats", "rail-form", "rail-records", "rail-map", "rail-settings", "rail-dashboard"] {
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
    assert!(cx.debug_bounds("rank-road-5-0").is_none(), "closed at first");
    click(cx, "rec-road-5");
    let ranks = (0..12).filter(|k| cx.debug_bounds(Box::leak(format!("rank-road-5-{k}").into_boxed_str())).is_some()).count();
    assert_eq!(ranks, 10, "ten best, though there are fifteen runs");
    // The row's own line closes it again.
    click_top(cx, "rec-road-5");
    assert!(cx.debug_bounds("rank-road-5-0").is_none());
    // A click on a ranked outing opens it (and does not close nor open anything else).
    click_top(cx, "rec-road-5");
    click(cx, "rank-road-5-3");
    assert!(matches!(page(&shell, cx), Page::Activity(_)));
}

/// The same pages in English: none outing, one, and a great many; the record rows keep their ids
/// whatever the language.
#[gpui::test]
fn every_page_draws_in_english(cx: &mut TestAppContext) {
    let _english = crate::lang::force(false);
    for n in [0, 1, 1500] {
        let (shell, cx) = start(cx, n);
        cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1600.)));
        for rail in ["rail-journal", "rail-calendar", "rail-stats", "rail-form", "rail-records", "rail-map", "rail-settings", "rail-dashboard"] {
            click(cx, rail);
        }
        assert_eq!(page(&shell, cx), Page::Dashboard, "{n} outings");
    }
    let (shell, cx) = start(cx, 30);
    cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1600.)));
    click(cx, "rail-records");
    click(cx, "rec-road-5");
    let ranks = (0..12).filter(|k| cx.debug_bounds(Box::leak(format!("rank-road-5-{k}").into_boxed_str())).is_some()).count();
    assert_eq!(ranks, 10);
    click(cx, "rank-road-5-3");
    assert!(matches!(page(&shell, cx), Page::Activity(_)));
}

/// `n` outings, one a day up to today, each with the time it spent in a band of heart rate; the
/// highest `max_hr` of them is 190, the second 188.
fn trained(cx: &mut TestAppContext, n: i64) -> (Entity<Shell>, &mut gpui::VisualTestContext) {
    cx.update(gpui_kit::init);
    cx.update(bind_keys);
    cx.update(init_fonts);
    let today = crate::views::today();
    let acts: Vec<_> = (0..n)
        .map(|i| {
            let mut a = outing(1000 - i, 1, "running");
            a.start_time_local = format!("{} 08:00:00", today - chrono::Days::new(i as u64));
            a
        })
        .collect();
    let derived = acts
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let mut bands = vec![0.0; 40];
            bands[28] = 2400.0 + i as f32 * 10.0; // 140-145 bpm
            (a.activity_id, crate::logic::fit::Derived { hr_bands: bands, max_hr: [190.0, 188.0, 170.0][i % 3], ..Default::default() })
        })
        .collect();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        let data = cx.new(|_| Data::with_tracks(Theme::for_appearance(window.appearance()), acts, derived));
        Shell::with_data(data, window, cx)
    });
    cx.run_until_parked();
    (shell, cx)
}

/// The form page says why when it has nothing to work out, and draws its figures when it has.
#[gpui::test]
fn the_form_page_draws_figures_when_heart_rates_are_known(cx: &mut TestAppContext) {
    for english in [false, true] {
        let _lang = crate::lang::force(!english);
        // Outings, but no track read: no maximum heart rate, so no load.
        let (_, cx) = start(cx, 5);
        click(cx, "rail-form");
        assert!(cx.debug_bounds("form-empty").is_some() && cx.debug_bounds("form-figures").is_none());
        // None at all.
        let (_, cx) = start(cx, 0);
        click(cx, "rail-form");
        assert!(cx.debug_bounds("form-empty").is_some());
        // One outing, a few days, weeks, more than a year: figures every time.
        for n in [1, 3, 8, 50, 400] {
            let (_, cx) = trained(cx, n);
            cx.simulate_resize(gpui::size(gpui::px(1300.), gpui::px(1500.)));
            click(cx, "rail-form");
            assert!(cx.debug_bounds("form-figures").is_some() && cx.debug_bounds("form-empty").is_none(), "{n} outings");
        }
    }
}

/// Timings on a large made-up history, for `book/src/performance.md`. Ignored by `cargo test`. Optimised, but
/// with the debug selectors the clicks need (a release build drops them):
/// `CARGO_PROFILE_DEV_OPT_LEVEL=3 cargo test -- --ignored --nocapture measure`. Prints, never fails on
/// a figure (shared CI runners vary too much); it fails only if a page stops drawing.
#[gpui::test]
#[ignore]
fn measure_a_large_history(cx: &mut TestAppContext) {
    use crate::logic::fit::{self, Derived, Effort, Sample, Track};
    use std::time::Instant;
    // `MEASURE_N` activities, 5,000 by default.
    let n: i64 = std::env::var("MEASURE_N").ok().and_then(|v| v.parse().ok()).unwrap_or(5000);
    let keys = ["running", "running", "trail_running", "road_biking", "lap_swimming", "walking"];
    let acts: Vec<_> = (0..n)
        .map(|i| {
            let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 25).unwrap() - chrono::Duration::days(i * 3 / 2);
            let mut a = outing(100_000 - i, 1, keys[i as usize % keys.len()]);
            a.start_time_local = format!("{day} 08:00:00");
            a.average_hr = Some(140.0 + (i % 20) as f64);
            a
        })
        .collect();
    // 400 points per track, around one place: 2 million points for the heat map.
    let derived: std::collections::HashMap<i64, Derived> = acts
        .iter()
        .map(|a| {
            let k = a.activity_id as f32;
            let line = (0..400).map(|j| [45.9 + 0.02 * (j as f32 / 64.0 + k).sin(), 6.13 + 0.03 * (j as f32 / 64.0 + k).cos()]).collect();
            let efforts = fit::EFFORTS.iter().take(6).map(|&m| Effort { metres: m as f32, seconds: m as f32 * 0.3 }).collect();
            (a.activity_id, Derived { line, efforts, profile: vec![500.0; fit::PROFILE], hr_bands: vec![60.0; 40], max_hr: 180.0, version: fit::VERSION })
        })
        .collect();
    let mut out = Vec::new();

    // The files read at launch.
    let t = Instant::now();
    let acts_json = serde_json::to_vec(&acts).unwrap();
    let derived_json = serde_json::to_vec(&derived).unwrap();
    out.push(format!("write {n} activities + tracks to JSON: {:?} ({} + {} KB)", t.elapsed(), acts_json.len() / 1024, derived_json.len() / 1024));
    let t = Instant::now();
    let _: Vec<crate::logic::activity::Activity> = serde_json::from_slice(&acts_json).unwrap();
    let _: std::collections::HashMap<i64, Derived> = serde_json::from_slice(&derived_json).unwrap();
    out.push(format!("read them back (what opening does): {:?}", t.elapsed()));

    // A long activity: 8 hours at one sample a second.
    let samples: Vec<Sample> = (0..28_800)
        .map(|s| {
            let f = s as f64;
            Sample { t: s as f32, lat: Some(45.9 + 0.05 * (f / 3000.0).sin()), lon: Some(6.13 + 0.05 * (f / 3000.0).cos()), dist: Some(s as f32 * 3.0), alt: Some(500.0 + 300.0 * (f / 5000.0).sin() as f32), hr: Some(140.0 + (s % 30) as f32), speed: Some(3.0) }
        })
        .collect();
    let track = Track { samples, laps: Vec::new() };
    let t = Instant::now();
    let d = fit::derive(&track);
    let splits = track.splits(1000.0);
    out.push(format!("derive an 8 h track (28,800 samples): {:?} ({} map points, {} splits)", t.elapsed(), d.line.len(), splits.len()));

    // The heat map's raster, drawn off the UI thread: every track, on a 2× screen.
    let lines: Vec<&[[f32; 2]]> = derived.values().map(|d| d.line.as_slice()).collect();
    let view = crate::logic::map::View::fit([45.88, 6.10, 45.92, 6.16], 2400.0, 1600.0, 52.0);
    let t = Instant::now();
    let counts = crate::logic::map::heat(&lines, &view, 2400, 1600);
    out.push(format!("heat raster of {} points, 2400 × 1600: {:?}", lines.iter().map(|l| l.len()).sum::<usize>(), t.elapsed()));
    assert!(counts.iter().any(|&c| c > 0));
    drop(lines);

    // The window: first draw, then each page opened from the rail; what a page starts in the
    // background (the heat raster) is counted with it.
    cx.update(gpui_kit::init);
    cx.update(bind_keys);
    cx.update(init_fonts);
    let t = Instant::now();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        let data = cx.new(|_| Data::with_tracks(Theme::for_appearance(window.appearance()), acts, derived));
        Shell::with_data(data, window, cx)
    });
    cx.run_until_parked();
    out.push(format!("open the window on Today: {:?}", t.elapsed()));
    for rail in ["rail-journal", "rail-calendar", "rail-stats", "rail-form", "rail-records", "rail-map", "rail-dashboard"] {
        let t = Instant::now();
        click(cx, rail);
        out.push(format!("{rail}: {:?}", t.elapsed()));
    }
    click(cx, "rail-journal");
    let t = Instant::now();
    for _ in 0..50 {
        cx.simulate_keystrokes("down");
    }
    out.push(format!("journal, 50 × ↓: {:?} each", t.elapsed() / 50));
    assert_eq!(page(&shell, cx), Page::Journal);
    println!("\n{n} activities\n{}", out.join("\n"));
}
