//! Terrain in 3D, drawn in software: a height grid as shaded triangles with a z-buffer, and a
//! route drawn over it. There is no 3D in the UI toolkit, so the picture is an image of bytes
//! (same layout as `relief::shade`: the three colour bytes as given by the caller, then alpha).
//! Pure: no gpui, no disk.


use crate::logic::map::world;
use std::sync::{Arc, Mutex};

/// Share of the light a surface turned away from the sun still gets.
const AMBIENT: f32 = 0.35;
/// Direction towards the light: north-west, 45° up (x east, y north, z up; unit length).
const LIGHT: [f32; 3] = [-0.5, 0.5, std::f32::consts::FRAC_1_SQRT_2];
/// Brightness of a flat surface before it is brought back to 1: flat ground keeps its colours.
const FLAT_LIT: f32 = AMBIENT + (1.0 - AMBIENT) * LIGHT[2];
/// The line is 0.5 % nearer than its own ground, so it is not buried in it.
const LINE_BIAS: f32 = 1.005;
/// Brightness of a wall turned away from the sun.
const WALL_SHADE: f32 = 0.7;
const NEAR: f64 = 1.0;
const BAND: usize = 16;

/// A regular grid of altitudes. Vertex `(i, j)` is `heights[j * cols + i]`, `i` going east and
/// `j` going north from the south-west corner.
#[derive(Clone, Debug)]
pub struct Terrain {
    pub cols: usize,
    pub rows: usize,
    pub heights: Vec<f32>,
    /// Metres between two vertices, east and north.
    pub dx: f64,
    pub dy: f64,
    /// South-west corner, in degrees.
    pub south: f64,
    pub west: f64,
    /// Metres per degree of longitude and of latitude around the middle of the grid.
    pub mx: f64,
    pub my: f64,
}

impl Terrain {
    /// Resamples the altitudes of `bbox` `[south, west, north, east]` onto at most `max_cells`
    /// vertices. `tile(x, y)` gives a 256 × 256 elevation tile of `zoom`; `None` when a tile
    /// the box needs is missing.
    pub fn from_tiles(bbox: [f64; 4], max_cells: usize, zoom: u8, tile: &dyn Fn(u32, u32) -> Option<Arc<Vec<f32>>>) -> Option<Terrain> {
        let [s, w, n, e] = bbox;
        let my = 110_540.0;
        let mx = 111_320.0 * ((s + n) / 2.0).to_radians().cos();
        let (width, height) = ((e - w) * mx, (n - s) * my);
        if !(width > 0.0 && height > 0.0) || max_cells < 4 {
            return None;
        }
        let cell = (width * height / max_cells as f64).sqrt();
        let mut cols = ((width / cell).floor() as usize + 1).max(2);
        let mut rows = ((height / cell).floor() as usize + 1).max(2);
        while cols * rows > max_cells {
            if cols >= rows && cols > 2 { cols -= 1 } else if rows > 2 { rows -= 1 } else { return None }
        }
        let size = 256i64;
        let max = size << zoom;
        let mut cache: std::collections::HashMap<(i64, i64), Option<Arc<Vec<f32>>>> = std::collections::HashMap::new();
        let mut at = |gx: i64, gy: i64| -> Option<f32> {
            if gx < 0 || gy < 0 || gx >= max || gy >= max {
                return None;
            }
            let t = cache.entry((gx / size, gy / size)).or_insert_with(|| tile((gx / size) as u32, (gy / size) as u32));
            t.as_ref().map(|t| t[((gy % size) * size + gx % size) as usize])
        };
        let mut heights = Vec::with_capacity(cols * rows);
        for j in 0..rows {
            let lat = s + (n - s) * j as f64 / (rows - 1) as f64;
            for i in 0..cols {
                let lon = w + (e - w) * i as f64 / (cols - 1) as f64;
                // Tile pixels are areas: their altitude sits at their centre.
                let (px, py) = world(lat, lon, zoom as f64);
                let (fx, fy) = (px - 0.5, py - 0.5);
                let (x0, y0) = (fx.floor(), fy.floor());
                let (tx, ty) = ((fx - x0) as f32, (fy - y0) as f32);
                let (x0, y0) = (x0 as i64, y0 as i64);
                let c = [at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1)];
                // A neighbour in a tile the caller did not fetch borrows from one that it did.
                let any = c.iter().flatten().next().copied()?;
                let [a, b, c2, d] = c.map(|v| v.unwrap_or(any));
                heights.push((a * (1.0 - tx) + b * tx) * (1.0 - ty) + (c2 * (1.0 - tx) + d * tx) * ty);
            }
        }
        Some(Terrain { cols, rows, heights, dx: width / (cols - 1) as f64, dy: height / (rows - 1) as f64, south: s, west: w, mx, my })
    }

    /// Metres east and north of the south-west corner.
    pub fn local(&self, lat: f64, lon: f64) -> [f64; 2] {
        [(lon - self.west) * self.mx, (lat - self.south) * self.my]
    }

    fn extent(&self) -> [f64; 2] {
        [self.cols.saturating_sub(1) as f64 * self.dx, self.rows.saturating_sub(1) as f64 * self.dy]
    }

    /// Altitude at `[east, north]` local metres (bilinear, clamped to the grid); 0 on an empty grid.
    pub fn height_at(&self, x: f64, y: f64) -> f64 {
        if self.cols < 2 || self.rows < 2 || self.heights.len() < self.cols * self.rows {
            return 0.0;
        }
        let fx = (x / self.dx).clamp(0.0, (self.cols - 1) as f64);
        let fy = (y / self.dy).clamp(0.0, (self.rows - 1) as f64);
        let (i, j) = ((fx.floor() as usize).min(self.cols - 2), (fy.floor() as usize).min(self.rows - 2));
        let (tx, ty) = (fx - i as f64, fy - j as f64);
        let h = |i: usize, j: usize| self.heights[j * self.cols + i] as f64;
        (h(i, j) * (1.0 - tx) + h(i + 1, j) * tx) * (1.0 - ty) + (h(i, j + 1) * (1.0 - tx) + h(i + 1, j + 1) * tx) * ty
    }
}

/// A picture laid over the whole ground: texel `(i, j)` is `px[j * w + i]`, `i` going east and `j`
/// going north, each an even share of the ground. Bytes in the caller's order.
#[derive(Clone, Debug)]
pub struct Texture {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[u8; 3]>,
    /// Share of the ground's width, from each edge, over which it fades into the background
    /// (0 = cut sharp).
    pub fade: f32,
}

impl Texture {
    /// The map tiles of `zoom` over `bbox` `[south, west, north, east]`, pixel for pixel.
    /// `tile(x, y)` gives the 256 × 256 pixels of a tile, 4 bytes each (the fourth is dropped);
    /// `None` when one is missing.
    pub fn from_tiles<'a>(bbox: [f64; 4], zoom: u8, fade: f32, tile: &dyn Fn(u32, u32) -> Option<&'a [u8]>) -> Option<Texture> {
        let m = Mosaic::over(bbox, zoom, &|x, y| tile(x, y).filter(|t| t.len() >= 256 * 256 * 4))?;
        let mut px = Vec::with_capacity(m.w * m.h);
        for j in 0..m.h {
            let gy = m.row(j);
            for i in 0..m.w {
                let (t, k) = m.at(m.col(i), gy);
                px.push([t[k * 4], t[k * 4 + 1], t[k * 4 + 2]]);
            }
        }
        Some(Texture { w: m.w, h: m.h, px, fade })
    }

    /// The same picture lit by the sun: each texel times the light there.
    pub fn lit(mut self, shade: &Shade) -> Texture {
        for j in 0..self.h {
            let v = (j as f32 + 0.5) / self.h as f32;
            for i in 0..self.w {
                let f = shade.at((i as f32 + 0.5) / self.w as f32, v);
                let p = &mut self.px[j * self.w + i];
                *p = p.map(|c| (c as f32 * f + 0.5).min(255.0) as u8);
            }
        }
        self
    }

    /// The colour at `u` east and `v` north, each 0..1 across the ground (bilinear).
    fn sample(&self, u: f32, v: f32) -> [f32; 3] {
        // Texels are areas: their colour sits at their centre.
        let (fx, fy) = ((u * self.w as f32 - 0.5).clamp(0.0, (self.w - 1) as f32), (v * self.h as f32 - 0.5).clamp(0.0, (self.h - 1) as f32));
        let (i, j) = ((fx as usize).min(self.w - 2), (fy as usize).min(self.h - 2));
        let (tx, ty) = (fx - i as f32, fy - j as f32);
        let k = j * self.w + i;
        let (a, b, c, d) = (self.px[k], self.px[k + 1], self.px[k + self.w], self.px[k + self.w + 1]);
        [0, 1, 2].map(|n| (a[n] as f32 * (1.0 - tx) + b[n] as f32 * tx) * (1.0 - ty) + (c[n] as f32 * (1.0 - tx) + d[n] as f32 * tx) * ty)
    }

    /// How much of the ground shows at `(u, v)`: 1 inside, down to 0 on the edges.
    fn shown(&self, u: f32, v: f32) -> f32 {
        if self.fade <= 0.0 {
            return 1.0;
        }
        let s = |d: f32| {
            let t = (d / self.fade).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        s(u) * s(1.0 - u) * s(v) * s(1.0 - v)
    }
}

/// The tiles of a zoom under a box, and a picture of the box at their scale: `w` × `h` texels,
/// `(i, j)` going east and north, each an even share of the box, read from the tile pixel under
/// its centre.
struct Mosaic<T> {
    w: usize,
    h: usize,
    tiles: Vec<T>,
    /// First tile, and how many across.
    tx0: i64,
    ty0: i64,
    across: usize,
    /// World pixels: the box's west and east, its south and north latitudes, the last pixel.
    x: [f64; 2],
    lat: [f64; 2],
    zoom: u8,
    max: i64,
}

impl<T: Copy> Mosaic<T> {
    /// `None` when the box is empty or too large, or a tile is missing.
    fn over(bbox: [f64; 4], zoom: u8, tile: &dyn Fn(u32, u32) -> Option<T>) -> Option<Mosaic<T>> {
        let [s, w_, n, e] = bbox;
        let (x0, y0) = world(n, w_, zoom as f64);
        let (x1, y1) = world(s, e, zoom as f64);
        let (w, h) = ((x1 - x0).ceil() as usize, (y1 - y0).ceil() as usize);
        if !(2..=8192).contains(&w) || !(2..=8192).contains(&h) {
            return None;
        }
        let max = (256i64 << zoom) - 1;
        // A pixel's margin, for the slopes at the edges.
        let (tx0, ty0) = ((x0 as i64 - 1).clamp(0, max) / 256, (y0 as i64 - 1).clamp(0, max) / 256);
        let (tx1, ty1) = ((x1 as i64 + 1).clamp(0, max) / 256, (y1 as i64 + 1).clamp(0, max) / 256);
        let across = (tx1 - tx0 + 1) as usize;
        let mut tiles = Vec::with_capacity(across * (ty1 - ty0 + 1) as usize);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                tiles.push(tile(tx as u32, ty as u32)?);
            }
        }
        Some(Mosaic { w, h, tiles, tx0, ty0, across, x: [x0, x1], lat: [s, n], zoom, max })
    }

    /// World pixel column under texel column `i`.
    fn col(&self, i: usize) -> i64 {
        (self.x[0] + (self.x[1] - self.x[0]) * (i as f64 + 0.5) / self.w as f64) as i64
    }

    /// World pixel row under texel row `j`: rows go north by even steps of latitude, as the
    /// ground's grid does, while the map's pixels go south.
    fn row(&self, j: usize) -> i64 {
        let lat = self.lat[0] + (self.lat[1] - self.lat[0]) * (j as f64 + 0.5) / self.h as f64;
        world(lat, 0.0, self.zoom as f64).1 as i64
    }

    /// The tile holding a world pixel (brought back within the tiles read) and the pixel's index in it.
    fn at(&self, gx: i64, gy: i64) -> (T, usize) {
        let rows = (self.tiles.len() / self.across) as i64;
        let gx = gx.clamp(0, self.max).clamp(self.tx0 * 256, (self.tx0 + self.across as i64) * 256 - 1);
        let gy = gy.clamp(0, self.max).clamp(self.ty0 * 256, (self.ty0 + rows) * 256 - 1);
        (self.tiles[(gy / 256 - self.ty0) as usize * self.across + (gx / 256 - self.tx0) as usize], ((gy % 256) * 256 + gx % 256) as usize)
    }
}

/// How much light each point of the ground gets, flat ground being 1: `w` × `h` values laid as a
/// `Texture`'s texels.
#[derive(Clone, Debug)]
pub struct Shade {
    pub w: usize,
    pub h: usize,
    /// 128 = 1.
    pub light: Vec<u8>,
}

impl Shade {
    /// The light on `bbox` from the elevation tiles of `zoom`, slope by slope at their full
    /// resolution (the ground's grid is coarser), heights times `exaggeration`. `tile(x, y)`
    /// gives a 256 × 256 elevation tile; `None` when one is missing.
    pub fn from_tiles(bbox: [f64; 4], zoom: u8, exaggeration: f32, tile: &dyn Fn(u32, u32) -> Option<Arc<Vec<f32>>>) -> Option<Shade> {
        let tiles: std::cell::RefCell<Vec<Arc<Vec<f32>>>> = Default::default();
        // The mosaic holds indices: the tiles themselves are not `Copy`.
        let m = Mosaic::over(bbox, zoom, &|x, y| {
            let t = tile(x, y).filter(|t| t.len() >= 256 * 256)?;
            tiles.borrow_mut().push(t);
            Some(tiles.borrow().len() - 1)
        })?;
        let tiles = tiles.into_inner();
        let alt = |gx: i64, gy: i64| {
            let (t, k) = m.at(gx, gy);
            tiles[t][k]
        };
        // Metres per tile pixel in the middle of the box.
        let metres = 40_075_016.7 * ((bbox[0] + bbox[2]) / 2.0).to_radians().cos() / (256u64 << zoom) as f64;
        let k = exaggeration / (2.0 * metres as f32);
        let mut light = Vec::with_capacity(m.w * m.h);
        for j in 0..m.h {
            let gy = m.row(j);
            for i in 0..m.w {
                let gx = m.col(i);
                // Slopes east and north; the tiles' rows go south.
                let (sx, sy) = ((alt(gx + 1, gy) - alt(gx - 1, gy)) * k, (alt(gx, gy - 1) - alt(gx, gy + 1)) * k);
                let lit = ((-sx * LIGHT[0] - sy * LIGHT[1] + LIGHT[2]) / (sx * sx + sy * sy + 1.0).sqrt()).max(0.0);
                light.push(((AMBIENT + (1.0 - AMBIENT) * lit) / FLAT_LIT * 128.0 + 0.5).min(255.0) as u8);
            }
        }
        Some(Shade { w: m.w, h: m.h, light })
    }

    /// The light at `u` east and `v` north, each 0..1 across the ground (bilinear).
    fn at(&self, u: f32, v: f32) -> f32 {
        let (fx, fy) = ((u * self.w as f32 - 0.5).clamp(0.0, (self.w - 1) as f32), (v * self.h as f32 - 0.5).clamp(0.0, (self.h - 1) as f32));
        let (i, j) = ((fx as usize).min(self.w.saturating_sub(2)), (fy as usize).min(self.h.saturating_sub(2)));
        let (tx, ty) = (fx - i as f32, fy - j as f32);
        let l = |i: usize, j: usize| self.light[(j.min(self.h - 1)) * self.w + i.min(self.w - 1)] as f32;
        ((l(i, j) * (1.0 - tx) + l(i + 1, j) * tx) * (1.0 - ty) + (l(i, j + 1) * (1.0 - tx) + l(i + 1, j + 1) * tx) * ty) / 128.0
    }
}

/// A triangle standing on the ground, of one colour (already lit): corners `[east, north,
/// altitude]` in local metres, bytes in the caller's order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub p: [[f32; 3]; 3],
    pub colour: [u8; 3],
}

/// A block standing on an outline (`[east, north]` local metres, not closed, either way round)
/// from `base` up to `top`: its roof in `roof`, flat, and its walls in `wall`, each lit by its
/// side to the sun. No floor. Fewer than three distinct points: nothing.
pub fn extrude(ring: &[[f32; 2]], base: f32, top: f32, wall: [u8; 3], roof: [u8; 3]) -> Vec<Face> {
    let mut pts: Vec<[f32; 2]> = Vec::with_capacity(ring.len());
    for p in ring {
        if p[0].is_finite() && p[1].is_finite() && pts.last() != Some(p) {
            pts.push(*p);
        }
    }
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    let area: f32 = (0..pts.len()).map(|i| { let (a, b) = (pts[i], pts[(i + 1) % pts.len()]); a[0] * b[1] - b[0] * a[1] }).sum();
    if pts.len() < 3 || area == 0.0 {
        return Vec::new();
    }
    // Counter-clockwise from above: the outside of a wall is on the right of its edge.
    if area < 0.0 {
        pts.reverse();
    }
    let n = pts.len();
    let mut out = Vec::with_capacity(3 * n);
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = (dx * dx + dy * dy).sqrt();
        // Softer than the ground's shade, and never as light as the roof: a town stays legible.
        let lit = ((dy * LIGHT[0] - dx * LIGHT[1]) / len).max(0.0);
        let f = WALL_SHADE + (1.0 - WALL_SHADE) * lit;
        let colour = wall.map(|c| (c as f32 * f + 0.5).min(255.0) as u8);
        out.push(Face { p: [[a[0], a[1], base], [b[0], b[1], base], [b[0], b[1], top]], colour });
        out.push(Face { p: [[a[0], a[1], base], [b[0], b[1], top], [a[0], a[1], top]], colour });
    }
    // The roof, ear by ear: a corner whose triangle turns left and holds no other point is cut
    // off, until three are left.
    let cross = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let mut left: Vec<usize> = (0..n).collect();
    while left.len() > 3 {
        let m = left.len();
        let ear = (0..m).find(|&i| {
            let (a, b, c) = (pts[left[(i + m - 1) % m]], pts[left[i]], pts[left[(i + 1) % m]]);
            cross(a, b, c) > 0.0
                && !left.iter().any(|&k| {
                    let p = pts[k];
                    p != a && p != b && p != c && cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
                })
        });
        // An outline that crosses itself has no ear left: the rest is closed as a fan.
        let i = ear.unwrap_or(0);
        let (a, b, c) = (pts[left[(i + m - 1) % m]], pts[left[i]], pts[left[(i + 1) % m]]);
        out.push(Face { p: [[a[0], a[1], top], [b[0], b[1], top], [c[0], c[1], top]], colour: roof });
        left.remove(i);
    }
    let (a, b, c) = (pts[left[0]], pts[left[1]], pts[left[2]]);
    out.push(Face { p: [[a[0], a[1], top], [b[0], b[1], top], [c[0], c[1], top]], colour: roof });
    out
}

/// Where the eye looks from: orbiting `target` (local metres east, north, altitude).
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub target: [f64; 3],
    /// 0 = looking north, growing clockwise (east).
    pub yaw: f64,
    /// Above the horizon, kept within 10°..85°.
    pub pitch: f64,
    pub dist: f64,
    /// Closest the eye may come to the target (`zoom` stops there).
    pub min_dist: f64,
    /// Vertical field of view.
    pub fov: f64,
}

const PITCH_MIN: f64 = 10.0 * std::f64::consts::PI / 180.0;
const PITCH_MAX: f64 = 85.0 * std::f64::consts::PI / 180.0;

impl Camera {
    /// The whole terrain, seen from the south-south-west at about 40°.
    pub fn fit(t: &Terrain) -> Camera {
        let [w, h] = t.extent();
        let (lo, hi) = t.heights.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        let relief = (hi - lo).max(0.0) as f64;
        let fov: f64 = 0.8;
        let radius = 0.5 * (w * w + h * h + relief * relief).sqrt();
        // ponytail: fits the vertical field only; a very narrow window crops the sides.
        Camera { target: [w / 2.0, h / 2.0, (lo + hi) as f64 / 2.0], yaw: 22.5f64.to_radians(), pitch: 40f64.to_radians(), dist: radius / (fov / 2.0).sin(), min_dist: 3.0 * t.dx.max(t.dy), fov }
    }

    pub fn orbit(&mut self, dyaw: f64, dpitch: f64) {
        self.yaw = (self.yaw + dyaw).rem_euclid(std::f64::consts::TAU);
        self.pitch = (self.pitch + dpitch).clamp(PITCH_MIN, PITCH_MAX);
    }

    /// Multiplies the distance by `factor` (below 1 comes closer).
    pub fn zoom(&mut self, factor: f64) {
        self.dist = (self.dist * factor).max(self.min_dist).min(1.0e6);
    }

    /// Slides the target on the ground: `right` and `forward` in metres, along the view. The target
    /// stays on the terrain, and the eye above it (see `lift`).
    pub fn pan(&mut self, t: &Terrain, exaggeration: f32, right: f64, forward: f64) {
        let (s, c) = self.yaw.sin_cos();
        // Never off the ground: there is nothing to look at beyond it.
        let [w, h] = t.extent();
        self.target[0] = (self.target[0] + right * c + forward * s).clamp(0.0, w);
        self.target[1] = (self.target[1] - right * s + forward * c).clamp(0.0, h);
        self.target[2] = t.height_at(self.target[0], self.target[1]);
        self.lift(t, exaggeration);
    }

    /// Raises the pitch, then backs off, until the eye is 20 m over the ground under it.
    pub fn lift(&mut self, t: &Terrain, exaggeration: f32) {
        let ex = exaggeration as f64;
        for _ in 0..200 {
            let (eye, _) = self.basis(ex);
            if eye[2] >= t.height_at(eye[0], eye[1]) * ex + 20.0 {
                break;
            }
            if self.pitch < PITCH_MAX {
                self.pitch = (self.pitch + 2f64.to_radians()).min(PITCH_MAX);
            } else {
                self.dist *= 1.1;
            }
        }
    }

    /// Pans as a drag of `dx`, `dy` pixels in a view `h` high: what is under the pointer near the
    /// target follows it.
    pub fn pan_screen(&mut self, t: &Terrain, exaggeration: f32, dx: f64, dy: f64, h: f64) {
        let k = self.dist * 2.0 * (self.fov / 2.0).tan() / h.max(1.0);
        // Dragging the ground right moves the target left; up on screen is forward on the ground.
        self.pan(t, exaggeration, -dx * k, dy * k / self.pitch.sin());
    }

    /// Eye position and the unit vectors right, up, forward, in scaled (exaggerated) metres.
    fn basis(&self, ex: f64) -> ([f64; 3], [[f64; 3]; 3]) {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let f = [sy * cp, cy * cp, -sp];
        let r = [cy, -sy, 0.0];
        let u = [sy * sp, cy * sp, cp];
        let tg = [self.target[0], self.target[1], self.target[2] * ex];
        ([tg[0] - f[0] * self.dist, tg[1] - f[1] * self.dist, tg[2] - f[2] * self.dist], [r, u, f])
    }
}

/// A point in view space: (right, up, depth along the view).
fn to_cam(eye: &[f64; 3], b: &[[f64; 3]; 3], p: [f64; 3]) -> [f64; 3] {
    let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
    [0, 1, 2].map(|k| d[0] * b[k][0] + d[1] * b[k][1] + d[2] * b[k][2])
}

/// (screen x, screen y, 1 / depth) of a view-space point in front of the camera.
fn screen(c: [f64; 3], focal: f64, w: f64, h: f64) -> [f32; 3] {
    [(w / 2.0 + c[0] / c[2] * focal) as f32, (h / 2.0 - c[1] / c[2] * focal) as f32, (1.0 / c[2]) as f32]
}

/// A triangle (view-space x, y, depth, then u, v, light) cut by the near plane: 0 to 4 vertices.
fn clip_near(tri: [&[f64; 6]; 3]) -> ([[f64; 6]; 4], usize) {
    let mut out = [[0.0; 6]; 4];
    let mut n = 0;
    for k in 0..3 {
        let (a, b) = (tri[k], tri[(k + 1) % 3]);
        let (ina, inb) = (a[2] >= NEAR, b[2] >= NEAR);
        if ina {
            out[n] = *a;
            n += 1;
        }
        if ina != inb {
            let t = (NEAR - a[2]) / (b[2] - a[2]);
            out[n] = std::array::from_fn(|i| a[i] + t * (b[i] - a[i]));
            out[n][2] = NEAR;
            n += 1;
        }
    }
    (out, n)
}

/// Screen position of a point `[east, north, altitude]` (local metres) in a `w` × `h` picture.
pub fn project(_t: &Terrain, cam: &Camera, exaggeration: f32, p: [f64; 3], w: usize, h: usize) -> Option<(f32, f32)> {
    let ex = exaggeration as f64;
    let (eye, b) = cam.basis(ex);
    let focal = h as f64 / 2.0 / (cam.fov / 2.0).tan();
    let c = to_cam(&eye, &b, [p[0], p[1], p[2] * ex]);
    (c[2] > NEAR).then(|| screen(c, focal, w as f64, h as f64)).map(|v| (v[0], v[1]))
}

/// Draws `t` with `texture` laid over it (its light is in the picture), the `solids` standing on
/// it, then `line` over it, edged with the background's colour. Each line point is `([east,
/// north, altitude], colour of the segment that starts there)`. `smooth` draws at twice the size
/// and averages, for soft edges (about a third more time: the triangles' set-up costs more than
/// their pixels). Returns `w * h` pixels of 4 bytes: the colour bytes as given, alpha 255;
/// `background` where nothing is drawn, and under the ground's faded edges.
#[allow(clippy::too_many_arguments)]
pub fn render(t: &Terrain, texture: &Texture, solids: &[Face], cam: &Camera, exaggeration: f32, line: &[([f64; 3], [u8; 3])], w: usize, h: usize, background: [u8; 4], smooth: bool) -> Vec<u8> {
    let mut out: Vec<u8> = background.iter().copied().cycle().take(w * h * 4).collect();
    if w == 0 || h == 0 || t.cols < 2 || t.rows < 2 || texture.w < 2 || texture.h < 2 || texture.px.len() < texture.w * texture.h {
        return out;
    }
    // Everything below is drawn on `ss` × `ss` samples per pixel.
    let ss = if smooth { 2 } else { 1 };
    let (sw, sh) = (w * ss, h * ss);
    let ex = exaggeration as f64;
    let (eye, b) = cam.basis(ex);
    let focal = sh as f64 / 2.0 / (cam.fov / 2.0).tan();
    let (wf, hf) = (sw as f64, sh as f64);

    // The triangles' set-up is shared between the processors, each taking a run of the ground's
    // rows or of the solids: it costs more than the pixels.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    // Vertices in view space, with their place on the texture and their light.
    let (cols, rows) = (t.cols, t.rows);
    let mut verts = vec![[0f64; 6]; cols * rows];
    let per = rows.div_ceil(threads).max(1);
    std::thread::scope(|sc| {
        for (n, part) in verts.chunks_mut(per * cols).enumerate() {
            let (eye, b) = (&eye, &b);
            sc.spawn(move || {
                for (k, v) in part.iter_mut().enumerate() {
                    let (i, j) = (k % cols, n * per + k / cols);
                    let s = to_cam(eye, b, [i as f64 * t.dx, j as f64 * t.dy, t.heights[j * cols + i] as f64 * ex]);
                    // The ground's light is in its picture (`Texture::lit`), finer than the vertices.
                    *v = [s[0], s[1], s[2], i as f64 / (cols - 1) as f64, j as f64 / (rows - 1) as f64, 1.0];
                }
            });
        }
    });
    // Screen triangles, cut at the near plane and the rest fanned. Each band of rows keeps the
    // list of the triangles that touch it; those out of the picture are dropped, and so are
    // the faces of a solid that turn their back (`cull`).
    type Tris = (Vec<[[f32; 6]; 3]>, Vec<Vec<u32>>);
    let band = (BAND * ss) as f32;
    let bands = h.div_ceil(BAND);
    let push = |(tris, bins): &mut Tris, tri: [&[f64; 6]; 3], cull: bool| {
        let (poly, n) = clip_near(tri);
        let p = poly.map(|v| {
            let s = screen([v[0], v[1], v[2].max(NEAR)], focal, wf, hf);
            [s[0], s[1], s[2], v[3] as f32, v[4] as f32, v[5] as f32]
        });
        for m in 1..n.saturating_sub(1) {
            let (a, b, c) = (p[0], p[m], p[m + 1]);
            let (top, bottom) = (a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]));
            if bottom < 0.0 || top >= hf as f32 || a[0].max(b[0]).max(c[0]) < 0.0 || a[0].min(b[0]).min(c[0]) >= wf as f32 {
                continue;
            }
            // Seen from outside, a face goes round clockwise on screen (y grows downwards).
            if cull && (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) >= 0.0 {
                continue;
            }
            let last = ((bottom / band) as usize).min(bins.len() - 1);
            for bin in &mut bins[(top.max(0.0) / band) as usize..=last] {
                bin.push(tris.len() as u32);
            }
            tris.push([a, b, c]);
        }
    };
    let (verts, push, eye_, b_) = (&verts, &push, &eye, &b);
    let cell_rows: Vec<usize> = (0..rows - 1).collect();
    let (ground_parts, solid_parts): (Vec<Tris>, Vec<Tris>) = std::thread::scope(|sc| {
        let ground: Vec<_> = cell_rows
            .chunks(cell_rows.len().div_ceil(threads).max(1))
            .map(|run| {
                sc.spawn(move || {
                    let mut out: Tris = (Vec::with_capacity(2 * run.len() * (cols - 1)), vec![Vec::new(); bands]);
                    for &j in run {
                        for i in 0..cols - 1 {
                            let (v00, v10, v01, v11) = (&verts[j * cols + i], &verts[j * cols + i + 1], &verts[(j + 1) * cols + i], &verts[(j + 1) * cols + i + 1]);
                            push(&mut out, [v00, v10, v11], false);
                            push(&mut out, [v00, v11, v01], false);
                        }
                    }
                    out
                })
            })
            .collect();
        // The solids' faces have their colour where the ground's have their place on the texture.
        let solid: Vec<_> = solids
            .chunks(solids.len().div_ceil(threads).max(1))
            .map(|run| {
                sc.spawn(move || {
                    let mut out: Tris = (Vec::with_capacity(run.len()), vec![Vec::new(); bands]);
                    for f in run {
                        let v = f.p.map(|p| {
                            let c = to_cam(eye_, b_, [p[0] as f64, p[1] as f64, p[2] as f64 * ex]);
                            [c[0], c[1], c[2], f.colour[0] as f64, f.colour[1] as f64, f.colour[2] as f64]
                        });
                        if v.iter().all(|v| v[..3].iter().all(|x| x.is_finite())) {
                            push(&mut out, [&v[0], &v[1], &v[2]], true);
                        }
                    }
                    out
                })
            })
            .collect();
        let done = |hs: Vec<std::thread::ScopedJoinHandle<Tris>>| hs.into_iter().map(|h| h.join().expect("a drawing thread")).collect();
        (done(ground), done(solid))
    });
    // One list: the ground's triangles first, then the solids'.
    let mut tris: Vec<[[f32; 6]; 3]> = Vec::with_capacity(ground_parts.iter().chain(&solid_parts).map(|p| p.0.len()).sum());
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); bands];
    let (mut ground_tris, ground_runs) = (0, ground_parts.len());
    for (n, (part, part_bins)) in ground_parts.into_iter().chain(solid_parts).enumerate() {
        let from = tris.len() as u32;
        tris.extend(part);
        for (bin, more) in bins.iter_mut().zip(part_bins) {
            bin.extend(more.into_iter().map(|k| k + from));
        }
        if n < ground_runs {
            ground_tris = tris.len();
        }
    }
    // Line segments cut at the near plane; a point that is not a number cuts the line there.
    let cams: Vec<[f64; 3]> = line.iter().map(|(p, _)| to_cam(&eye, &b, [p[0], p[1], p[2] * ex])).collect();
    let mut segs: Vec<([f32; 3], [f32; 3], [u8; 3])> = Vec::new();
    for (k, pair) in cams.windows(2).enumerate() {
        let (mut a, mut b) = (pair[0], pair[1]);
        if !(a.iter().chain(&b).all(|v| v.is_finite())) || (a[2] < NEAR && b[2] < NEAR) {
            continue;
        }
        let cut = |p: [f64; 3], q: [f64; 3]| {
            let t = (NEAR - p[2]) / (q[2] - p[2]);
            [p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1]), NEAR]
        };
        if a[2] < NEAR {
            a = cut(a, b);
        } else if b[2] < NEAR {
            b = cut(b, a);
        }
        segs.push((screen(a, focal, wf, hf), screen(b, focal, wf, hf), line[k].1));
    }
    let radius = (sw as f32 * 3.0 / 1200.0).max(1.0);
    let edging = radius + (radius * 0.4).max(1.0);
    let edge_colour = [background[0], background[1], background[2]];
    let ground = Ground { texture, background: background.map(|v| v as f32) };

    let bands = Mutex::new(out.chunks_mut(BAND * w * 4).enumerate());
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| {
                let mut zbuf = vec![0f32; BAND * ss * sw];
                let mut buf = vec![0u8; BAND * ss * sw * 4];
                let mut veiled = vec![false; BAND * ss * sw];
                loop {
                    let next = bands.lock().unwrap().next();
                    let Some((n, rows_out)) = next else { break };
                    let rows = rows_out.len() / (w * 4);
                    let (y0, y1) = (n * BAND * ss, (n * BAND + rows) * ss);
                    let buf = &mut buf[..rows * ss * sw * 4];
                    buf.chunks_exact_mut(4).for_each(|p| p.copy_from_slice(&background));
                    zbuf.iter_mut().for_each(|z| *z = 0.0);
                    for k in &bins[n] {
                        let v = &tris[*k as usize];
                        triangle([&v[0], &v[1], &v[2]], ((*k as usize) < ground_tris).then_some(&ground), y0, y1, sw, buf, &mut zbuf);
                    }
                    // The edging first and without depth of its own: no segment's edging covers
                    // another's line. Then what a hill or a building hides of the line, faint and
                    // without edging, so that it is not lost in a town; then the line in sight.
                    for (a, b, _) in &segs {
                        segment(a, b, edge_colour, edging, Part::Edging, y0, y1, sw, buf, &mut zbuf, &mut veiled);
                    }
                    veiled.iter_mut().for_each(|v| *v = false);
                    for (a, b, color) in &segs {
                        segment(a, b, *color, radius, Part::Hidden, y0, y1, sw, buf, &mut zbuf, &mut veiled);
                    }
                    for (a, b, color) in &segs {
                        segment(a, b, *color, radius, Part::Seen, y0, y1, sw, buf, &mut zbuf, &mut veiled);
                    }
                    if ss == 1 {
                        rows_out.copy_from_slice(buf);
                        continue;
                    }
                    // Each pixel is the mean of its samples.
                    for (y, row) in rows_out.chunks_exact_mut(w * 4).enumerate() {
                        let (top, bottom) = buf[y * 2 * sw * 4..(y * 2 + 2) * sw * 4].split_at(sw * 4);
                        for (x, p) in row.chunks_exact_mut(4).enumerate() {
                            for k in 0..4 {
                                let i = x * 8 + k;
                                p[k] = ((top[i] as u16 + top[i + 4] as u16 + bottom[i] as u16 + bottom[i + 4] as u16 + 2) / 4) as u8;
                            }
                        }
                    }
                }
            });
        }
    });
    out
}

/// What a point of the ground looks like: its picture, and the background it fades into.
struct Ground<'a> {
    texture: &'a Texture,
    background: [f32; 4],
}

fn put(px: &mut [u8], c: [f32; 4]) {
    px.copy_from_slice(&c.map(|v| (v + 0.5).clamp(0.0, 255.0) as u8));
}

/// One triangle, restricted to rows `y0..y1`: of the ground `g` (place on the texture, light and
/// 1/depth are interpolated on screen), or else of one colour, held by its vertices.
/// ponytail: the texture is not perspective-corrected; with small cells the error is invisible.
fn triangle(v: [&[f32; 6]; 3], g: Option<&Ground>, y0: usize, y1: usize, w: usize, rows_out: &mut [u8], zbuf: &mut [f32]) {
    let [a, b, c] = v;
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if area.abs() < 1e-6 {
        return;
    }
    let (minx, maxx) = (a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
    let (miny, maxy) = (a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]));
    let xs = (minx - 0.5).ceil().max(0.0) as usize;
    let xe = (maxx - 0.5).floor().min(w as f32 - 1.0);
    let ys = (miny - 0.5).ceil().max(y0 as f32) as usize;
    let ye = (maxy - 0.5).floor().min(y1 as f32 - 1.0);
    if xe < 0.0 || ye < 0.0 || xs > xe as usize || ys > ye as usize {
        return;
    }
    let inv = 1.0 / area;
    // Barycentric weights are linear in x and y.
    let edge = |p: &[f32; 6], q: &[f32; 6], x: f32, y: f32| ((q[0] - p[0]) * (y - p[1]) - (q[1] - p[1]) * (x - p[0])) * inv;
    for y in ys..=ye as usize {
        let py = y as f32 + 0.5;
        for x in xs..=xe as usize {
            let px = x as f32 + 0.5;
            let (wa, wb, wc) = (edge(b, c, px, py), edge(c, a, px, py), edge(a, b, px, py));
            if wa < -1e-4 || wb < -1e-4 || wc < -1e-4 {
                continue;
            }
            let iz = wa * a[2] + wb * b[2] + wc * c[2];
            let zi = (y - y0) * w + x;
            if iz > zbuf[zi] {
                zbuf[zi] = iz;
                let Some(g) = g else {
                    put(&mut rows_out[zi * 4..zi * 4 + 4], [a[3], a[4], a[5], 255.0]);
                    continue;
                };
                let [u, v, light] = [3, 4, 5].map(|k| wa * a[k] + wb * b[k] + wc * c[k]);
                let (tex, shown, bg) = (g.texture.sample(u, v), g.texture.shown(u, v), g.background);
                let lit = [tex[0] * light, tex[1] * light, tex[2] * light, 255.0];
                put(&mut rows_out[zi * 4..zi * 4 + 4], [0, 1, 2, 3].map(|k| bg[k] + (lit[k] - bg[k]) * shown));
            }
        }
    }
}

/// What is drawn of a segment of the line.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    /// Where it is in sight, without hiding what is drawn afterwards.
    Edging,
    /// Where something stands in front of it: laid faintly over that, once per sample (`veiled`),
    /// so that the segments' overlapping ends do not add up.
    Hidden,
    /// Where it is in sight; it hides what is drawn behind it afterwards.
    Seen,
}

/// How much of the line shows through what hides it.
const HIDDEN_LINE: f32 = 0.4;

/// A thick segment (a capsule on screen), nearer than the surface by `LINE_BIAS`.
#[allow(clippy::too_many_arguments)]
fn segment(a: &[f32; 3], b: &[f32; 3], color: [u8; 3], r: f32, part: Part, y0: usize, y1: usize, w: usize, rows_out: &mut [u8], zbuf: &mut [f32], veiled: &mut [bool]) {
    let ys = (a[1].min(b[1]) - r).floor().max(y0 as f32) as usize;
    let ye = (a[1].max(b[1]) + r).ceil().min(y1 as f32 - 1.0);
    let xs = (a[0].min(b[0]) - r).floor().max(0.0) as usize;
    let xe = (a[0].max(b[0]) + r).ceil().min(w as f32 - 1.0);
    if xe < 0.0 || ye < 0.0 || ys > ye as usize || xs > xe as usize {
        return;
    }
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let col = [color[0] as f32, color[1] as f32, color[2] as f32, 255.0];
    for y in ys..=ye as usize {
        for x in xs..=xe as usize {
            let (px, py) = (x as f32 + 0.5 - a[0], y as f32 + 0.5 - a[1]);
            let t = if len2 > 1e-9 { ((px * dx + py * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let (ex, ey) = (px - t * dx, py - t * dy);
            if ex * ex + ey * ey > r * r {
                continue;
            }
            let iz = (a[2] + t * (b[2] - a[2])) * LINE_BIAS;
            let zi = (y - y0) * w + x;
            let px = &mut rows_out[zi * 4..zi * 4 + 4];
            match (part, iz > zbuf[zi]) {
                (Part::Edging, true) => put(px, col),
                (Part::Seen, true) => {
                    zbuf[zi] = iz;
                    put(px, col);
                }
                (Part::Hidden, false) if !veiled[zi] => {
                    veiled[zi] = true;
                    put(px, [0, 1, 2, 3].map(|k| px[k] as f32 + (col[k] - px[k] as f32) * HIDDEN_LINE));
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: usize, f: impl Fn(usize, usize) -> f32) -> Terrain {
        let heights = (0..n * n).map(|k| f(k % n, k / n)).collect();
        Terrain { cols: n, rows: n, heights, dx: 100.0, dy: 100.0, south: 44.0, west: 6.0, mx: 80_000.0, my: 110_540.0 }
    }

    fn pixel(img: &[u8], w: usize, x: f32, y: f32) -> [u8; 4] {
        let i = (y as usize * w + x as usize) * 4;
        img[i..i + 4].try_into().unwrap()
    }

    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    /// One colour all over, cut sharp at the edges.
    fn plain(c: [u8; 3]) -> Texture {
        Texture { w: 2, h: 2, px: vec![c; 4], fade: 0.0 }
    }

    #[test]
    fn flat_terrain_from_above_shows_its_lit_colour() {
        let t = grid(21, |_, _| 0.0);
        let cam = Camera { target: [1000.0, 1000.0, 0.0], yaw: 0.0, pitch: PITCH_MAX, dist: 3000.0, min_dist: 300.0, fov: 0.8 };
        let img = render(&t, &plain([200, 100, 50]), &[], &cam, 1.0, &[], 200, 150, CLEAR, false);
        let p = pixel(&img, 200, 100.0, 75.0);
        for (got, want) in p.iter().zip([200.0, 100.0, 50.0, 255.0]) {
            assert!((*got as f32 - want).abs() <= 1.5, "{p:?} vs {want}");
        }
        assert_eq!(pixel(&img, 200, 0.0, 0.0), CLEAR, "the corner is background");
    }

    /// A ridge across the whole grid at row 5, and a red line behind it, seen from the south.
    #[allow(clippy::type_complexity)]
    fn ridge_scene(ridge: f32) -> (Terrain, Camera, Vec<([f64; 3], [u8; 3])>) {
        let t = grid(21, |_, j| if j == 5 { ridge } else { 0.0 });
        let cam = Camera { target: [1000.0, 1000.0, 0.0], yaw: 0.0, pitch: 15f64.to_radians(), dist: 4000.0, min_dist: 300.0, fov: 0.8 };
        let line = vec![([900.0, 1500.0, 0.0], [255, 0, 0]), ([1100.0, 1500.0, 0.0], [255, 0, 0])];
        (t, cam, line)
    }

    #[test]
    fn line_shows_on_flat_ground() {
        let (t, cam, line) = ridge_scene(0.0);
        let img = render(&t, &plain([40, 160, 40]), &[], &cam, 1.0, &line, 1200, 800, CLEAR, false);
        let (x, y) = project(&t, &cam, 1.0, [1000.0, 1500.0, 0.0], 1200, 800).unwrap();
        assert_eq!(pixel(&img, 1200, x, y)[..3], [255, 0, 0]);
        // About 3 px thick: the pixels 4 px above and below are ground.
        assert_ne!(pixel(&img, 1200, x, y - 4.0)[..3], [255, 0, 0]);
        assert_ne!(pixel(&img, 1200, x, y + 4.0)[..3], [255, 0, 0]);
    }

    #[test]
    fn a_peak_in_front_hides_the_line() {
        let (t, cam, line) = ridge_scene(1500.0);
        let img = render(&t, &plain([40, 160, 40]), &[], &cam, 1.0, &line, 1200, 800, CLEAR, false);
        let (x, y) = project(&t, &cam, 1.0, [1000.0, 1500.0, 0.0], 1200, 800).unwrap();
        assert_eq!(pixel(&img, 1200, x, y)[3], 255, "the ridge covers that pixel");
        // The line shows faintly through the ridge (40 % of its red over the ridge's green),
        // without its edging, and evenly: where two segments overlap it is no stronger.
        let ridge = pixel(&img, 1200, x, y + 40.0);
        assert_eq!(ridge, [40, 160, 40, 255]);
        let faint = [(40.0 + 215.0 * 0.4) as u8, (160.0 * 0.6) as u8, (40.0 * 0.6) as u8, 255];
        assert_eq!(pixel(&img, 1200, x, y), faint);
        let joined = vec![line[0], ([1000.0, 1500.0, 0.0], [255, 0, 0]), ([1000.0, 1500.0, 0.0], [255, 0, 0]), line[1]];
        let img = render(&t, &plain([40, 160, 40]), &[], &cam, 1.0, &joined, 1200, 800, CLEAR, false);
        assert_eq!(pixel(&img, 1200, x, y), faint);
        let around: Vec<[u8; 4]> = (-12..=12).map(|d| pixel(&img, 1200, x, y + d as f32)).collect();
        assert!(around.iter().all(|p| *p == faint || *p == ridge), "no edging: {around:?}");
    }

    #[test]
    fn target_projects_to_the_centre_and_behind_is_none() {
        let t = grid(5, |_, _| 300.0);
        let mut cam = Camera::fit(&t);
        cam.orbit(1.0, 0.3);
        cam.zoom(0.5);
        let (x, y) = project(&t, &cam, 2.0, cam.target, 1200, 800).unwrap();
        assert!((x - 600.0).abs() < 0.01 && (y - 400.0).abs() < 0.01, "{x} {y}");
        let eye_side = [cam.target[0] - 1.0e5 * cam.yaw.sin(), cam.target[1] - 1.0e5 * cam.yaw.cos(), 0.0];
        assert!(project(&t, &cam, 1.0, eye_side, 1200, 800).is_none());
        cam.orbit(0.0, 10.0);
        assert!((cam.pitch - PITCH_MAX).abs() < 1e-9);
        cam.orbit(0.0, -10.0);
        assert!((cam.pitch - PITCH_MIN).abs() < 1e-9);
    }

    #[test]
    fn tiles_join_without_a_seam() {
        // Two tiles side by side at zoom 10; the altitude is the world x pixel, so a ramp.
        let z = 10u8;
        let tile = |x: u32, y: u32| -> Option<Arc<Vec<f32>>> {
            (y == 400 && (x == 500 || x == 501)).then(|| Arc::new((0..256 * 256).map(|i| (x - 500) as f32 * 256.0 + (i % 256) as f32).collect()))
        };
        // Spans the boundary at world x = 501 * 256.
        let lon_at = |wx: f64| wx / (256.0 * 1024.0) * 360.0 - 180.0;
        let lat_at = |wy: f64| (std::f64::consts::PI * (1.0 - 2.0 * wy / (256.0 * 1024.0))).sinh().atan().to_degrees();
        let bbox = [lat_at(400.0 * 256.0 + 200.0), lon_at(500.0 * 256.0 + 156.0), lat_at(400.0 * 256.0 + 100.0), lon_at(500.0 * 256.0 + 356.0)];
        let t = Terrain::from_tiles(bbox, 41 * 41, z, &tile).expect("both tiles present");
        assert!(t.cols * t.rows <= 41 * 41);
        // Heights follow the ramp (value = wx - 0.5) on both sides of the seam, with no jump.
        let mut prev = None;
        for i in 0..t.cols {
            let lon = bbox[1] + (bbox[3] - bbox[1]) * i as f64 / (t.cols - 1) as f64;
            let want = world(bbox[0], lon, z as f64).0 - 500.0 * 256.0 - 0.5;
            let got = t.heights[i] as f64;
            assert!((got - want).abs() < 0.05, "col {i}: {got} vs {want}");
            if let Some(p) = prev {
                assert!((got - p) < 12.0 && got > p, "steady ramp across the seam");
            }
            prev = Some(got);
        }
        // A missing tile is no terrain.
        assert!(Terrain::from_tiles(bbox, 400, z, &|_, _| None).is_none());
        let [e, n] = t.local(bbox[2], bbox[3]);
        assert!((e - (t.cols - 1) as f64 * t.dx).abs() < 1e-6 && (n - (t.rows - 1) as f64 * t.dy).abs() < 1e-6);
    }

    /// Share of background pixels in the lower half of a 1200 x 800 picture.
    fn lower_half_background(img: &[u8]) -> f64 {
        let n = img[400 * 1200 * 4..].chunks(4).filter(|p| p[3] == 0).count();
        n as f64 / (400.0 * 1200.0)
    }

    #[test]
    fn a_close_camera_leaves_no_hole_in_the_ground() {
        let t = grid(301, |_, _| 100.0);
        for dist in [50.0, 200.0, 300.0] {
            let cam = Camera { target: [15_000.0, 15_000.0, 100.0], yaw: 0.4, pitch: PITCH_MIN, dist, min_dist: 300.0, fov: 0.8 };
            let img = render(&t, &plain([40, 160, 40]), &[], &cam, 1.0, &[], 1200, 800, CLEAR, false);
            let bg = lower_half_background(&img);
            assert!(bg < 0.01, "dist {dist}: {:.1} % background", bg * 100.0);
        }
    }

    #[test]
    fn a_line_crossing_the_near_plane_is_drawn_up_to_it() {
        let t = grid(31, |_, _| 0.0);
        let cam = Camera { target: [1500.0, 1500.0, 0.0], yaw: 0.0, pitch: PITCH_MIN, dist: 200.0, min_dist: 300.0, fov: 0.8 };
        // From ahead of the eye to behind it.
        let line = [([1500.0, 1450.0, 0.0], [255, 0, 0]), ([1500.0, 1100.0, 0.0], [255, 0, 0])];
        let img = render(&t, &plain([40, 160, 40]), &[], &cam, 1.0, &line, 1200, 800, CLEAR, false);
        assert_eq!(pixel(&img, 1200, 600.0, 799.0)[..3], [255, 0, 0], "the line reaches the bottom edge");
    }

    #[test]
    fn a_nan_point_cuts_the_line_without_painting_a_band() {
        let t = grid(21, |_, _| 0.0);
        let cam = Camera { target: [1000.0, 1000.0, 0.0], yaw: 0.0, pitch: 40f64.to_radians(), dist: 3000.0, min_dist: 300.0, fov: 0.8 };
        let red = [255, 0, 0];
        let line = [([f64::NAN, 1000.0, 0.0], red), ([900.0, 1000.0, 0.0], red), ([1100.0, 1000.0, 0.0], red)];
        let img = render(&t, &plain([40, 160, 40]), &[], &cam, 1.0, &line, 1200, 800, CLEAR, false);
        let (x, y) = project(&t, &cam, 1.0, [1000.0, 1000.0, 0.0], 1200, 800).unwrap();
        assert_eq!(pixel(&img, 1200, x, y)[..3], [255, 0, 0], "the good segment is drawn");
        let reds = img.chunks(4).filter(|p| p[..3] == [255, 0, 0]).count();
        assert!(reds < 2000, "{reds} red pixels");
    }

    #[test]
    fn an_empty_grid_has_no_extent_to_overflow() {
        let t = Terrain { cols: 0, rows: 0, heights: vec![], dx: 0.0, dy: 0.0, south: 0.0, west: 0.0, mx: 1.0, my: 1.0 };
        let cam = Camera::fit(&t);
        assert_eq!(render(&t, &plain([0; 3]), &[], &cam, 1.0, &[], 4, 4, [1, 2, 3, 4], false).len(), 64);
        assert_eq!(t.height_at(5.0, 5.0), 0.0);
    }

    #[test]
    fn zoom_stops_at_three_cells() {
        let t = grid(21, |_, _| 0.0);
        let mut cam = Camera::fit(&t);
        cam.zoom(1e-9);
        assert_eq!(cam.dist, 300.0);
    }

    #[test]
    fn pan_keeps_the_target_on_the_ground_and_the_eye_above_it() {
        // A 45° slope rising to the east; looking west, down it, the eye sits up the slope.
        let t = grid(21, |i, _| i as f32 * 100.0);
        for ex in [1.0f32, 2.0] {
            let mut cam = Camera { target: [1500.0, 1000.0, 1500.0], yaw: 270f64.to_radians(), pitch: PITCH_MIN, dist: 800.0, min_dist: 300.0, fov: 0.8 };
            cam.pan(&t, ex, 0.0, 1000.0);
            assert!((cam.target[0] - 500.0).abs() < 1e-6 && (cam.target[2] - 500.0).abs() < 1e-6, "{:?}", cam.target);
            let (eye, _) = cam.basis(ex as f64);
            assert!(eye[2] >= t.height_at(eye[0], eye[1]) * ex as f64 + 20.0 - 1e-6, "ex {ex}: eye {eye:?}");
        }
    }

    #[test]
    fn tiles_join_north_to_south_too() {
        // Two tiles stacked at zoom 10; the altitude is the world y pixel, so a ramp going south.
        let z = 10u8;
        let tile = |x: u32, y: u32| -> Option<Arc<Vec<f32>>> {
            (x == 500 && (y == 400 || y == 401)).then(|| Arc::new((0..256 * 256).map(|i| (y - 400) as f32 * 256.0 + (i / 256) as f32).collect()))
        };
        let lon_at = |wx: f64| wx / (256.0 * 1024.0) * 360.0 - 180.0;
        let lat_at = |wy: f64| (std::f64::consts::PI * (1.0 - 2.0 * wy / (256.0 * 1024.0))).sinh().atan().to_degrees();
        let bbox = [lat_at(400.0 * 256.0 + 356.0), lon_at(500.0 * 256.0 + 100.0), lat_at(400.0 * 256.0 + 156.0), lon_at(500.0 * 256.0 + 200.0)];
        let t = Terrain::from_tiles(bbox, 41 * 41, z, &tile).expect("both tiles present");
        let mut prev = None;
        for j in 0..t.rows {
            let lat = bbox[0] + (bbox[2] - bbox[0]) * j as f64 / (t.rows - 1) as f64;
            let want = world(lat, bbox[1], z as f64).1 - 400.0 * 256.0 - 0.5;
            let got = t.heights[j * t.cols] as f64;
            assert!((got - want).abs() < 0.05, "row {j}: {got} vs {want}");
            if let Some(p) = prev {
                assert!(got < p && p - got < 12.0, "steady ramp across the seam, north is lower");
            }
            prev = Some(got);
        }
    }

    #[test]
    fn map_tiles_are_laid_north_up_pixel_for_pixel() {
        // Tiles whose pixels tell where they are: column, row, tile.
        let z = 12u8;
        let tiles: std::collections::HashMap<(u32, u32), Vec<u8>> = [(2130u32, 1493u32), (2131, 1493), (2130, 1494), (2131, 1494)]
            .into_iter()
            .map(|(x, y)| ((x, y), (0..256 * 256).flat_map(|k| [(k % 256) as u8, (k / 256) as u8, ((x - 2130) * 2 + y - 1493) as u8, 255]).collect()))
            .collect();
        // A box across the corner where the four tiles meet.
        let lat = |py: f64| (std::f64::consts::PI * (1.0 - 2.0 * py / (256.0 * 4096.0))).sinh().atan().to_degrees();
        let lon = |px: f64| px / (256.0 * 4096.0) * 360.0 - 180.0;
        let (cx, cy) = (2131.0 * 256.0, 1494.0 * 256.0);
        let bbox = [lat(cy + 40.0), lon(cx - 60.0), lat(cy - 20.0), lon(cx + 30.0)];
        let t = Texture::from_tiles(bbox, z, 0.1, &|x, y| tiles.get(&(x, y)).map(|v| v.as_slice())).unwrap();
        assert_eq!((t.w, t.h, t.px.len()), (90, 60, 90 * 60));
        // South-west first: column 196 of the west tiles, row 39 of the south ones.
        assert_eq!(t.px[0], [196, 39, 1]);
        assert_eq!(t.px[89], [29, 39, 3], "south-east");
        assert_eq!(t.px[59 * 90], [196, 236, 0], "north-west");
        assert_eq!(t.px[59 * 90 + 89], [29, 236, 2], "north-east");
        // Between texels the colours blend; a missing tile, or a short one: no picture.
        assert_eq!(t.sample(0.0, 0.0), [196.0, 39.0, 1.0]);
        let mid = t.sample(1.0 / 90.0, 0.0);
        assert!((mid[0] - 196.5).abs() < 0.01, "{mid:?}");
        assert!(Texture::from_tiles(bbox, z, 0.1, &|x, y| tiles.get(&(x, y)).filter(|_| x == 2130).map(|v| v.as_slice())).is_none());
        assert!(Texture::from_tiles(bbox, z, 0.1, &|_, _| Some(&[0u8; 16][..])).is_none());
        assert!(Texture::from_tiles([44.0, 6.0, 44.0, 6.0], z, 0.1, &|_, _| None).is_none(), "an empty box");
    }

    #[test]
    fn slopes_take_the_light_tile_pixel_by_tile_pixel() {
        // One tile at zoom 13 around 43.7°N: planes falling by 10 m a pixel (about 14 m) to the
        // north-west, which face the light, or rising that way, which turn their back to it.
        let (z, tx, ty) = (13u8, 4261u32, 2987u32);
        let tile = |f: &dyn Fn(i32, i32) -> f32| Arc::new((0..256 * 256).map(|k| f(k % 256, k / 256)).collect::<Vec<f32>>());
        let lat = |py: f64| (std::f64::consts::PI * (1.0 - 2.0 * py / (256.0 * 8192.0))).sinh().atan().to_degrees();
        let lon = |px: f64| px / (256.0 * 8192.0) * 360.0 - 180.0;
        let (x0, y0) = (tx as f64 * 256.0, ty as f64 * 256.0);
        let bbox = [lat(y0 + 200.0), lon(x0 + 50.0), lat(y0 + 100.0), lon(x0 + 150.0)];
        let shade = |f: &dyn Fn(i32, i32) -> f32, ex: f32| {
            let t = tile(f);
            Shade::from_tiles(bbox, z, ex, &|x, y| ((x, y) == (tx, ty)).then(|| t.clone())).unwrap()
        };
        let flat = shade(&|_, _| 250.0, 1.5);
        assert_eq!((flat.w, flat.h), (100, 100));
        assert!(flat.light.iter().all(|l| *l == 128), "flat ground keeps its colours");
        let towards = shade(&|x, y| 10.0 * (x + y) as f32, 1.0);
        let away = shade(&|x, y| 5000.0 - 10.0 * (x + y) as f32, 1.0);
        assert!(towards.light.iter().all(|l| *l > 140) && away.light.iter().all(|l| *l < 70), "{:?} {:?}", (towards.light.iter().min(), towards.light.iter().max()), (away.light.iter().min(), away.light.iter().max()));
        // Steeper when the heights are drawn higher.
        assert!(shade(&|x, y| 5000.0 - (x + y) as f32, 2.0).light[0] < shade(&|x, y| 5000.0 - (x + y) as f32, 1.0).light[0]);
        // A furrow one tile pixel wide, running north, at column 100: its west bank faces east,
        // away from the light, and is darker than its east bank, texel by texel.
        let furrow = shade(&|x, _| if x == 100 { 200.0 } else { 250.0 }, 1.0);
        let row = &furrow.light[50 * 100..51 * 100];
        assert!(row[49] < 128 && row[51] > 128 && row[40] == 128 && row[60] == 128, "{:?}", &row[45..56]);
        // A missing tile: no light.
        assert!(Shade::from_tiles(bbox, z, 1.0, &|_, _| None).is_none());
        // On a picture: each texel times its light, whatever the two sizes.
        let lit = Texture { w: 2, h: 2, px: vec![[100, 200, 50]; 4], fade: 0.0 }.lit(&away);
        assert!(lit.px.iter().all(|p| p[0] < 55 && p[1] < 110 && p[1] == 2 * p[0] || p[1] == 2 * p[0] + 1 || p[1] + 1 == 2 * p[0]), "{:?}", lit.px);
        assert_eq!(Texture { w: 300, h: 7, px: vec![[100, 200, 50]; 2100], fade: 0.0 }.lit(&flat).px[2099], [100, 200, 50]);
    }

    #[test]
    fn a_block_has_lit_walls_and_a_roof_that_fills_its_outline() {
        let (wall, roof) = ([200, 200, 200], [100, 150, 250]);
        let area = |f: &Face| ((f.p[1][0] - f.p[0][0]) * (f.p[2][1] - f.p[0][1]) - (f.p[1][1] - f.p[0][1]) * (f.p[2][0] - f.p[0][0])).abs() / 2.0;
        let roofs = |faces: &[Face]| faces.iter().filter(|f| f.colour == roof).map(area).sum::<f32>();
        // A square of 10 m, 5 m to 17 m up: 8 wall triangles, 2 for the roof, 100 m² of it.
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let faces = extrude(&square, 5.0, 17.0, wall, roof);
        assert_eq!(faces.len(), 10);
        assert!((roofs(&faces) - 100.0).abs() < 1e-3);
        assert!(faces.iter().all(|f| f.p.iter().all(|p| p[2] == 5.0 || p[2] == 17.0)));
        assert!(faces.iter().filter(|f| f.colour == roof).all(|f| f.p.iter().all(|p| p[2] == 17.0)));
        // The light comes from the north-west: the north and west walls are lighter than the
        // south and east ones. A wall is the two triangles with x (or y) constant.
        let side = |k: usize, v: f32| faces.iter().find(|f| f.colour != roof && f.p.iter().all(|p| p[k] == v)).unwrap().colour[0];
        let (west, east, south, north) = (side(0, 0.0), side(0, 10.0), side(1, 0.0), side(1, 10.0));
        assert!(west > east + 20 && north > south + 20 && west == north && east == south && west < 200, "{west} {east} {south} {north}");
        // Drawn the other way round, closed, or with a point twice: the same block.
        let back: Vec<[f32; 2]> = square.iter().rev().copied().collect();
        let sorted = |f: Vec<Face>| { let mut c: Vec<[u8; 3]> = f.iter().map(|f| f.colour).collect(); c.sort(); (c, roofs(&f)) };
        assert_eq!(sorted(extrude(&back, 5.0, 17.0, wall, roof)), sorted(faces.clone()));
        assert_eq!(extrude(&[[0.0, 0.0], [10.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]], 5.0, 17.0, wall, roof).len(), 10);
        // An L: the roof covers the L (75 m²), not its hollow.
        let l = [[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [5.0, 5.0], [5.0, 10.0], [0.0, 10.0]];
        let faces = extrude(&l, 0.0, 8.0, wall, roof);
        assert_eq!(faces.len(), 12 + 4);
        assert!((roofs(&faces) - 75.0).abs() < 1e-3, "{}", roofs(&faces));
        // A long outline with a hollow (a comb of 40 teeth): still its own area.
        let comb: Vec<[f32; 2]> = (0..40).flat_map(|k| { let x = k as f32 * 2.0; [[x, 10.0], [x, 0.0], [x + 1.0, 0.0], [x + 1.0, 10.0]] }).chain([[79.0, 12.0], [0.0, 12.0]]).collect();
        let faces = extrude(&comb, 0.0, 8.0, wall, roof);
        assert!((roofs(&faces) - (40.0 * 10.0 + 79.0 * 2.0)).abs() < 0.5, "{}", roofs(&faces));
        // Nothing to stand on: no point, a line, the same point, not a number. A bow tie: no panic.
        for ring in [vec![], vec![[0.0, 0.0], [1.0, 1.0]], vec![[2.0, 2.0]; 5], vec![[0.0, 0.0], [5.0, 5.0], [9.0, 9.0]], vec![[f32::NAN, 0.0], [1.0, 1.0], [0.0, 1.0]]] {
            assert!(extrude(&ring, 0.0, 8.0, wall, roof).is_empty(), "{ring:?}");
        }
        assert!(!extrude(&[[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.5]], 0.0, 8.0, wall, roof).is_empty());
    }

    #[test]
    fn blocks_stand_on_the_ground_and_hide_what_is_behind() {
        // Flat ground seen from the south at 30°: a red line runs east-west, a block stands in
        // front of its middle.
        let t = grid(21, |_, _| 0.0);
        let cam = Camera { target: [1000.0, 1000.0, 0.0], yaw: 0.0, pitch: 30f64.to_radians(), dist: 1500.0, min_dist: 300.0, fov: 0.8 };
        let line = vec![([600.0, 1100.0, 0.0], [255, 0, 0]), ([1400.0, 1100.0, 0.0], [255, 0, 0])];
        let block = extrude(&[[950.0, 950.0], [1050.0, 950.0], [1050.0, 1050.0], [950.0, 1050.0]], 0.0, 120.0, [90, 90, 90], [10, 20, 250]);
        let at = |solids: &[Face], p: [f64; 3]| {
            let img = render(&t, &plain([40, 160, 40]), solids, &cam, 1.0, &line, 600, 400, CLEAR, false);
            let (x, y) = project(&t, &cam, 1.0, p, 600, 400).unwrap();
            pixel(&img, 600, x, y)
        };
        // Without the block the line's middle shows; with it, the block's south wall does, and
        // the roof on top; beside the block the line is still there, and the ground.
        assert_eq!(at(&[], [1000.0, 1100.0, 0.0]), [255, 0, 0, 255]);
        let wall = at(&block, [1000.0, 950.0, 60.0]);
        assert!(wall[0] == wall[1] && wall[1] == wall[2] && wall[0] < 70, "a south wall, in the shade: {wall:?}");
        let behind = at(&block, [1000.0, 1100.0, 0.0]);
        assert!(behind[0] > wall[0] + 60 && behind[1] < wall[1] && behind[0] < 200, "the line is behind the block, faint: {behind:?}");
        assert_eq!(at(&block, [1000.0, 1000.0, 120.0]), [10, 20, 250, 255]);
        assert_eq!(at(&block, [700.0, 1100.0, 0.0]), [255, 0, 0, 255]);
        assert_eq!(at(&block, [700.0, 900.0, 0.0]), [40, 160, 40, 255]);
        // Heights are drawn higher with the ground's: the roof is where `project` says.
        let img = render(&t, &plain([40, 160, 40]), &block, &cam, 2.0, &[], 600, 400, CLEAR, false);
        let (x, y) = project(&t, &cam, 2.0, [1000.0, 1000.0, 120.0], 600, 400).unwrap();
        assert_eq!(pixel(&img, 600, x, y), [10, 20, 250, 255]);
    }

    /// Flat ground from above, north up, under a picture with one colour per quarter.
    fn quarters(fade: f32, smooth: bool) -> Vec<u8> {
        let t = grid(21, |_, _| 0.0);
        let cam = Camera { target: [1000.0, 1000.0, 0.0], yaw: 0.0, pitch: PITCH_MAX, dist: 3500.0, min_dist: 300.0, fov: 0.8 };
        let px = (0..40 * 40).map(|k| match (k % 40 < 20, k / 40 < 20) {
            (true, true) => [250, 0, 0],
            (false, true) => [0, 250, 0],
            (true, false) => [0, 0, 250],
            (false, false) => [250, 250, 0],
        });
        render(&t, &Texture { w: 40, h: 40, px: px.collect(), fade }, &[], &cam, 1.0, &[], 300, 300, [10, 20, 30, 255], smooth)
    }

    #[test]
    fn the_picture_lies_on_the_ground_east_right_and_north_up() {
        let img = quarters(0.0, false);
        assert_eq!(pixel(&img, 300, 100.0, 200.0), [250, 0, 0, 255], "south-west");
        assert_eq!(pixel(&img, 300, 200.0, 200.0), [0, 250, 0, 255], "south-east");
        assert_eq!(pixel(&img, 300, 100.0, 100.0), [0, 0, 250, 255], "north-west");
        assert_eq!(pixel(&img, 300, 200.0, 100.0), [250, 250, 0, 255], "north-east");
    }

    #[test]
    fn the_edges_fade_into_the_background() {
        let img = quarters(0.25, false);
        // Along the row 200 from the middle of the south-west quarter to the west edge: full
        // colour, then less and less, then the background, never a jump.
        let reds: Vec<u8> = (0..=110).rev().map(|x| pixel(&img, 300, x as f32, 200.0)[0]).collect();
        assert_eq!(reds[0], 250);
        assert_eq!(*reds.last().unwrap(), 10, "background beyond the ground");
        assert!(reds.windows(2).all(|p| p[1] <= p[0] && p[0] - p[1] < 30), "{reds:?}");
        assert!(reds.iter().any(|r| (100..200).contains(r)));
        // Cut sharp, the same row jumps from the colour to the background.
        let sharp: Vec<u8> = (0..=110).map(|x| pixel(&quarters(0.0, false), 300, x as f32, 200.0)[0]).collect();
        assert!(sharp.iter().all(|r| *r == 10 || *r == 250), "{sharp:?}");
    }

    #[test]
    fn smooth_softens_the_silhouette_and_the_line() {
        // A sharp-cut ground turned a little, so its edges cross the pixels at a slant.
        let t = grid(21, |_, _| 0.0);
        let cam = Camera { target: [1000.0, 1000.0, 0.0], yaw: 0.3, pitch: PITCH_MAX, dist: 4500.0, min_dist: 300.0, fov: 0.8 };
        let line = vec![([500.0, 500.0, 0.0], [255, 255, 255]), ([1500.0, 900.0, 0.0], [255, 255, 255])];
        let between = |smooth: bool| {
            let img = render(&t, &plain([200, 0, 0]), &[], &cam, 1.0, &line, 300, 300, [0, 0, 0, 255], smooth);
            assert_eq!(img.len(), 300 * 300 * 4);
            // Neither ground, background nor line: a blend.
            img.chunks_exact(4).filter(|p| ![0, 200, 255].contains(&p[0])).count()
        };
        assert_eq!(between(false), 0);
        assert!(between(true) > 200, "{}", between(true));
    }

    #[test]
    fn the_line_is_edged_with_the_background() {
        let t = grid(21, |_, _| 0.0);
        let cam = Camera { target: [1000.0, 1000.0, 0.0], yaw: 0.0, pitch: PITCH_MAX, dist: 3000.0, min_dist: 300.0, fov: 0.8 };
        let line = vec![([500.0, 1000.0, 0.0], [255, 0, 0]), ([1500.0, 1000.0, 0.0], [255, 0, 0])];
        let img = render(&t, &plain([40, 160, 40]), &[], &cam, 1.0, &line, 1200, 800, [9, 9, 9, 255], false);
        // Down from the line's middle: the line, its edging, the ground.
        let col: Vec<[u8; 4]> = (0..10).map(|d| pixel(&img, 1200, 600.0, 400.0 + d as f32)).collect();
        assert_eq!(col[0], [255, 0, 0, 255]);
        let edge = col.iter().position(|p| *p == [9, 9, 9, 255]).expect("an edging");
        let ground = col.iter().position(|p| *p == [40, 160, 40, 255]).expect("the ground");
        assert!((2..=4).contains(&edge) && ground > edge && ground <= 6, "{col:?}");
    }

    #[test]
    #[ignore]
    fn timing_1200x800_500x500() {
        let t = grid(500, |i, j| 800.0 + 300.0 * ((i as f32 / 20.0).sin() + (j as f32 / 15.0).cos()));
        let colors = Texture { w: 1500, h: 1500, px: (0..1500 * 1500).map(|k| [(k % 251) as u8, 120, 60]).collect(), fade: 0.12 };
        let cam = Camera::fit(&t);
        let line: Vec<_> = (0..500).map(|k| ([k as f64 * 50.0, 15_000.0 + (k as f64 / 20.0).sin() * 8000.0, 900.0], [255, 40, 40])).collect();
        render(&t, &colors, &[], &cam, 1.5, &line, 1200, 800, CLEAR, true);
        let start = std::time::Instant::now();
        let runs = 10;
        for _ in 0..runs {
            std::hint::black_box(render(&t, &colors, &[], &cam, 1.5, &line, 1200, 800, CLEAR, true));
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / runs as f64;
        println!("terrain render 1200x800, 500x500 vertices, smooth: {ms:.1} ms");
        assert!(ms < 60.0, "{ms} ms");
        // From close, over a town: 10 000 blocks of 20 m within 1.2 km of what the eye looks at.
        let cam = Camera { dist: 1200.0, pitch: 0.6, ..cam };
        let blocks: Vec<Face> = (0..10_000)
            .flat_map(|k| {
                let (x, y) = (cam.target[0] as f32 - 1200.0 + (k % 100) as f32 * 24.0, cam.target[1] as f32 - 1200.0 + (k / 100) as f32 * 24.0);
                let g = t.height_at(x as f64, y as f64) as f32;
                extrude(&[[x, y], [x + 20.0, y], [x + 20.0, y + 20.0], [x, y + 20.0]], g, g + 15.0, [150; 3], [200; 3])
            })
            .collect();
        let start = std::time::Instant::now();
        for _ in 0..runs {
            std::hint::black_box(render(&t, &colors, &blocks, &cam, 1.5, &line, 1200, 800, CLEAR, true));
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / runs as f64;
        println!("the same from 1200 m with {} faces of buildings: {ms:.1} ms", blocks.len());
        assert!(ms < 60.0, "{ms} ms");
    }
}
