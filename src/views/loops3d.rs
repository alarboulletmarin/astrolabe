//! The chosen loop over its ground, in 3D: the ground made from the elevation tiles, its picture
//! drawn in the background, and the pointer that turns it. Editing the loop stays on the map.

use super::{LoopsView, PANEL, slope_ramp};
use crate::lang::tr;
use crate::logic::loops::Loop;
use crate::logic::map::world;
use crate::logic::slope;
use crate::logic::buildings::{self, Building};
use crate::logic::graph::Graph;
use crate::logic::osm::Highway;
use crate::logic::terrain::{self, Camera, Face, Inset, Shade, Terrain, Texture};
use crate::ui::render_image;
use gpui_kit::*;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Heights drawn a fifth higher: the slopes read better a little steeper, and no more is
/// needed now that the light follows every fold.
const EXAGGERATION: f32 = 1.2;
/// Most vertices of the ground: about 26 ms for a picture of 1200 × 800.
const CELLS: usize = 500 * 500;
/// Radians turned per pixel dragged.
const TURN: f64 = 0.006;
/// Zoom of the elevation tiles at the most: about 14 m a pixel here, finer than what they were
/// made from (about 30 m).
const MAX_ZOOM: u8 = 13;
/// Share of the ground, from each edge, that fades into the page: no cut sides.
const FADE: f32 = 0.16;
/// The map laid on the ground is at least this many pixels along its longer side (and less
/// than twice as many): about one per pixel of the screen.
const MAP_PIXELS: f64 = 1024.0;
/// The finer photos laid where the eye is close: this many pixels a side each, of a zoom up to
/// about 60 cm a pixel; at most `LEVELS` of them, each twice as coarse and twice as wide as the
/// one before, from the ground nearest the eye outwards.
const INSET_PIXELS: f64 = 1536.0;
const INSET_MAX_ZOOM: u8 = 18;
const LEVELS: i32 = 4;
/// The grey ground and its roads, before the map's tiles: this many texels along its longer side.
const GROUND_PIXELS: usize = 1536;
/// The loop fills this share of the free area's width, or of its height.
const FILL: f64 = 0.8;

/// What an inset was made for: its zoom, the point (local metres) it is centred on, and whether
/// the photos were all there (otherwise: the roads over the grey ground).
type InsetAt = (u8, [f64; 2], bool);

/// The finer photos the eye wants, finest first: each its zoom, the point (local metres) it is
/// centred on and its half side in metres. The finest is as fine as the screen where the ground
/// is nearest the eye (the bottom of the view); each next one twice as coarse and twice as wide,
/// down to the photo over the whole ground (`drape`). Each lies mostly ahead of that ground: the
/// view is in front of it.
fn levels(cam: &Camera, cos: f64, drape: u8, height: f64) -> Vec<(u8, [f64; 2], f64)> {
    let below = (cam.pitch + cam.fov / 2.0).min(89f64.to_radians());
    let up = cam.dist * cam.pitch.sin();
    let (sy, cy) = cam.yaw.sin_cos();
    let back = cam.dist * cam.pitch.cos() - up / below.tan();
    let near = [cam.target[0] - sy * back, cam.target[1] - cy * back];
    let per_px = up / below.sin() * 2.0 * (cam.fov / 2.0).tan() / height.max(1.0);
    let finest = ((156_543.0 * cos / per_px).log2().ceil() as i32).min(INSET_MAX_ZOOM as i32);
    (0..LEVELS)
        .map(|k| finest - k)
        .take_while(|z| *z > drape as i32)
        .map(|z| {
            let half = INSET_PIXELS / 2.0 * 156_543.0 * cos / 2f64.powi(z);
            (z as u8, [near[0] + sy * half / 2.0, near[1] + cy * half / 2.0], half)
        })
        .collect()
}

/// How wide a road is drawn on the 3D ground, in metres, and whether it is a path (drawn thin,
/// in a darker grey, as on the map).
fn road_width(h: Highway) -> (f64, bool) {
    match h {
        Highway::Primary => (12.0, false),
        Highway::Secondary => (10.0, false),
        Highway::Tertiary => (9.0, false),
        Highway::Residential | Highway::Unclassified | Highway::LivingStreet => (7.0, false),
        Highway::Pedestrian => (6.0, false),
        Highway::Service => (4.5, false),
        Highway::Cycleway | Highway::Track => (3.0, true),
        Highway::Path | Highway::Footway | Highway::Bridleway | Highway::Steps => (2.0, true),
    }
}

/// The roads of `g` over `bbox`, as lines to stroke on a picture of it (`Texture::stroke`) whose
/// texels are `texel` metres: the roads' edges, then the paths, then the roads.
fn roads(g: &Graph, bbox: [f64; 4], texel: f64, dark: bool) -> Vec<terrain::Stroke> {
    let (fill, edge, path) = if dark { ([92; 3], [22; 3], [120; 3]) } else { ([255; 3], [176; 3], [140; 3]) };
    let (du, dv) = (bbox[3] - bbox[1], bbox[2] - bbox[0]);
    let (mut edges, mut paths, mut fills) = (Vec::new(), Vec::new(), Vec::new());
    for e in g.edges_in(bbox) {
        let pts: Vec<[f32; 2]> = g.line(e).iter().map(|(p, _)| [((p[1] as f64 / 1e7 - bbox[1]) / du) as f32, ((p[0] as f64 / 1e7 - bbox[0]) / dv) as f32]).collect();
        let (m, thin) = road_width(g.edges[e as usize].class.highway());
        let w = (m / texel) as f32;
        if thin {
            paths.push((pts, w, path));
        } else {
            if w >= 2.0 {
                edges.push((pts.clone(), w + 2.0, edge));
            }
            fills.push((pts, w, fill));
        }
    }
    edges.extend(paths);
    edges.extend(fills);
    edges
}

/// `p`, a picture of `bbox` `width` metres across, with the roads of `g` drawn on it.
fn with_roads(mut p: Texture, g: Option<&Graph>, bbox: [f64; 4], width: f64, dark: bool) -> Texture {
    if let Some(g) = g {
        p.stroke(&roads(g, bbox, width / p.w.max(1) as f64, dark));
    }
    p
}

/// Buildings stand within this many metres of the loop.
const BESIDE: f64 = 400.0;
/// They are drawn once the eye is this close to what it looks at, in metres, and then within
/// that distance of it: from further away they are specks, and too many.
const BUILDINGS_FROM: f64 = 3000.0;

/// The buildings beside the loop, as blocks ready to draw.
struct Blocks {
    faces: Vec<Face>,
    /// Each block: where it stands (local metres) and its faces in `faces`.
    at: Vec<([f32; 2], std::ops::Range<usize>)>,
}

impl Blocks {
    /// The blocks of `tiles` that stand beside `line`, on the ground `t`; walls and roofs in
    /// these colours (the picture's byte order).
    fn beside(line: &[[f64; 2]], tiles: &[Arc<Vec<Building>>], t: &Terrain, wall: [u8; 3], roof: [u8; 3]) -> Blocks {
        // The cells (of `BESIDE` metres) the line goes through.
        let cell = |p: [f64; 2]| ((p[0] / BESIDE).floor() as i32, (p[1] / BESIDE).floor() as i32);
        let along: std::collections::HashSet<(i32, i32)> = line.iter().map(|p| cell(t.local(p[0], p[1]))).collect();
        let mut out = Blocks { faces: Vec::new(), at: Vec::new() };
        for b in tiles.iter().flat_map(|t| t.iter()) {
            let ring: Vec<[f64; 2]> = b.ring.iter().map(|p| t.local(p[0] as f64 / 1e7, p[1] as f64 / 1e7)).collect();
            let Some(first) = ring.first() else { continue };
            let (cx, cy) = cell(*first);
            if !(-1..=1).any(|dx| (-1..=1).any(|dy| along.contains(&(cx + dx, cy + dy)))) {
                continue;
            }
            // From the lowest ground under it (no gap on a slope) to its height over the highest;
            // heights are drawn higher with the ground's, a building's own is not.
            let (lo, hi) = ring.iter().map(|p| t.height_at(p[0], p[1])).fold((f64::MAX, f64::MIN), |(lo, hi), h| (lo.min(h), hi.max(h)));
            let flat: Vec<[f32; 2]> = ring.iter().map(|p| [p[0] as f32, p[1] as f32]).collect();
            let faces = terrain::extrude(&flat, lo as f32, hi as f32 + b.height / EXAGGERATION, wall, roof);
            if !faces.is_empty() {
                out.at.push(([first[0] as f32, first[1] as f32], out.faces.len()..out.faces.len() + faces.len()));
                out.faces.extend(faces);
            }
        }
        out
    }

    /// The faces to draw for an eye: none from afar, those around what it looks at from close.
    fn seen(&self, cam: &Camera) -> Vec<Face> {
        if cam.dist > BUILDINGS_FROM {
            return Vec::new();
        }
        let (x, y, r2) = (cam.target[0] as f32, cam.target[1] as f32, (cam.dist * cam.dist) as f32);
        self.at.iter().filter(|(p, _)| (p[0] - x).powi(2) + (p[1] - y).powi(2) <= r2).flat_map(|(_, r)| &self.faces[r.clone()]).copied().collect()
    }
}

/// Flying over the loop: the eye this far from the point it follows, this high over the horizon,
/// a picture every `FLY_TICK`; the whole loop in about a minute, within these speeds (m/s).
const FLY_DIST: f64 = 900.0;
const FLY_PITCH: f64 = 0.5;
const FLY_TICK: std::time::Duration = std::time::Duration::from_millis(33);
const FLY_SPEED: (f64, f64) = (60.0, 300.0);
/// The eye looks this many metres ahead along the line, and turns to it at this rate (per second).
const FLY_AHEAD: f64 = 200.0;
const FLY_TURN: f64 = 2.5;

/// A flight over the loop: how far along it (metres), and whether it goes on.
struct Fly {
    at: f64,
    playing: bool,
    /// Which run of the clock moves it: a flight started again leaves the old clock behind.
    run: u64,
}

/// Where a flight is `at` metres along a line (`along`: metres from the start at each point):
/// the point before it and the share of the way to the next one.
fn fly_at(along: &[f32], at: f64) -> (usize, f64) {
    let i = along.partition_point(|a| (*a as f64) <= at).saturating_sub(1).min(along.len().saturating_sub(2));
    let (a, b) = (along.get(i).copied().unwrap_or(0.0) as f64, along.get(i + 1).copied().unwrap_or(0.0) as f64);
    (i, if b > a { ((at - a) / (b - a)).clamp(0.0, 1.0) } else { 0.0 })
}

/// The shortest turn from one heading to another, radians within ±π.
fn turn(from: f64, to: f64) -> f64 {
    (to - from + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

pub(super) struct Scene {
    /// The loop and theme it was made for.
    key: u64,
    terrain: Arc<Terrain>,
    /// The same ground on a quarter of the triangles: what is drawn while the eye moves.
    rough: Arc<Terrain>,
    /// The ground's box, `[south, west, north, east]`.
    bbox: [f64; 4],
    /// The light on the ground, from the elevation tiles at their full resolution.
    shade: Arc<Shade>,
    /// The ground grey by altitude (`base`, unlit); laid on it and lit (`texture`): the roads of
    /// the region's map until the photos are all there, then the photos (`draped`).
    base: Arc<Texture>,
    texture: Arc<Texture>,
    draped: bool,
    /// The finer photos where the eye is close (see `levels`), each with what it was made for;
    /// and whether one is being made.
    insets: Vec<(Arc<Inset>, InsetAt)>,
    inset_busy: bool,
    /// The buildings beside the loop, once read (`None` until then, and in the tests).
    blocks: Option<Arc<Blocks>>,
    blocks_busy: bool,
    pub(super) cam: Camera,
    image: Option<Arc<RenderImage>>,
    /// Pictures shown before, to be taken off the window's atlas (each would hold its memory on
    /// the graphics card until then).
    pub(super) stale: Vec<Arc<RenderImage>>,
    /// The eye and the size `image` was drawn with. While the pointer moves the ground the picture
    /// on screen is a drawing late; what is laid over it is placed with these, not with `cam`.
    shown: Option<(Camera, usize, usize)>,
    busy: bool,
    drawn: Option<u64>,
    /// Where the pointer was at the last move of a drag, and whether it turns the ground
    /// (otherwise it moves it).
    drag: Option<(Point<Pixels>, bool)>,
    /// The view's size the loop was framed for, and whether the eye was moved by hand since:
    /// until then, a view that changes size (a window being laid out, resized) is framed again.
    framed: (f64, f64),
    moved: bool,
    fly: Option<Fly>,
}

/// The box of a loop, `[south, west, north, east]`.
fn bounds(line: &[[f64; 2]]) -> Option<[f64; 4]> {
    let (mut s, mut w, mut n, mut e) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in line {
        (s, w, n, e) = (s.min(p[0]), w.min(p[1]), n.max(p[0]), e.max(p[1]));
    }
    (s <= n).then_some([s, w, n, e])
}

/// Metres along the longer side of a box, and the cosine of its latitude.
fn span(bbox: [f64; 4]) -> (f64, f64) {
    let cos = ((bbox[0] + bbox[2]) / 2.0).to_radians().cos();
    (((bbox[2] - bbox[0]) * 110_540.0).max((bbox[3] - bbox[1]) * 111_320.0 * cos), cos)
}

/// The ground around a loop's box: the loop's country on every side (half the box's longer side,
/// at least 1.5 km; its edges fade away), and the zoom of its elevation tiles: as fine as the
/// map's, which is finer than the ground's cells (the light is worked out tile pixel by tile
/// pixel), within what the tiles are worth.
fn around([s, w, n, e]: [f64; 4]) -> ([f64; 4], u8) {
    let (long, cos) = span([s, w, n, e]);
    let m = (long / 2.0).max(1500.0) / 110_540.0;
    let bbox = [s - m, w - m / cos, n + m, e + m / cos];
    (bbox, map_zoom(bbox).min(MAX_ZOOM))
}

/// The zoom of the map tiles laid on the ground.
fn map_zoom(bbox: [f64; 4]) -> u8 {
    let (long, cos) = span(bbox);
    (MAP_PIXELS * 156_543.0 * cos / long).log2().ceil().clamp(5.0, 16.0) as u8
}

/// The tiles of `zoom` that `bbox` needs, a pixel's margin included.
fn tiles(bbox: [f64; 4], zoom: u8) -> Vec<(u32, u32)> {
    let (x0, y0) = world(bbox[2], bbox[1], zoom as f64);
    let (x1, y1) = world(bbox[0], bbox[3], zoom as f64);
    let max = (1u32 << zoom) - 1;
    let t = |v: f64| ((v / 256.0).floor().max(0.0) as u32).min(max);
    (t(y0 - 1.0)..=t(y1 + 1.0)).flat_map(|y| (t(x0 - 1.0)..=t(x1 + 1.0)).map(move |x| (x, y))).collect()
}

/// The ground's colour at a height while the photos are awaited, and where there is none: grey,
/// darker as it rises (lighter in the dark theme); the sea of the blue of the photos' (out at
/// sea they have none), darker in the dark theme as they are. Blue, green, red.
fn tint(h: f32, lo: f32, hi: f32, dark: bool) -> [u8; 3] {
    if h <= 0.5 {
        return if dark { [45, 30, 20] } else { [63, 41, 26] };
    }
    let v = {
        let t = ((h - lo.max(0.0)) / (hi - lo.max(0.0)).max(1.0)).clamp(0.0, 1.0);
        if dark { 70.0 + 80.0 * t } else { 238.0 - 78.0 * t }
    };
    [v.round() as u8; 3]
}

/// A colour in the picture's byte order (blue, green, red).
fn bytes(c: Hsla) -> [u8; 3] {
    let c = Rgba::from(c);
    [c.b, c.g, c.r].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

impl Scene {
    /// The ground of a loop from the elevation tiles, the loop seen whole in what the panel leaves
    /// of a view of `size` (see `frame`); `None` while a tile is missing.
    pub(super) fn of(line: &[[f64; 2]], key: u64, dark: bool, size: (f64, f64), tile: &mut dyn FnMut(u8, u32, u32) -> Option<Arc<Vec<f32>>>) -> Option<Scene> {
        let inner = bounds(line)?;
        let (bbox, z) = around(inner);
        // Every tile asked for at once, so that they come together; the scene waits for them all.
        let got: Vec<_> = tiles(bbox, z).into_iter().map(|(x, y)| ((x, y), tile(z, x, y))).collect();
        let needed: std::collections::HashMap<(u32, u32), Arc<Vec<f32>>> = got.into_iter().map(|(k, t)| Some((k, t?))).collect::<Option<_>>()?;
        let t = Terrain::from_tiles(bbox, CELLS, z, &|x, y| needed.get(&(x, y)).cloned())?;
        let (lo, hi) = t.heights.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        let shade = Shade::from_tiles(bbox, z, EXAGGERATION, &|x, y| needed.get(&(x, y)).cloned())?;
        let base = Texture { w: t.cols, h: t.rows, px: t.heights.iter().map(|h| tint(*h, lo, hi, dark)).collect(), fade: FADE };
        let texture = base.clone().lit(&shade);
        let cam = Camera::fit(&t);
        let mut s = Scene { key, rough: Arc::new(t.coarser()), terrain: Arc::new(t), stale: Vec::new(), bbox, shade: Arc::new(shade), base: Arc::new(base), texture: Arc::new(texture), draped: false, insets: Vec::new(), inset_busy: false, blocks: None, blocks_busy: false, cam, image: None, shown: None, busy: false, drawn: None, drag: None, framed: size, moved: false, fly: None };
        s.frame(line, size);
        Some(s)
    }

    /// The ground's size, metres east and north.
    fn extent(&self) -> (f64, f64) {
        let t = &self.terrain;
        ((t.cols - 1) as f64 * t.dx, (t.rows - 1) as f64 * t.dy)
    }

    /// Frames the loop again for a view that changed size, unless the eye was moved by hand.
    fn follow_size(&mut self, line: &[[f64; 2]], size: (f64, f64)) {
        if !self.moved && self.framed != size && size.0 >= 8.0 && size.1 >= 8.0 {
            self.framed = size;
            self.frame(line, size);
        }
    }

    /// Puts the eye so that the loop, rather than the whole ground (which runs out of the view),
    /// is in the middle of what the panel leaves of a view of `size`, and fills it.
    fn frame(&mut self, line: &[[f64; 2]], size: (f64, f64)) {
        let (w, h) = (size.0.max(PANEL * 2.0), size.1.max(200.0));
        let (left, free) = (PANEL + 28.0, w - PANEL - 28.0);
        let pts: Vec<[f64; 3]> = line.iter().map(|p| self.place(*p)).collect();
        // Each round measures the loop on screen, then backs off and slides to fit; a few settle.
        for _ in 0..12 {
            let seen = pts.iter().filter_map(|p| terrain::project(&self.terrain, &self.cam, EXAGGERATION, *p, w as usize, h as usize));
            let (x0, y0, x1, y1) = seen.fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |b, (x, y)| (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y)));
            if x0 > x1 {
                break;
            }
            let (x0, y0, x1, y1) = (x0 as f64, y0 as f64, x1 as f64, y1 as f64);
            let k = ((x1 - x0) / (free * FILL)).max((y1 - y0) / (h * FILL)).max(0.05);
            self.cam.dist = (self.cam.dist * k).max(self.cam.min_dist);
            // Backing off shrinks the picture around the view's centre.
            let (cx, cy) = (w / 2.0 + ((x0 + x1) / 2.0 - w / 2.0) / k, h / 2.0 + ((y0 + y1) / 2.0 - h / 2.0) / k);
            self.cam.pan_screen(&self.terrain, EXAGGERATION, left + free / 2.0 - cx, h / 2.0 - cy, h);
        }
        self.cam.lift(&self.terrain, EXAGGERATION);
    }

    /// Puts the eye over the loop `at` metres along it, looking the way it goes; `dt` seconds
    /// have passed since the last time (the eye turns at its own pace, not with every bend).
    /// Returns the point of the line it is at.
    fn fly_to(&mut self, l: &Loop, at: f64, dt: f64) -> usize {
        let on = |at: f64| {
            let (i, f) = fly_at(&l.along, at);
            let (a, b) = (self.place(l.line[i]), self.place(l.line[(i + 1).min(l.line.len() - 1)]));
            [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * f)
        };
        let total = l.along.last().copied().unwrap_or(0.0) as f64;
        let here = on(at);
        // Ahead; at the very end, the way it came.
        let (from, to) = if at + FLY_AHEAD <= total { (here, on(at + FLY_AHEAD)) } else { (on((total - FLY_AHEAD).max(0.0)), on(total)) };
        self.cam.target = here;
        if (to[0] - from[0]).hypot(to[1] - from[1]) > 1.0 {
            let heading = (to[0] - from[0]).atan2(to[1] - from[1]);
            self.cam.orbit(turn(self.cam.yaw, heading) * (dt * FLY_TURN).min(1.0), 0.0);
        }
        self.cam.lift(&self.terrain, EXAGGERATION);
        self.moved = true;
        fly_at(&l.along, at).0
    }

    /// Where a point of the loop is on the ground, local metres and altitude.
    fn place(&self, p: [f64; 2]) -> [f64; 3] {
        let q = self.terrain.local(p[0], p[1]);
        [q[0], q[1], self.terrain.height_at(q[0], q[1])]
    }

    /// Screen position of a point of the loop in a `w` × `h` view, on the picture as it is
    /// shown there (stretched to the view if it was drawn for another size).
    fn screen(&self, p: [f64; 2], w: f64, h: f64) -> Option<(f32, f32)> {
        let (cam, pw, ph) = self.shown.unwrap_or((self.cam, w as usize, h as usize));
        let (x, y) = terrain::project(&self.terrain, &cam, EXAGGERATION, self.place(p), pw, ph)?;
        Some((x * w as f32 / pw.max(1) as f32, y * h as f32 / ph.max(1) as f32))
    }
}

impl LoopsView {
    /// The scene for the chosen loop, made again when the loop or the theme changed.
    fn scene_key(&self, l: &Loop) -> u64 {
        l.line.len() as u64 ^ l.dist.to_bits() ^ l.up.to_bits().rotate_left(17) ^ self.snap.theme.dark as u64
    }

    fn ensure_scene(&mut self, l: &Loop, size: (f64, f64), cx: &mut Context<Self>) {
        let key = self.scene_key(l);
        if self.scene.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        // Not laid out yet: the loop can't be framed. Once more, with the view's size.
        if size.0 < 8.0 || size.1 < 8.0 {
            cx.spawn(async move |this, cx| this.update(cx, |_, cx| cx.notify()).ok()).detach();
            return;
        }
        let data = self.data.clone();
        let mut tile = |z: u8, x: u32, y: u32| data.update(cx, |d, cx| d.relief_tile((z, x, y), cx));
        if let Some(mut s) = Scene::of(&l.line, key, self.snap.theme.dark, size, &mut tile) {
            // The last loop's picture goes off the atlas with the next frame.
            if let Some(old) = self.scene.take() {
                s.stale.extend(old.stale.into_iter().chain(old.image));
            }
            self.scene = Some(s);
            self.paint_roads(cx);
        }
    }

    /// The roads, from the region's own map, on the grey ground (about 90 ms, in the background):
    /// the plain ground, and the photos' until they are all there.
    fn paint_roads(&mut self, cx: &mut Context<Self>) {
        let (Some(g), Some(s)) = (self.data.read(cx).loops.graph.clone(), self.scene.as_ref()) else { return };
        let (gw, gh) = s.extent();
        let (w, h) = if gw >= gh { (GROUND_PIXELS, (GROUND_PIXELS as f64 * gh / gw) as usize) } else { ((GROUND_PIXELS as f64 * gw / gh) as usize, GROUND_PIXELS) };
        let (base, shade, bbox, dark, key) = (s.base.clone(), s.shade.clone(), s.bbox, self.snap.theme.dark, s.key);
        cx.spawn(async move |this, cx| {
            let texture = cx
                .background_executor()
                .spawn(async move {
                    let mut p = base.crop([0.0, 0.0, 1.0, 1.0], w.max(2), h.max(2));
                    p.fade = FADE;
                    with_roads(p, Some(&g), bbox, gw, dark).lit(&shade)
                })
                .await;
            this.update(cx, |v, cx| {
                if let Some(s) = v.scene.as_mut().filter(|s| s.key == key && !s.draped) {
                    s.texture = Arc::new(texture);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Photos on the 3D ground, or the plain ground with the roads: kept in the settings. The eye
    /// stays where it is.
    pub fn set_photos(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.photos == on {
            return;
        }
        self.photos = on;
        self.data.update(cx, |d, cx| d.set_setting("loops.photos", if on { "1" } else { "0" }.to_string(), cx));
        if let Some(s) = self.scene.as_mut() {
            s.draped = false;
            s.insets.clear();
            s.texture = Arc::new((*s.base).clone().lit(&s.shade));
        }
        self.paint_roads(cx);
        cx.notify();
    }

    /// Lays the aerial photos on the ground once they are all there; where there is none (abroad,
    /// at sea) the grey ground shows. Until then, and offline, the ground stays grey.
    fn ensure_drape(&mut self, cx: &mut Context<Self>) {
        let Some(s) = self.scene.as_mut().filter(|s| !s.draped && self.photos) else { return };
        let z = map_zoom(s.bbox);
        // Every tile asked for at once, so that they come together.
        let got: Vec<_> = tiles(s.bbox, z).into_iter().map(|(x, y)| ((x, y), self.data.update(cx, |d, cx| d.ground_tile((z, x, y), cx)))).collect();
        let Some(have) = got.into_iter().map(|(k, t)| Some((k, t?))).collect::<Option<std::collections::HashMap<(u32, u32), Arc<Vec<u8>>>>>() else { return };
        // ponytail: resampled and lit on the main thread, once per loop; in the background if it shows.
        if let Some(t) = Texture::from_tiles(s.bbox, z, FADE, Some((&s.base, [0.0, 0.0, 1.0, 1.0])), &|x, y| have.get(&(x, y)).map(|t| t.as_slice())) {
            s.texture = Arc::new(t.lit(&s.shade));
            s.draped = true;
            // The roads over grey laid while waiting would hide the photos: they go.
            s.insets.retain(|(_, (_, _, photos))| *photos);
        }
    }

    /// Lays finer photos where the eye is close (see `levels`): the ground nearest the eye as
    /// sharp as the screen, coarser further on. One is made at a time (in the background), the
    /// finest first, again when the eye has moved a quarter of it away; the last ones show until
    /// then. Before the photos are there, and on the plain ground, the roads of the region's map
    /// over the grey ground; once they are, the coarser photos show until the finer ones are all
    /// there, so that the ground doesn't flicker grey while the eye moves.
    fn ensure_inset(&mut self, size: (f64, f64), cx: &mut Context<Self>) {
        let (graph, dark, photos) = (self.data.read(cx).loops.graph.clone(), self.snap.theme.dark, self.photos);
        let Some(s) = self.scene.as_mut().filter(|s| !s.inset_busy) else { return };
        let t = s.terrain.clone();
        let (gw, gh) = ((t.cols - 1) as f64 * t.dx, (t.rows - 1) as f64 * t.dy);
        let cos = ((s.bbox[0] + s.bbox[2]) / 2.0).to_radians().cos();
        let (bbox, wanted) = (s.bbox, levels(&s.cam, cos, map_zoom(s.bbox), size.1));
        // Coarsest first: it stays right longest while the eye moves; a finer level waits for the
        // coarser one's photos (else, moving, it would take every download for tiles already
        // behind the eye).
        for (z, centre, half) in wanted.iter().rev().copied() {
            let (lat, lon) = (t.south + centre[1] / t.my, t.west + centre[0] / t.mx);
            let (dlat, dlon) = (half / t.my, half / t.mx);
            let b = [(lat - dlat).max(bbox[0]), (lon - dlon).max(bbox[1]), (lat + dlat).min(bbox[2]), (lon + dlon).min(bbox[3])];
            if b[0] >= b[2] || b[1] >= b[3] {
                continue;
            }
            // The photos are asked for at every frame until they are all there, the finest level's
            // first.
            let got: Vec<_> = if photos { tiles(b, z).into_iter().map(|(x, y)| ((x, y), self.data.update(cx, |d, cx| d.ground_tile((z, x, y), cx)))).collect() } else { Vec::new() };
            let complete = photos && got.iter().all(|(_, t)| t.is_some());
            let Some(s) = self.scene.as_mut() else { return };
            let here: Vec<bool> = s.insets.iter().filter(|(_, (iz, c, _))| *iz == z && (c[0] - centre[0]).abs().max((c[1] - centre[1]).abs()) < half / 2.0).map(|(_, (_, _, done))| *done).collect();
            if here.contains(&true) {
                continue;
            }
            if !complete && (!here.is_empty() || graph.is_none() || s.draped || s.insets.iter().any(|(_, (_, _, done))| *done)) {
                if photos {
                    return;
                }
                continue;
            }
            let have: std::collections::HashMap<(u32, u32), Arc<Vec<u8>>> = got.into_iter().filter_map(|(k, t)| Some((k, t?))).collect();
            let uv = [(b[1] - t.west) * t.mx / gw, (b[0] - t.south) * t.my / gh, (b[3] - t.west) * t.mx / gw, (b[2] - t.south) * t.my / gh].map(|v| v as f32);
            let (bw, bh) = ((b[3] - b[1]) * t.mx, (b[2] - b[0]) * t.my);
            let texel = 156_543.0 * cos / 2f64.powi(z as i32);
            let (pw, ph) = (((bw / texel) as usize).clamp(2, 2048), ((bh / texel) as usize).clamp(2, 2048));
            s.inset_busy = true;
            let (shade, key, base, graph) = (s.shade.clone(), s.key, s.base.clone(), graph.clone());
            let zooms: Vec<u8> = wanted.iter().map(|w| w.0).collect();
            cx.spawn(async move |this, cx| {
                let inset = cx
                    .background_executor()
                    .spawn(async move {
                        let p = if complete { Texture::from_tiles(b, z, 0.0, Some((&base, uv)), &|x, y| have.get(&(x, y)).map(|t| t.as_slice())) } else { Some(with_roads(base.crop(uv, pw, ph), graph.as_deref(), b, bw, dark)) };
                        p.map(|p| Inset { texture: p.lit_in(&shade, uv), uv })
                    })
                    .await;
                this.update(cx, |v, cx| {
                    let same = v.photos == photos;
                    if let Some(s) = v.scene.as_mut().filter(|s| s.key == key) {
                        s.inset_busy = false;
                        // Made for the other ground (photos or plain), or roads over grey once the
                        // photos are there: dropped.
                        let keep = same && (complete || !s.draped);
                        if let Some(i) = inset.filter(|_| keep) {
                            // It takes its level's place; levels no longer wanted go.
                            s.insets.retain(|(_, (iz, _, _))| *iz != z && zooms.contains(iz));
                            s.insets.push((Arc::new(i), (z, centre, complete)));
                            s.insets.sort_by(|a, b| b.1.0.cmp(&a.1.0));
                        }
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
            return;
        }
    }

    /// Stands the buildings beside the loop on the ground, once they are read (from the cache or
    /// the region's extract); they are made into blocks in the background.
    fn ensure_blocks(&mut self, l: &Loop, cx: &mut Context<Self>) {
        let t = self.snap.theme;
        let Some(s) = self.scene.as_mut().filter(|s| s.blocks.is_none() && !s.blocks_busy) else { return };
        let Some(tiles) = self.data.update(cx, |d, cx| d.buildings(&buildings::tiles_along(&l.line, BESIDE), cx)) else { return };
        s.blocks_busy = true;
        // Walls and roofs: the page's neutrals, a roof lighter than its walls.
        let (wall, roof) = (bytes(crate::ui::theme::blend(t.dim, t.bg, if t.dark { 0.45 } else { 0.35 })), bytes(crate::ui::theme::blend(t.dim, t.bg, if t.dark { 0.7 } else { 0.12 })));
        let (line, terrain, key) = (l.line.clone(), s.terrain.clone(), s.key);
        cx.spawn(async move |this, cx| {
            let blocks = cx.background_executor().spawn(async move { Blocks::beside(&line, &tiles, &terrain, wall, roof) }).await;
            this.update(cx, |v, cx| {
                if let Some(s) = v.scene.as_mut().filter(|s| s.key == key) {
                    s.blocks = Some(Arc::new(blocks));
                    s.blocks_busy = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Draws the scene again in the background when the eye, the line or the size changed, at
    /// full size and smoothed even while the pointer moves it (about 16 ms for 1200 × 800). One
    /// drawing at a time, the latest wins.
    fn ensure_picture(&mut self, l: &Loop, size: (f64, f64), cx: &mut Context<Self>) {
        let t = self.snap.theme;
        let ramp = slope_ramp(&t, true).map(bytes);
        let classes = self.show_slope.then(|| slope::runs(&l.along, &l.eles));
        let climb = self.climb_hover.and_then(|i| slope::climbs(&l.along, &l.eles).get(i).map(|c| c.from..c.to));
        let (accent, ink, bg) = (bytes(t.accent), bytes(t.text), bytes(t.bg));
        let Some(s) = self.scene.as_mut() else { return };
        let (w, h) = (size.0 as usize, size.1 as usize);
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
        // While the eye moves (the picture shown is from another eye), a quick one: the rough
        // ground, not smoothed. Once it stays, the full one.
        let fine = s.image.is_none() || s.shown.is_some_and(|(c, sw, sh)| c == s.cam && (sw, sh) == (w, h));
        let key = {
            let mut hsh = std::collections::hash_map::DefaultHasher::new();
            let c = s.cam;
            (c.target.map(f64::to_bits), c.yaw.to_bits(), c.pitch.to_bits(), c.dist.to_bits(), w, h, s.key, s.draped, s.blocks.is_some(), fine).hash(&mut hsh);
            (Arc::as_ptr(&s.texture) as usize, s.insets.iter().map(|(i, _)| Arc::as_ptr(i) as usize).collect::<Vec<_>>()).hash(&mut hsh);
            line.iter().for_each(|(_, c)| c.hash(&mut hsh));
            hsh.finish()
        };
        if s.drawn == Some(key) || s.busy {
            return;
        }
        s.busy = true;
        let (terrain, texture, blocks, cam, insets) = (if fine { s.terrain.clone() } else { s.rough.clone() }, s.texture.clone(), s.blocks.clone(), s.cam, s.insets.iter().map(|(i, _)| i.clone()).collect::<Vec<_>>());
        cx.spawn(async move |this, cx| {
            let img = cx
                .background_executor()
                .spawn(async move {
                    let solids = blocks.map(|b| b.seen(&cam)).unwrap_or_default();
                    let insets: Vec<&Inset> = insets.iter().map(|i| i.as_ref()).collect();
                    let px = terrain::render(&terrain, &texture, &solids, &cam, EXAGGERATION, &line, w, h, [bg[0], bg[1], bg[2], 255], fine, &insets);
                    render_image(w as u32, h as u32, px).map(Arc::new)
                })
                .await;
            this.update(cx, |v, cx| {
                if let Some(s) = v.scene.as_mut() {
                    s.busy = false;
                    s.drawn = Some(key);
                    if img.is_some() {
                        s.stale.extend(std::mem::replace(&mut s.image, img));
                        s.shown = Some((cam, w, h));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Starts flying over the chosen loop, pauses, or goes on; from the start again once at the end.
    pub fn fly(&mut self, cx: &mut Context<Self>) {
        let found = self.snap_found(cx);
        let (Some(l), Some(s)) = (found.get(self.selected), self.scene.as_mut()) else { return };
        let total = l.along.last().copied().unwrap_or(0.0) as f64;
        if l.line.len() < 2 || l.along.len() != l.line.len() || total <= 0.0 {
            return;
        }
        let run = s.fly.as_ref().map_or(0, |f| f.run) + 1;
        match s.fly.as_mut() {
            Some(f) if f.playing => f.playing = false,
            Some(f) if f.at < total => *f = Fly { at: f.at, playing: true, run },
            _ => {
                // Close to the ground, looking along the line from its first step.
                s.cam.dist = FLY_DIST.max(s.cam.min_dist);
                s.cam.pitch = FLY_PITCH;
                s.fly = Some(Fly { at: 0.0, playing: true, run });
                s.fly_to(l, 0.0, 1.0);
            }
        }
        cx.notify();
        if !s.fly.as_ref().is_some_and(|f| f.playing) {
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(FLY_TICK).await;
                if !this.update(cx, |v, cx| v.fly_on(run, cx)).unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Space: pauses the flight or goes on. `false` when there is no flight (the key is not ours).
    pub fn fly_pause(&mut self, cx: &mut Context<Self>) -> bool {
        let flying = self.three_d && self.scene.as_ref().is_some_and(|s| s.fly.is_some());
        if flying {
            self.fly(cx);
        }
        flying
    }

    /// One step of the flight's clock `run`; `false` once it is over, paused or replaced.
    fn fly_on(&mut self, run: u64, cx: &mut Context<Self>) -> bool {
        let found = self.snap_found(cx);
        let (Some(l), Some(s)) = (found.get(self.selected), self.scene.as_mut()) else { return false };
        let Some(f) = s.fly.as_mut().filter(|f| f.playing && f.run == run && self.three_d) else { return false };
        let total = l.along.last().copied().unwrap_or(0.0) as f64;
        let dt = FLY_TICK.as_secs_f64();
        f.at = (f.at + (total / 60.0).clamp(FLY_SPEED.0, FLY_SPEED.1) * dt).min(total);
        // At the end it stops there.
        f.playing = f.at < total;
        let (at, on) = (f.at, f.playing);
        self.hover = Some(s.fly_to(l, at, dt).min(l.line.len() - 1));
        if !on {
            self.hover = Some(l.line.len() - 1);
        }
        cx.notify();
        on
    }

    /// The chosen loop in 3D, filling the page: drag to move over the ground, right-drag or
    /// shift-drag to turn it, the wheel to come closer. The pointer over the line shows where on
    /// the profile.
    pub(super) fn view_3d(&mut self, l: &Loop, cx: &mut Context<Self>) -> AnyElement {
        let t = self.snap.theme;
        let bounds = self.plots.get("loops-3d");
        let b = bounds.get();
        let dims = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
        self.ensure_scene(l, dims, cx);
        if let Some(s) = self.scene.as_mut() {
            s.follow_size(&l.line, dims);
        }
        self.ensure_drape(cx);
        self.ensure_inset(dims, cx);
        self.ensure_blocks(l, cx);
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
            .child({
                let (label, tip) = match s.fly.as_ref() {
                    Some(f) if f.playing => (tr("Pause", "Pause"), tr("Space: pause", "Espace : mettre en pause")),
                    Some(f) if f.at > 0.0 && f.at < l.along.last().copied().unwrap_or(0.0) as f64 => (tr("Resume", "Reprendre"), tr("Space: go on", "Espace : reprendre")),
                    _ => (tr("Fly over", "Survoler"), tr("Follow the loop from above", "Suivre la boucle vue d'en haut")),
                };
                // Above the profile band, at its right end (the help is in the middle).
                div().absolute().bottom(px((super::BAND + 24.0 + 10.0) as f32)).right(px(14.)).child(
                    crate::ui::button("loops-fly", label, false, t)
                        .bg(t.bg)
                        .tooltip_show_delay(crate::ui::TIP_DELAY)
                        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx))
                        // The button's own: the view under it must not take the press for a drag.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, _, cx| this.fly(cx))),
                )
            })
            // The ground carries the IGN's photos, and the buildings and roads of OpenStreetMap:
            // their makers are named, as on the map.
            .child(
                div()
                    .id("loops-3d-credit")
                    .absolute()
                    // Under the profile band, where the map has its own.
                    .bottom(px(4.))
                    .right(px(14.))
                    .px(px(4.))
                    .rounded(px(4.))
                    .bg(t.bg.opacity(0.8))
                    .text_size(px(11.))
                    .text_color(t.dim)
                    .cursor_pointer()
                    .on_click(|_, _, cx| cx.open_url("https://www.openstreetmap.org/copyright"))
                    .child(if self.photos { tr("Photos IGN · © OpenStreetMap contributors", "Photos IGN · © les contributeurs d'OpenStreetMap") } else { tr("© OpenStreetMap contributors", "© les contributeurs d'OpenStreetMap") }),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| this.grab_3d(e, e.modifiers.shift, cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, _, cx| this.grab_3d(e, true, cx)))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| this.move_3d(e, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _: &MouseUpEvent, _, cx| this.release_3d(cx)))
            .on_mouse_up(MouseButton::Right, cx.listener(|this, _: &MouseUpEvent, _, cx| this.release_3d(cx)))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                let dy = (e.delta.pixel_delta(px(20.)).y / px(1.)) as f64;
                if let Some(s) = this.scene.as_mut() {
                    s.moved = true;
                    s.cam.zoom(2f64.powf(-dy / 240.0));
                    s.cam.lift(&s.terrain, EXAGGERATION);
                    cx.notify();
                }
            }))
            .into_any_element()
    }

    fn grab_3d(&mut self, e: &MouseDownEvent, turn: bool, cx: &mut Context<Self>) {
        if let Some(s) = self.scene.as_mut() {
            s.drag = Some((e.position, turn));
            // Taking the ground in hand pauses the flight.
            if let Some(f) = s.fly.as_mut() {
                f.playing = false;
            }
            cx.notify();
        }
    }

    fn release_3d(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = self.scene.as_mut() {
            s.drag = None;
            cx.notify();
        }
    }

    fn move_3d(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        let b = self.plots.get("loops-3d").get();
        let size = ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64);
        let found = self.snap_found(cx);
        let Some(s) = self.scene.as_mut() else { return };
        match s.drag.filter(|_| matches!(e.pressed_button, Some(MouseButton::Left | MouseButton::Right))) {
            Some((from, turn)) => {
                let (dx, dy) = (((e.position.x - from.x) / px(1.)) as f64, ((e.position.y - from.y) / px(1.)) as f64);
                if turn {
                    s.cam.orbit(-dx * TURN, dy * TURN);
                    s.cam.lift(&s.terrain, EXAGGERATION);
                } else {
                    s.cam.pan_screen(&s.terrain, EXAGGERATION, dx, dy, size.1);
                }
                s.drag = Some((e.position, turn));
                s.moved = true;
                cx.notify();
            }
            // In flight the mark on the profile is the flight's, not the pointer's.
            None if s.fly.as_ref().is_some_and(|f| f.playing) => s.drag = None,
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
        let b = self.plots.get("loops-3d").get();
        let key = self.scene_key(l);
        self.scene = Scene::of(&l.line, key, false, ((b.size.width / px(1.)) as f64, (b.size.height / px(1.)) as f64), tile);
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

    /// Whether a flight is going on (`None`: there is none).
    #[cfg(test)]
    pub fn flying(&self) -> Option<bool> {
        self.scene.as_ref().and_then(|s| s.fly.as_ref()).map(|f| f.playing)
    }

    /// Puts a flight this many metres before the end of the chosen loop, for the tests.
    #[cfg(test)]
    pub fn fly_seek(&mut self, before_end: f64, cx: &App) {
        let total = self.snap_found(cx).get(self.selected).and_then(|l| l.along.last().copied()).unwrap_or(0.0) as f64;
        if let Some(f) = self.scene.as_mut().and_then(|s| s.fly.as_mut()) {
            f.at = (total - before_end).max(0.0);
        }
    }

    #[cfg(test)]
    pub fn show_3d(&mut self, on: bool) {
        self.three_d = on;
    }
}

#[cfg(test)]
mod tests {
    use super::{Scene, around, bounds, map_zoom, span, tiles, tint, world};
    use std::sync::Arc;

    #[test]
    fn buildings_stand_beside_the_loop_and_show_from_close() {
        use super::{BUILDINGS_FROM, Blocks, EXAGGERATION};
        use crate::logic::buildings::Building;
        let line = [[43.70, 7.25], [43.72, 7.27], [43.71, 7.29]];
        let mut slope = |_: u8, _: u32, _: u32| Some(Arc::new((0..256 * 256).map(|i| 300.0 - (i / 256) as f32).collect::<Vec<f32>>()));
        let s = Scene::of(&line, 0, false, (1200.0, 800.0), &mut slope).unwrap();
        // A house of 10 m by the start, 9 m high; one a kilometre from the line; a way of two points.
        let house = |lat: f64, lon: f64| Building { ring: [[0.0, 0.0], [0.0001, 0.0], [0.0001, 0.00012], [0.0, 0.00012]].iter().map(|d| [((lat + d[0]) * 1e7) as i32, ((lon + d[1]) * 1e7) as i32]).collect(), height: 9.0 };
        let tiles = [Arc::new(vec![house(43.7001, 7.2502), house(43.70, 7.2650), Building { ring: vec![[437_000_000, 72_500_000], [437_000_100, 72_500_000]], height: 9.0 }]), Arc::new(vec![])];
        let b = Blocks::beside(&line, &tiles, &s.terrain, [100; 3], [200; 3]);
        assert_eq!(b.at.len(), 1, "only the house by the line");
        assert_eq!(b.faces.len(), 10);
        // It stands on the ground: from the lowest ground under it to 9 m (drawn as the ground's
        // heights are) over the highest.
        let at = b.at[0].0;
        let ground = s.terrain.height_at(at[0] as f64, at[1] as f64) as f32;
        let (lo, hi) = b.faces.iter().flat_map(|f| f.p).fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
        assert!((lo - ground).abs() < 12.0 && lo <= ground + 0.01, "{lo} {ground}");
        assert!(hi - lo >= 9.0 / EXAGGERATION && hi - lo < 9.0 / EXAGGERATION + 15.0, "{lo} {hi}");
        // Seen from close when the eye looks near it; not from afar, nor when it looks elsewhere.
        let mut cam = s.cam;
        cam.target = [at[0] as f64, at[1] as f64, ground as f64];
        cam.dist = 800.0;
        assert_eq!(b.seen(&cam).len(), 10);
        cam.dist = BUILDINGS_FROM + 1.0;
        assert!(b.seen(&cam).is_empty());
        cam.dist = 800.0;
        cam.target[0] += 900.0;
        assert!(b.seen(&cam).is_empty());
        assert!(Blocks::beside(&line, &[], &s.terrain, [0; 3], [0; 3]).seen(&s.cam).is_empty());
    }

    #[test]
    fn a_flight_knows_where_it_is_and_which_way_to_turn() {
        use super::{fly_at, turn};
        let along = [0.0, 100.0, 100.0, 250.0];
        assert_eq!(fly_at(&along, 0.0), (0, 0.0));
        assert_eq!(fly_at(&along, 50.0), (0, 0.5));
        // Two points at the same place: the one after them.
        assert_eq!(fly_at(&along, 100.0), (2, 0.0));
        assert_eq!(fly_at(&along, 175.0), (2, 0.5));
        // The end, beyond it, before the start, a line of one point or none: no step out of it.
        assert_eq!(fly_at(&along, 250.0), (2, 1.0));
        assert_eq!(fly_at(&along, 1.0e9), (2, 1.0));
        assert_eq!(fly_at(&along, -5.0), (0, 0.0));
        assert_eq!(fly_at(&[0.0], 10.0), (0, 0.0));
        assert_eq!(fly_at(&[], 10.0), (0, 0.0));
        // The short way round, through north too.
        let deg = |d: f64| d.to_radians();
        assert!((turn(deg(350.0), deg(10.0)) - deg(20.0)).abs() < 1e-9);
        assert!((turn(deg(10.0), deg(350.0)) + deg(20.0)).abs() < 1e-9);
        assert!((turn(deg(90.0), deg(90.0))).abs() < 1e-9);
        assert!((turn(0.0, deg(179.0)) - deg(179.0)).abs() < 1e-9);
    }

    /// The loop's box on screen, in a view of `size`.
    fn on_screen(s: &Scene, line: &[[f64; 2]], size: (f64, f64)) -> (f32, f32, f32, f32) {
        line.iter().map(|p| s.screen(*p, size.0, size.1).unwrap()).fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |b, (x, y)| (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y)))
    }

    #[test]
    fn the_loop_is_in_the_middle_of_the_free_area_and_fills_it() {
        use super::PANEL;
        let mut hills = |_: u8, _: u32, _: u32| Some(Arc::new((0..256 * 256).map(|i| 200.0 + 150.0 * ((i % 256) as f32 / 30.0).sin() + (i / 256) as f32).collect::<Vec<f32>>()));
        // A loop wider than high, one higher than wide, a long thin one, a tiny one.
        let ring = |lat: f64, lon: f64, dlat: f64, dlon: f64| (0..60).map(|k| { let a = k as f64 / 60.0 * std::f64::consts::TAU; [lat + dlat * a.sin(), lon + dlon * a.cos()] }).collect::<Vec<_>>();
        let loops = [ring(43.71, 7.27, 0.01, 0.06), ring(43.71, 7.27, 0.05, 0.01), ring(43.71, 7.27, 0.0005, 0.08), ring(43.71, 7.27, 0.001, 0.001)];
        for line in &loops {
            for size in [(1200.0, 800.0), (2400.0, 1300.0), (900.0, 1000.0), (760.0, 480.0)] {
                let s = Scene::of(line, 0, false, size, &mut hills).unwrap();
                let (x0, y0, x1, y1) = on_screen(&s, line, size);
                let (left, free) = ((PANEL + 28.0) as f32, (size.0 - PANEL - 28.0) as f32);
                let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                let what = format!("{:?} in {size:?}: {x0} {y0} {x1} {y1}", line[1]);
                assert!((cx - (left + free / 2.0)).abs() < free * 0.03 && (cy - size.1 as f32 / 2.0).abs() < size.1 as f32 * 0.03, "centred: {what}");
                // Whole in the free area, and large: 80 % of its width or of its height.
                assert!(x0 >= left && x1 <= size.0 as f32 && y0 >= 0.0 && y1 <= size.1 as f32, "inside: {what}");
                let fill = ((x1 - x0) / free).max((y1 - y0) / size.1 as f32);
                assert!((0.74..=0.86).contains(&fill), "fills {fill}: {what}");
            }
        }
    }

    #[test]
    fn a_view_that_changes_size_is_framed_again_until_the_eye_is_moved() {
        let line = [[43.70, 7.25], [43.72, 7.27], [43.71, 7.29]];
        let mut flat = |_: u8, _: u32, _: u32| Some(Arc::new(vec![100.0f32; 256 * 256]));
        // Made while the window was still small, as when it opens.
        let mut s = Scene::of(&line, 0, false, (700.0, 400.0), &mut flat).unwrap();
        let big = (1600.0, 1000.0);
        let centre = |s: &Scene| { let b = on_screen(s, &line, big); ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0) };
        let want = ((super::PANEL as f32 + 28.0 + 1600.0) / 2.0, 500.0);
        assert!((centre(&s).0 - want.0).abs() > 60.0, "framed for the small view, off in the large one");
        s.follow_size(&line, big);
        let c = centre(&s);
        assert!((c.0 - want.0).abs() < 40.0 && (c.1 - want.1).abs() < 30.0, "{c:?} {want:?}");
        // Not laid out: nothing changes. Moved by hand: the eye is left where it was put.
        let cam = s.cam;
        s.follow_size(&line, (0.0, 0.0));
        assert_eq!(format!("{:?}", s.cam), format!("{cam:?}"));
        s.moved = true;
        s.follow_size(&line, (900.0, 700.0));
        assert_eq!(format!("{:?}", s.cam), format!("{cam:?}"));
    }

    #[test]
    fn marks_stay_on_the_picture_shown_while_the_eye_moves() {
        let line = [[43.70, 7.25], [43.72, 7.27], [43.71, 7.29]];
        let mut flat = |_: u8, _: u32, _: u32| Some(Arc::new(vec![100.0f32; 256 * 256]));
        let mut s = Scene::of(&line, 0, false, (1200.0, 800.0), &mut flat).unwrap();
        let at = |s: &Scene| s.screen(line[0], 1200.0, 800.0).unwrap();
        // A picture drawn with this eye is on screen.
        let before = at(&s);
        s.shown = Some((s.cam, 1200, 800));
        // The eye turns and moves; the next picture is still being drawn: the mark has not moved.
        s.cam.orbit(0.4, 0.1);
        s.cam.pan_screen(&s.terrain, super::EXAGGERATION, 90.0, 40.0, 800.0);
        assert_eq!(at(&s), before);
        // The new picture comes: the mark follows it.
        s.shown = Some((s.cam, 1200, 800));
        let after = at(&s);
        assert!((after.0 - before.0).abs() > 20.0, "{before:?} {after:?}");
        // A picture drawn at another size is stretched to the view, and the mark with it.
        s.shown = Some((s.cam, 600, 400));
        let half = at(&s);
        assert!((half.0 - after.0).abs() < 1.0 && (half.1 - after.1).abs() < 1.0, "{after:?} {half:?}");
    }

    #[test]
    fn the_box_and_its_tiles_cover_the_loop() {
        let inner = bounds(&[[43.70, 7.25], [43.72, 7.27], [43.71, 7.29]]).unwrap();
        assert_eq!(inner, [43.70, 7.25, 43.72, 7.29]);
        let (bbox, z) = around(inner);
        // About 2.2 × 3.2 km, 1.6 km more on each side.
        let m = 1600.0 / 110_540.0;
        assert!((bbox[0] - (43.70 - m)).abs() < 2e-4 && (bbox[2] - (43.72 + m)).abs() < 2e-4, "{bbox:?}");
        assert!(bbox[1] < 7.25 - m && bbox[3] > 7.29 + m, "{bbox:?}");
        // A small box: the finest elevation tiles.
        assert_eq!(z, 13);
        let t = tiles(bbox, z);
        let (x, y) = world(43.71, 7.27, z as f64);
        assert!(t.contains(&((x / 256.0) as u32, (y / 256.0) as u32)));
        assert!(t.len() <= 16, "{t:?}");
        // A single point still has a box of 3 km; no line, none.
        let (b, _) = around(bounds(&[[43.7, 7.25]]).unwrap());
        assert!((span(b).0 - 3000.0).abs() < 30.0, "{b:?}");
        assert!(bounds(&[]).is_none());
        // A long loop takes coarser tiles.
        assert!(around([43.5, 7.0, 43.9, 7.5]).1 < 12);
        // Never more elevation tiles than the map's own.
        for inner in [[43.7, 7.25, 43.7, 7.25], [43.0, 6.0, 44.0, 7.9], [68.0, 15.0, 68.4, 16.0]] {
            let (b, z) = around(inner);
            assert!(z <= map_zoom(b) && tiles(b, z).len() <= 100, "{inner:?}");
        }
    }

    #[test]
    fn the_map_on_the_ground_is_about_a_pixel_per_pixel() {
        // From a point's 3 km to a ride's 150 km: 1024 to 2048 pixels along, at most 9 × 9 tiles.
        for inner in [[43.7, 7.25, 43.7, 7.25], [43.70, 7.25, 43.72, 7.29], [43.5, 7.0, 43.9, 7.5], [43.0, 6.0, 44.0, 7.9], [-0.3, 10.0, 0.3, 10.2], [68.0, 15.0, 68.4, 16.0]] {
            let (bbox, _) = around(inner);
            let z = map_zoom(bbox);
            let (x0, y0) = world(bbox[2], bbox[1], z as f64);
            let (x1, y1) = world(bbox[0], bbox[3], z as f64);
            let long = (x1 - x0).max(y1 - y0);
            assert!((1000.0..2100.0).contains(&long), "{inner:?}: zoom {z}, {long} px");
            assert!(tiles(bbox, z).len() <= 100, "{inner:?}: {} tiles", tiles(bbox, z).len());
        }
    }

    #[test]
    fn the_photos_get_finer_toward_the_eye() {
        use super::{INSET_MAX_ZOOM, levels};
        use crate::logic::terrain::Camera;
        // Looking north from 3 km, 30° down, on a view 1000 pixels high.
        let cam = Camera { target: [5000.0, 5000.0, 0.0], yaw: 0.0, pitch: 30f64.to_radians(), dist: 3000.0, min_dist: 100.0, fov: 0.8 };
        let l = levels(&cam, 0.7, 11, 1000.0);
        assert_eq!(l.len(), 4, "{l:?}");
        // The finest first: about the screen's pixel where the ground is nearest (1.6 m at 1.9 km).
        assert_eq!(l[0].0, 17);
        for w in l.windows(2) {
            assert_eq!(w[0].0, w[1].0 + 1);
            assert!((w[1].2 / w[0].2 - 2.0).abs() < 1e-9);
        }
        // Between the eye (2.4 km south of the target) and the target, ahead of the nearest ground.
        let eye = 5000.0 - 3000.0 * 30f64.to_radians().cos();
        assert!(l.iter().all(|(_, c, _)| (c[0] - 5000.0).abs() < 1e-6 && c[1] > eye), "{l:?}");
        assert!(l[0].1[1] < 5000.0 && l[0].1[1] < l[1].1[1]);
        // From far, the photo over the whole ground is enough; from very close, the finest zoom.
        assert!(levels(&Camera { dist: 200_000.0, ..cam }, 0.7, 11, 1000.0).is_empty());
        assert_eq!(levels(&Camera { dist: 50.0, ..cam }, 0.7, 11, 1000.0)[0].0, INSET_MAX_ZOOM);
        // Looking straight down: around the target.
        let down = levels(&Camera { pitch: 85f64.to_radians(), ..cam }, 0.7, 11, 1000.0);
        assert!((down[0].1[1] - 5000.0).abs() < down[0].2, "{down:?}");
    }

    #[test]
    fn the_ground_darkens_as_it_rises() {
        // The sea blue, deeper in the dark; the land grey.
        assert_eq!(tint(0.0, 0.0, 500.0, false), [63, 41, 26]);
        assert!(tint(0.0, 0.0, 500.0, true)[0] < 63);
        assert!(tint(10.0, 0.0, 500.0, false).iter().all(|c| *c == tint(10.0, 0.0, 500.0, false)[0]));
        assert!(tint(10.0, 0.0, 500.0, false)[0] > tint(400.0, 0.0, 500.0, false)[0]);
        assert!(tint(10.0, 0.0, 500.0, true)[0] < tint(400.0, 0.0, 500.0, true)[0]);
        // Flat land, or below the sea: no division by nothing.
        assert_eq!(tint(100.0, 100.0, 100.0, false), tint(100.0, 0.0, 1.0e6, false));
        assert_eq!(tint(-5.0, -5.0, 300.0, false), tint(0.0, 0.0, 300.0, false));
    }
}
