//! Loops: click a start on the map, ask for a distance (exact or a range) and what to run on, and
//! the page proposes loops on the region's paths, with their climb, their profile and a GPX file
//! for each. Everything is computed on the machine, from the region's OpenStreetMap extract.

use crate::data::{Data, Snapshot};
use crate::lang::{decimal, tr, trf};
use crate::logic::activity::{CLIMB_COST, effort_pace, fmt_duration, fmt_km, fmt_m, group};
use crate::logic::loops::{Loop, Request, Route, Target, Usual};
use crate::logic::map::{self, View};
use crate::logic::relief;
use crate::logic::slope;
use crate::logic::route::{Places, Prefs, Zone};
use crate::ui::chart::{Series, line_chart, nearest, range};
use crate::ui::theme::blend;
use crate::ui::{Plots, button, heading, icon, num, render_image, segmented};
use crate::views::Page;
use crate::views::map::{MapState, map_widget};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{prelude::FluentBuilder, *};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[path = "loops3d.rs"]
mod three;

/// The height of the band under the map that holds the profile and the climbs.
const BAND: f64 = 132.0;

/// A setting that is on or off: a box, ticked with the accent, and what it says.
fn check(id: &'static str, label: &'static str, on: bool, t: crate::ui::theme::Theme) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .child(
            div()
                .size(px(15.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .bg(if on { blend(t.accent, t.bg, 0.12) } else { t.bg })
                .border_1()
                .border_color(if on { t.accent } else { t.border })
                .when(on, |d| d.child(icon(IconName::Check, t.accent, 11.))),
        )
        .child(div().text_size(px(13.)).text_color(t.text).child(label))
}

/// A loop's ground as a bar: asphalt, other ways, trails; the shares in its tooltip.
fn surfaces(l: &Loop, i: usize, t: crate::ui::theme::Theme) -> Stateful<Div> {
    let (trail, paved) = (l.trail.clamp(0.0, 1.0), l.paved.clamp(0.0, 1.0 - l.trail.clamp(0.0, 1.0)));
    let other = (1.0 - trail - paved).max(0.0);
    let pct = |f: f32| format!("{:.0}{}", (f * 100.0).round() + 0.0, crate::lang::pct());
    let tip = format!("{} {} · {} {} · {} {}", pct(paved), tr("asphalt", "bitume"), pct(other), tr("other ways", "autres chemins"), pct(trail), tr("trails", "sentiers"));
    let part = |f: f32, c: Hsla| div().h_full().w(relative(f)).bg(c);
    div()
        .id(("loop-ground", i))
        .h(px(4.))
        .w_full()
        .flex()
        .rounded(px(2.))
        .overflow_hidden()
        .child(part(paved, blend(t.text, t.bg, 0.55)))
        .child(part(other, blend(t.text, t.bg, 0.25)))
        .child(part(trail, t.accent))
        .tooltip_show_delay(crate::ui::TIP_DELAY)
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
}

/// The letter that names the `i`-th loop proposed, in the list and on the map.
fn letter(i: usize) -> char {
    (b'A' + (i % 26) as u8) as char
}

/// How many loops are proposed at once.
const COUNT: usize = 3;
/// The panel's width, over the map.
const PANEL: f64 = 340.0;
/// How near the pointer must come to a point on the map to take it, in pixels.
const GRAB: f32 = 12.0;
/// A setting changed: the loops are planned again after this pause, so a few quick changes plan
/// once.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(400);

/// What the pointer is moving on the map.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Handle {
    Start,
    /// A point asked for, before the loops (index in `pins`).
    Pin(usize),
    /// A point of the loop chosen (index in its waypoints, as in `draft`).
    Point(usize),
    /// A zone to keep out of: its centre, or its edge (its size).
    Zone(usize),
    ZoneEdge(usize),
}

/// A new zone's radius, metres.
const ZONE_RADIUS: f64 = 250.0;

/// Zones as kept in the settings: `lat,lon,radius` separated by `;`.
fn read_zones(s: &str) -> Vec<Zone> {
    s.split(';')
        .filter_map(|z| {
            let mut v = z.split(',').map(|n| n.trim().parse::<f64>().ok().filter(|f| f.is_finite()));
            let (lat, lon, radius) = (v.next()??, v.next()??, v.next()??);
            ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) && radius > 0.0).then_some(Zone { lat, lon, radius: radius.min(50_000.0) })
        })
        .collect()
}

fn write_zones(z: &[Zone]) -> String {
    z.iter().map(|z| format!("{:.6},{:.6},{:.0}", z.lat, z.lon, z.radius)).collect::<Vec<_>>().join(";")
}

/// (lat, lon) `m` metres east of a position.
fn east_of(lat: f64, lon: f64, m: f64) -> [f64; 2] {
    [lat, lon + m / (111_195.0 * lat.to_radians().cos().max(0.01))]
}

/// What undo puts back.
#[derive(Clone)]
struct Step {
    start: Option<(f64, f64)>,
    pins: Vec<(f64, f64)>,
    zones: Vec<Zone>,
    found: Arc<Vec<Loop>>,
    selected: usize,
}

/// How far a pointer is from a line on screen, and the index of the line's point nearest to it.
fn from_line(line: &[(f32, f32)], (x, y): (f32, f32)) -> Option<(f32, usize)> {
    let mut best: Option<(f32, usize)> = None;
    for (i, w) in line.windows(2).enumerate() {
        let ((ax, ay), (bx, by)) = (w[0], w[1]);
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((x - ax) * dx + (y - ay) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let d = (ax + t * dx - x).hypot(ay + t * dy - y);
        if best.is_none_or(|b| d < b.0) {
            best = Some((d, if t < 0.5 { i } else { i + 1 }));
        }
    }
    best
}

/// Where a point taken on a loop's line at `at` (an index of its line) goes among its waypoints:
/// after those the line reaches before it.
fn insert_at(line: &[[f64; 2]], waypoints: &[[f64; 2]], at: usize) -> usize {
    let index = |p: [f64; 2]| line.iter().enumerate().map(|(i, q)| (crate::logic::fit::metres(*q, p), i)).fold((f64::MAX, 0), |a, b| if b.0 < a.0 { b } else { a }).1;
    waypoints.iter().filter(|w| index(**w) < at).count()
}

#[derive(Default)]
struct Relief {
    /// The shade and the view it was drawn for.
    image: Option<(Arc<RenderImage>, View, (f64, f64))>,
    busy: bool,
    drawn: Option<u64>,
}

pub struct LoopsView {
    data: Entity<Data>,
    snap: Snapshot,
    map: MapState,
    start: Option<(f64, f64)>,
    exact: bool,
    km: Entity<InputState>,
    min_km: Entity<InputState>,
    max_km: Entity<InputState>,
    climb: Entity<InputState>,
    /// 0: streets, 1: either, 2: trails.
    trail: usize,
    avoid_paved: bool,
    avoid_industrial: bool,
    lit: bool,
    /// One's own tracks: 1 run them again, −1 discover others, 0 either.
    mine: i8,
    zones: Vec<Zone>,
    /// The next click on the map draws a zone.
    placing_zone: bool,
    /// The avoid settings unfolded.
    avoid_open: bool,
    /// The settings unfolded while there are loops (folded to a line, they leave room for them).
    settings_open: bool,
    /// The loop pointed at in the list: its line comes forward on the map.
    card_hover: Option<usize>,
    /// The chosen loop drawn in the colours of its grade.
    show_slope: bool,
    /// The IGN's photos on the 3D ground (else the plain ground with the roads).
    pub(super) photos: bool,
    /// The climb pointed at in the list.
    climb_hover: Option<usize>,
    /// The chosen loop's stretches by grade, and which loop they are of.
    runs: Option<(u64, Arc<Vec<(usize, usize, u8)>>)>,
    seed: u64,
    selected: usize,
    /// The point of the profile pointed at.
    hover: Option<usize>,
    show_relief: bool,
    relief: Relief,
    /// The chosen loop shown in 3D, and its scene.
    three_d: bool,
    scene: Option<three::Scene>,
    plots: Plots,
    /// What the input asked that can't be read.
    bad_input: Option<String>,
    /// The point being dragged on the map.
    drag: Option<Handle>,
    /// Bumped by each setting changed: only the last one's pause plans.
    pending: u64,
    /// Points the loops must pass by, (lat, lon).
    pins: Vec<(f64, f64)>,
    /// The chosen loop's waypoints as they are being moved, until it is routed again.
    draft: Option<Vec<[f64; 2]>>,
    /// Where the pointer took a point (barely moved, it was a click), and what was there before.
    grabbed_at: Option<(Point<Pixels>, Step)>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// The name to keep the chosen loop under.
    name: Entity<InputState>,
    /// The kept route shown, if the loops are one: keeping again updates it.
    opened: Option<u64>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Page> for LoopsView {}

/// A distance typed in km, with a comma or a dot.
fn read_km(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.5 && *v <= 200.0).map(|v| v * 1000.0)
}

impl LoopsView {
    pub fn new(data: Entity<Data>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = data.read(cx).settings.clone();
        let get = |k: &str, default: &str| settings.get(k).cloned().unwrap_or_else(|| default.into());
        let input = |value: String, window: &mut Window, cx: &mut Context<Self>| cx.new(|cx| InputState::new(window, cx).default_value(value));
        let km = input(get("loops.km", "12"), window, cx);
        let min_km = input(get("loops.min", "10"), window, cx);
        let max_km = input(get("loops.max", "15"), window, cx);
        let climb = cx.new(|cx| InputState::new(window, cx).placeholder(tr("any", "libre")).default_value(get("loops.climb", "")));
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(tr("Name (optional)", "Nom (facultatif)")));
        let mut subs = vec![cx.observe(&data, |this, data, cx| {
            this.snap = data.read(cx).snapshot();
            cx.notify();
        })];
        for i in [&km, &min_km, &max_km, &climb] {
            subs.push(cx.subscribe(i, |this, _, e: &InputEvent, cx| {
                match e {
                    InputEvent::PressEnter { .. } => this.propose(false, cx),
                    InputEvent::Change => this.settle(cx),
                    _ => {}
                }
            }));
        }
        LoopsView {
            snap: data.read(cx).snapshot(),
            exact: get("loops.mode", "exact") != "range",
            trail: get("loops.trail", "1").parse().unwrap_or(1).min(2),
            avoid_paved: get("loops.paved", "0") == "1",
            avoid_industrial: get("loops.industrial", "0") == "1",
            lit: get("loops.lit", "0") == "1",
            mine: get("loops.mine", "0").parse::<i8>().unwrap_or(0).clamp(-1, 1),
            zones: read_zones(&get("loops.zones", "")),
            placing_zone: false,
            avoid_open: false,
            settings_open: false,
            card_hover: None,
            show_slope: get("loops.slope", "1") == "1",
            photos: get("loops.photos", "1") == "1",
            runs: None,
            climb_hover: None,
            data,
            map: MapState::new(),
            start: None,
            km,
            min_km,
            max_km,
            climb,
            seed: 1,
            selected: 0,
            hover: None,
            show_relief: true,
            relief: Relief::default(),
            three_d: false,
            scene: None,
            plots: Plots::default(),
            bad_input: None,
            drag: None,
            pending: 0,
            pins: Vec::new(),
            draft: None,
            grabbed_at: None,
            undo: Vec::new(),
            redo: Vec::new(),
            name,
            opened: None,
            _subscriptions: subs,
        }
    }

    /// The request the panel describes, or why it can't be one.
    fn request(&self, places: Places, cx: &App) -> Result<Request, String> {
        let (lat, lon) = self.start.ok_or(tr("Click on the map to set the start.", "Clique sur la carte pour poser le départ."))?;
        let region = self.data.read(cx).loops.region;
        if !region.holds(lat, lon) {
            return Err(trf(format!("The start is outside {}, the region downloaded.", region.name()), format!("Le départ est hors de {}, la région téléchargée.", region.name())));
        }
        let value = |i: &Entity<InputState>| i.read(cx).value().to_string();
        let bad = || tr("A distance between 0.5 and 200 km, please.", "Une distance entre 0,5 et 200 km, s'il te plaît.").to_string();
        let target = if self.exact {
            Target::Exact(read_km(&value(&self.km)).ok_or_else(bad)?)
        } else {
            let (a, b) = (read_km(&value(&self.min_km)).ok_or_else(bad)?, read_km(&value(&self.max_km)).ok_or_else(bad)?);
            Target::Range(a.min(b), a.max(b))
        };
        let climb = value(&self.climb).trim().replace(',', ".").parse::<f64>().ok().filter(|v| *v > 0.0);
        let prefs = self.prefs();
        Ok(Request { lat, lon, target, prefs, climb, seed: self.seed, count: COUNT, via: self.pins.iter().map(|p| [p.0, p.1]).collect(), places })
    }

    /// Plans with the panel's settings; `again`: other loops (a new seed).
    pub fn propose(&mut self, again: bool, cx: &mut Context<Self>) {
        if again {
            self.seed = crate::logic::loops::Rng::new(self.seed ^ 0x5eed).next();
        }
        let km = |i: &Entity<InputState>| i.read(cx).value().to_string();
        let saved = [
            ("loops.km", km(&self.km)),
            ("loops.min", km(&self.min_km)),
            ("loops.max", km(&self.max_km)),
            ("loops.climb", km(&self.climb)),
            ("loops.mode", if self.exact { "exact" } else { "range" }.to_string()),
            ("loops.trail", self.trail.to_string()),
            ("loops.paved", if self.avoid_paved { "1" } else { "0" }.to_string()),
            ("loops.industrial", if self.avoid_industrial { "1" } else { "0" }.to_string()),
            ("loops.lit", if self.lit { "1" } else { "0" }.to_string()),
            ("loops.mine", self.mine.to_string()),
            ("loops.zones", write_zones(&self.zones)),
        ];
        self.data.update(cx, |d, cx| {
            for (k, v) in saved {
                if d.settings.get(k) != Some(&v) {
                    d.set_setting(k, v, cx);
                }
            }
        });
        let places = self.places(cx);
        match self.request(places, cx) {
            Ok(req) => {
                self.bad_input = None;
                self.selected = 0;
                // New loops: no longer the route opened.
                self.opened = None;
                self.data.update(cx, |d, cx| d.plan_loops(req, cx));
            }
            Err(e) => self.bad_input = self.start.is_some().then_some(e),
        }
        cx.notify();
    }

    /// Plans again once the settings have stopped changing, if there is a start and a network.
    fn settle(&mut self, cx: &mut Context<Self>) {
        self.pending += 1;
        if self.start.is_none() || self.data.read(cx).loops.graph.is_none() {
            return;
        }
        let n = self.pending;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            this.update(cx, |v, cx| {
                if v.pending == n {
                    v.propose(false, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn prefs(&self) -> Prefs {
        Prefs { trail: [0.0, 0.5, 1.0][self.trail], avoid_paved: self.avoid_paved, avoid_industrial: self.avoid_industrial, lit: self.lit }
    }

    /// The zones, and one's tracks when they matter.
    fn places(&mut self, cx: &mut Context<Self>) -> Places {
        let known = (self.mine != 0).then(|| self.data.update(cx, |d, _| d.known_tracks()));
        Places { zones: self.zones.clone(), known, mine: self.mine }
    }

    /// How many pixels a zone's radius takes on the map.
    fn zone_px(&self, z: &Zone) -> f32 {
        let (a, b) = (self.on_map(z.lat, z.lon), { let e = east_of(z.lat, z.lon, z.radius); self.on_map(e[0], e[1]) });
        (b.0 - a.0).hypot(b.1 - a.1)
    }

    /// Where a position is on the map, in pixels from its top left corner.
    fn on_map(&self, lat: f64, lon: f64) -> (f32, f32) {
        let s = self.map.bounds.get().size;
        let (x, y) = self.map.view.screen(lat, lon, (s.width / px(1.)) as f64, (s.height / px(1.)) as f64);
        (x as f32, y as f32)
    }

    /// The position under a point of the window.
    fn under(&self, p: Point<Pixels>) -> (f64, f64) {
        let b = self.map.bounds.get();
        self.map.view.position(((p.x - b.origin.x) / px(1.)) as f64, ((p.y - b.origin.y) / px(1.)) as f64, (b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64)
    }

    /// The chosen loop's waypoints: as being moved, or as routed.
    fn points(&self, cx: &App) -> Vec<[f64; 2]> {
        self.draft.clone().unwrap_or_else(|| self.snap_found(cx).get(self.selected).map(|l| l.waypoints.clone()).unwrap_or_default())
    }

    /// The point of the map under the pointer that can be moved: the chosen loop's points first,
    /// then the points asked for, then the start.
    fn handle_at(&self, p: Point<Pixels>, cx: &App) -> Option<Handle> {
        let b = self.map.bounds.get();
        let at = (((p.x - b.origin.x) / px(1.)), ((p.y - b.origin.y) / px(1.)));
        let near = |q: [f64; 2]| {
            let (hx, hy) = self.on_map(q[0], q[1]);
            (hx - at.0).hypot(hy - at.1) <= GRAB
        };
        // A zone's edge first: it lies where nothing else is meant to be grabbed.
        if let Some(i) = self.zones.iter().position(|z| near(east_of(z.lat, z.lon, z.radius))) {
            return Some(Handle::ZoneEdge(i));
        }
        if let Some(i) = self.points(cx).iter().position(|q| near(*q)) {
            return Some(Handle::Point(i));
        }
        if let Some(i) = self.pins.iter().position(|q| near([q.0, q.1])) {
            return Some(Handle::Pin(i));
        }
        if self.start.is_some_and(|s| near([s.0, s.1])) {
            return Some(Handle::Start);
        }
        self.zones.iter().position(|z| near([z.lat, z.lon])).map(Handle::Zone)
    }

    /// Where the chosen loop's line passes under the pointer: the index of its point there.
    fn line_at(&self, p: Point<Pixels>, cx: &App) -> Option<usize> {
        let found = self.snap_found(cx);
        let l = found.get(self.selected)?;
        let b = self.map.bounds.get();
        let at = (((p.x - b.origin.x) / px(1.)), ((p.y - b.origin.y) / px(1.)));
        let line: Vec<(f32, f32)> = l.line.iter().map(|q| self.on_map(q[0], q[1])).collect();
        from_line(&line, at).filter(|(d, _)| *d <= GRAB / 2.0 + 2.0).map(|(_, i)| i)
    }

    /// The loop, other than the chosen one, whose line passes under the pointer.
    fn other_at(&self, p: Point<Pixels>, cx: &App) -> Option<usize> {
        let b = self.map.bounds.get();
        let at = (((p.x - b.origin.x) / px(1.)), ((p.y - b.origin.y) / px(1.)));
        self.snap_found(cx)
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != self.selected)
            .filter_map(|(i, l)| {
                let line: Vec<(f32, f32)> = l.line.iter().map(|q| self.on_map(q[0], q[1])).collect();
                from_line(&line, at).filter(|(d, _)| *d <= GRAB / 2.0 + 2.0).map(|(d, _)| (d, i))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, i)| i)
    }

    fn step_now(&self, cx: &App) -> Step {
        Step { start: self.start, pins: self.pins.clone(), zones: self.zones.clone(), found: self.snap_found(cx), selected: self.selected }
    }

    /// Keeps what is there, for undo, before a change.
    fn remember(&mut self, cx: &App) {
        let step = self.step_now(cx);
        self.undo.push(step);
        // ponytail: the last 100 changes; nobody undoes further.
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn restore(&mut self, s: Step, cx: &mut Context<Self>) {
        self.start = s.start;
        self.pins = s.pins;
        self.zones = s.zones;
        self.selected = s.selected;
        self.draft = None;
        self.drag = None;
        self.hover = None;
        self.data.update(cx, |d, cx| d.set_found(s.found, cx));
        cx.notify();
    }

    /// Undoes the last change of the start, the points or a loop (`back`), or does it again.
    pub fn undo(&mut self, back: bool, cx: &mut Context<Self>) {
        let Some(s) = (if back { self.undo.pop() } else { self.redo.pop() }) else { return };
        let now = self.step_now(cx);
        if back { self.redo.push(now) } else { self.undo.push(now) }
        self.restore(s, cx);
    }

    fn grab(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) {
        let before = self.step_now(cx);
        let mut h = self.handle_at(e.position, cx);
        // On another loop's line: that loop is chosen.
        if h.is_none()
            && self.line_at(e.position, cx).is_none()
            && let Some(i) = self.other_at(e.position, cx)
        {
            self.selected = i;
            self.hover = None;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // On the chosen loop's line: a new point there, to drag. Not on a track brought from a
        // file: routed again, it would no longer be that track.
        let found = self.snap_found(cx);
        if h.is_none()
            && let Some(at) = self.line_at(e.position, cx)
            && !found[self.selected].track
        {
            let l = &found[self.selected];
            let mut points = self.points(cx);
            let i = insert_at(&l.line, &points, at);
            let p = self.under(e.position);
            points.insert(i, [p.0, p.1]);
            self.draft = Some(points);
            h = Some(Handle::Point(i));
        }
        if let Some(h) = h {
            self.drag = Some(h);
            self.grabbed_at = Some((e.position, before));
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn move_to(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(h) = self.drag else {
            // Over the chosen loop: its point shows on the profile.
            if e.pressed_button.is_none() {
                let hover = self.line_at(e.position, cx);
                if hover != self.hover {
                    self.hover = hover;
                    cx.notify();
                }
            }
            return;
        };
        if e.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            self.draft = None;
            return;
        }
        let at = self.under(e.position);
        match h {
            Handle::Start => self.start = Some(at),
            Handle::Pin(i) => self.pins[i] = at,
            Handle::Zone(i) => (self.zones[i].lat, self.zones[i].lon) = at,
            Handle::ZoneEdge(i) => {
                let z = &mut self.zones[i];
                z.radius = crate::logic::fit::metres([z.lat, z.lon], [at.0, at.1]).clamp(30.0, 50_000.0);
            }
            Handle::Point(i) => {
                let mut points = self.points(cx);
                if let Some(p) = points.get_mut(i) {
                    *p = [at.0, at.1];
                }
                self.draft = Some(points);
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn drop_at(&mut self, e: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(h) = self.drag.take() else { return };
        cx.stop_propagation();
        let Some((from, before)) = self.grabbed_at.take() else { return };
        if ((e.position.x - from.x) / px(1.)).abs() + ((e.position.y - from.y) / px(1.)).abs() < 4.0 {
            // A click on a point: nothing moves.
            self.start = before.start;
            self.pins = before.pins;
            self.zones = before.zones;
            self.draft = None;
            cx.notify();
            return;
        }
        self.undo.push(before);
        self.redo.clear();
        let at = self.under(e.position);
        match h {
            Handle::Start => {
                self.start = Some(at);
                self.propose(false, cx);
            }
            Handle::Pin(i) => {
                self.pins[i] = at;
                self.propose(false, cx);
            }
            Handle::Point(i) => self.edit(Some(i), cx),
            Handle::Zone(_) | Handle::ZoneEdge(_) => self.propose(false, cx),
        }
    }

    /// Routes the chosen loop again through the draft's points; `moved`: the point that moved, if
    /// a point asked for, follows it.
    fn edit(&mut self, moved: Option<usize>, cx: &mut Context<Self>) {
        let (Some(points), Some(start)) = (self.draft.clone(), self.start) else { return };
        let found = self.snap_found(cx);
        let Some(l) = found.get(self.selected) else { return };
        // A point asked for that was moved: the one it was, moved with it.
        if let Some(i) = moved
            && l.pinned.get(i) == Some(&true)
            && let Some(old) = l.waypoints.get(i)
            && let Some(pin) = self.pins.iter_mut().min_by(|a, b| crate::logic::fit::metres(*old, [a.0, a.1]).total_cmp(&crate::logic::fit::metres(*old, [b.0, b.1])))
            && crate::logic::fit::metres(*old, [pin.0, pin.1]) < 300.0
        {
            *pin = (points[i][0], points[i][1]);
        }
        let (prefs, places, i) = (self.prefs(), self.places(cx), self.selected);
        self.data.update(cx, |d, cx| d.reroute(i, [start.0, start.1], points, prefs, places, cx));
        cx.notify();
    }

    /// A right click: takes away the point of the loop or the point asked for under the pointer.
    fn take_away(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) {
        match self.handle_at(e.position, cx) {
            Some(Handle::Point(i)) => {
                cx.stop_propagation();
                let mut points = self.points(cx);
                if points.len() < 2 {
                    return;
                }
                self.remember(cx);
                let gone = points.remove(i);
                let found = self.snap_found(cx);
                let pinned = found.get(self.selected).is_some_and(|l| l.pinned.get(i) == Some(&true));
                self.draft = Some(points);
                self.edit(None, cx);
                if pinned && let Some(k) = self.pins.iter().position(|p| crate::logic::fit::metres(gone, [p.0, p.1]) < 300.0) {
                    self.pins.remove(k);
                }
            }
            Some(Handle::Pin(i)) => {
                cx.stop_propagation();
                self.remember(cx);
                self.pins.remove(i);
                self.propose(false, cx);
            }
            _ => {
                // Inside a zone: the zone goes.
                let b = self.map.bounds.get();
                let at = (((e.position.x - b.origin.x) / px(1.)), ((e.position.y - b.origin.y) / px(1.)));
                let inside = self.zones.iter().position(|z| {
                    let c = self.on_map(z.lat, z.lon);
                    (c.0 - at.0).hypot(c.1 - at.1) <= self.zone_px(z).max(GRAB)
                });
                if let Some(i) = inside {
                    cx.stop_propagation();
                    self.remember(cx);
                    self.zones.remove(i);
                    self.propose(false, cx);
                }
            }
        }
    }

    /// A click on the map, the start set: a point the loops must pass by.
    fn pin(&mut self, at: (f64, f64), cx: &mut Context<Self>) {
        self.remember(cx);
        self.pins.push(at);
        self.propose(false, cx);
    }

    /// Forgets the points asked for, and plans without them.
    fn unpin(&mut self, cx: &mut Context<Self>) {
        self.remember(cx);
        self.pins.clear();
        self.propose(false, cx);
    }

    /// Keeps the chosen loop under the name typed (or its length): the route opened is updated,
    /// unless `copy`.
    pub fn keep(&mut self, copy: bool, cx: &mut Context<Self>) {
        let found = self.snap_found(cx);
        let (Some(l), Some(start)) = (found.get(self.selected), self.start) else { return };
        let typed = self.name.read(cx).value().trim().to_string();
        let name = if typed.is_empty() { format!("{} {}", tr("Loop", "Boucle"), fmt_km(l.dist)) } else { typed };
        let next = self.data.read(cx).loops.routes.iter().map(|r| r.id + 1).max().unwrap_or(0);
        let id = match self.opened {
            Some(id) if !copy => id,
            _ => (chrono::Utc::now().timestamp_millis().max(0) as u64).max(next),
        };
        let route = Route { id, name, start: [start.0, start.1], pins: self.pins.iter().map(|p| [p.0, p.1]).collect(), prefs: self.prefs(), shape: l.clone(), trashed: false };
        self.opened = Some(id);
        self.data.update(cx, |d, cx| d.keep_route(route, cx));
        cx.notify();
    }

    /// Asks for GPX files and keeps each one's track as a route, the last one shown.
    fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some(tr("Import", "Importer").into()) });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                this.update_in(cx, |v, window, cx| v.import_paths(paths, window, cx)).ok();
            }
        })
        .detach();
    }

    /// Reads GPX files (in the background) and keeps each one's track as a route, named after
    /// it (else after the file); the last one is shown. Altitudes a file lacks are read from the
    /// elevation tiles on disk.
    pub fn import_paths(&mut self, paths: Vec<std::path::PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let graph = self.data.read(cx).loops.graph.clone();
        let prefs = self.prefs();
        cx.spawn_in(window, async move |this, cx| {
            let read = cx.background_executor().spawn(async move { paths.iter().map(|p| read_track(p, graph.as_deref())).collect::<Vec<_>>() }).await;
            this.update_in(cx, |v, window, cx| {
                let mut last = None;
                let mut next = v.data.read(cx).loops.routes.iter().map(|r| r.id + 1).max().unwrap_or(0).max(chrono::Utc::now().timestamp_millis().max(0) as u64);
                let mut bad = Vec::new();
                for r in read {
                    match r {
                        // The same track again: the route already kept.
                        Ok((_, l)) if let Some(r) = v.data.read(cx).loops.routes.iter().find(|r| !r.trashed && same_line(&r.shape.line, &l.line)) => last = Some(r.id),
                        Ok((name, l)) => {
                            let route = Route { id: next, name, start: l.line[0], pins: Vec::new(), prefs, shape: l, trashed: false };
                            v.data.update(cx, |d, cx| d.keep_route(route, cx));
                            last = Some(next);
                            next += 1;
                        }
                        Err(file) => bad.push(file),
                    }
                }
                if !bad.is_empty() {
                    let files = bad.join(", ");
                    v.data.update(cx, |d, cx| {
                        d.note = Some(trf(format!("no track could be read in {files}"), format!("aucune trace lisible dans {files}")));
                        cx.notify();
                    });
                }
                if let Some(id) = last {
                    v.open_route(id, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Shows a kept route, with its start, its points and its settings, to change it.
    pub fn open_route(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.data.read(cx).loops.routes.iter().find(|r| r.id == id).cloned() else { return };
        self.remember(cx);
        self.start = Some((r.start[0], r.start[1]));
        self.pins = r.pins.iter().map(|p| (p[0], p[1])).collect();
        self.trail = if r.prefs.trail < 0.25 { 0 } else if r.prefs.trail > 0.75 { 2 } else { 1 };
        self.avoid_paved = r.prefs.avoid_paved;
        self.selected = 0;
        self.draft = None;
        self.hover = None;
        self.opened = Some(id);
        let name = r.name.clone();
        self.name.update(cx, |s, cx| s.set_value(name, window, cx));
        self.data.update(cx, |d, cx| d.show_route(&r, cx));
        cx.notify();
    }

    /// Forgets the start and the loops, to begin again elsewhere.
    fn clear(&mut self, cx: &mut Context<Self>) {
        self.remember(cx);
        self.start = None;
        self.pins.clear();
        self.drag = None;
        self.draft = None;
        self.bad_input = None;
        self.data.update(cx, |d, cx| d.clear_loops(cx));
        cx.notify();
    }

    /// Selects the next (or previous) loop proposed.
    pub fn step(&mut self, by: i32, cx: &mut Context<Self>) {
        let n = self.snap_found(cx).len();
        if n > 0 {
            self.selected = (self.selected as i32 + by).rem_euclid(n as i32) as usize;
            self.hover = None;
            cx.notify();
        }
    }

    /// Where a position is drawn in the window, for the tests' clicks.
    #[cfg(test)]
    pub fn screen_of(&self, lat: f64, lon: f64) -> Point<Pixels> {
        let b = self.map.bounds.get();
        let (x, y) = self.map.view.screen(lat, lon, (b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
        point(b.origin.x + px(x as f32), b.origin.y + px(y as f32))
    }

    /// Centres the map on a position at zoom `z`, as a user would by dragging and zooming.
    #[cfg(test)]
    pub fn look_at(&mut self, lat: f64, lon: f64, z: f64) {
        let (cx, cy) = map::world(lat, lon, z);
        self.map.view = View { cx, cy, z };
    }

    /// Types a distance in the km field.
    #[cfg(test)]
    pub fn type_km(&mut self, km: &str, window: &mut Window, cx: &mut Context<Self>) {
        let km = km.to_string();
        self.km.update(cx, |s, cx| s.set_value(km, window, cx));
    }

    /// Puts the cursor at the end of the km field, as a click in it would.
    #[cfg(test)]
    pub fn focus_km(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.km.update(cx, |s, cx| s.focus(window, cx));
    }

    /// Puts the cursor in the name field, as a click in it would.
    #[cfg(test)]
    pub fn focus_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.name.update(cx, |s, cx| s.focus(window, cx));
    }

    /// Types the name to keep a loop under.
    #[cfg(test)]
    pub fn name_as(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = name.to_string();
        self.name.update(cx, |s, cx| s.set_value(name, window, cx));
    }

    #[cfg(test)]
    pub fn hover(&self) -> Option<usize> {
        self.hover
    }

    #[cfg(test)]
    pub fn zones(&self) -> Vec<Zone> {
        self.zones.clone()
    }

    #[cfg(test)]
    pub fn pins(&self) -> Vec<(f64, f64)> {
        self.pins.clone()
    }

    #[cfg(test)]
    pub fn start(&self) -> Option<(f64, f64)> {
        self.start
    }

    #[cfg(test)]
    pub fn selected(&self) -> usize {
        self.selected
    }

    fn snap_found(&self, cx: &App) -> Arc<Vec<Loop>> {
        self.data.read(cx).loops.found.clone()
    }

    /// Asks where to save the loop selected, then writes it as GPX.
    pub fn export(&mut self, cx: &mut Context<Self>) {
        let found = self.snap_found(cx);
        let Some(l) = found.get(self.selected).cloned() else { return };
        let name = format!("{}-{}km-{}.gpx", tr("loop", "boucle"), (l.dist / 1000.0).round(), l.seed % 100_000);
        let rx = cx.prompt_for_new_path(&crate::platform::store::export_dir(), Some(&name));
        let data = self.data.clone();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(path))) = rx.await {
                data.update(cx, |d, cx| d.export_loop(&l, path, cx));
            }
        })
        .detach();
    }

    /// Redraws the relief when the view changed; one drawing at a time, the latest wins.
    fn ensure_relief(&mut self, size: (f64, f64), cx: &mut Context<Self>) {
        if !self.show_relief || size.0 < 8.0 || size.1 < 8.0 {
            return;
        }
        let view = self.map.view;
        let dark = self.snap.theme.dark;
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (view.cx.to_bits(), view.cy.to_bits(), view.z.to_bits(), size.0.to_bits(), size.1.to_bits(), dark).hash(&mut h);
            h.finish()
        };
        if self.relief.drawn == Some(key) || self.relief.busy {
            return;
        }
        // Every tile first: the shade waits for them all.
        let z = relief::zoom_for(&view);
        let needed = relief::tiles(&view, size.0, size.1);
        let mut tiles = std::collections::HashMap::new();
        for (x, y) in &needed {
            if let Some(t) = self.data.update(cx, |d, cx| d.relief_tile((z, *x, *y), cx)) {
                tiles.insert((*x, *y), t);
            }
        }
        if tiles.len() < needed.len() {
            return;
        }
        self.relief.busy = true;
        // At half the screen's pixels: the tiles hold less than that anyway.
        let (w, h) = ((size.0 / 2.0) as usize, (size.1 / 2.0) as usize);
        let half = View { cx: view.cx / 2.0, cy: view.cy / 2.0, z: view.z - 1.0 };
        cx.spawn(async move |this, cx| {
            let img = cx
                .background_executor()
                .spawn(async move {
                    let shade = relief::shade(&half, w, h, dark, &|x, y| tiles.get(&(x, y)).cloned());
                    render_image(w as u32, h as u32, shade).map(Arc::new)
                })
                .await;
            this.update(cx, |v, cx| {
                v.relief.busy = false;
                v.relief.drawn = Some(key);
                if let Some(img) = img {
                    v.relief.image = Some((img, view, size));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for LoopsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.data.update(cx, |d, cx| d.open_loops(cx));
        for image in self.scene.iter_mut().flat_map(|s| s.stale.drain(..)) {
            window.drop_image(image).ok();
        }
        let t = self.snap.theme;
        let found = self.snap_found(cx);
        if self.selected >= found.len() {
            self.selected = 0;
        }
        let region = self.data.read(cx).loops.region;
        // The region at first; then each new set of loops, all of them in sight.
        let chosen = found.get(self.selected);
        let generation = self.data.read(cx).loops.generation;
        let bbox = if found.is_empty() {
            Some(region.bbox)
        } else {
            let lines: Vec<Vec<[f32; 2]>> = found.iter().map(|l| l.line.iter().map(|p| [p[0] as f32, p[1] as f32]).collect()).collect();
            map::bbox(lines.iter().map(Vec::as_slice), 0.0)
        };
        // The panel covers the map's left: the loops are fitted in the rest.
        let width = (self.map.bounds.get().size.width / px(1.)) as f64;
        // The profile's band covers its bottom.
        let height = (self.map.bounds.get().size.height / px(1.)) as f64;
        let band = if found.is_empty() { 0.0 } else { BAND + 24.0 };
        let bbox = bbox.map(|[s, w, n, e]| {
            let free = (width - PANEL - 28.0).max(width * 0.3);
            let tall = (height - band).max(height * 0.4);
            [s - (n - s) * (height - tall) / tall, w - (e - w) * (width - free) / free, n, e]
        });
        self.map.inset = (PANEL as f32 + 14.0, band as f32);
        self.map.fit(generation, bbox, false);

        let b = self.map.bounds.get();
        let (w, h) = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
        // The first click sets the start; the next ones, points to pass by.
        if let Some((sx, sy)) = self.map.click.take() {
            let at = self.map.view.position(sx, sy, w, h);
            if self.placing_zone {
                self.placing_zone = false;
                self.remember(cx);
                self.zones.push(Zone { lat: at.0, lon: at.1, radius: ZONE_RADIUS });
                self.propose(false, cx);
            } else if self.start.is_none() {
                self.remember(cx);
                self.start = Some(at);
                if self.data.read(cx).loops.graph.is_some() {
                    self.propose(false, cx);
                }
            } else {
                self.pin(at, cx);
            }
        }
        // The loop moved by hand is routed: its own points show again.
        if self.drag.is_none() && !self.data.read(cx).loops.editing {
            self.draft = None;
        }
        self.ensure_relief((w, h), cx);

        let view = self.map.view;
        let shade = self.relief.image.as_ref().filter(|_| self.show_relief).map(|(raster, v0, s0)| {
            let k = 2f64.powf(view.z - v0.z);
            let left = (v0.cx - s0.0 / 2.0) * k - (view.cx - w / 2.0);
            let top = (v0.cy - s0.1 / 2.0) * k - (view.cy - h / 2.0);
            img(raster.clone()).absolute().left(px(left as f32)).top(px(top as f32)).w(px((s0.0 * k) as f32)).h(px((s0.1 * k) as f32)).into_any_element()
        });
        let lines: Vec<(Vec<[f64; 2]>, bool)> = found.iter().enumerate().map(|(i, l)| (l.line.clone(), i == self.selected)).collect();
        // Which way the chosen loop runs: a chevron every 800 m, pointing to a point a little ahead.
        let arrows: Vec<([f64; 2], [f64; 2])> = chosen
            .map(|l| {
                let mut out = Vec::new();
                let mut next = 400.0f32;
                for (i, a) in l.along.iter().enumerate() {
                    if *a >= next {
                        let ahead = l.along[i..].iter().position(|b| *b >= a + 25.0).map_or(l.line.len() - 1, |k| i + k);
                        if ahead > i {
                            out.push((l.line[i], l.line[ahead]));
                        }
                        next += 800.0;
                    }
                }
                out
            })
            .unwrap_or_default();
        // The loop pointed at in the list, brought forward.
        let pointed = self.card_hover.filter(|i| *i != self.selected).and_then(|i| found.get(i)).map(|l| l.line.clone());
        // Loops of the last settings, while those of the new ones are looked for: faded.
        let fade = if self.data.read(cx).loops.planning { 0.45 } else { 1.0 };
        let start = self.start;
        let pins = self.pins.clone();
        let zones = self.zones.clone();
        let points = self.points(cx);
        let pinned: Vec<bool> = chosen.map(|l| l.pinned.clone()).unwrap_or_default();
        let show_points = self.hover.is_some() || self.drag.is_some() || self.draft.is_some();
        // Being moved: straight lines from point to point, until routed.
        let draft = self.draft.clone().zip(start).map(|(d, s)| std::iter::once([s.0, s.1]).chain(d).chain(std::iter::once([s.0, s.1])).collect::<Vec<_>>());
        let hover = chosen.zip(self.hover).and_then(|(l, i)| l.line.get(i).copied());
        let (accent, ink, bg) = (t.accent, t.text, t.bg);
        // The chosen loop's grade, by class, and the climb pointed at.
        let classes: Option<Arc<Vec<(usize, usize, u8)>>> = match chosen.filter(|_| self.show_slope) {
            Some(l) => {
                let key = l.line.len() as u64 ^ l.dist.to_bits() ^ l.up.to_bits().rotate_left(17);
                if self.runs.as_ref().is_none_or(|r| r.0 != key) {
                    self.runs = Some((key, Arc::new(slope::runs(&l.along, &l.eles))));
                }
                self.runs.as_ref().map(|r| r.1.clone())
            }
            None => None,
        };
        let ramp = slope_ramp(&t, false);
        let climb = chosen.zip(self.climb_hover).and_then(|(l, i)| slope::climbs(&l.along, &l.eles).get(i).map(|c| l.line[c.from..=c.to].to_vec()));
        let bounds = self.map.bounds.clone();
        let drawing = canvas(
            |_, _, _| {},
            move |b, _, window, _| {
                let s = bounds.get().size;
                let (w, h) = ((s.width / px(1.)) as f64, (s.height / px(1.)) as f64);
                let at = |p: [f64; 2]| {
                    let (x, y) = view.screen(p[0], p[1], w, h);
                    point(b.origin.x + px(x as f32), b.origin.y + px(y as f32))
                };
                // Zones under the loops: a faint disc with a dashed edge, its centre, and the handle
                // that sizes it.
                for z in &zones {
                    let (c, e) = (at([z.lat, z.lon]), at(east_of(z.lat, z.lon, z.radius)));
                    let r = (e.x - c.x).max(px(2.));
                    window.paint_quad(fill(Bounds::new(point(c.x - r, c.y - r), size(r * 2., r * 2.)), ink.opacity(0.07)).corner_radii(r));
                    let ring: Vec<Point<Pixels>> = (0..=72).map(|k| {
                        let a = k as f32 / 72.0 * std::f32::consts::TAU;
                        point(c.x + r * a.cos(), c.y + r * a.sin())
                    }).collect();
                    let mut path = PathBuilder::stroke(px(1.5)).dash_array(&[px(6.), px(4.)]);
                    path.add_polygon(&ring, false);
                    if let Ok(p) = path.build() {
                        window.paint_path(p, ink.opacity(0.55));
                    }
                    let disc = |p: Point<Pixels>, r: f32, col: Hsla, window: &mut Window| {
                        window.paint_quad(fill(Bounds::new(point(p.x - px(r), p.y - px(r)), size(px(2. * r), px(2. * r))), col).corner_radii(px(r)));
                    };
                    disc(c, 3.0, ink.opacity(0.7), window);
                    disc(e, 5.0, bg, window);
                    disc(e, 3.5, ink.opacity(0.7), window);
                }
                let stroke = |line: &[[f64; 2]], width: f32, colour: Hsla, window: &mut Window| {
                    if line.len() < 2 {
                        return;
                    }
                    let mut path = line_path(width);
                    path.move_to(at(line[0]));
                    line[1..].iter().for_each(|p| path.line_to(at(*p)));
                    if let Ok(p) = path.build() {
                        window.paint_path(p, colour);
                    }
                };
                // The others thin and light, the one pointed at in the list darker, the chosen
                // one on top, over a halo of the page's colour that parts it from the map.
                if let Some(line) = &pointed {
                    stroke(line, 7.0, bg.opacity(0.8 * fade), window);
                }
                for (line, on) in lines.iter().filter(|l| !l.1).chain(lines.iter().filter(|l| l.1)) {
                    if line.len() < 2 {
                        continue;
                    }
                    if *on {
                        stroke(line, 8.5, bg.opacity(0.85 * fade), window);
                    }
                    if *on && let Some(classes) = &classes {
                        // The whole line in the palest colour (no gaps where runs meet), then
                        // runs of one class, each its colour.
                        let mut path = line_path(4.5);
                        path.move_to(at(line[0]));
                        line[1..].iter().for_each(|p| path.line_to(at(*p)));
                        if let Ok(p) = path.build() {
                            window.paint_path(p, ramp[0].opacity(fade));
                        }
                        for (i, j, k) in classes.iter().copied().filter(|r| r.2 > 0) {
                            let mut path = line_path(4.5);
                            path.move_to(at(line[i]));
                            line[i + 1..=j.min(line.len() - 1)].iter().for_each(|p| path.line_to(at(*p)));
                            if let Ok(p) = path.build() {
                                window.paint_path(p, ramp[k as usize].opacity(fade));
                            }
                        }
                        continue;
                    }
                    if *on {
                        stroke(line, 4.5, accent.opacity(fade), window);
                    } else {
                        stroke(line, 2.0, ink.opacity(0.38 * fade), window);
                    }
                }
                if let Some(line) = &pointed {
                    stroke(line, 3.0, ink.opacity(0.8 * fade), window);
                }
                for (p, q) in &arrows {
                    let (a, b) = (at(*p), at(*q));
                    let (dx, dy) = ((b.x - a.x) / px(1.), (b.y - a.y) / px(1.));
                    let n = (dx * dx + dy * dy).sqrt();
                    if n < 0.5 {
                        continue;
                    }
                    let (ux, uy) = (dx / n, dy / n);
                    let wing = |side: f32| point(a.x - px(4.0 * ux - side * 3.5 * uy), a.y - px(4.0 * uy + side * 3.5 * ux));
                    let mut path = line_path(1.8);
                    path.move_to(wing(1.0));
                    path.line_to(point(a.x + px(2.0 * ux), a.y + px(2.0 * uy)));
                    path.line_to(wing(-1.0));
                    if let Ok(p) = path.build() {
                        window.paint_path(p, bg.opacity(fade));
                    }
                }
                // The climb pointed at in the list: a dark band over it.
                if let Some(c) = climb.as_ref().filter(|c| c.len() > 1) {
                    let mut path = line_path(8.);
                    path.move_to(at(c[0]));
                    c[1..].iter().for_each(|p| path.line_to(at(*p)));
                    if let Ok(p) = path.build() {
                        window.paint_path(p, ink.opacity(0.45));
                    }
                }
                let dot = |p: Point<Pixels>, r: f32, c: Hsla, window: &mut Window| {
                    window.paint_quad(fill(Bounds::new(point(p.x - px(r), p.y - px(r)), size(px(2. * r), px(2. * r))), c).corner_radii(px(r)));
                };
                if let Some(d) = &draft {
                    let mut path = PathBuilder::stroke(px(1.5));
                    path.move_to(at(d[0]));
                    d[1..].iter().for_each(|p| path.line_to(at(*p)));
                    if let Ok(p) = path.build() {
                        window.paint_path(p, ink.opacity(0.6));
                    }
                }
                // Three marks: the start (ink), the points asked for (accent), and the points that
                // shape the loop, small rings, only while the pointer is on it.
                for (i, p) in points.iter().enumerate() {
                    if pinned.get(i) == Some(&true) && draft.is_none() {
                        dot(at(*p), 6.5, bg, window);
                        dot(at(*p), 5.0, accent, window);
                    } else if show_points {
                        dot(at(*p), 4.5, ink, window);
                        dot(at(*p), 3.0, bg, window);
                    }
                }
                for p in &pins {
                    dot(at([p.0, p.1]), 6.5, bg, window);
                    dot(at([p.0, p.1]), 5.0, accent, window);
                }
                if let Some((lat, lon)) = start {
                    dot(at([lat, lon]), 8.0, bg, window);
                    dot(at([lat, lon]), 6.0, ink, window);
                }
                if let Some(p) = hover {
                    dot(at(p), 7.0, bg, window);
                    dot(at(p), 5.0, accent, window);
                }
            },
        )
        .absolute()
        .size_full()
        .into_any_element();
        // Each loop's letter, at its point farthest from the start, as in the list.
        let letters = found.iter().enumerate().filter_map(|(i, l)| {
            let first = *l.line.first()?;
            let far = l.line.iter().copied().max_by(|a, b| crate::logic::fit::metres(first, *a).total_cmp(&crate::logic::fit::metres(first, *b)))?;
            let (x, y) = view.screen(far[0], far[1], w, h);
            let on = i == self.selected;
            Some(
                div()
                    .absolute()
                    .left(px(x as f32 - 9.))
                    .top(px(y as f32 - 9.))
                    .size(px(18.))
                    .rounded(px(9.))
                    .bg(t.bg)
                    .shadow_sm()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(num(letter(i).to_string()).text_size(px(11.)).font_weight(FontWeight::BOLD).text_color(if on { t.accent } else { t.soft })),
            )
        });
        // Before a start: one's usual starts, with how many outings start there.
        let usual = if self.start.is_none() { self.usual_starts(cx) } else { Vec::new() };
        // Busiest first: one that would cover a busier one's label stays hidden.
        let mut shown: Vec<(f64, f64)> = Vec::new();
        let usual = usual.into_iter().filter_map(|u| {
            let (x, y) = view.screen(u.at[0], u.at[1], w, h);
            if shown.iter().any(|(sx, sy)| (x - sx).abs() < 110.0 && (y - sy).abs() < 22.0) {
                return None;
            }
            shown.push((x, y));
            let label = match u.place {
                Some(place) => format!("{place} · {}", u.count),
                None => u.count.to_string(),
            };
            Some(div()
                .absolute()
                .left(px(x as f32 - 4.))
                .top(px(y as f32 - 4.))
                .flex()
                .items_center()
                .gap(px(4.))
                .child(div().size(px(8.)).rounded(px(4.)).bg(t.text).border_1().border_color(t.bg))
                .child(div().px(px(5.)).rounded(px(4.)).bg(t.bg.opacity(0.9)).shadow_sm().whitespace_nowrap().child(num(label).text_size(px(11.)).text_color(t.soft))))
        });
        let overlay = div()
            .absolute()
            .size_full()
            .children(shade)
            .child(drawing)
            .children(letters)
            .children(usual)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.grab(e, cx)))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| this.move_to(e, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, e: &MouseUpEvent, _, cx| this.drop_at(e, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, _, cx| this.take_away(e, cx)))
            .into_any_element();

        // The chosen loop over its ground, in 3D, instead of the map.
        let ground = match chosen.filter(|_| self.three_d) {
            Some(l) => self.view_3d(l, cx),
            None => map_widget(&self.map, |v: &mut LoopsView| &mut v.map, &self.data, "loops-map", Some(overlay), bbox, false, cx).into_any_element(),
        };
        let three = chosen.is_some() && self.three_d;
        let band = (!found.is_empty()).then(|| self.band(&found, cx));
        // What a click does now, said where one clicks.
        let ready = self.data.read(cx).loops.graph.is_some();
        let hint = (ready && (!three || !found.is_empty())).then(|| {
            let text = if three {
                tr("Drag: move · right button or Shift: turn · wheel: zoom", "Glisser : se déplacer · bouton droit ou Maj : tourner · molette : zoomer")
            } else if self.placing_zone {
                tr("Click on the map to place the zone to avoid", "Clique sur la carte pour poser la zone à éviter")
            } else if self.start.is_none() {
                tr("Click on the map to set the start", "Clique sur la carte pour poser le départ")
            } else if found.is_empty() || chosen.is_some_and(|l| l.track) {
                tr("Click: pass by here · drag the start to move it", "Clic : passer par ici · glisser le départ pour le déplacer")
            } else {
                tr("Click: pass by here · drag a point or the line: move · right click: take away", "Clic : passer par ici · glisser un point ou le tracé : déplacer · clic droit : retirer")
            };
            div()
                .id("loops-hint")
                .absolute()
                .left(px(PANEL as f32 + 28.0))
                // In 3D the fly-over button is at the right end of the row.
                .right(px(if three { 120. } else { 14. }))
                .bottom(px(if found.is_empty() { 26.0 } else { (BAND + 24.0 + 10.0) as f32 }))
                .flex()
                .justify_center()
                .child(div().min_w(px(0.)).truncate().px(px(8.)).py(px(3.)).rounded(px(6.)).bg(t.bg.opacity(0.9)).shadow_sm().text_size(px(12.)).text_color(t.soft).child(text))
        });
        div()
            .size_full()
            .relative()
            .child(ground)
            .child(self.panel(&found, cx))
            .children(band)
            .children(hint)
            .child(
                div()
                    .absolute()
                    .top(px(14.0f32.max(crate::ui::top() - 14.0) + 40.0))
                    .right(px(14.))
                    .flex()
                    .gap(px(6.))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .when(three, |d| {
                        d.child(div().bg(t.bg).rounded(px(8.)).shadow_sm().child(segmented("loops-ground", vec![(tr("Photo", "Photo").into(), self.photos), (tr("Plain", "Plan").into(), !self.photos)], t, cx, |this: &mut LoopsView, i, _, cx| {
                            this.set_photos(i == 0, cx);
                        })))
                    })
                    .when(!three, |d| {
                        d.child(div().bg(t.bg).rounded(px(8.)).shadow_sm().child(segmented("loops-relief", vec![(tr("Shading", "Ombrage").into(), self.show_relief), (tr("None", "Sans").into(), !self.show_relief)], t, cx, |this: &mut LoopsView, i, _, _| {
                            this.show_relief = i == 0;
                            this.relief.drawn = None;
                        })))
                    })
                    .when(chosen.is_some(), |d| {
                        d.child(div().bg(t.bg).rounded(px(8.)).shadow_sm().child(segmented("loops-3d-toggle", vec![("2D".into(), !three), ("3D".into(), three)], t, cx, |this: &mut LoopsView, i, _, _| {
                            this.three_d = i == 1;
                            this.hover = None;
                        })))
                    }),
            )
            .when(chosen.is_some(), |d| d.child(
                div()
                    .absolute()
                    .top(px(14.0f32.max(crate::ui::top() - 14.0) + 80.0))
                    .right(px(14.))
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(6.))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(div().bg(t.bg).rounded(px(8.)).shadow_sm().child(segmented(
                        "loops-slope",
                        vec![(tr("Grade", "Pente").into(), self.show_slope), (tr("Plain", "Uni").into(), !self.show_slope)],
                        t,
                        cx,
                        |this: &mut LoopsView, i, _, cx| {
                            this.show_slope = i == 0;
                            let v = if this.show_slope { "1" } else { "0" };
                            this.data.update(cx, |d, cx| d.set_setting("loops.slope", v.to_string(), cx));
                        },
                    )))
                    .when(self.show_slope, |d| d.child(slope_legend(&t, three))),
            ))
    }
}

/// The colours of the grade: the flat in a neutral grey that holds on any ground, then the
/// accent, deeper as it steepens. In the dark theme the gentle grades keep more of the accent,
/// or they would sink into the dark map. In 3D the flat is a pale accent: grey there is the
/// buildings' colour, and the loop must be the one coloured thing.
fn slope_ramp(t: &crate::ui::theme::Theme, three: bool) -> [Hsla; 5] {
    let [b, c] = if t.dark { [0.68, 0.84] } else { [0.5, 0.78] };
    let flat = if three { blend(t.accent, t.bg, if t.dark { 0.5 } else { 0.32 }) } else { blend(t.text, t.bg, 0.5) };
    [flat, blend(t.accent, t.bg, b), blend(t.accent, t.bg, c), t.accent, blend(t.text, t.accent, 0.55)]
}

/// The same points, but for what a save and a read back change (about 10 cm).
fn same_line(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (p[0] - q[0]).abs() < 1e-6 && (p[1] - q[1]).abs() < 1e-6)
}

/// A GPX file's track as a loop, with its name (the file's when it has none); altitudes it lacks
/// from the elevation tiles on disk. `Err`: the file's name, when it can't be read or holds no
/// track.
fn read_track(path: &std::path::Path, g: Option<&crate::logic::graph::Graph>) -> Result<(String, Loop), String> {
    use crate::logic::elevation;
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let t = std::fs::read(path).ok().and_then(|b| crate::logic::gpx::read(&String::from_utf8_lossy(&b))).ok_or_else(|| file.clone())?;
    let mut eles = t.eles;
    // The tests never read the user's tiles.
    if !cfg!(test) && eles.iter().any(Option::is_none) {
        let mut dem = elevation::Dem::new(16, |x, y| crate::platform::elevation::cached(elevation::ZOOM, x, y).and_then(|b| elevation::decode(&b)));
        for (e, p) in eles.iter_mut().zip(&t.line) {
            if e.is_none() {
                *e = dem.at(p[0], p[1]);
            }
        }
    }
    let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or(file);
    Ok((t.name.unwrap_or(stem), Loop::from_track(t.line, eles, g)))
}

/// What the colours of the grade say.
fn slope_legend(t: &crate::ui::theme::Theme, three: bool) -> Div {
    let ramp = slope_ramp(t, three);
    let labels = ["< 3 %", "3–6", "6–10", "10–15", "> 15 %"];
    div().flex().items_center().gap(px(6.)).px(px(8.)).py(px(5.)).bg(t.bg).rounded(px(8.)).shadow_sm().children(ramp.iter().zip(labels).map(|(c, l)| {
        div().flex().items_center().gap(px(3.)).child(div().w(px(12.)).h(px(4.)).rounded(px(2.)).bg(*c)).child(div().text_size(px(11.)).text_color(t.soft).child(l))
    }))
}

impl LoopsView {
    fn panel(&mut self, found: &Arc<Vec<Loop>>, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = self.snap.theme;
        let d = self.data.read(cx);
        let l = &d.loops;
        let region = l.region.name();
        let ready = l.graph.is_some();
        let job = d.loops_line();
        let running = job.is_some();
        let (has_extract, error, planning, plan_error, editing) = (l.extract.is_some(), l.error.clone(), l.planning, l.plan_error.clone(), l.editing);
        let built = l.graph.as_ref().map(|g| g.meta.source_time);
        let small = |text: String, color: Hsla| div().text_size(px(12.)).text_color(color).child(text);

        let mut body = div().flex().flex_col().gap(px(12.));
        if let Some((line, share)) = job {
            body = body.child(small(line, t.soft)).when_some(share, |d, f| {
                d.child(div().h(px(6.)).w_full().rounded(px(3.)).bg(t.border).child(div().h_full().rounded(px(3.)).bg(t.accent).w(relative(f))))
            });
            body = body.child(button("loops-stop", tr("Stop", "Arrêter"), false, t).on_click(cx.listener(|this, _, _, cx| this.data.update(cx, |d, cx| d.stop_loops_job(cx)))));
        } else if !ready {
            body = body.child(small(
                if has_extract {
                    tr("The region's paths are on disk: build the network to plan loops.", "Les chemins de la région sont sur le disque : construis le réseau pour tracer des boucles.").to_string()
                } else {
                    trf(
                        format!("To plan loops, Astrolabe downloads the paths of {region} once from Geofabrik (OpenStreetMap, a few hundred MB), and the elevation tiles that cover them. Then everything is computed here, offline."),
                        format!("Pour tracer des boucles, Astrolabe télécharge une fois les chemins de {region} chez Geofabrik (OpenStreetMap, quelques centaines de Mo), et les tuiles d'altitude qui les couvrent. Ensuite tout est calculé ici, hors ligne."),
                    )
                },
                t.soft,
            ));
            body = body.child(if has_extract {
                button("loops-build", tr("Build the network", "Construire le réseau"), true, t).on_click(cx.listener(|this, _, _, cx| this.data.update(cx, |d, cx| d.build_graph(cx))))
            } else {
                button("loops-download", tr("Download the region", "Télécharger la région"), true, t).on_click(cx.listener(|this, _, _, cx| this.data.update(cx, |d, cx| d.download_region(cx))))
            });
        }
        if let Some(e) = error {
            body = body.child(small(e, t.bad()));
        }
        if ready {
            body = body.child(self.settings(!found.is_empty(), cx));
            if let Some(e) = self.bad_input.clone() {
                body = body.child(small(e, t.bad()));
            }
            if planning || editing {
                body = body.child(small(if planning { tr("Looking for loops…", "Recherche de boucles…") } else { tr("Routing the loop…", "Calcul du tracé…") }.into(), t.soft));
            } else if let Some(e) = plan_error {
                body = body.child(small(e, t.bad()));
            }
            let has_start = self.start.is_some();
            if let Some(start) = self.start {
                if self.zones.iter().any(|z| crate::logic::fit::metres([z.lat, z.lon], [start.0, start.1]) < z.radius) {
                    body = body.child(small(tr("The start is in a zone to avoid: the loops leave it as soon as they can.", "Le départ est dans une zone à éviter : les boucles en sortent au plus vite.").into(), t.soft));
                }
            } else {
                body = body.child(self.usual(cx));
            }
            // The edits: a new start, the points away, and undo and redo, which can bring back a
            // start taken away.
            let n = self.pins.len();
            let (can_undo, can_redo) = (!self.undo.is_empty(), !self.redo.is_empty());
            let key = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
            if has_start || can_undo || can_redo {
                body = body.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .when(has_start, |d| {
                            d.child(button("loops-clear", tr("New start", "Nouveau départ"), false, t).debug_selector(|| "loops-clear".into()).on_click(cx.listener(|this, _, _, cx| this.clear(cx))))
                        })
                        .when(can_undo, |d| {
                            let tip = format!("{key}+Z");
                            d.child(
                                button("loops-undo", tr("Undo", "Annuler"), false, t)
                                    .debug_selector(|| "loops-undo".into())
                                    .tooltip_show_delay(crate::ui::TIP_DELAY)
                                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                                    .on_click(cx.listener(|this, _, _, cx| this.undo(true, cx))),
                            )
                        })
                        .when(can_redo, |d| {
                            let tip = format!("{key}+{}+Z", tr("Shift", "Maj"));
                            d.child(
                                button("loops-redo", tr("Redo", "Rétablir"), false, t)
                                    .debug_selector(|| "loops-redo".into())
                                    .tooltip_show_delay(crate::ui::TIP_DELAY)
                                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                                    .on_click(cx.listener(|this, _, _, cx| this.undo(false, cx))),
                            )
                        })
                        .when(n > 0, |d| {
                            d.child(
                                button(
                                    "loops-unpin",
                                    if n > 1 { tr("Take the points away", "Retirer les points") } else { tr("Take the point away", "Retirer le point") },
                                    false,
                                    t,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.unpin(cx))),
                            )
                        }),
                );
            }
            if !found.is_empty() {
                body = body.child(self.list(found, cx)).child(self.keeper(cx));
            }
            body = body.child(self.routes(cx));
        }
        // Where the data comes from, and its update: one quiet line at the end.
        let date = built.filter(|t| *t > 0).and_then(|t| chrono::DateTime::from_timestamp(t, 0));
        body = body.child(
            div()
                .flex()
                .flex_wrap()
                .gap_x(px(6.))
                .text_size(px(11.))
                .text_color(t.dim)
                .when_some(date, |d, date| d.child(trf(format!("OpenStreetMap of {}", date.format("%Y-%m-%d")), format!("OpenStreetMap du {}", date.format("%d/%m/%Y")))))
                .child(tr("· elevation: Mapzen Terrain Tiles (SRTM, Copernicus EU-DEM…)", "· altitude : Mapzen Terrain Tiles (SRTM, Copernicus EU-DEM…)"))
                .when(ready && !running, |d| {
                    d.child(
                        div()
                            .id("loops-update")
                            .debug_selector(|| "loops-update".into())
                            .cursor_pointer()
                            .text_color(t.soft)
                            .hover(|d| d.text_color(t.text))
                            .child(tr("· download again", "· télécharger à nouveau"))
                            .on_click(cx.listener(|this, _, _, cx| this.data.update(cx, |d, cx| d.download_region(cx)))),
                    )
                }),
        );

        div()
            .id("loops-panel")
            .absolute()
            .top(px(14.0f32.max(crate::ui::top() - 14.0)))
            .left(px(14.))
            .w(px(PANEL as f32))
            .max_h(relative(0.92))
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
            .child(div().flex().justify_between().items_baseline().child(heading(tr("Loops", "Boucles"))).child(small(region.into(), t.soft)))
            .child(body)
    }

    /// The climb asked for, if any.
    fn wished_climb(&self, cx: &App) -> Option<f64> {
        self.climb.read(cx).value().trim().replace(',', ".").parse::<f64>().ok().filter(|v| *v > 0.0)
    }

    /// The chosen loop the other way round.
    fn reverse(&mut self, cx: &mut Context<Self>) {
        let found = self.snap_found(cx);
        if found.get(self.selected).is_none() {
            return;
        }
        self.remember(cx);
        let mut v = (*found).clone();
        v[self.selected] = v[self.selected].reversed();
        self.hover = None;
        self.climb_hover = None;
        self.data.update(cx, |d, cx| d.set_found(Arc::new(v), cx));
    }

    /// The settings in a few words, for the line they fold to.
    fn summary(&self, cx: &App) -> String {
        let value = |i: &Entity<InputState>| i.read(cx).value().trim().to_string();
        let mut said = vec![if self.exact { format!("{} km", value(&self.km)) } else { format!("{}–{} km", value(&self.min_km), value(&self.max_km)) }];
        let ground = [tr("streets", "routes"), tr("streets or trails", "routes ou sentiers"), tr("trails", "sentiers")][self.trail.min(2)];
        said.push(if self.avoid_paved { format!("{ground}, {}", tr("no asphalt", "sans bitume")) } else { ground.to_string() });
        let climb = value(&self.climb);
        if !climb.is_empty() {
            said.push(format!("D+ {climb} m"));
        }
        match self.mine {
            1 => said.push(tr("my ways", "mes chemins").into()),
            -1 => said.push(tr("new ways", "chemins nouveaux").into()),
            _ => {}
        }
        let avoided = self.avoided();
        if !avoided.is_empty() {
            said.push(format!("{} {}", tr("avoid:", "éviter :"), avoided));
        }
        said.join(" · ")
    }

    /// What is avoided, in a few words; empty when nothing is.
    fn avoided(&self) -> String {
        let mut said: Vec<String> = Vec::new();
        if self.avoid_industrial {
            said.push(tr("industry", "industrie").into());
        }
        if self.lit {
            said.push(tr("unlit ways", "voies non éclairées").into());
        }
        let zones = self.zones.len();
        if zones > 0 {
            said.push(format!("{zones} zone{}", if zones > 1 { "s" } else { "" }));
        }
        said.join(" · ")
    }

    /// The settings; while there are loops, folded to a line that unfolds them.
    fn settings(&mut self, has_loops: bool, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let fold = |open: bool, text: Option<String>, cx: &mut Context<Self>| {
            div()
                .id("loops-settings")
                .debug_selector(|| "loops-settings".into())
                .flex()
                .items_start()
                .gap(px(4.))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings_open = !this.settings_open;
                    cx.notify();
                }))
                .child(div().pt(px(2.)).child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }, t.soft, 12.)))
                .child(match text {
                    Some(text) => div().flex_1().min_w(px(0.)).text_size(px(12.)).text_color(t.text).child(text),
                    None => div().text_size(px(12.)).text_color(t.soft).child(tr("Settings", "Réglages")),
                })
        };
        if has_loops && !self.settings_open {
            let text = self.summary(cx);
            return div().child(fold(false, Some(text), cx));
        }
        let label = |s: &'static str| div().text_size(px(12.)).text_color(t.soft).child(s);
        let mode = segmented("loops-mode", vec![(tr("Exact", "Exacte").into(), self.exact), (tr("Range", "Fourchette").into(), !self.exact)], t, cx, |this: &mut LoopsView, i, _, cx| {
            this.exact = i == 0;
            this.settle(cx);
        });
        let trail = segmented(
            "loops-trail",
            vec![(tr("Streets", "Routes").into(), self.trail == 0), (tr("Both", "Les deux").into(), self.trail == 1), (tr("Trails", "Sentiers").into(), self.trail == 2)],
            t,
            cx,
            |this: &mut LoopsView, i, _, cx| {
                this.trail = i;
                this.settle(cx);
            },
        );
        let paved = check("loops-paved", tr("Avoid asphalt", "Éviter le bitume"), self.avoid_paved, t).on_click(cx.listener(|this, _, _, cx| {
            this.avoid_paved = !this.avoid_paved;
            this.settle(cx);
        }));
        let field = |i: &Entity<InputState>| div().w(px(80.)).child(Input::new(i).small());
        let mine = segmented(
            "loops-mine",
            vec![(tr("My ways", "Mes chemins").into(), self.mine > 0), (tr("Any", "Peu importe").into(), self.mine == 0), (tr("Elsewhere", "Ailleurs").into(), self.mine < 0)],
            t,
            cx,
            |this: &mut LoopsView, i, _, cx| {
                this.mine = [1, 0, -1][i];
                this.settle(cx);
            },
        );
        let industrial = check("loops-industrial", tr("Industrial areas", "Zones industrielles"), self.avoid_industrial, t).on_click(cx.listener(|this, _, _, cx| {
            this.avoid_industrial = !this.avoid_industrial;
            this.settle(cx);
        }));
        let lit = check("loops-lit", tr("Unlit ways", "Voies non éclairées"), self.lit, t).on_click(cx.listener(|this, _, _, cx| {
            this.lit = !this.lit;
            this.settle(cx);
        }));
        let zones = self.zones.len();
        let summary = match self.avoided() {
            a if a.is_empty() => tr("nothing", "rien").to_string(),
            a => a,
        };
        let draw = button("loops-zone", if self.placing_zone { tr("Click on the map…", "Clique sur la carte…") } else { tr("Draw a zone to avoid", "Dessiner une zone à éviter") }, self.placing_zone, t)
            .debug_selector(|| "loops-zone".into())
            .on_click(cx.listener(|this, _, _, cx| {
                this.placing_zone = !this.placing_zone;
                cx.notify();
            }));
        let distance = if self.exact {
            div().flex().items_center().gap(px(6.)).child(field(&self.km)).child(label("km"))
        } else {
            div().flex().items_center().gap(px(6.)).child(field(&self.min_km)).child(label(tr("to", "à"))).child(field(&self.max_km)).child(label("km"))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .when(has_loops, |d| d.child(fold(true, None, cx)))
            .child(div().flex().flex_col().gap(px(5.)).child(label(tr("Distance", "Distance"))).child(mode).child(distance))
            .child(div().flex().flex_col().gap(px(5.)).child(label(tr("Ground", "Terrain"))).child(trail).child(paved))
            .child(div().flex().flex_col().gap(px(5.)).child(label(tr("Where to run", "Où courir"))).child(mine))
            .child(div().flex().items_center().gap(px(6.)).child(label(tr("Climb wished", "D+ souhaité"))).child(field(&self.climb)).child(label("m")))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        div()
                            .id("loops-avoid")
                            .debug_selector(|| "loops-avoid".into())
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.avoid_open = !this.avoid_open;
                                cx.notify();
                            }))
                            .child(icon(if self.avoid_open { IconName::ChevronDown } else { IconName::ChevronRight }, t.soft, 12.))
                            .child(label(tr("Avoid", "Éviter")))
                            .when(!self.avoid_open, |d| d.child(div().text_size(px(12.)).text_color(t.text).child(summary))),
                    )
                    .when(self.avoid_open, |d| {
                        d.child(industrial).child(lit).child(div().flex().flex_wrap().gap(px(8.)).child(draw).when(zones > 0, |d| {
                            d.child(button("loops-zones-clear", tr("Take the zones away", "Retirer les zones"), false, t).on_click(cx.listener(|this, _, _, cx| {
                                this.remember(cx);
                                this.zones.clear();
                                this.propose(false, cx);
                            })))
                        }))
                    }),
            )

    }

    fn list(&mut self, found: &Arc<Vec<Loop>>, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        // `+ 0.0`: an empty sum is −0.0, said "-0".
        let pct = |f: f32| format!("{:.0}{}", (f * 100.0).round() + 0.0, crate::lang::pct());
        // The mini-profiles share one scale, height and length, so that they compare.
        let all: Vec<Option<f32>> = found.iter().flat_map(|l| l.eles.iter().copied()).collect();
        let scale = range(&all);
        let longest = found.iter().map(|l| l.along.last().copied().unwrap_or(0.0)).fold(1.0f32, f32::max);
        let pace = effort_pace(&self.data.read(cx).acts);
        // A track brought from a file wasn't planned: what was asked doesn't apply to it.
        let planned = !found.iter().all(|l| l.track);
        let wished = self.wished_climb(cx).filter(|_| planned);
        let rows = found.iter().enumerate().map(|(i, l)| {
            let on = i == self.selected;
            let xs: Arc<Vec<f32>> = Arc::new(l.along.clone());
            let share = l.along.last().copied().unwrap_or(0.0) / longest;
            let mini = div().h(px(26.)).w(relative(share.clamp(0.05, 1.0))).relative().when(scale.is_some(), |d| {
                d.child(
                    line_chart(
                        Series { xs, ys: Arc::new(l.eles.clone()), scale, line: if on { t.accent } else { t.soft }, fill: blend(if on { t.accent } else { t.soft }, t.bg, 0.12), invert: false },
                        None,
                        t.soft,
                    )
                    .size_full(),
                )
            });
            div()
                .id(("loop", i))
                .debug_selector(move || format!("loop-{i}"))
                .flex()
                .flex_col()
                .gap(px(3.))
                .px(px(8.))
                .py(px(6.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(on, |d| d.bg(t.selection))
                .hover(|d| d.bg(t.border))
                .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                    this.card_hover = if *over { Some(i) } else { this.card_hover.filter(|k| *k != i) };
                    cx.notify();
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected = i;
                    this.hover = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .justify_between()
                        .gap_x(px(10.))
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(8.))
                                .child(num(letter(i).to_string()).text_color(t.soft))
                                .child(num(fmt_km(l.dist)).font_weight(FontWeight::BOLD).text_color(if on { t.accent } else { t.text }))
                                // At one's own pace, from one's last runs.
                                .children(pace.map(|p| num(format!("≈ {}", fmt_duration(p * (l.dist + CLIMB_COST * l.up)))).text_size(px(12.)).text_color(t.soft))),
                        )
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(6.))
                                .child(num(format!("+{} −{}", fmt_m(l.up), fmt_m(l.down))).text_size(px(12.)))
                                // How far from the climb asked for.
                                .children(wished.map(|w| {
                                    let gap = l.up - w;
                                    num(format!("{}{}", if gap < 0.0 { "−" } else { "+" }, fmt_m(gap.abs()))).text_size(px(11.)).text_color(t.soft)
                                })),
                        ),
                )
                .child(mini)
                .child(surfaces(l, i, t))
                .when(!l.track, |d| d.child(div().text_size(px(11.)).text_color(t.soft).child(format!(
                    "{} {}{}",
                    pct(l.back),
                    tr("out and back", "aller-retour"),
                    l.mine.map(|m| format!(" · {} {}", pct(m), tr("on my tracks", "sur mes traces"))).unwrap_or_default()
                ))))
        });
        // What was asked and could not be had, said rather than left to guess.
        let mut unmet: Vec<String> = Vec::new();
        if planned && self.avoid_paved && found.iter().all(|l| l.paved > 0.5) {
            unmet.push(tr("Few unpaved ways around the start: the asphalt could not be avoided.", "Peu de chemins non revêtus autour du départ : le bitume n'a pas pu être évité.").into());
        }
        if let Some(w) = wished {
            if found.iter().all(|l| l.up < w * 0.75) {
                unmet.push(trf(format!("Not enough relief around the start for {} of climb at this distance.", fmt_m(w)), format!("Pas assez de relief autour du départ pour {} de D+ à cette distance.", fmt_m(w))));
            } else if found.iter().all(|l| l.up > w * 1.25) {
                unmet.push(trf(format!("The ground around the start climbs more than {}.", fmt_m(w)), format!("Le terrain autour du départ monte plus que {}.", fmt_m(w))));
            }
        }
        div().flex().flex_col().gap(px(4.)).children(rows).children(unmet.into_iter().map(|u| div().px(px(8.)).text_size(px(12.)).text_color(t.soft).child(u))).child(
            div()
                .pt(px(4.))
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .child(button("loops-again", tr("Other loops", "Autres boucles"), false, t).on_click(cx.listener(|this, _, _, cx| this.propose(true, cx))))
                .child(button("loops-reverse", tr("Other way round", "Sens inverse"), false, t).debug_selector(|| "loops-reverse".into()).on_click(cx.listener(|this, _, _, cx| this.reverse(cx))))
                .child(button("loops-export", tr("Export as GPX", "Exporter en GPX"), false, t).on_click(cx.listener(|this, _, _, cx| this.export(cx)))),
        )
    }

    /// Where one's outings start most often in the region, the three busiest, named after the
    /// place the watch named them after.
    fn usual_starts(&self, cx: &App) -> Vec<Usual> {
        let d = self.data.read(cx);
        let region = d.loops.region;
        let places: std::collections::HashMap<i64, String> = d.acts.iter().filter_map(|a| Some((a.activity_id, crate::logic::journal::place(a)?))).collect();
        let outings = d.derived.iter().map(|(id, x)| (x.line.as_slice(), places.get(id).cloned()));
        crate::logic::loops::usual_starts(outings, 12).into_iter().filter(|u| region.holds(u.at[0], u.at[1])).take(3).collect()
    }

    /// Before a start: where one's outings start most often, to start from there in a click.
    fn usual(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let starts = self.usual_starts(cx);
        let hint = div().text_size(px(12.)).text_color(t.soft);
        if starts.is_empty() {
            return div().child(hint.child(tr("Click on the map to set the start.", "Clique sur la carte pour poser le départ.")));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(hint.child(tr("Click on the map to set the start, or start where you usually do:", "Clique sur la carte pour poser le départ, ou pars d'où tu pars d'habitude :")))
            .child(div().flex().flex_wrap().gap(px(8.)).children(starts.into_iter().enumerate().map(|(i, u)| {
                let (p, n) = (u.at, u.count);
                // The place, as on the dot on the map; the count says which dot without one.
                let count = trf(format!("{n} outing{}", if n > 1 { "s" } else { "" }), format!("{n} sortie{}", if n > 1 { "s" } else { "" }));
                let label = match &u.place {
                    Some(place) => format!("{place} · {count}"),
                    None => count,
                };
                button(("loops-usual", i), label, false, t).debug_selector(move || format!("loops-usual-{i}")).on_click(cx.listener(move |this, _, _, cx| {
                    this.remember(cx);
                    this.start = Some((p[0], p[1]));
                    this.propose(false, cx);
                }))
            })))
    }

    /// The name field and the buttons that keep the chosen loop.
    fn keeper(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w(px(0.)).child(Input::new(&self.name).small()))
                    .child(button("loops-keep", tr("Keep", "Garder"), true, t).debug_selector(|| "loops-keep".into()).on_click(cx.listener(|this, _, _, cx| this.keep(false, cx)))),
            )
            .when(self.opened.is_some(), |d| {
                d.child(div().child(button("loops-copy", tr("Keep a copy", "Garder une copie"), false, t).debug_selector(|| "loops-copy".into()).on_click(cx.listener(|this, _, _, cx| this.keep(true, cx)))))
            })
    }

    /// The routes kept: a click opens one, the cross takes it away; tracks from elsewhere join
    /// them by importing a GPX file.
    fn routes(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let routes: Vec<Route> = self.data.read(cx).loops.routes.iter().filter(|r| !r.trashed).cloned().collect();
        let import = div().flex().child(
            button("loops-import", tr("Import a GPX…", "Importer un GPX…"), false, t)
                .debug_selector(|| "loops-import".into())
                .tooltip_show_delay(crate::ui::TIP_DELAY)
                .tooltip(|window, cx| Tooltip::new(tr("A track from a watch or another app, kept with the routes", "Une trace d'une montre ou d'une autre application, gardée avec les parcours")).build(window, cx))
                .on_click(cx.listener(|this, _, window, cx| this.import(window, cx))),
        );
        if routes.is_empty() {
            return div().child(import);
        }
        let rows = routes.into_iter().rev().enumerate().map(|(i, r)| {
            let (id, on) = (r.id, self.opened == Some(r.id));
            div()
                .id(("route", i))
                .debug_selector(move || format!("route-{i}"))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .py(px(5.))
                .rounded(px(6.))
                .cursor_pointer()
                .when(on, |d| d.bg(t.selection))
                .hover(|d| d.bg(t.border))
                .on_click(cx.listener(move |this, _, window, cx| this.open_route(id, window, cx)))
                .child(div().flex_1().min_w(px(0.)).overflow_hidden().text_ellipsis().whitespace_nowrap().text_size(px(13.)).text_color(if on { t.accent } else { t.text }).child(r.name.clone()))
                .child(num(format!("{} · +{}", fmt_km(r.shape.dist), fmt_m(r.shape.up))).text_size(px(12.)).text_color(t.soft))
                .child(
                    div()
                        .id(("route-trash", i))
                        .debug_selector(move || format!("route-trash-{i}"))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if this.opened == Some(id) {
                                this.opened = None;
                            }
                            this.data.update(cx, |d, cx| d.trash_route(id, cx));
                        }))
                        .child(icon(IconName::X, t.soft, 13.)),
                )
        });
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(div().text_size(px(12.)).text_color(t.soft).child(tr("Routes kept", "Parcours gardés")))
            .children(rows)
            .child(div().pt(px(4.)).child(import))
    }

    /// Along the bottom of the map, right of the panel: the chosen loop's profile and its climbs,
    /// always in sight.
    fn band(&mut self, found: &Arc<Vec<Loop>>, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = self.snap.theme;
        div()
            .id("loops-band")
            .debug_selector(|| "loops-band".into())
            .absolute()
            .left(px(PANEL as f32 + 28.0))
            .right(px(14.))
            .bottom(px(24.))
            .h(px(BAND as f32))
            .bg(blend(t.bg, t.panel, 0.94))
            .rounded(px(8.))
            .shadow_md()
            .px(px(14.))
            .py(px(10.))
            .flex()
            .gap(px(16.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(div().flex_1().min_w(px(0.)).child(self.profile(found, cx)))
            // A narrow window shares the band: the climbs keep at most 300 px, their lines cut short.
            .child(div().id("loops-climbs").w(relative(0.42)).max_w(px(300.)).flex_none().overflow_y_scroll().child(self.climbs(found, cx)))
    }

    /// The chosen loop's climbs; one pointed at is outlined on the map and the profile.
    fn climbs(&mut self, found: &Arc<Vec<Loop>>, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let Some(l) = found.get(self.selected) else { return div() };
        let climbs = slope::climbs(&l.along, &l.eles);
        if climbs.is_empty() {
            return div().text_size(px(12.)).text_color(t.soft).child(tr("No climb", "Aucune montée"));
        }
        let pct = |g: f32| format!("{:.0}{}", (g * 100.0).round() + 0.0, crate::lang::pct());
        let rows = climbs.iter().enumerate().map(|(i, c)| {
            let on = self.climb_hover == Some(i);
            div()
                .id(("climb", i))
                .flex()
                .justify_between()
                .gap(px(8.))
                .px(px(8.))
                .py(px(3.))
                .rounded(px(6.))
                .when(on, |d| d.bg(t.border))
                .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                    this.climb_hover = if *over { Some(i) } else { this.climb_hover.filter(|k| *k != i) };
                    cx.notify();
                }))
                .child(num(decimal(format!("km {:.1}", l.along[c.from] / 1000.0))).text_size(px(12.)).text_color(t.soft))
                .child(num(format!("{} · +{} · {} ({} max)", fmt_km(c.dist as f64), fmt_m(c.gain as f64), pct(c.grade), pct(c.steepest))).text_size(px(12.)).min_w(px(0.)).truncate())
        });
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(div().text_size(px(12.)).text_color(t.soft).child(trf(
                format!("{} climb{}", climbs.len(), if climbs.len() > 1 { "s" } else { "" }),
                format!("{} montée{}", climbs.len(), if climbs.len() > 1 { "s" } else { "" }),
            )))
            .children(rows)
    }

    fn profile(&mut self, found: &Arc<Vec<Loop>>, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let Some(l) = found.get(self.selected) else { return div() };
        let xs: Arc<Vec<f32>> = Arc::new(l.along.iter().map(|m| m / 1000.0).collect());
        let ys: Arc<Vec<Option<f32>>> = Arc::new(l.eles.clone());
        if ys.iter().flatten().nth(1).is_none() {
            return div().text_size(px(12.)).text_color(t.soft).child(tr("No altitude for this loop.", "Pas d'altitude pour cette boucle."));
        }
        let scale = range(&ys);
        let bounds = self.plots.get("loop-profile");
        let cell = bounds.clone();
        let label = match self.hover.and_then(|i| Some((xs.get(i)?, ys.get(i).copied().flatten()?))) {
            Some((x, y)) => format!("{} · {}", decimal(format!("km {x:.2}")), fmt_m(y as f64)),
            None => format!("{} {}", tr("Altitude", "Altitude"), group(l.eles.iter().flatten().fold(f32::MIN, |a, b| a.max(*b)) as i64) + " m max"),
        };
        let xs2 = xs.clone();
        // The grade along it, in the colours of the map.
        let total = l.along.last().copied().unwrap_or(0.0).max(1.0);
        let ramp = slope_ramp(&t, self.three_d);
        let strip = self.show_slope.then(|| {
            div().h(px(4.)).w_full().flex().rounded(px(2.)).overflow_hidden().children(
                slope::runs(&l.along, &l.eles).into_iter().map(|(i, j, k)| div().h_full().w(relative((l.along[j] - l.along[i]) / total)).bg(ramp[k as usize])),
            )
        });
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(div().flex().justify_between().gap(px(8.)).text_size(px(12.)).text_color(t.soft).child(tr("Profile", "Profil")).child(num(label).min_w(px(0.)).truncate()))
            .child(
                div()
                    .id("loop-profile")
                    .relative()
                    .h(px(78.))
                    .child(canvas(move |b, _, _| cell.set(b), |_, _, _, _| {}).absolute().size_full())
                    .child(line_chart(Series { xs, ys, scale, line: t.accent, fill: blend(t.accent, t.bg, 0.12), invert: false }, self.hover, t.soft).size_full())
                    .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, _, cx| {
                        let b = bounds.get();
                        if b.contains(&e.position) && b.size.width > px(0.) {
                            let f = ((e.position.x - b.origin.x) / b.size.width).clamp(0.0, 1.0);
                            let x1 = xs2.last().copied().unwrap_or(0.0);
                            this.hover = nearest(&xs2, f * x1);
                            cx.notify();
                        }
                    }))
                    .on_hover(cx.listener(|this, on: &bool, _, cx| {
                        if !on {
                            this.hover = None;
                            cx.notify();
                        }
                    })),
            )
            .children(strip)
    }
}

#[cfg(test)]
mod tests {
    use super::{read_km, read_zones, write_zones};

    #[test]
    fn zones_read_back_and_bad_ones_are_dropped() {
        assert!(read_zones("").is_empty());
        assert!(read_zones(";;").is_empty());
        let z = read_zones("43.7,7.25,300;garbage;1,2;NaN,7,10;43.7,7.25,-5;91,7,10;43.7,7.25,1e9");
        assert_eq!(z.len(), 2);
        assert_eq!((z[0].lat, z[0].lon, z[0].radius), (43.7, 7.25, 300.0));
        assert_eq!(z[1].radius, 50_000.0);
        assert_eq!(read_zones(&write_zones(&z)), z);
    }

    #[test]
    fn distances_read_with_a_comma_or_a_dot() {
        assert_eq!(read_km("12"), Some(12_000.0));
        assert_eq!(read_km(" 7,5 "), Some(7_500.0));
        assert_eq!(read_km("0.2"), None);
        assert_eq!(read_km("abc"), None);
        assert_eq!(read_km("1e9"), None);
    }
}

/// A stroke for a line on the map, bevelled at its turns: a mitre on a tight hairpin (or on the
/// altitudes' noise) would shoot a spike far out of the line.
fn line_path(width: f32) -> PathBuilder {
    PathBuilder::stroke(px(width)).with_style(PathStyle::Stroke(StrokeOptions::default().with_line_width(width).with_miter_limit(1.0)))
}
