//! Maps: Web Mercator projection, the tiles a view needs, tiles recolored in the theme,
//! and the heat raster (how many activities pass through each pixel). Pure functions.

use crate::fit::metres;

pub const TILE: f64 = 256.0;
pub const MIN_Z: f64 = 2.0;
pub const MAX_Z: f64 = 18.0;

/// World pixel of a position at zoom `z` (fractional zooms scale continuously).
pub fn world(lat: f64, lon: f64, z: f64) -> (f64, f64) {
    let n = TILE * 2f64.powf(z);
    let lat = lat.clamp(-85.05112878, 85.05112878).to_radians();
    let x = (lon + 180.0) / 360.0 * n;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    (x, y)
}

/// What a map shows: its centre, in world pixels at zoom `z`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub cx: f64,
    pub cy: f64,
    pub z: f64,
}

impl Default for View {
    fn default() -> Self {
        let (cx, cy) = world(46.5, 2.5, 5.0);
        View { cx, cy, z: 5.0 }
    }
}

/// Bounding box `[lat_min, lon_min, lat_max, lon_max]` of the points, leaving out the `trim`
/// fraction at each end (a trip abroad shouldn't shrink home to a dot).
pub fn bbox<'a>(lines: impl Iterator<Item = &'a [[f32; 2]]>, trim: f64) -> Option<[f64; 4]> {
    let (mut lats, mut lons): (Vec<f64>, Vec<f64>) = lines.flatten().map(|p| (p[0] as f64, p[1] as f64)).unzip();
    if lats.is_empty() {
        return None;
    }
    lats.sort_by(f64::total_cmp);
    lons.sort_by(f64::total_cmp);
    let k = ((lats.len() as f64 * trim) as usize).min(lats.len() - 1);
    let hi = lats.len() - 1 - k;
    Some([lats[k], lons[k], lats[hi.max(k)], lons[hi.max(k)]])
}

impl View {
    /// The largest zoom at which the box fits `w × h`, minus `pad` pixels on each side.
    pub fn fit(b: [f64; 4], w: f64, h: f64, pad: f64) -> View {
        let (w, h) = ((w - 2.0 * pad).max(32.0), (h - 2.0 * pad).max(32.0));
        let (x0, y1) = world(b[0], b[1], 0.0);
        let (x1, y0) = world(b[2], b[3], 0.0);
        let (dx, dy) = ((x1 - x0).max(1e-9), (y1 - y0).max(1e-9));
        let z = (w / dx).min(h / dy).log2().clamp(MIN_Z, 16.0);
        let s = 2f64.powf(z);
        View { cx: (x0 + x1) / 2.0 * s, cy: (y0 + y1) / 2.0 * s, z }
    }

    /// Screen position of a world position, the view being `w × h`.
    pub fn screen(&self, lat: f64, lon: f64, w: f64, h: f64) -> (f64, f64) {
        let (x, y) = world(lat, lon, self.z);
        (x - self.cx + w / 2.0, y - self.cy + h / 2.0)
    }

    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.cx -= dx;
        self.cy -= dy;
    }

    /// Zooms by `dz`, keeping the point under (`sx`, `sy`) where it is.
    pub fn zoom_at(&mut self, dz: f64, sx: f64, sy: f64, w: f64, h: f64) {
        let z = (self.z + dz).clamp(MIN_Z, MAX_Z);
        let k = 2f64.powf(z - self.z);
        let (px, py) = (self.cx + sx - w / 2.0, self.cy + sy - h / 2.0);
        self.cx = px * k - (sx - w / 2.0);
        self.cy = py * k - (sy - h / 2.0);
        self.z = z;
    }

    /// Tiles covering the view: (z, x, y, left, top, size) in screen pixels.
    pub fn tiles(&self, w: f64, h: f64) -> Vec<(u8, u32, u32, f64, f64, f64)> {
        let tz = self.z.round().clamp(0.0, MAX_Z);
        let size = TILE * 2f64.powf(self.z - tz);
        let n = 2u32.pow(tz as u32);
        let (left, top) = (self.cx - w / 2.0, self.cy - h / 2.0);
        let (x0, y0) = ((left / size).floor() as i64, (top / size).floor() as i64);
        let (x1, y1) = (((left + w) / size).floor() as i64, ((top + h) / size).floor() as i64);
        let mut out = Vec::new();
        for ty in y0.max(0)..=y1.min(n as i64 - 1) {
            for tx in x0..=x1 {
                // The world repeats sideways.
                let wx = tx.rem_euclid(n as i64) as u32;
                out.push((tz as u8, wx, ty as u32, tx as f64 * size - left, ty as f64 * size - top, size));
            }
        }
        out
    }
}

/// How many of the `lines` cross each pixel of a `w × h` view; an activity counts once per pixel,
/// however many times it loops there. Jumps over 300 m (GPS lost) aren't drawn.
pub fn heat(lines: &[&[[f32; 2]]], view: &View, w: usize, h: usize) -> Vec<u16> {
    let mut count = vec![0u16; w * h];
    let mut stamp = vec![0u32; w * h];
    for (n, line) in lines.iter().enumerate() {
        let id = n as u32 + 1;
        let mut mark = |x: i64, y: i64| {
            if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                let i = y as usize * w + x as usize;
                if stamp[i] != id {
                    stamp[i] = id;
                    count[i] = count[i].saturating_add(1);
                }
            }
        };
        let mut prev: Option<([f32; 2], (f64, f64))> = None;
        for p in line.iter() {
            let s = view.screen(p[0] as f64, p[1] as f64, w as f64, h as f64);
            if let Some((q, a)) = prev {
                let far = metres([q[0] as f64, q[1] as f64], [p[0] as f64, p[1] as f64]) > 300.0;
                let off = (a.0 < -2.0 && s.0 < -2.0)
                    || (a.1 < -2.0 && s.1 < -2.0)
                    || (a.0 > w as f64 + 2.0 && s.0 > w as f64 + 2.0)
                    || (a.1 > h as f64 + 2.0 && s.1 > h as f64 + 2.0);
                if !far && !off {
                    let steps = (s.0 - a.0).abs().max((s.1 - a.1).abs()).ceil().max(1.0) as i64;
                    for k in 0..=steps {
                        let f = k as f64 / steps as f64;
                        let (x, y) = (a.0 + (s.0 - a.0) * f, a.1 + (s.1 - a.1) * f);
                        // Two pixels wide, so a line stays visible at any angle.
                        mark(x as i64, y as i64);
                        mark(x as i64 + 1, y as i64);
                        mark(x as i64, y as i64 + 1);
                    }
                }
            } else {
                mark(s.0 as i64, s.1 as i64);
            }
            prev = Some((*p, s));
        }
    }
    count
}

/// Ids of the lines passing within `r` pixels of the screen point (`sx`, `sy`).
pub fn near(lines: &[(i64, &[[f32; 2]])], view: &View, w: f64, h: f64, sx: f64, sy: f64, r: f64) -> Vec<i64> {
    let dist = |a: (f64, f64), b: (f64, f64)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx * dx + dy * dy;
        let t = if len > 0.0 { (((sx - a.0) * dx + (sy - a.1) * dy) / len).clamp(0.0, 1.0) } else { 0.0 };
        ((a.0 + t * dx - sx).powi(2) + (a.1 + t * dy - sy).powi(2)).sqrt()
    };
    lines
        .iter()
        .filter(|(_, line)| {
            let pts: Vec<(f64, f64)> = line.iter().map(|p| view.screen(p[0] as f64, p[1] as f64, w, h)).collect();
            match pts.len() {
                0 => false,
                1 => dist(pts[0], pts[0]) <= r,
                _ => pts.windows(2).any(|s| dist(s[0], s[1]) <= r),
            }
        })
        .map(|(id, _)| *id)
        .collect()
}

/// Counts to BGRA pixels: `light` where one activity passed, `deep` where the most did
/// (log scale, the top 1 % saturating).
pub fn colorize(count: &[u16], light: [u8; 3], deep: [u8; 3]) -> Vec<u8> {
    let mut nonzero: Vec<u16> = count.iter().copied().filter(|c| *c > 0).collect();
    nonzero.sort_unstable();
    let top = nonzero.get(nonzero.len().saturating_sub(1) * 99 / 100).copied().unwrap_or(1).max(2) as f32;
    let mut out = vec![0u8; count.len() * 4];
    for (i, c) in count.iter().enumerate() {
        if *c == 0 {
            continue;
        }
        let f = ((*c as f32).ln() / top.ln()).clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f) as u8;
        let px = &mut out[i * 4..i * 4 + 4];
        px[0] = mix(light[2], deep[2]);
        px[1] = mix(light[1], deep[1]);
        px[2] = mix(light[0], deep[0]);
        px[3] = (200.0 + 55.0 * f) as u8;
    }
    out
}

/// A tile in the theme: its luminance laid between the background (light areas) and the text
/// (dark details), faintly, so the tracks stay in front. BGRA out.
pub fn tint(png: &[u8], bg: [u8; 3], fg: [u8; 3]) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for p in img.pixels() {
        let lum = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0;
        // Light map areas sit near 0.9: stretch the bright end so roads, water and woods still read.
        let s = (0.62 + 0.38 * lum.powf(2.2)).min(1.0);
        let mix = |a: u8, b: u8| (b as f32 + (a as f32 - b as f32) * s) as u8;
        out.extend_from_slice(&[mix(bg[2], fg[2]), mix(bg[1], fg[1]), mix(bg[0], fg[0]), 255]);
    }
    Some((w, h, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection() {
        assert_eq!(world(0.0, -180.0, 0.0), (0.0, 128.0));
        let (x, y) = world(0.0, 0.0, 1.0);
        assert!((x - 256.0).abs() < 1e-9 && (y - 256.0).abs() < 1e-9);
        // Poles are clamped, never infinite.
        assert!(world(90.0, 0.0, 3.0).1.is_finite() && world(-90.0, 0.0, 3.0).1.is_finite());
    }

    #[test]
    fn fit_zoom_pan() {
        let b = [45.70, 4.80, 45.80, 4.90];
        let v = View::fit(b, 800.0, 600.0, 20.0);
        let (sx, sy) = v.screen(45.75, 4.85, 800.0, 600.0);
        assert!((sx - 400.0).abs() < 2.0 && (sy - 300.0).abs() < 4.0, "{sx} {sy}");
        let (ax, ay) = v.screen(b[0], b[1], 800.0, 600.0);
        let (bx, by) = v.screen(b[2], b[3], 800.0, 600.0);
        assert!(ax >= 19.0 && bx <= 781.0 && by >= 19.0 && ay <= 581.0, "{ax} {bx} {ay} {by}");
        // A single point or a tiny window: no NaN, zoom clamped.
        let p = View::fit([45.0, 4.0, 45.0, 4.0], 10.0, 10.0, 20.0);
        assert!(p.z.is_finite() && p.z <= 16.0);
        // Zooming keeps the point under the cursor.
        let mut z = v;
        let before = z.screen(45.72, 4.83, 800.0, 600.0);
        z.zoom_at(1.3, before.0, before.1, 800.0, 600.0);
        let after = z.screen(45.72, 4.83, 800.0, 600.0);
        assert!((after.0 - before.0).abs() < 1e-6 && (after.1 - before.1).abs() < 1e-6);
        z.zoom_at(100.0, 0.0, 0.0, 800.0, 600.0);
        assert_eq!(z.z, MAX_Z);
        let mut p = v;
        p.pan(10.0, -5.0);
        assert_eq!((p.cx, p.cy), (v.cx - 10.0, v.cy + 5.0));
    }

    #[test]
    fn tiles_cover_the_view() {
        let v = View { cx: 1000.0, cy: 1000.0, z: 4.0 };
        let t = v.tiles(512.0, 256.0);
        assert!(t.iter().all(|t| t.0 == 4 && t.5 == 256.0));
        assert_eq!(t.len(), 3 * 2);
        // Fractional zoom: tiles of the nearest level, scaled.
        let f = View { cx: 1000.0, cy: 1000.0, z: 4.4 }.tiles(512.0, 256.0);
        assert!(f.iter().all(|t| t.0 == 4 && (t.5 - 256.0 * 2f64.powf(0.4)).abs() < 1e-9));
        // Nothing above the top of the world, wrap sideways.
        let top = View { cx: 0.0, cy: 0.0, z: 1.0 }.tiles(512.0, 512.0);
        assert!(top.iter().all(|t| t.1 < 2 && t.2 < 2));
    }

    #[test]
    fn heat_counts_each_activity_once_per_pixel() {
        let v = View::fit([45.0, 4.0, 45.01, 4.01], 100.0, 100.0, 10.0);
        // A point every ~14 m along the diagonal, like a simplified track.
        let a: Vec<[f32; 2]> = (0..=100).map(|i| [45.0 + i as f32 * 1e-4, 4.0 + i as f32 * 1e-4]).collect();
        // The same activity going back and forth counts once.
        let back: Vec<[f32; 2]> = a.iter().chain(a.iter().rev()).copied().collect();
        let jump: Vec<[f32; 2]> = vec![[45.009, 4.0], [46.0, 5.0]]; // GPS lost: only its first point
        let c = heat(&[&a, &back, &jump], &v, 100, 100);
        assert_eq!(*c.iter().max().unwrap(), 2);
        assert!(c.iter().filter(|x| **x > 0).count() > 100);
        assert!(heat(&[], &v, 100, 100).iter().all(|x| *x == 0));
        assert!(heat(&[&a], &v, 0, 0).is_empty());
        let px = colorize(&c, [250, 200, 180], [120, 40, 10]);
        assert_eq!(px.len(), 100 * 100 * 4);
        assert!(px.chunks(4).zip(&c).all(|(p, n)| (p[3] == 0) == (*n == 0)));
    }

    #[test]
    fn near_finds_lines_under_the_pointer() {
        let v = View::fit([45.0, 4.0, 45.01, 4.01], 200.0, 200.0, 10.0);
        let a: Vec<[f32; 2]> = vec![[45.0, 4.0], [45.01, 4.01]];
        let b: Vec<[f32; 2]> = vec![[45.0, 4.01]];
        let mid = v.screen(45.005, 4.005, 200.0, 200.0);
        assert_eq!(near(&[(1, &a), (2, &b)], &v, 200.0, 200.0, mid.0, mid.1, 6.0), [1]);
        let corner = v.screen(45.0, 4.01, 200.0, 200.0);
        assert_eq!(near(&[(1, &a), (2, &b)], &v, 200.0, 200.0, corner.0, corner.1, 6.0), [2]);
        assert!(near(&[(1, &[][..])], &v, 200.0, 200.0, 0.0, 0.0, 6.0).is_empty());
    }

    #[test]
    fn tint_maps_luminance_into_the_theme() {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_fn(2, 1, |x, _| if x == 0 { image::Rgba([255, 255, 255, 255]) } else { image::Rgba([0, 0, 0, 255]) });
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let (w, h, px) = tint(&png, [250, 250, 248], [38, 35, 31]).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(&px[0..4], &[248, 250, 250, 255], "white becomes the background (BGRA)");
        assert!(px[6] < 200, "black leans to the text");
        assert!(tint(b"no png", [0; 3], [0; 3]).is_none());
    }
}
