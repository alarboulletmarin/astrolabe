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

/// With a key, Settings shows where it is kept and hides its field until asked to change it;
/// without one, the field shows.
#[gpui::test]
fn the_key_field_shows_when_asked(cx: &mut TestAppContext) {
    let (shell, cx) = start(cx, 3);
    let data = shell.read_with(cx, |s, _| s.data.clone());
    click(cx, "rail-settings");
    let shown = |cx: &mut gpui::VisualTestContext| cx.debug_bounds("save-key").is_some();
    assert!(shown(cx) && cx.debug_bounds("change-key").is_none());
    data.update(cx, |d, cx| {
        d.key = Some("GCDC_test".into());
        cx.notify();
    });
    cx.run_until_parked();
    assert!(!shown(cx) && cx.debug_bounds("sync-now").is_some());
    click(cx, "change-key");
    assert!(shown(cx) && cx.debug_bounds("delete-key").is_some());
    click(cx, "cancel-key");
    assert!(!shown(cx) && cx.debug_bounds("change-key").is_some());
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
        for rail in ["rail-journal", "rail-calendar", "rail-stats", "rail-form", "rail-records", "rail-map", "rail-loops", "rail-settings", "rail-dashboard"] {
            click(cx, rail);
        }
        assert_eq!(page(&shell, cx), Page::Dashboard, "{n} outings");
    }
}

/// The Loops page: a click on the map sets the start and loops come; a click or the arrows pick
/// one; the save key writes it as GPX where the dialog says. The region is the made-up town,
/// already built.
#[gpui::test]
fn a_click_on_the_map_plans_loops_and_one_is_saved_as_gpx(cx: &mut TestAppContext) {
    let (shell, cx) = start(cx, 3);
    shell.update(cx, |s, cx| s.data.update(cx, |d, _| d.loops.graph = Some(std::sync::Arc::new(crate::logic::graph::tests::town()))));
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    assert_eq!(page(&shell, cx), Page::Loops);
    // The whole region shows at first: zoomed on the town, a click on its south street, 1 km in.
    let (lat, lon) = (43.70, 7.25 + 1000.0 / (111_195.0 * 0.7230));
    shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| {
        l.look_at(lat, lon, 15.0);
        l.type_km("5", window, cx);
        cx.notify();
    }));
    cx.run_until_parked();
    let at = shell.read_with(cx, |s, cx| s.loops.read(cx).screen_of(lat, lon));
    cx.simulate_click(at, gpui::Modifiers::none());
    cx.run_until_parked();
    let found = shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.len());
    assert!(found >= 2, "{found} loops, {:?}", shell.read_with(cx, |s, cx| s.data.read(cx).loops.plan_error.clone()));
    click(cx, "loop-1");
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).selected()), 1);
    cx.simulate_keystrokes("down");
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).selected()), 2 % found);
    cx.simulate_keystrokes("up up");
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).selected()), 0);
    // Saved where the dialog says.
    let out = std::env::temp_dir().join(format!("astrolabe-loop-{}.gpx", std::process::id()));
    let _ = std::fs::remove_file(&out);
    cx.simulate_keystrokes(&format!("{mod_key}-s"));
    let chosen = out.clone();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    cx.run_until_parked();
    let gpx = std::fs::read_to_string(&out).unwrap();
    assert!(gpx.contains("<trkseg>") && gpx.matches("<trkpt").count() > 20);
    std::fs::remove_file(&out).unwrap();
    // Other loops for the same start.
    let first = shell.read_with(cx, |s, cx| s.data.read(cx).loops.found[0].seed);
    cx.simulate_keystrokes(&format!("{mod_key}-n"));
    cx.run_until_parked();
    assert_ne!(shell.read_with(cx, |s, cx| s.data.read(cx).loops.found[0].seed), first);
}

/// GPX files imported join the routes kept, the last one shown; one that can't be read is named.
#[gpui::test]
fn a_gpx_track_is_imported_as_a_route(cx: &mut TestAppContext) {
    let (shell, cx) = start(cx, 3);
    let town = crate::logic::graph::tests::town();
    // Two of the town's streets end to end: a track of a few hundred metres.
    let pts = |e: u32| town.line(e).iter().map(|(p, _)| [p[0] as f64 / 1e7, p[1] as f64 / 1e7]).collect::<Vec<_>>();
    let line: Vec<[f64; 2]> = pts(0).into_iter().chain(pts(0).into_iter().rev().skip(1)).collect();
    shell.update(cx, |s, cx| s.data.update(cx, |d, _| d.loops.graph = Some(std::sync::Arc::new(town))));
    let dir = std::env::temp_dir().join(format!("astrolabe-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let named = dir.join("watch.gpx");
    std::fs::write(&named, crate::logic::gpx::write("Tour du parc ✓", &line, &vec![Some(50.0); line.len()])).unwrap();
    let unnamed = dir.join("sans nom.gpx");
    let body: String = line[..line.len() - 1].iter().map(|p| format!("<trkpt lat=\"{}\" lon=\"{}\"/>", p[0], p[1])).collect();
    std::fs::write(&unnamed, format!("<gpx><trk><trkseg>{body}</trkseg></trk></gpx>")).unwrap();
    let broken = dir.join("broken.gpx");
    std::fs::write(&broken, "<gpx>nothing</gpx>").unwrap();
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    click(cx, "loops-import");
    assert!(cx.did_prompt_for_paths());
    let chosen = vec![named.clone(), broken.clone(), unnamed.clone()];
    cx.simulate_path_prompt_response(move |_| Some(chosen));
    cx.run_until_parked();
    let (names, shown, note) = shell.read_with(cx, |s, cx| {
        let d = s.data.read(cx);
        (d.loops.routes.iter().map(|r| r.name.clone()).collect::<Vec<_>>(), d.loops.found.first().cloned(), d.note.clone())
    });
    assert_eq!(names, ["Tour du parc ✓", "sans nom"], "named after the track, else after the file");
    let shown = shown.expect("the last one shown");
    assert_eq!(shown.line.len(), line.len() - 1);
    assert!(shown.track, "marked as brought from a file");
    assert!(shown.dist > 100.0, "{}", shown.dist);
    assert!(note.is_some_and(|n| n.contains("broken.gpx")), "the file that could not be read is named");
    assert_eq!(cx.debug_bounds("route-1").is_some(), true, "both in the list");
    // Opening the first one shows its track, with its altitudes.
    click(cx, "route-1");
    let first = shell.read_with(cx, |s, cx| s.data.read(cx).loops.found[0].clone());
    assert_eq!((first.eles[0], first.up, first.down), (Some(50.0), 0.0, 0.0));
    // A dialog closed without a file: nothing changes.
    click(cx, "loops-import");
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    assert_eq!(shell.read_with(cx, |s, cx| s.data.read(cx).loops.routes.len()), 2);
    // The same file again: the route already kept is shown, not a second one.
    click(cx, "loops-import");
    let again = named.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![again]));
    cx.run_until_parked();
    let (n, shown) = shell.read_with(cx, |s, cx| (s.data.read(cx).loops.routes.len(), s.data.read(cx).loops.found[0].eles[0]));
    assert_eq!((n, shown), (2, Some(50.0)));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Loops page after the first click: a second click doesn't move the start, dragging it
/// does and plans from there, and a distance typed plans again once the typing stops.
#[gpui::test]
fn the_start_is_dragged_and_a_new_distance_plans_again(cx: &mut TestAppContext) {
    use gpui::{Modifiers, MouseButton};
    let (shell, cx) = start(cx, 3);
    shell.update(cx, |s, cx| s.data.update(cx, |d, _| d.loops.graph = Some(std::sync::Arc::new(crate::logic::graph::tests::town()))));
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    let lon_at = |m: f64| 7.25 + m / (111_195.0 * 0.7230);
    let (lat, lon) = (43.70, lon_at(1000.0));
    shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| {
        l.look_at(lat, lon, 15.0);
        l.type_km("5", window, cx);
        cx.notify();
    }));
    cx.run_until_parked();
    let screen = |cx: &mut gpui::VisualTestContext, la: f64, lo: f64| shell.read_with(cx, |s, cx| s.loops.read(cx).screen_of(la, lo));
    let at = screen(cx, lat, lon);
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    let generation = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| s.data.read(cx).loops.generation);
    let first = generation(cx);
    assert!(first > 0 && shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.len()) >= 2);
    let set = shell.read_with(cx, |s, cx| s.loops.read(cx).start()).unwrap();
    // The map fitted the loops: the start is somewhere else on screen now.
    let at = screen(cx, set.0, set.1);
    // Another click, off the start: the start stays, a point to pass by is set; undone.
    cx.simulate_click(gpui::point(at.x + gpui::px(60.), at.y - gpui::px(60.)), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).start()), Some(set));
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).pins().len()), 1);
    // The change is said under the map, and undo, on the map, names it.
    use crate::views::loops::Gesture;
    let change = |cx: &mut gpui::VisualTestContext, back: bool| shell.read_with(cx, |s, cx| s.loops.read(cx).next_change(back));
    let said = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| s.loops.read(cx).said());
    assert_eq!(change(cx, true), Some(Gesture::Pin));
    assert!(said(cx).is_some_and(|s| s.starts_with("Point de passage ajouté · ") && s.ends_with("+Z pour annuler")), "{:?}", said(cx));
    assert!(cx.debug_bounds("loops-undo").is_some() && cx.debug_bounds("loops-unpin").is_some());
    cx.simulate_keystrokes(&format!("{mod_key}-z"));
    cx.run_until_parked();
    assert!(shell.read_with(cx, |s, cx| s.loops.read(cx).pins().is_empty()));
    assert_eq!((change(cx, true), change(cx, false)), (Some(Gesture::Start), Some(Gesture::Pin)));
    assert!(said(cx).is_some_and(|s| s.starts_with("Annulé : point de passage ajouté")), "{:?}", said(cx));
    assert!(cx.debug_bounds("loops-unpin").is_none());
    // The line goes after three seconds.
    cx.executor().advance_clock(std::time::Duration::from_secs(3));
    cx.run_until_parked();
    assert_eq!(said(cx), None);
    let at = screen(cx, set.0, set.1);
    // Dragged 500 m east along the street: loops from there.
    let to = screen(cx, lat, lon_at(1500.0));
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(gpui::point((at.x + to.x) / 2., at.y), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let (la, lo) = shell.read_with(cx, |s, cx| s.loops.read(cx).start()).unwrap();
    assert!(crate::logic::fit::metres([la, lo], [lat, lon_at(1500.0)]) < 30.0, "{la} {lo}");
    assert!(generation(cx) > first);
    assert_eq!(change(cx, true), Some(Gesture::StartMoved));
    let begins = shell.read_with(cx, |s, cx| s.data.read(cx).loops.found[0].line[0]);
    assert!(crate::logic::fit::metres(begins, [lat, lon_at(1500.0)]) < 40.0, "{begins:?}");
    // A distance typed: nothing until the typing stops, then loops of that length.
    let before = generation(cx);
    // With loops, the settings are folded to a line: unfolded first.
    assert!(cx.debug_bounds("loops-zone").is_none());
    click(cx, "loops-settings");
    shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| l.focus_km(window, cx)));
    cx.simulate_keystrokes("backspace 7");
    cx.run_until_parked();
    assert_eq!(generation(cx), before);
    cx.executor().advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    assert!(generation(cx) > before);
    let d = shell.read_with(cx, |s, cx| s.data.read(cx).loops.found[0].dist);
    assert!((d - 7000.0).abs() < 7000.0 * 0.05, "{d}");
    // A new start: none, no loops, and the next click sets it again.
    click(cx, "loops-clear");
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).start()), None);
    assert!(shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.is_empty()));
    let at = screen(cx, lat, lon);
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert!(shell.read_with(cx, |s, cx| s.loops.read(cx).start()).is_some());
    assert!(!shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.is_empty()));
}

/// A click on the map while a loop other than the first is chosen, its grade shown: the click
/// selects the first loop again, and the frame that handles it paints one loop, not the grade of
/// the one chosen before on the line of the first (out of bounds when that one is shorter).
#[gpui::test]
fn a_point_asked_with_another_loop_chosen_paints_one_loop(cx: &mut TestAppContext) {
    use crate::logic::loops::Loop;
    use gpui::Modifiers;
    let (shell, cx) = start(cx, 3);
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    let (lat, lon) = (43.70, 7.26);
    shell.update_in(cx, |s, _, cx| s.loops.update(cx, |l, cx| {
        l.look_at(lat, lon, 15.0);
        cx.notify();
    }));
    cx.run_until_parked();
    let screen = |cx: &mut gpui::VisualTestContext, la: f64, lo: f64| shell.read_with(cx, |s, cx| s.loops.read(cx).screen_of(la, lo));
    // No network: the first click only sets the start.
    let at = screen(cx, lat, lon);
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert!(shell.read_with(cx, |s, cx| s.loops.read(cx).start()).is_some());
    // Two loops, flat then climbing 8 %, the first shorter: the second chosen.
    let climbing = |n: usize| {
        let line: Vec<[f64; 2]> = (0..n).map(|i| [lat + i as f64 * 1e-4, lon]).collect();
        let eles = (0..n).map(|i| Some(100.0 + i.saturating_sub(n / 2) as f32 * 0.9)).collect();
        Loop::from_track(line, eles, None)
    };
    shell.update(cx, |s, cx| s.data.update(cx, |d, _| d.loops.found = std::sync::Arc::new(vec![climbing(40), climbing(120)])));
    shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| l.step(1, cx)));
    cx.run_until_parked();
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).selected()), 1);
    // A point asked: the first loop is selected again, in the frame that paints the second.
    let set = shell.read_with(cx, |s, cx| s.loops.read(cx).start()).unwrap();
    let at = screen(cx, set.0, set.1);
    cx.simulate_click(gpui::point(at.x + gpui::px(60.), at.y - gpui::px(60.)), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).pins().len()), 1);
}

/// Points on the Loops page: a click asks the loops to pass by a point; undo and redo; a point
/// of the loop dragged, one taken away by a right click, one made by dragging the line.
#[gpui::test]
fn the_loop_is_shaped_by_hand(cx: &mut TestAppContext) {
    use gpui::{Modifiers, MouseButton};
    let (shell, cx) = start(cx, 3);
    shell.update(cx, |s, cx| s.data.update(cx, |d, _| d.loops.graph = Some(std::sync::Arc::new(crate::logic::graph::tests::town()))));
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    let at = |east: f64, north: f64| [43.70 + north / 111_195.0, 7.25 + east / (111_195.0 * 0.7230)];
    let s0 = at(1000.0, 0.0);
    shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| {
        l.look_at(43.70 + 1500.0 / 111_195.0, s0[1], 14.0);
        l.type_km("7", window, cx);
        cx.notify();
    }));
    cx.run_until_parked();
    let screen = |cx: &mut gpui::VisualTestContext, p: [f64; 2]| shell.read_with(cx, |s, cx| s.loops.read(cx).screen_of(p[0], p[1]));
    let found = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.clone());
    let selected = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| s.loops.read(cx).selected());
    let near = |line: &[[f64; 2]], p: [f64; 2]| line.iter().map(|q| crate::logic::fit::metres(*q, p)).fold(f64::MAX, f64::min);
    let p = screen(cx, s0);
    cx.simulate_click(p, Modifiers::none());
    cx.run_until_parked();
    let first = found(cx);
    assert!(!first.is_empty());
    // A point to pass by: every loop goes there.
    let via = at(1500.0, 1500.0);
    let p = screen(cx, via);
    cx.simulate_click(p, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).pins().len()), 1);
    let with = found(cx);
    assert!(!with.is_empty() && with.iter().all(|l| near(&l.line, via) < 40.0), "{:?}", with.iter().map(|l| near(&l.line, via)).collect::<Vec<_>>());
    // Undone: no point, the loops of before; done again.
    cx.simulate_keystrokes(&format!("{mod_key}-z"));
    cx.run_until_parked();
    assert!(shell.read_with(cx, |s, cx| s.loops.read(cx).pins().is_empty()));
    assert_eq!(found(cx)[0].line, first[0].line);
    cx.simulate_keystrokes(&format!("{mod_key}-shift-z"));
    cx.run_until_parked();
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).pins().len()), 1);
    assert_eq!(found(cx)[0].line, with[0].line);
    // The whole town in sight, right of the panel (each new set of loops is fitted).
    shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| {
        l.look_at(43.70 + 1500.0 / 111_195.0, 7.25 + 500.0 / (111_195.0 * 0.7230), 14.0);
        cx.notify();
    }));
    cx.run_until_parked();
    // A point of the loop dragged elsewhere: the loop goes there now, the others stay.
    let i = selected(cx);
    let l = found(cx)[i].clone();
    let k = l.pinned.iter().position(|p| !p).unwrap_or(0);
    let (from, to) = (screen(cx, l.waypoints[k]), at(2500.0, 2500.0));
    let to_px = screen(cx, to);
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(gpui::point((from.x + to_px.x) / 2., (from.y + to_px.y) / 2.), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to_px, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to_px, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let moved = found(cx)[i].clone();
    assert!(near(&moved.line, to) < 40.0 && near(&moved.line, via) < 40.0, "{} {}", near(&moved.line, to), near(&moved.line, via));
    assert_eq!(moved.waypoints.len(), l.waypoints.len());
    assert!(moved.pinned.iter().all(|p| *p), "a loop shaped by hand keeps all its points");
    assert_eq!(found(cx).len(), with.len());
    // A right click takes a point away.
    let n = moved.waypoints.len();
    let p = screen(cx, moved.waypoints[k]);
    cx.simulate_mouse_down(p, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(p, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(found(cx)[i].waypoints.len(), n - 1);
    assert!(near(&found(cx)[i].line, via) < 40.0, "the point asked for stays");
    // Dragging the line makes a point there.
    let l = found(cx)[i].clone();
    let mid = l.line[l.line.len() / 3];
    // Somewhere on a street, away from the loop: on a loop of the first set.
    let goal = first.iter().flat_map(|f| f.line.iter().copied()).max_by(|a, b| near(&l.line, *a).total_cmp(&near(&l.line, *b))).unwrap();
    let (from, to) = (screen(cx, mid), screen(cx, goal));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(gpui::point(from.x + gpui::px(10.), from.y), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(found(cx)[i].waypoints.len(), n);
    assert!(near(&l.line, goal) > 200.0);
    assert!(near(&found(cx)[i].line, goal) < 40.0, "{}", near(&found(cx)[i].line, goal));
    // The pointer over the loop shows where on the profile; off it, nothing.
    let l = found(cx)[i].clone();
    let k = l.line.len() / 2;
    let p = screen(cx, l.line[k]);
    cx.simulate_mouse_move(p, None, Modifiers::none());
    cx.run_until_parked();
    let h = shell.read_with(cx, |s, cx| s.loops.read(cx).hover()).expect("a point of the profile");
    assert!(crate::logic::fit::metres(l.line[h], l.line[k]) < 30.0);
    let off = screen(cx, at(-2000.0, -2000.0));
    cx.simulate_mouse_move(off, None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).hover()), None);
    // A click on a point moves nothing.
    let before = found(cx)[i].clone();
    let p = screen(cx, before.waypoints[0]);
    cx.simulate_click(p, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(found(cx)[i].line, before.line);
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).pins().len()), 1);
}

/// With loops: the settings fold to a line, the profile shows in a band under the map, and a
/// click on another loop's line chooses it.
#[gpui::test]
fn a_loop_is_chosen_on_the_map(cx: &mut TestAppContext) {
    use gpui::{Modifiers, MouseButton};
    let (shell, cx) = start(cx, 3);
    shell.update(cx, |s, cx| s.data.update(cx, |d, _| d.loops.graph = Some(std::sync::Arc::new(crate::logic::graph::tests::town()))));
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    let s0 = [43.70, 7.25 + 1000.0 / (111_195.0 * 0.7230)];
    shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| {
        l.look_at(43.70 + 1500.0 / 111_195.0, s0[1], 14.0);
        l.type_km("7", window, cx);
        cx.notify();
    }));
    cx.run_until_parked();
    // No loops yet: the settings are open, no band.
    assert!(cx.debug_bounds("loops-band").is_none() && cx.debug_bounds("loops-settings").is_none());
    let p = shell.read_with(cx, |s, cx| s.loops.read(cx).screen_of(s0[0], s0[1]));
    cx.simulate_click(p, Modifiers::none());
    cx.run_until_parked();
    let found = shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.clone());
    assert!(found.len() >= 2);
    assert!(cx.debug_bounds("loops-band").is_some() && cx.debug_bounds("loops-settings").is_some());
    // Folded, then unfolded and folded again.
    assert!(cx.debug_bounds("loops-avoid").is_none());
    click(cx, "loops-settings");
    assert!(cx.debug_bounds("loops-avoid").is_some());
    click(cx, "loops-settings");
    assert!(cx.debug_bounds("loops-avoid").is_none());
    // The whole town in sight, then a point of another loop far from the chosen one.
    shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| {
        l.look_at(43.70 + 1500.0 / 111_195.0, 7.25 + 500.0 / (111_195.0 * 0.7230), 14.0);
        cx.notify();
    }));
    cx.run_until_parked();
    let chosen = shell.read_with(cx, |s, cx| s.loops.read(cx).selected());
    let near = |line: &[[f64; 2]], p: [f64; 2]| line.iter().map(|q| crate::logic::fit::metres(*q, p)).fold(f64::MAX, f64::min);
    let (k, q) = found
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != chosen)
        .flat_map(|(i, l)| l.line.iter().map(move |q| (i, *q)))
        .max_by(|a, b| near(&found[chosen].line, a.1).total_cmp(&near(&found[chosen].line, b.1)))
        .unwrap();
    assert!(near(&found[chosen].line, q) > 60.0, "the loops differ somewhere");
    let p = shell.read_with(cx, |s, cx| s.loops.read(cx).screen_of(q[0], q[1]));
    cx.simulate_mouse_down(p, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(p, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).selected()), k);
    // Nothing else changed: no point to pass by, the same loops.
    assert!(shell.read_with(cx, |s, cx| s.loops.read(cx).pins().is_empty()));
    assert_eq!(shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.len()), found.len());
    // The other way round, and back by undoing; each card says its ground in a bar.
    assert!(cx.debug_bounds("loops-reverse").is_some());
    click(cx, "loops-reverse");
    let r = shell.read_with(cx, |s, cx| s.data.read(cx).loops.found[k].clone());
    assert_eq!(r.line, found[k].line.iter().rev().copied().collect::<Vec<_>>());
    assert_eq!((r.up, r.down), (found[k].down, found[k].up));
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-z"));
    cx.run_until_parked();
    assert_eq!(shell.read_with(cx, |s, cx| s.data.read(cx).loops.found[k].line.clone()), found[k].line);
}

/// Before a start, the spots one's outings start from are offered; one click starts there.
/// Undo and redo have their buttons; a setting that is on or off is a box to tick.
#[gpui::test]
fn the_loop_starts_where_one_usually_does(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(bind_keys);
    cx.update(init_fonts);
    let home = [43.70f32, 7.25 + (1000.0 / (111_195.0 * 0.7230)) as f32];
    let acts: Vec<_> = (0..3).map(|i| outing(1000 - i, (25 - i) as u32, "running")).collect();
    let derived = acts.iter().map(|a| (a.activity_id, crate::logic::fit::Derived { line: vec![home, [43.71, 7.26]], ..Default::default() })).collect();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        let data = cx.new(|_| Data::with_tracks(Theme::for_appearance(window.appearance()), acts, derived));
        Shell::with_data(data, window, cx)
    });
    cx.run_until_parked();
    shell.update(cx, |s, cx| s.data.update(cx, |d, _| d.loops.graph = Some(std::sync::Arc::new(crate::logic::graph::tests::town()))));
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| l.type_km("6", window, cx)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("loops-undo").is_none() && cx.debug_bounds("loops-usual-1").is_none());
    click(cx, "loops-usual-0");
    let start = shell.read_with(cx, |s, cx| s.loops.read(cx).start()).expect("a start");
    assert!(crate::logic::fit::metres([start.0, start.1], [home[0] as f64, home[1] as f64]) < 1.0);
    assert!(!shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.is_empty()));
    // Undone by its button, done again by the other.
    click(cx, "loops-undo");
    assert!(shell.read_with(cx, |s, cx| s.loops.read(cx).start()).is_none());
    click(cx, "loops-redo");
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).start()), Some(start));
    // A box ticked: the setting is kept once the pause is over.
    click(cx, "loops-settings");
    click(cx, "loops-paved");
    cx.executor().advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    assert_eq!(shell.read_with(cx, |s, cx| s.data.read(cx).settings.get("loops.paved").cloned()), Some("1".into()));
}

/// The chosen loop in 3D: the relief awaited, then the ground; a drag turns it, shift-drag moves
/// it, the wheel comes closer, the pointer over the line shows where on the profile; 2D again.
#[gpui::test]
fn the_loop_is_seen_in_3d(cx: &mut TestAppContext) {
    use gpui::{Modifiers, MouseButton, ScrollDelta, ScrollWheelEvent};
    let (shell, cx) = start(cx, 3);
    shell.update(cx, |s, cx| s.data.update(cx, |d, _| d.loops.graph = Some(std::sync::Arc::new(crate::logic::graph::tests::town()))));
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    let s0 = [43.70, 7.25 + 1000.0 / (111_195.0 * 0.7230)];
    shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| {
        l.look_at(43.70 + 1500.0 / 111_195.0, s0[1], 14.0);
        l.type_km("7", window, cx);
        cx.notify();
    }));
    cx.run_until_parked();
    let p = shell.read_with(cx, |s, cx| s.loops.read(cx).screen_of(s0[0], s0[1]));
    cx.simulate_click(p, Modifiers::none());
    cx.run_until_parked();
    assert!(!shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.is_empty()));
    shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| {
        l.show_3d(true);
        cx.notify();
    }));
    cx.run_until_parked();
    // No tile in the tests: the relief is awaited.
    assert!(shell.read_with(cx, |s, cx| s.loops.read(cx).camera()).is_none());
    // A slope rising north, 1 m per tile pixel.
    let mut slope = |_z: u8, _x: u32, _y: u32| Some(std::sync::Arc::new((0..256 * 256).map(|i| 300.0 - (i / 256) as f32).collect::<Vec<f32>>()));
    shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| {
        l.ground_from(&mut slope, cx);
        cx.notify();
    }));
    cx.run_until_parked();
    let cam = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| s.loops.read(cx).camera()).expect("a scene");
    let b = shell.update(cx, |s, cx| s.loops.update(cx, |l, _| l.bounds_3d()));
    assert!(b.size.width > gpui::px(400.) && b.size.height > gpui::px(200.), "{b:?}");
    let mid = gpui::point(b.origin.x + b.size.width * 0.75, b.origin.y + b.size.height * 0.5);
    let by = |dx: f32, dy: f32| gpui::point(mid.x + gpui::px(dx), mid.y + gpui::px(dy));
    // Dragged with the right button, or with Shift: turned.
    let shift = Modifiers { shift: true, ..Modifiers::none() };
    let mut c1 = cam(cx);
    for (button, mods) in [(MouseButton::Right, Modifiers::none()), (MouseButton::Left, shift)] {
        let c0 = c1;
        cx.simulate_mouse_down(mid, button, mods);
        cx.simulate_mouse_move(by(60., 0.), button, mods);
        cx.simulate_mouse_move(by(120., 30.), button, mods);
        cx.simulate_mouse_up(by(120., 30.), button, mods);
        cx.run_until_parked();
        c1 = cam(cx);
        assert!((c1.yaw - c0.yaw).abs() > 0.5 && c1.pitch > c0.pitch && c1.target == c0.target, "{button:?} {c0:?} {c1:?}");
    }
    // Dragged: moved over the ground, not turned.
    cx.simulate_mouse_down(mid, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(by(80., 40.), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(by(80., 40.), MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let c2 = cam(cx);
    assert!(c2.target != c1.target && c2.yaw == c1.yaw && c2.pitch == c1.pitch, "{c1:?} {c2:?}");
    // The wheel: closer.
    cx.simulate_event(ScrollWheelEvent { position: mid, delta: ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(120.))), ..Default::default() });
    cx.run_until_parked();
    assert!(cam(cx).dist < c2.dist);
    // The pointer over the line: that point on the profile; far from it, none.
    let line = shell.read_with(cx, |s, cx| s.data.read(cx).loops.found[s.loops.read(cx).selected()].line.clone());
    let mid_line = line.len() / 2;
    let (k, p) = shell
        .update(cx, |s, cx| s.loops.update(cx, |l, cx| (mid_line..line.len()).chain(0..mid_line).find_map(|i| l.screen_3d(i, cx).filter(|p| b.contains(p) && p.x > b.origin.x + gpui::px(400.)).map(|p| (i, p)))))
        .expect("a point of the line in sight");
    cx.simulate_mouse_move(p, None, Modifiers::none());
    cx.run_until_parked();
    let h = shell.read_with(cx, |s, cx| s.loops.read(cx).hover()).expect("a point of the profile");
    assert!(crate::logic::fit::metres(line[h], line[k]) < 60.0);
    cx.simulate_mouse_move(gpui::point(b.origin.x + b.size.width - gpui::px(2.), b.origin.y + gpui::px(2.)), None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).hover()), None);
    // 2D again: the map, and its points under the pointer.
    shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| {
        l.show_3d(false);
        cx.notify();
    }));
    cx.run_until_parked();
    let p = shell.read_with(cx, |s, cx| s.loops.read(cx).screen_of(line[mid_line][0], line[mid_line][1]));
    cx.simulate_mouse_move(p, None, Modifiers::none());
    cx.run_until_parked();
    assert!(shell.read_with(cx, |s, cx| s.loops.read(cx).hover()).is_some());
    // Space without a flight is nobody's: nothing starts.
    cx.simulate_keystrokes("space");
    cx.run_until_parked();
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).flying()), None);
    // Flying over the loop, in 3D: the eye follows the line and the mark goes along the profile.
    shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| {
        l.show_3d(true);
        l.fly(cx);
    }));
    let state = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| (s.loops.read(cx).hover(), s.loops.read(cx).flying(), s.loops.read(cx).camera().map(|c| c.target)));
    // A few steps of the flight's clock (each draws a picture: kept short).
    let wait = |cx: &mut gpui::VisualTestContext, steps: u64| {
        for _ in 0..steps {
            cx.executor().advance_clock(std::time::Duration::from_millis(34));
            cx.run_until_parked();
        }
    };
    wait(cx, 2);
    let a = state(cx);
    assert_eq!(a.1, Some(true));
    wait(cx, 8);
    let b = state(cx);
    assert!(b.0 >= a.0 && b.2 != a.2 && b.1 == Some(true), "{a:?} {b:?}");
    assert!(cam(cx).dist < 1000.0, "close to the ground");
    // Space pauses; the clock goes on, the flight does not. Space again: it goes on.
    cx.simulate_keystrokes("space");
    wait(cx, 4);
    let c = state(cx);
    assert!(c.1 == Some(false) && c.2 == b.2 && c.0 == b.0, "{b:?} {c:?}");
    cx.simulate_keystrokes("space");
    wait(cx, 4);
    let d = state(cx);
    assert!(d.1 == Some(true) && d.2 != c.2, "{c:?} {d:?}");
    // Taking the ground in hand pauses it too.
    cx.simulate_mouse_down(mid, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(mid, MouseButton::Left, Modifiers::none());
    wait(cx, 2);
    assert_eq!(state(cx).1, Some(false));
    // Near the end: it stops on the last point, and stays there; started again, from the start.
    shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| l.fly_seek(30.0, cx)));
    cx.simulate_keystrokes("space");
    wait(cx, 30);
    let end = state(cx);
    assert_eq!((end.0, end.1), (Some(line.len() - 1), Some(false)));
    wait(cx, 3);
    assert_eq!(state(cx), end);
    cx.simulate_keystrokes("space");
    wait(cx, 2);
    let again = state(cx);
    assert!(again.1 == Some(true) && again.0 < Some(line.len() / 2), "{again:?}");
    // A space typed in a field is the field's: the flight goes on.
    shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| l.focus_name(window, cx)));
    cx.simulate_keystrokes("space");
    wait(cx, 2);
    assert_eq!(state(cx).1, Some(true));
    // The plain ground instead of the photos, kept in the settings; and back.
    let photos = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| s.data.read(cx).settings.get("loops.photos").cloned());
    shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| l.set_photos(false, cx)));
    wait(cx, 2);
    assert_eq!(photos(cx).as_deref(), Some("0"));
    shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| l.set_photos(true, cx)));
    wait(cx, 2);
    assert_eq!(photos(cx).as_deref(), Some("1"));
    // Back on the map the flight stops.
    shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| {
        l.show_3d(false);
        cx.notify();
    }));
    wait(cx, 2);
    let off = state(cx);
    wait(cx, 3);
    assert_eq!(state(cx).2, off.2);
}

/// A loop kept under a name: listed, written to disk, opened again as it was, changed and kept
/// again (the same route), kept as a copy, taken away from the list (still in the file).
#[gpui::test]
fn a_loop_is_kept_and_opened_again(cx: &mut TestAppContext) {
    use gpui::{Modifiers, MouseButton};
    let root = std::env::temp_dir().join(format!("astrolabe-routes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    crate::platform::store::use_root(root.clone());
    let (shell, cx) = start(cx, 3);
    shell.update(cx, |s, cx| s.data.update(cx, |d, _| d.loops.graph = Some(std::sync::Arc::new(crate::logic::graph::tests::town()))));
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    let s0 = [43.70, 7.25 + 1000.0 / (111_195.0 * 0.7230)];
    let look = |shell: &gpui::Entity<Shell>, cx: &mut gpui::VisualTestContext| {
        shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| {
            l.look_at(43.70 + 1500.0 / 111_195.0, 7.25 + 500.0 / (111_195.0 * 0.7230), 14.0);
            cx.notify();
        }));
        cx.run_until_parked();
    };
    look(&shell, cx);
    shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| l.type_km("6", window, cx)));
    let screen = |cx: &mut gpui::VisualTestContext, p: [f64; 2]| shell.read_with(cx, |s, cx| s.loops.read(cx).screen_of(p[0], p[1]));
    let found = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.clone());
    let routes = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| s.data.read(cx).loops.routes.clone());
    let p = screen(cx, s0);
    cx.simulate_click(p, Modifiers::none());
    cx.run_until_parked();
    click(cx, "loop-1");
    let kept = found(cx)[1].clone();
    shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| l.name_as("Tour du parc ✓", window, cx)));
    click(cx, "loops-keep");
    assert_eq!(routes(cx).len(), 1);
    assert_eq!(routes(cx)[0].name, "Tour du parc ✓");
    let file = root.join("data/routes.json");
    assert!(std::fs::read_to_string(&file).unwrap().contains("Tour du parc"));
    // Begun again elsewhere, then the route opened: its loop, its start, its points.
    click(cx, "loops-clear");
    assert!(found(cx).is_empty());
    click(cx, "route-0");
    assert_eq!(found(cx).len(), 1);
    // Read back from the file the next time; here from memory: the same, to the last digit.
    assert_eq!(found(cx)[0].line, kept.line);
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).start()), Some((routes(cx)[0].start[0], routes(cx)[0].start[1])));
    // Changed by hand and kept again: the same route, changed.
    look(&shell, cx);
    let l = found(cx)[0].clone();
    let from = screen(cx, l.waypoints[0]);
    let to = gpui::point(from.x + gpui::px(80.), from.y - gpui::px(80.));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(gpui::point(from.x + gpui::px(40.), from.y), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_ne!(found(cx)[0].line, kept.line);
    click(cx, "loops-keep");
    assert_eq!(routes(cx).len(), 1);
    assert_eq!(routes(cx)[0].shape.line, found(cx)[0].line);
    // A copy; then the first taken away: out of the list, still in the file.
    click(cx, "loops-copy");
    assert_eq!(routes(cx).len(), 2);
    assert_ne!(routes(cx)[0].id, routes(cx)[1].id);
    click(cx, "route-trash-1");
    assert_eq!(routes(cx).iter().filter(|r| !r.trashed).count(), 1);
    assert!(cx.debug_bounds("route-1").is_none());
    let on_disk = crate::platform::store::load_routes().unwrap();
    assert_eq!(on_disk.len(), 2);
    assert_eq!(on_disk.iter().filter(|r| r.trashed).count(), 1);
    // Read back, a route is the loop it was, to a millimetre (JSON may round the last digit).
    let (a, b) = (on_disk.iter().find(|r| !r.trashed).unwrap(), routes(cx).into_iter().find(|r| !r.trashed).unwrap());
    assert_eq!(a.shape.line.len(), b.shape.line.len());
    assert!(a.shape.line.iter().zip(&b.shape.line).all(|(p, q)| crate::logic::fit::metres(*p, *q) < 0.001));
    assert_eq!((a.name.as_str(), a.shape.dist, a.pins.len()), (b.name.as_str(), b.shape.dist, b.pins.len()));
    // A planned loop is read back as planned; a track kept before tracks were marked, as a track.
    assert!(!a.shape.track);
    let mut old = a.clone();
    (old.shape.waypoints, old.shape.seed) = (Vec::new(), 0);
    let json = serde_json::to_string(&[old]).unwrap().replace(r#","track":false"#, "");
    std::fs::write(crate::platform::store::data_dir().join("routes.json"), json).unwrap();
    assert!(crate::platform::store::load_routes().unwrap()[0].shape.track);
    std::fs::remove_dir_all(&root).ok();
}

/// A zone to avoid: drawn by a button then a click, sized by its edge, kept in the settings,
/// the loops kept out of it, taken away by a right click, put back by undo.
#[gpui::test]
fn a_zone_is_drawn_and_kept_out_of(cx: &mut TestAppContext) {
    use gpui::{Modifiers, MouseButton};
    let (shell, cx) = start(cx, 3);
    shell.update(cx, |s, cx| s.data.update(cx, |d, _| d.loops.graph = Some(std::sync::Arc::new(crate::logic::graph::tests::town()))));
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    let at = |east: f64, north: f64| [43.70 + north / 111_195.0, 7.25 + east / (111_195.0 * 0.7230)];
    let look = |cx: &mut gpui::VisualTestContext| {
        shell.update(cx, |s, cx| s.loops.update(cx, |l, cx| {
            l.look_at(43.70 + 1500.0 / 111_195.0, 7.25 + 500.0 / (111_195.0 * 0.7230), 14.0);
            cx.notify();
        }));
        cx.run_until_parked();
    };
    look(cx);
    shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| l.type_km("6", window, cx)));
    let screen = |cx: &mut gpui::VisualTestContext, p: [f64; 2]| shell.read_with(cx, |s, cx| s.loops.read(cx).screen_of(p[0], p[1]));
    let zones = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| s.loops.read(cx).zones());
    // Drawn before the start: kept, no complaint. The button is in the folded "Avoid" group.
    assert!(cx.debug_bounds("loops-zone").is_none());
    click(cx, "loops-avoid");
    click(cx, "loops-zone");
    // The region is fitted at the first frame: in close now.
    look(cx);
    let c = at(1500.0, 1500.0);
    let p = screen(cx, c);
    cx.simulate_click(p, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(zones(cx).len(), 1);
    assert!(shell.read_with(cx, |s, cx| s.loops.read(cx).start()).is_none(), "the click drew a zone, it set no start");
    assert!(shell.read_with(cx, |s, cx| s.data.read(cx).settings.get("loops.zones").is_some_and(|z| z.contains(','))));
    // Sized by its edge: dragged 200 m farther east.
    let z = zones(cx)[0];
    let edge = [z.lat, z.lon + z.radius / (111_195.0 * z.lat.to_radians().cos())];
    let (from, to) = (screen(cx, edge), screen(cx, [edge[0], edge[1] + 200.0 / (111_195.0 * z.lat.to_radians().cos())]));
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(gpui::point((from.x + to.x) / 2., from.y), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let r = zones(cx)[0].radius;
    assert!((r - (z.radius + 200.0)).abs() < 15.0, "{r}");
    // A start, loops: none runs in the zone.
    let p = screen(cx, at(1000.0, 0.0));
    cx.simulate_click(p, Modifiers::none());
    cx.run_until_parked();
    let found = shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.clone());
    assert!(!found.is_empty());
    let zone = zones(cx)[0];
    assert!(found.iter().all(|l| l.line.iter().all(|p| !zone.holds(p[0], p[1]))));
    // A right click inside takes it away; undo puts it back.
    look(cx);
    let p = screen(cx, [zone.lat + 50.0 / 111_195.0, zone.lon]);
    cx.simulate_mouse_down(p, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(p, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    assert!(zones(cx).is_empty());
    cx.simulate_keystrokes(&format!("{mod_key}-z"));
    cx.run_until_parked();
    assert_eq!(zones(cx).len(), 1);
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
        for rail in ["rail-journal", "rail-calendar", "rail-stats", "rail-form", "rail-records", "rail-map", "rail-loops", "rail-settings", "rail-dashboard"] {
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

/// The pointer on the profile shows its point on the map, under the band the profile is in;
/// off the map, the point goes.
#[gpui::test]
fn the_profile_points_on_the_map(cx: &mut TestAppContext) {
    use gpui::Modifiers;
    let (shell, cx) = start(cx, 3);
    shell.update(cx, |s, cx| s.data.update(cx, |d, _| {
        // Rising eastward, so the loops have a profile.
        let mut g = crate::logic::graph::tests::town();
        g.set_elevation(&mut crate::logic::elevation::Dem::new(8, |_, _| Some((0..256 * 256).map(|i| (i % 256) as f32).collect())));
        d.loops.graph = Some(std::sync::Arc::new(g));
    }));
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    let (lat, lon) = (43.70, 7.25 + 1500.0 / (111_195.0 * 0.7230));
    shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| {
        l.look_at(lat, lon, 15.0);
        l.type_km("5", window, cx);
        cx.notify();
    }));
    cx.run_until_parked();
    let at = shell.read_with(cx, |s, cx| s.loops.read(cx).screen_of(lat, lon));
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert!(!shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.is_empty()));
    let hover = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| s.loops.read(cx).hover());
    let profile = cx.debug_bounds("loop-profile").expect("the profile");
    let mut seen = Vec::new();
    for f in [0.3, 0.6] {
        cx.simulate_mouse_move(gpui::point(profile.origin.x + profile.size.width * f, profile.center().y), None, Modifiers::none());
        cx.run_until_parked();
        seen.push(hover(cx).unwrap_or_else(|| panic!("at {f} of the profile")));
    }
    assert!(seen[1] > seen[0]);
    // Off the profile, onto the band's climbs: none.
    let band = cx.debug_bounds("loops-band").unwrap();
    cx.simulate_mouse_move(gpui::point(band.origin.x + band.size.width - gpui::px(20.), band.center().y), None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(hover(cx), None);
}

/// The start typed in: coordinates, then an address the place search finds (a stand-in here),
/// another of its answers, none, and one outside the region.
#[gpui::test]
fn a_typed_start_is_found_and_brought_into_view(cx: &mut TestAppContext) {
    use crate::views::loops::Gesture;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    let (shell, cx) = start(cx, 3);
    let lon_at = |m: f64| 7.25 + m / (111_195.0 * 0.7230);
    // The stand-in search: two places in the town, none, or Paris.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = asked.clone();
    let (a, b) = (lon_at(1000.0), lon_at(2000.0));
    std::thread::spawn(move || {
        for mut conn in listener.incoming().flatten() {
            let mut head = Vec::new();
            let mut byte = [0u8];
            while !head.ends_with(b"\r\n\r\n") && conn.read(&mut byte).unwrap_or(0) == 1 {
                head.push(byte[0]);
            }
            let path = String::from_utf8_lossy(&head).split(' ').nth(1).unwrap_or("").to_string();
            let body = if path.contains("q=nowhere") {
                "[]".to_string()
            } else if path.contains("q=paris") {
                r#"[{"lat": "48.8584", "lon": "2.2945", "name": "Tour Eiffel", "display_name": "Tour Eiffel, Paris", "address": {"city": "Paris"}}]"#.to_string()
            } else {
                format!(r#"[{{"lat": "43.70", "lon": "{a}", "name": "", "display_name": "1, Rue Longue, Ville", "address": {{"house_number": "1", "road": "Rue Longue", "city": "Ville"}}}},
                           {{"lat": "43.70", "lon": "{b}", "name": "Place Haute", "display_name": "Place Haute, Ville", "address": {{"city": "Ville"}}}}]"#)
            };
            log.lock().unwrap().push(path);
            let _ = conn.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
        }
    });
    shell.update(cx, |s, cx| s.data.update(cx, |d, _| {
        d.loops.graph = Some(std::sync::Arc::new(crate::logic::graph::tests::town()));
        d.loops.places_base = base;
    }));
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    let typed = |cx: &mut gpui::VisualTestContext, text: &str| {
        shell.update_in(cx, |s, window, cx| s.loops.update(cx, |l, cx| {
            l.type_km("5", window, cx);
            l.type_place(text, window, cx);
        }));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
    };
    let start_of = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| s.loops.read(cx).start());
    let said = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| s.loops.read(cx).place_said());
    let near = |p: Option<(f64, f64)>, lat: f64, lon: f64| p.is_some_and(|p| crate::logic::fit::metres([p.0, p.1], [lat, lon]) < 1.0);
    // Coordinates: the start at once, no search, and loops.
    typed(cx, &format!("43.70, {:.6}", lon_at(1500.0)));
    assert!(near(start_of(cx), 43.70, lon_at(1500.0)), "{:?}", start_of(cx));
    assert!(asked.lock().unwrap().is_empty());
    assert!(!shell.read_with(cx, |s, cx| s.data.read(cx).loops.found.is_empty()));
    // An address: the first place found is the start, moved there, named; the other offered.
    typed(cx, "rue longue");
    assert!(near(start_of(cx), 43.70, a), "{:?}", start_of(cx));
    assert_eq!(said(cx), (Some("1 Rue Longue, Ville".into()), None));
    assert_eq!(shell.read_with(cx, |s, cx| s.loops.read(cx).next_change(true)), Some(Gesture::StartMoved));
    let path = asked.lock().unwrap().last().cloned().unwrap();
    assert!(path.contains("q=rue+longue") || path.contains("q=rue%20longue"), "{path}");
    assert!(path.contains("bounded=1") && path.contains("viewbox=") && path.contains("addressdetails=1"), "{path}");
    click(cx, "loops-other-0");
    assert!(near(start_of(cx), 43.70, b));
    assert_eq!(said(cx).0.as_deref(), Some("Place Haute, Ville"));
    assert!(cx.debug_bounds("loops-other-0").is_none(), "the other, taken, is no longer offered");
    // Undo (the arrow: the field has the keys): the start back where the address put it.
    click(cx, "loops-undo");
    assert!(near(start_of(cx), 43.70, a));
    // Nothing found, or found outside the region: said, and the start stays.
    let before = start_of(cx);
    typed(cx, "nowhere");
    assert!(said(cx).1.is_some_and(|e| e.contains("Provence")));
    assert_eq!(start_of(cx), before);
    typed(cx, "paris");
    assert!(said(cx).1.is_some_and(|e| e.contains("Maps") || e.contains("Cartes")), "{:?}", said(cx));
    assert_eq!(start_of(cx), before);
    // Empty: nothing asked.
    let n = asked.lock().unwrap().len();
    typed(cx, "   ");
    assert_eq!(asked.lock().unwrap().len(), n);
}

/// Photo or plain map in 2D too, before any loop: kept in the settings, shared with the 3D.
#[gpui::test]
fn the_map_shows_photos_in_2d(cx: &mut TestAppContext) {
    let (shell, cx) = start(cx, 3);
    let mod_key = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.simulate_keystrokes(&format!("{mod_key}-l"));
    cx.run_until_parked();
    let state = |cx: &mut gpui::VisualTestContext| shell.read_with(cx, |s, cx| (s.loops.read(cx).map_photos(), s.data.read(cx).settings.get("loops.photos").cloned()));
    // Photos by default.
    assert_eq!(state(cx), (true, None));
    click(cx, "loops-ground-1");
    assert_eq!(state(cx), (false, Some("0".into())));
    click(cx, "loops-ground-0");
    assert_eq!(state(cx), (true, Some("1".into())));
}
