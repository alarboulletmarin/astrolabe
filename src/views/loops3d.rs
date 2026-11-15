//! The chosen loop over its ground, in 3D: the ground made from the elevation tiles, its picture
//! drawn in the background, and the pointer that turns it. Editing the loop stays on the map.

use super::{LoopsView, PANEL, slope_ramp};
use crate::lang::tr;
use crate::logic::loops::Loop;
use crate::logic::map::world;
use crate::logic::slope;
use crate::logic::terrain::{self, Camera, Terrain};
use crate::ui::render_image;
use gpui_kit::*;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Heights drawn half again as high: the slopes read better a little steeper.
const EXAGGERATION: f32 = 1.5;
/// Most vertices of the ground: about 30 ms for a picture of 1200 × 800.
const CELLS: usize = 300 * 300;
/// Radians turned per pixel dragged.
const TURN: f64 = 0.006;
/// Zoom of the tiles at the most.
const MAX_ZOOM: u8 = 12;

pub(super) struct Scene {
    /// The loop and theme it was made for.
    key: u64,
    terrain: Arc<Terrain>,
    colors: Arc<Vec<[u8; 3]>>,
    pub(super) cam: Camera,
    image: Option<Arc<RenderImage>>,
    busy: bool,
    drawn: Option<u64>,
    /// Where the pointer was at the last move of a drag.
    drag: Option<Point<Pixels>>,
}

/// The box around a loop, with a margin of a fifth of it (at least 650 m), `[south, west, north,
/// east]`, and the tile zoom whose pixels are about as fine as the ground's cells.
fn around(line: &[[f64; 2]]) -> Option<([f64; 4], u8)> {
    let (mut s, mut w, mut n, mut e) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in line {
        (s, w, n, e) = (s.min(p[0]), w.min(p[1]), n.max(p[0]), e.max(p[1]));
    }
    if s > n {
        return None;
    }
    let cos = ((s + n) / 2.0).to_radians().cos();
    let (my, mx) = (((n - s) / 5.0).max(0.006), ((e - w) / 5.0).max(0.006 / cos));
    let bbox = [s - my, w - mx, n + my, e + mx];
    let span = ((bbox[2] - bbox[0]) * 110_540.0).max((bbox[3] - bbox[1]) * 111_320.0 * cos);
    let cell = span / (CELLS as f64).sqrt();
    let z = (156_543.0 * cos / cell).log2().ceil().clamp(8.0, MAX_ZOOM as f64) as u8;
    Some((bbox, z))
}

/// The tiles of `zoom` that `bbox` needs, a pixel's margin included.
fn tiles(bbox: [f64; 4], zoom: u8) -> Vec<(u32, u32)> {
    let (x0, y0) = world(bbox[2], bbox[1], zoom as f64);
    let (x1, y1) = world(bbox[0], bbox[3], zoom as f64);
    let max = (1u32 << zoom) - 1;
    let t = |v: f64| ((v / 256.0).floor().max(0.0) as u32).min(max);
    (t(y0 - 1.0)..=t(y1 + 1.0)).flat_map(|y| (t(x0 - 1.0)..=t(x1 + 1.0)).map(move |x| (x, y))).collect()
}

/// The ground's colour at a height: grey, darker as it rises (lighter in the dark theme), the sea
/// a shade apart. Bytes in the picture's order; brightened so that flat ground shows them as is.
fn tint(h: f32, lo: f32, hi: f32, dark: bool) -> [u8; 3] {
    let v = if h <= 0.5 {
        if dark { 44.0 } else { 208.0 }
    } else {
        let t = ((h - lo.max(0.0)) / (hi - lo.max(0.0)).max(1.0)).clamp(0.0, 1.0);
        if dark { 70.0 + 80.0 * t } else { 238.0 - 78.0 * t }
    };
    let v = (v / terrain::FLAT_LIT).round().min(255.0) as u8;
    [v, v, v]
}

/// A colour in the picture's byte order (blue, green, red).
fn bytes(c: Hsla) -> [u8; 3] {
    let c = Rgba::from(c);
    [c.b, c.g, c.r].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

impl Scene {
    /// The ground of a loop from the elevation tiles, seen whole beside the panel `width` wide;
    /// `None` while a tile is missing.
    pub(super) fn of(l: &Loop, key: u64, dark: bool, width: f64, tile: &mut dyn FnMut(u8, u32, u32) -> Option<Arc<Vec<f32>>>) -> Option<Scene> {
        let (bbox, z) = around(&l.line)?;
        // Every tile asked for at once, so that they come together; the scene waits for them all.
        let got: Vec<_> = tiles(bbox, z).into_iter().map(|(x, y)| ((x, y), tile(z, x, y))).collect();
        let needed: std::collections::HashMap<(u32, u32), Arc<Vec<f32>>> = got.into_iter().map(|(k, t)| Some((k, t?))).collect::<Option<_>>()?;
        let t = Terrain::from_tiles(bbox, CELLS, z, &|x, y| needed.get(&(x, y)).cloned())?;
        let (lo, hi) = t.heights.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        let colors = t.heights.iter().map(|h| tint(*h, lo, hi, dark)).collect();
        let mut cam = Camera::fit(&t);
        // The panel hides the left: the ground is seen in the rest.
        cam.pan_screen(&t, EXAGGERATION, (PANEL + 28.0) / 2.0, 0.0, width.max(PANEL * 2.0));
        cam.lift(&t, EXAGGERATION);
        Some(Scene { key, terrain: Arc::new(t), colors: Arc::new(colors), cam, image: None, busy: false, drawn: None, drag: None })
    }

    /// Where a point of the loop is on the ground, local metres and altitude.
    fn place(&self, p: [f64; 2]) -> [f64; 3] {
        let q = self.terrain.local(p[0], p[1]);
        [q[0], q[1], self.terrain.height_at(q[0], q[1])]
    }

    /// Screen position of a point of the loop in a `w` × `h` view.
    fn screen(&self, p: [f64; 2], w: f64, h: f64) -> Option<(f32, f32)> {
        terrain::project(&self.terrain, &self.cam, EXAGGERATION, self.place(p), w as usize, h as usize)
    }
}

impl LoopsView {
    /// The scene for the chosen loop, made again when the loop or the theme changed.
    fn scene_key(&self, l: &Loop) -> u64 {
        l.line.len() as u64 ^ l.dist.to_bits() ^ l.up.to_bits().rotate_left(17) ^ self.snap.theme.dark as u64
    }

    fn ensure_scene(&mut self, l: &Loop, width: f64, cx: &mut Context<Self>) {
        let key = self.scene_key(l);
        if self.scene.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        let data = self.data.clone();
        let mut tile = |z: u8, x: u32, y: u32| data.update(cx, |d, cx| d.relief_tile((z, x, y), cx));
        if let Some(s) = Scene::of(l, key, self.snap.theme.dark, width, &mut tile) {
            self.scene = Some(s);
        }
    }

    /// Draws the scene again in the background when the eye, the line or the size changed; at
    /// half size while the pointer turns it. One drawing at a time, the latest wins.
    fn ensure_picture(&mut self, l: &Loop, size: (f64, f64), cx: &mut Context<Self>) {
        let t = self.snap.theme;
        let ramp = slope_ramp(&t).map(bytes);
        let classes = self.show_slope.then(|| slope::runs(&l.along, &l.eles));
        let climb = self.climb_hover.and_then(|i| slope::climbs(&l.along, &l.eles).get(i).map(|c| c.from..c.to));
        let (accent, ink, bg) = (bytes(t.accent), bytes(t.text), bytes(t.bg));
        let Some(s) = self.scene.as_mut() else { return };
        let half = s.drag.is_some();
        let (w, h) = if half { ((size.0 / 2.0) as usize, (size.1 / 2.0) as usize) } else { (size.0 as usize, size.1 as usize) };
        if w < 8 || h < 8 {
            return;
        }
        let mut colour = vec![accent; l.line.len()];
        for (i, j, k) in classes.iter().flatten() {
            colour[*i..*j].fill(ramp[*k as usize]);
        }
        if let Some(r) = climb {
            colour[r].fill(ink);
        }
        let line: Vec<([f64; 3], [u8; 3])> = l.line.iter().zip(colour).map(|(p, c)| (s.place(*p), c)).collect();
        let key = {
            let mut hsh = std::collections::hash_map::DefaultHasher::new();
            let c = s.cam;
            (c.target.map(f64::to_bits), c.yaw.to_bits(), c.pitch.to_bits(), c.dist.to_bits(), w, h, s.key).hash(&mut hsh);
            line.iter().for_each(|(_, c)| c.hash(&mut hsh));
            hsh.finish()
        };
        if s.drawn == Some(key) || s.busy {
            return;
        }
        s.busy = true;
        let (terrain, colors, cam) = (s.terrain.clone(), s.colors.clone(), s.cam);
        cx.spawn(async move |this, cx| {
            let img = cx
                .background_executor()
                .spawn(async move {
                    let px = terrain::render(&terrain, &colors, &cam, EXAGGERATION, &line, w, h, [bg[0], bg[1], bg[2], 255]);
                    render_image(w as u32, h as u32, px).map(Arc::new)
                })
                .await;
            this.update(cx, |v, cx| {
                if let Some(s) = v.scene.as_mut() {
                    s.busy = false;
                    s.drawn = Some(key);
                    if img.is_some() {
                        s.image = img;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The chosen loop in 3D, filling the page: drag to turn it, shift-drag to move it, the wheel
    /// to come closer. The pointer over the line shows where on the profile.
    pub(super) fn view_3d(&mut self, l: &Loop, cx: &mut Context<Self>) -> AnyElement {
        let t = self.snap.theme;
        let bounds = self.plots.get("loops-3d");
        let b = bounds.get();
        let dims = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
        self.ensure_scene(l, dims.0, cx);
        self.ensure_picture(l, dims, cx);
        let cell = bounds.clone();
        let base = div().id("loops-3d").absolute().size_full().bg(t.bg).child(canvas(move |b, _, _| cell.set(b), |_, _, _, _| {}).absolute().size_full());
        let Some(s) = self.scene.as_ref() else {
            let free = div().absolute().top_0().bottom_0().right_0().left(px(PANEL as f32 + 28.0)).flex().items_center().justify_center();
            return base.child(free.text_size(px(13.)).text_color(t.soft).child(tr("Loading the relief…", "Chargement du relief…"))).into_any_element();
        };
        let marks: Vec<((f32, f32), bool)> = [self.start.map(|p| [p.0, p.1]), self.hover.and_then(|i| l.line.get(i).copied())]
            .into_iter()
            .enumerate()
            .filter_map(|(k, p)| Some((s.screen(p?, dims.0, dims.1)?, k == 1)))
            .collect();
        let (accent, ink, bg) = (t.accent, t.text, t.bg);
        base.children(s.image.clone().map(|i| img(i).absolute().size_full().into_any_element()))
            .child(
                canvas(
                    |_, _, _| {},
                    move |b, _, window, _| {
                        let dot = |(x, y): (f32, f32), r: f32, c: Hsla, window: &mut Window| {
                            let p = point(b.origin.x + px(x - r), b.origin.y + px(y - r));
                            window.paint_quad(fill(Bounds::new(p, size(px(2. * r), px(2. * r))), c).corner_radii(px(r)));
                        };
                        for (p, hover) in &marks {
                            dot(*p, 8.0, bg, window);
                            dot(*p, 6.0, if *hover { accent } else { ink }, window);
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| {
                if let Some(s) = this.scene.as_mut() {
                    s.drag = Some(e.position);
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| this.move_3d(e, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _: &MouseUpEvent, _, cx| {
                if let Some(s) = this.scene.as_mut() {
                    s.drag = None;
                    cx.notify();
                }
            }))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                let dy = (e.delta.pixel_delta(px(20.)).y / px(1.)) as f64;
                if let Some(s) = this.scene.as_mut() {
                    s.cam.zoom(2f64.powf(-dy / 240.0));
                    s.cam.lift(&s.terrain, EXAGGERATION);
                    cx.notify();
                }
            }))
            .into_any_element()
    }

    fn move_3d(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        let b = self.plots.get("loops-3d").get();
        let size = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
        let found = self.snap_found(cx);
        let Some(s) = self.scene.as_mut() else { return };
        match s.drag.filter(|_| e.pressed_button == Some(MouseButton::Left)) {
            Some(from) => {
                let (dx, dy) = (((e.position.x - from.x) / px(1.)) as f64, ((e.position.y - from.y) / px(1.)) as f64);
                if e.modifiers.shift {
                    s.cam.pan_screen(&s.terrain, EXAGGERATION, dx, dy, size.0);
                } else {
                    s.cam.orbit(-dx * TURN, dy * TURN);
                    s.cam.lift(&s.terrain, EXAGGERATION);
                }
                s.drag = Some(e.position);
                cx.notify();
            }
            None => {
                s.drag = None;
                // The point of the line nearest the pointer, if near enough.
                let (x, y) = ((e.position.x - b.origin.x) / px(1.), (e.position.y - b.origin.y) / px(1.));
                let near = found.get(self.selected).and_then(|l| {
                    l.line
                        .iter()
                        .enumerate()
                        .filter_map(|(i, p)| s.screen(*p, size.0, size.1).map(|(sx, sy)| ((sx - x).powi(2) + (sy - y).powi(2), i)))
                        .filter(|(d, _)| *d <= super::GRAB * super::GRAB)
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .map(|(_, i)| i)
                });
                if near != self.hover {
                    self.hover = near;
                    cx.notify();
                }
            }
        }
    }

    /// The 3D view's bounds as laid out.
    #[cfg(test)]
    pub fn bounds_3d(&mut self) -> Bounds<Pixels> {
        self.plots.get("loops-3d").get()
    }

    /// Makes the scene of the chosen loop from these tiles, for the tests (which never go online).
    #[cfg(test)]
    pub fn ground_from(&mut self, tile: &mut dyn FnMut(u8, u32, u32) -> Option<Arc<Vec<f32>>>, cx: &mut Context<Self>) {
        let found = self.snap_found(cx);
        let l = &found[self.selected];
        let width = (self.plots.get("loops-3d").get().size.width / px(1.)) as f64;
        let key = self.scene_key(l);
        self.scene = Scene::of(l, key, false, width, tile);
    }

    #[cfg(test)]
    pub fn camera(&self) -> Option<Camera> {
        self.scene.as_ref().map(|s| s.cam)
    }

    /// Where a point of the chosen loop's line is drawn in 3D, for the tests' pointer.
    #[cfg(test)]
    pub fn screen_3d(&mut self, i: usize, cx: &App) -> Option<Point<Pixels>> {
        let b = self.plots.get("loops-3d").get();
        let p = self.snap_found(cx).get(self.selected)?.line.get(i).copied()?;
        let (x, y) = self.scene.as_ref()?.screen(p, (b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64)?;
        Some(point(b.origin.x + px(x), b.origin.y + px(y)))
    }

    #[cfg(test)]
    pub fn show_3d(&mut self, on: bool) {
        self.three_d = on;
    }
}

#[cfg(test)]
mod tests {
    use super::{around, terrain, tiles, tint, world};

    #[test]
    fn the_box_and_its_tiles_cover_the_loop() {
        let (bbox, z) = around(&[[43.70, 7.25], [43.72, 7.27], [43.71, 7.29]]).unwrap();
        assert!(bbox[0] < 43.70 && bbox[2] > 43.72 && bbox[1] < 7.25 && bbox[3] > 7.29);
        // About 2.2 × 3.2 km, a fifth more each side: cells of about 15 m, tiles of zoom 12.
        assert_eq!(z, 12);
        let t = tiles(bbox, z);
        let (x, y) = world(43.71, 7.27, z as f64);
        assert!(t.contains(&((x / 256.0) as u32, (y / 256.0) as u32)));
        assert!(t.len() <= 9, "{t:?}");
        // A single point still has a box; no line, none.
        assert!(around(&[[43.7, 7.25]]).is_some_and(|(b, _)| b[2] - b[0] > 0.011));
        assert!(around(&[]).is_none());
        // A long loop takes coarser tiles.
        assert!(around(&[[43.5, 7.0], [43.9, 7.5]]).unwrap().1 < 12);
    }

    #[test]
    fn the_ground_darkens_as_it_rises() {
        let lit = |c: [u8; 3]| (c[0] as f32 * terrain::FLAT_LIT).round();
        assert!((lit(tint(0.0, 0.0, 500.0, false)) - 208.0).abs() <= 1.0);
        assert!(tint(10.0, 0.0, 500.0, false)[0] > tint(400.0, 0.0, 500.0, false)[0]);
        assert!(tint(10.0, 0.0, 500.0, true)[0] < tint(400.0, 0.0, 500.0, true)[0]);
        // Flat land, or below the sea: no division by nothing.
        assert_eq!(tint(100.0, 100.0, 100.0, false), tint(100.0, 0.0, 1.0e6, false));
        assert_eq!(tint(-5.0, -5.0, 300.0, false), tint(0.0, 0.0, 300.0, false));
    }
}
