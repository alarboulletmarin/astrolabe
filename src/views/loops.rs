//! Loops: click a start on the map, ask for a distance (exact or a range) and what to run on, and
//! the page proposes loops on the region's paths, with their climb, their profile and a GPX file
//! for each. Everything is computed on the machine, from the region's OpenStreetMap extract.

use crate::data::{Data, Snapshot};
use crate::lang::{decimal, tr, trf};
use crate::logic::activity::{fmt_km, fmt_m, group};
use crate::logic::loops::{Loop, Request, Target};
use crate::logic::map::{self, View};
use crate::logic::relief;
use crate::logic::route::Prefs;
use crate::ui::chart::{Series, line_chart, nearest, range};
use crate::ui::theme::blend;
use crate::ui::{Plots, button, heading, num, render_image, segmented};
use crate::views::Page;
use crate::views::map::{MapState, map_widget};
use gpui_kit::component::Sizable;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{prelude::FluentBuilder, *};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

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
    seed: u64,
    selected: usize,
    /// The point of the profile pointed at.
    hover: Option<usize>,
    show_relief: bool,
    relief: Relief,
    plots: Plots,
    /// What the input asked that can't be read.
    bad_input: Option<String>,
    /// The point being dragged on the map.
    drag: Option<Handle>,
    /// Bumped by each setting changed: only the last one's pause plans.
    pending: u64,
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
            plots: Plots::default(),
            bad_input: None,
            drag: None,
            pending: 0,
            _subscriptions: subs,
        }
    }

    /// The request the panel describes, or why it can't be one.
    fn request(&self, cx: &App) -> Result<Request, String> {
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
        let prefs = Prefs { trail: [0.0, 0.5, 1.0][self.trail], avoid_paved: self.avoid_paved };
        Ok(Request { lat, lon, target, prefs, climb, seed: self.seed, count: COUNT })
    }

    /// Plans with the panel's settings; `again`: other loops (a new seed).
    pub fn propose(&mut self, again: bool, cx: &mut Context<Self>) {
        if again {
            self.seed = crate::logic::loops::Rng::new(self.seed ^ 0x5eed).next();
        }
        match self.request(cx) {
            Ok(req) => {
                self.bad_input = None;
                self.selected = 0;
                self.hover = None;
                let km = |i: &Entity<InputState>| i.read(cx).value().to_string();
                let saved = [
                    ("loops.km", km(&self.km)),
                    ("loops.min", km(&self.min_km)),
                    ("loops.max", km(&self.max_km)),
                    ("loops.climb", km(&self.climb)),
                    ("loops.mode", if self.exact { "exact" } else { "range" }.to_string()),
                    ("loops.trail", self.trail.to_string()),
                    ("loops.paved", if self.avoid_paved { "1" } else { "0" }.to_string()),
                ];
                self.data.update(cx, |d, cx| {
                    for (k, v) in saved {
                        if d.settings.get(k) != Some(&v) {
                            d.set_setting(k, v, cx);
                        }
                    }
                    d.plan_loops(req, cx)
                });
            }
            Err(e) => self.bad_input = Some(e),
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

    /// The point of the map under the pointer that can be dragged, if any.
    fn handle_at(&self, p: Point<Pixels>) -> Option<Handle> {
        let b = self.map.bounds.get();
        let (x, y) = (((p.x - b.origin.x) / px(1.)), ((p.y - b.origin.y) / px(1.)));
        let near = |(hx, hy): (f32, f32)| (hx - x).hypot(hy - y) <= GRAB;
        self.start.filter(|s| near(self.on_map(s.0, s.1))).map(|_| Handle::Start)
    }

    fn grab(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) {
        if let Some(h) = self.handle_at(e.position) {
            self.drag = Some(h);
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn move_to(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(h) = self.drag else { return };
        if e.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let at = self.under(e.position);
        match h {
            Handle::Start => self.start = Some(at),
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn drop_at(&mut self, e: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(h) = self.drag.take() else { return };
        let at = self.under(e.position);
        match h {
            Handle::Start => {
                self.start = Some(at);
                self.propose(false, cx);
            }
        }
        cx.stop_propagation();
    }

    /// Forgets the start and the loops, to begin again elsewhere.
    fn clear(&mut self, cx: &mut Context<Self>) {
        self.start = None;
        self.drag = None;
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.data.update(cx, |d, cx| d.open_loops(cx));
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
        let bbox = bbox.map(|[s, w, n, e]| {
            let free = (width - PANEL - 28.0).max(width * 0.3);
            [s, w - (e - w) * (width - free) / free, n, e]
        });
        self.map.fit(generation, bbox, false);

        let b = self.map.bounds.get();
        let (w, h) = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
        // The first click sets the start; then the start is dragged.
        if let Some((sx, sy)) = self.map.click.take()
            && self.start.is_none()
        {
            self.start = Some(self.map.view.position(sx, sy, w, h));
            if self.data.read(cx).loops.graph.is_some() {
                self.propose(false, cx);
            }
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
        // Loops of the last settings, while those of the new ones are looked for: faded.
        let fade = if self.data.read(cx).loops.planning { 0.45 } else { 1.0 };
        let start = self.start;
        let hover = chosen.zip(self.hover).and_then(|(l, i)| l.line.get(i).copied());
        let (accent, ink, bg) = (t.accent, t.text, t.bg);
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
                // The others thin, the chosen one on top.
                for (line, on) in lines.iter().filter(|l| !l.1).chain(lines.iter().filter(|l| l.1)) {
                    if line.len() < 2 {
                        continue;
                    }
                    let mut path = PathBuilder::stroke(px(if *on { 4. } else { 2.5 }));
                    path.move_to(at(line[0]));
                    line[1..].iter().for_each(|p| path.line_to(at(*p)));
                    if let Ok(p) = path.build() {
                        window.paint_path(p, if *on { accent.opacity(fade) } else { ink.opacity(0.4 * fade) });
                    }
                }
                let dot = |p: Point<Pixels>, r: f32, c: Hsla, window: &mut Window| {
                    window.paint_quad(fill(Bounds::new(point(p.x - px(r), p.y - px(r)), size(px(2. * r), px(2. * r))), c).corner_radii(px(r)));
                };
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
        let overlay = div()
            .absolute()
            .size_full()
            .children(shade)
            .child(drawing)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.grab(e, cx)))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| this.move_to(e, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, e: &MouseUpEvent, _, cx| this.drop_at(e, cx)))
            .into_any_element();

        div()
            .size_full()
            .relative()
            .child(map_widget(&self.map, |v: &mut LoopsView| &mut v.map, &self.data, "loops-map", Some(overlay), bbox, false, cx))
            .child(self.panel(&found, cx))
            .child(
                div()
                    .absolute()
                    .top(px(14.0f32.max(crate::ui::top() - 14.0) + 40.0))
                    .right(px(14.))
                    .bg(t.bg)
                    .rounded(px(8.))
                    .shadow_sm()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(segmented("loops-relief", vec![(tr("Relief", "Relief").into(), self.show_relief), (tr("Flat", "Plate").into(), !self.show_relief)], t, cx, |this: &mut LoopsView, i, _, _| {
                        this.show_relief = i == 0;
                        this.relief.drawn = None;
                    })),
            )
    }
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
        let (has_extract, error, planning, plan_error) = (l.extract.is_some(), l.error.clone(), l.planning, l.plan_error.clone());
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
            body = body.child(self.settings(cx));
            if let Some(e) = self.bad_input.clone() {
                body = body.child(small(e, t.bad()));
            }
            if planning {
                body = body.child(small(tr("Looking for loops…", "Recherche de boucles…").into(), t.soft));
            } else if let Some(e) = plan_error {
                body = body.child(small(e, t.bad()));
            }
            if self.start.is_none() {
                body = body.child(small(tr("Click on the map to set the start.", "Clique sur la carte pour poser le départ.").into(), t.soft));
            } else {
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(small(tr("Drag the start to move it.", "Glisse le départ pour le déplacer.").into(), t.soft))
                        .child(button("loops-clear", tr("New start", "Nouveau départ"), false, t).debug_selector(|| "loops-clear".into()).on_click(cx.listener(|this, _, _, cx| this.clear(cx)))),
                );
            }
            if !found.is_empty() {
                body = body.child(self.list(found, cx)).child(self.profile(found, cx));
            }
            if let Some(time) = built.filter(|t| *t > 0)
                && let Some(date) = chrono::DateTime::from_timestamp(time, 0)
            {
                body = body.child(small(
                    trf(format!("OpenStreetMap data of {}.", date.format("%Y-%m-%d")), format!("Données OpenStreetMap du {}.", date.format("%d/%m/%Y"))),
                    t.dim,
                ));
            }
            if !running {
                body = body.child(
                    div().child(button("loops-update", tr("Download the region again", "Télécharger à nouveau la région"), false, t).on_click(cx.listener(|this, _, _, cx| this.data.update(cx, |d, cx| d.download_region(cx))))),
                );
            }
        }
        body = body.child(small(tr("Elevation: Mapzen Terrain Tiles (SRTM, Copernicus EU-DEM and others).", "Altitude : Mapzen Terrain Tiles (SRTM, Copernicus EU-DEM et d'autres).").into(), t.dim));

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

    fn settings(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        let label = |s: &'static str| div().text_size(px(12.)).text_color(t.soft).child(s);
        let mode = segmented("loops-mode", vec![(tr("Exact", "Exacte").into(), self.exact), (tr("Range", "Fourchette").into(), !self.exact)], t, cx, |this: &mut LoopsView, i, _, cx| {
            this.exact = i == 0;
            this.settle(cx);
        });
        let trail = segmented(
            "loops-trail",
            vec![(tr("Streets", "Routes").into(), self.trail == 0), (tr("Either", "Égal").into(), self.trail == 1), (tr("Trails", "Sentiers").into(), self.trail == 2)],
            t,
            cx,
            |this: &mut LoopsView, i, _, cx| {
                this.trail = i;
                this.settle(cx);
            },
        );
        let paved = segmented("loops-paved", vec![(tr("Asphalt allowed", "Bitume permis").into(), !self.avoid_paved), (tr("Avoid asphalt", "Éviter le bitume").into(), self.avoid_paved)], t, cx, |this: &mut LoopsView, i, _, cx| {
            this.avoid_paved = i == 1;
            this.settle(cx);
        });
        let field = |i: &Entity<InputState>| div().w(px(80.)).child(Input::new(i).small());
        let distance = if self.exact {
            div().flex().items_center().gap(px(6.)).child(field(&self.km)).child(label("km"))
        } else {
            div().flex().items_center().gap(px(6.)).child(field(&self.min_km)).child(label(tr("to", "à"))).child(field(&self.max_km)).child(label("km"))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().flex().flex_col().gap(px(5.)).child(label(tr("Distance", "Distance"))).child(mode).child(distance))
            .child(div().flex().flex_col().gap(px(5.)).child(label(tr("Ground", "Terrain"))).child(trail).child(paved))
            .child(div().flex().items_center().gap(px(6.)).child(label(tr("Climb wished", "D+ souhaité"))).child(field(&self.climb)).child(label("m")))

    }

    fn list(&mut self, found: &Arc<Vec<Loop>>, cx: &mut Context<Self>) -> Div {
        let t = self.snap.theme;
        // `+ 0.0`: an empty sum is −0.0, said "-0".
        let pct = |f: f32| format!("{:.0}{}", (f * 100.0).round() + 0.0, crate::lang::pct());
        // The mini-profiles share one scale, height and length, so that they compare.
        let all: Vec<Option<f32>> = found.iter().flat_map(|l| l.eles.iter().copied()).collect();
        let scale = range(&all);
        let longest = found.iter().map(|l| l.along.last().copied().unwrap_or(0.0)).fold(1.0f32, f32::max);
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
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected = i;
                    this.hover = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .child(num(fmt_km(l.dist)).font_weight(FontWeight::BOLD).text_color(if on { t.accent } else { t.text }))
                        .child(num(format!("+{} −{}", fmt_m(l.up), fmt_m(l.down))).text_size(px(12.))),
                )
                .child(mini)
                .child(div().text_size(px(11.)).text_color(t.soft).child(format!(
                    "{} {} · {} {} · {} {}",
                    pct(l.trail),
                    tr("trail", "sentier"),
                    pct(l.paved),
                    tr("asphalt", "bitume"),
                    pct(l.back),
                    tr("twice", "en double")
                )))
        });
        div().flex().flex_col().gap(px(4.)).children(rows).child(
            div()
                .pt(px(4.))
                .flex()
                .gap(px(8.))
                .child(button("loops-again", tr("Other loops", "Autres boucles"), false, t).on_click(cx.listener(|this, _, _, cx| this.propose(true, cx))))
                .child(button("loops-export", tr("Save as GPX", "Enregistrer en GPX"), false, t).on_click(cx.listener(|this, _, _, cx| this.export(cx)))),
        )
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
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(div().flex().justify_between().text_size(px(12.)).text_color(t.soft).child(tr("Profile", "Profil")).child(num(label)))
            .child(
                div()
                    .id("loop-profile")
                    .relative()
                    .h(px(90.))
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
    }
}

#[cfg(test)]
mod tests {
    use super::read_km;

    #[test]
    fn distances_read_with_a_comma_or_a_dot() {
        assert_eq!(read_km("12"), Some(12_000.0));
        assert_eq!(read_km(" 7,5 "), Some(7_500.0));
        assert_eq!(read_km("0.2"), None);
        assert_eq!(read_km("abc"), None);
        assert_eq!(read_km("1e9"), None);
    }
}
