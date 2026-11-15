//! Terrain in 3D, drawn in software: a height grid as shaded triangles with a z-buffer, and a
//! route drawn over it. There is no 3D in the UI toolkit, so the picture is an image of bytes
//! (same layout as `relief::shade`: the three colour bytes as given by the caller, then alpha).
//! Pure: no gpui, no disk.

// ponytail: nothing calls this yet; drop the allow once the planner view draws it.
#![allow(dead_code)]

use crate::logic::map::world;
use std::sync::{Arc, Mutex};

/// Share of the light a surface turned away from the sun still gets.
const AMBIENT: f32 = 0.45;
/// Direction towards the light: north-west, 45° up (x east, y north, z up; unit length).
const LIGHT: [f32; 3] = [-0.5, 0.5, std::f32::consts::FRAC_1_SQRT_2];
/// Brightness of a flat surface, which tests and callers can expect.
pub const FLAT_LIT: f32 = AMBIENT + (1.0 - AMBIENT) * LIGHT[2];
/// The line is 0.5 % nearer than its own ground, so it is not buried in it.
const LINE_BIAS: f32 = 1.005;
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
        [(self.cols - 1) as f64 * self.dx, (self.rows - 1) as f64 * self.dy]
    }
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
        Camera { target: [w / 2.0, h / 2.0, (lo + hi) as f64 / 2.0], yaw: 22.5f64.to_radians(), pitch: 40f64.to_radians(), dist: radius / (fov / 2.0).sin(), fov }
    }

    pub fn orbit(&mut self, dyaw: f64, dpitch: f64) {
        self.yaw = (self.yaw + dyaw).rem_euclid(std::f64::consts::TAU);
        self.pitch = (self.pitch + dpitch).clamp(PITCH_MIN, PITCH_MAX);
    }

    /// Multiplies the distance by `factor` (below 1 comes closer).
    pub fn zoom(&mut self, factor: f64) {
        self.dist = (self.dist * factor).clamp(50.0, 1.0e6);
    }

    /// Slides the target on the ground: `right` and `forward` in metres, along the view.
    pub fn pan(&mut self, right: f64, forward: f64) {
        let (s, c) = self.yaw.sin_cos();
        self.target[0] += right * c + forward * s;
        self.target[1] += -right * s + forward * c;
    }

    /// Pans by a share of the distance, e.g. a drag of `dx` pixels over a window `w` wide.
    pub fn pan_screen(&mut self, dx: f64, dy: f64, w: f64) {
        let k = self.dist * 2.0 * (self.fov / 2.0).tan() / w.max(1.0);
        // Dragging the ground right moves the target left; up on screen is forward on the ground.
        self.pan(-dx * k, dy * k / self.pitch.sin());
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

/// A point in view space: (screen x, screen y, 1 / depth), `None` behind the camera.
fn to_screen(eye: &[f64; 3], b: &[[f64; 3]; 3], focal: f64, w: f64, h: f64, p: [f64; 3]) -> Option<[f32; 3]> {
    let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
    let dot = |v: &[f64; 3]| d[0] * v[0] + d[1] * v[1] + d[2] * v[2];
    let depth = dot(&b[2]);
    if depth <= NEAR {
        return None;
    }
    Some([(w / 2.0 + dot(&b[0]) / depth * focal) as f32, (h / 2.0 - dot(&b[1]) / depth * focal) as f32, (1.0 / depth) as f32])
}

/// Screen position of a point `[east, north, altitude]` (local metres) in a `w` × `h` picture.
pub fn project(_t: &Terrain, cam: &Camera, exaggeration: f32, p: [f64; 3], w: usize, h: usize) -> Option<(f32, f32)> {
    let ex = exaggeration as f64;
    let (eye, b) = cam.basis(ex);
    let focal = h as f64 / 2.0 / (cam.fov / 2.0).tan();
    to_screen(&eye, &b, focal, w as f64, h as f64, [p[0], p[1], p[2] * ex]).map(|v| (v[0], v[1]))
}

/// Draws `t` with one colour per vertex (`colors`, lit by the sun), then `line` over it. Each
/// line point is `([east, north, altitude], colour of the segment that starts there)`.
/// Returns `w * h` pixels of 4 bytes: the colour bytes as given, alpha 255; `background` where
/// nothing is drawn.
#[allow(clippy::too_many_arguments)]
pub fn render(t: &Terrain, colors: &[[u8; 3]], cam: &Camera, exaggeration: f32, line: &[([f64; 3], [u8; 3])], w: usize, h: usize, background: [u8; 4]) -> Vec<u8> {
    let mut out: Vec<u8> = background.iter().copied().cycle().take(w * h * 4).collect();
    if w == 0 || h == 0 || t.cols < 2 || t.rows < 2 || colors.len() < t.heights.len() {
        return out;
    }
    let ex = exaggeration as f64;
    let (eye, b) = cam.basis(ex);
    let focal = h as f64 / 2.0 / (cam.fov / 2.0).tan();
    let (wf, hf) = (w as f64, h as f64);

    // Vertices: screen position, 1/depth and lit colour; 1/depth = 0 when behind the camera.
    let (cols, rows) = (t.cols, t.rows);
    let mut verts = vec![[0f32; 6]; cols * rows];
    for j in 0..rows {
        for i in 0..cols {
            let k = j * cols + i;
            let z = t.heights[k] as f64 * ex;
            let Some(s) = to_screen(&eye, &b, focal, wf, hf, [i as f64 * t.dx, j as f64 * t.dy, z]) else { continue };
            let at = |i: usize, j: usize| t.heights[j * cols + i] as f64;
            let (i0, i1, j0, j1) = (i.saturating_sub(1), (i + 1).min(cols - 1), j.saturating_sub(1), (j + 1).min(rows - 1));
            let gx = (at(i1, j) - at(i0, j)) * ex / ((i1 - i0) as f64 * t.dx);
            let gy = (at(i, j1) - at(i, j0)) * ex / ((j1 - j0) as f64 * t.dy);
            let norm = (gx * gx + gy * gy + 1.0).sqrt();
            let lit = ((-gx * LIGHT[0] as f64 - gy * LIGHT[1] as f64 + LIGHT[2] as f64) / norm).max(0.0) as f32;
            let f = AMBIENT + (1.0 - AMBIENT) * lit;
            let c = colors[k];
            verts[k] = [s[0], s[1], s[2], c[0] as f32 * f, c[1] as f32 * f, c[2] as f32 * f];
        }
    }
    // Vertical extent of each cell, to skip the bands it does not touch.
    let mut span = vec![(f32::INFINITY, f32::NEG_INFINITY); (cols - 1) * (rows - 1)];
    for j in 0..rows - 1 {
        for i in 0..cols - 1 {
            let q = [j * cols + i, j * cols + i + 1, (j + 1) * cols + i, (j + 1) * cols + i + 1].map(|k| &verts[k]);
            // ponytail: a cell with a vertex behind the camera is dropped, not clipped.
            if q.iter().all(|v| v[2] > 0.0) {
                span[j * (cols - 1) + i] = q.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(v[1]), hi.max(v[1])));
            }
        }
    }
    let pts: Vec<Option<[f32; 3]>> = line.iter().map(|(p, _)| to_screen(&eye, &b, focal, wf, hf, [p[0], p[1], p[2] * ex])).collect();
    let radius = (w as f32 * 1.5 / 1200.0).max(1.0);

    let bands = Mutex::new(out.chunks_mut(BAND * w * 4).enumerate());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| {
                let mut zbuf = vec![0f32; BAND * w];
                loop {
                    let next = bands.lock().unwrap().next();
                    let Some((n, rows_out)) = next else { break };
                    let y0 = n * BAND;
                    let y1 = y0 + rows_out.len() / (w * 4);
                    zbuf.iter_mut().for_each(|z| *z = 0.0);
                    let (lo, hi) = (y0 as f32, y1 as f32);
                    for j in 0..rows - 1 {
                        for i in 0..cols - 1 {
                            let (a, z) = span[j * (cols - 1) + i];
                            if z < lo || a > hi {
                                continue;
                            }
                            let (v00, v10, v01, v11) = (&verts[j * cols + i], &verts[j * cols + i + 1], &verts[(j + 1) * cols + i], &verts[(j + 1) * cols + i + 1]);
                            triangle([v00, v10, v11], y0, y1, w, rows_out, &mut zbuf);
                            triangle([v00, v11, v01], y0, y1, w, rows_out, &mut zbuf);
                        }
                    }
                    for (k, pair) in pts.windows(2).enumerate() {
                        if let [Some(a), Some(b)] = pair {
                            segment(a, b, line[k].1, radius, y0, y1, w, rows_out, &mut zbuf);
                        }
                    }
                }
            });
        }
    });
    out
}

fn put(px: &mut [u8], c: [f32; 3]) {
    px.copy_from_slice(&[(c[0] + 0.5).min(255.0) as u8, (c[1] + 0.5).min(255.0) as u8, (c[2] + 0.5).min(255.0) as u8, 255]);
}

/// One triangle, restricted to rows `y0..y1`; colour and 1/depth are interpolated on screen.
/// ponytail: colours are not perspective-corrected; with small cells the error is invisible.
fn triangle(v: [&[f32; 6]; 3], y0: usize, y1: usize, w: usize, rows_out: &mut [u8], zbuf: &mut [f32]) {
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
                let col = [0, 1, 2].map(|k| wa * a[3 + k] + wb * b[3 + k] + wc * c[3 + k]);
                put(&mut rows_out[zi * 4..zi * 4 + 4], col);
            }
        }
    }
}

/// A thick segment (a capsule on screen), nearer than the surface by `LINE_BIAS`.
#[allow(clippy::too_many_arguments)]
fn segment(a: &[f32; 3], b: &[f32; 3], color: [u8; 3], r: f32, y0: usize, y1: usize, w: usize, rows_out: &mut [u8], zbuf: &mut [f32]) {
    let ys = (a[1].min(b[1]) - r).floor().max(y0 as f32) as usize;
    let ye = (a[1].max(b[1]) + r).ceil().min(y1 as f32 - 1.0);
    let xs = (a[0].min(b[0]) - r).floor().max(0.0) as usize;
    let xe = (a[0].max(b[0]) + r).ceil().min(w as f32 - 1.0);
    if xe < 0.0 || ye < 0.0 || ys > ye as usize || xs > xe as usize {
        return;
    }
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let col = color.map(|c| c as f32);
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
            if iz > zbuf[zi] {
                zbuf[zi] = iz;
                put(&mut rows_out[zi * 4..zi * 4 + 4], col);
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

    #[test]
    fn flat_terrain_from_above_shows_its_lit_colour() {
        let t = grid(21, |_, _| 0.0);
        let cam = Camera { target: [1000.0, 1000.0, 0.0], yaw: 0.0, pitch: PITCH_MAX, dist: 3000.0, fov: 0.8 };
        let img = render(&t, &vec![[200, 100, 50]; 441], &cam, 1.0, &[], 200, 150, CLEAR);
        let p = pixel(&img, 200, 100.0, 75.0);
        for (got, want) in p.iter().zip([200.0 * FLAT_LIT, 100.0 * FLAT_LIT, 50.0 * FLAT_LIT, 255.0]) {
            assert!((*got as f32 - want).abs() <= 1.5, "{p:?} vs {want}");
        }
        assert_eq!(pixel(&img, 200, 0.0, 0.0), CLEAR, "the corner is background");
    }

    /// A ridge across the whole grid at row 5, and a red line behind it, seen from the south.
    #[allow(clippy::type_complexity)]
    fn ridge_scene(ridge: f32) -> (Terrain, Camera, Vec<([f64; 3], [u8; 3])>) {
        let t = grid(21, |_, j| if j == 5 { ridge } else { 0.0 });
        let cam = Camera { target: [1000.0, 1000.0, 0.0], yaw: 0.0, pitch: 15f64.to_radians(), dist: 4000.0, fov: 0.8 };
        let line = vec![([900.0, 1500.0, 0.0], [255, 0, 0]), ([1100.0, 1500.0, 0.0], [255, 0, 0])];
        (t, cam, line)
    }

    #[test]
    fn line_shows_on_flat_ground() {
        let (t, cam, line) = ridge_scene(0.0);
        let img = render(&t, &vec![[40, 160, 40]; 441], &cam, 1.0, &line, 1200, 800, CLEAR);
        let (x, y) = project(&t, &cam, 1.0, [1000.0, 1500.0, 0.0], 1200, 800).unwrap();
        assert_eq!(pixel(&img, 1200, x, y)[..3], [255, 0, 0]);
        // About 3 px thick: the pixels 4 px above and below are ground.
        assert_ne!(pixel(&img, 1200, x, y - 4.0)[..3], [255, 0, 0]);
        assert_ne!(pixel(&img, 1200, x, y + 4.0)[..3], [255, 0, 0]);
    }

    #[test]
    fn a_peak_in_front_hides_the_line() {
        let (t, cam, line) = ridge_scene(1500.0);
        let img = render(&t, &vec![[40, 160, 40]; 441], &cam, 1.0, &line, 1200, 800, CLEAR);
        let (x, y) = project(&t, &cam, 1.0, [1000.0, 1500.0, 0.0], 1200, 800).unwrap();
        assert_ne!(pixel(&img, 1200, x, y)[..3], [255, 0, 0]);
        assert_eq!(pixel(&img, 1200, x, y)[3], 255, "the ridge covers that pixel");
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

    #[test]
    #[ignore]
    fn timing_1200x800_300x300() {
        let t = grid(300, |i, j| 800.0 + 300.0 * ((i as f32 / 20.0).sin() + (j as f32 / 15.0).cos()));
        let colors: Vec<[u8; 3]> = t.heights.iter().map(|&h| [(h / 5.0) as u8, 120, 60]).collect();
        let cam = Camera::fit(&t);
        let line: Vec<_> = (0..500).map(|k| ([k as f64 * 50.0, 15_000.0 + (k as f64 / 20.0).sin() * 8000.0, 900.0], [255, 40, 40])).collect();
        render(&t, &colors, &cam, 1.5, &line, 1200, 800, CLEAR);
        let start = std::time::Instant::now();
        let runs = 10;
        for _ in 0..runs {
            std::hint::black_box(render(&t, &colors, &cam, 1.5, &line, 1200, 800, CLEAR));
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / runs as f64;
        println!("terrain render 1200x800, 300x300 vertices: {ms:.1} ms");
        assert!(ms < 60.0, "{ms} ms");
    }
}
