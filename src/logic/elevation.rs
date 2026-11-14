//! Altitude from Terrarium tiles (PNG, `R·256 + G + B/256 − 32768` metres): decoding, reading a
//! point between pixels, and climb with a threshold against noise. Pure: the caller hands over
//! the tiles' bytes.

use crate::logic::map::{TILE, world};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// The zoom read: about 28 m a pixel at Nice, the source's own resolution in France (~30 m).
pub const ZOOM: u8 = 12;
const SIZE: usize = 256;
/// Climb is read every this many metres along a line.
pub const STEP: f64 = 10.0;
/// A rise or a fall smaller than this, between two turns, is noise and not counted.
pub const THRESHOLD: f32 = 2.0;
/// The stored altitude when there was no tile to read it from.
pub const UNKNOWN: u16 = u16::MAX;

/// A tile's altitudes, row by row (256 × 256), or `None` when it isn't Terrarium.
pub fn decode(png: &[u8]) -> Option<Vec<f32>> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.to_rgb8();
    if img.dimensions() != (SIZE as u32, SIZE as u32) {
        return None;
    }
    Some(img.pixels().map(|p| p[0] as f32 * 256.0 + p[1] as f32 + p[2] as f32 / 256.0 - 32768.0).collect())
}

/// The tile at `ZOOM` that holds a position.
pub fn tile_of(lat: f64, lon: f64) -> (u32, u32) {
    let (x, y) = world(lat, lon, ZOOM as f64);
    let max = (1u32 << ZOOM) - 1;
    (((x / TILE) as u32).min(max), ((y / TILE) as u32).min(max))
}

/// Altitudes kept in 16 bits: decimetres from −500 m, up to 6,053 m.
pub fn pack(m: f32) -> u16 {
    ((m + 500.0) * 10.0).round().clamp(0.0, (UNKNOWN - 1) as f32) as u16
}

pub fn unpack(v: u16) -> Option<f32> {
    (v != UNKNOWN).then(|| v as f32 / 10.0 - 500.0)
}

/// Reads altitudes, loading tiles on demand through `load` and keeping the last few decoded.
pub struct Dem<'a> {
    load: Box<dyn FnMut(u32, u32) -> Option<Vec<f32>> + 'a>,
    tiles: HashMap<(u32, u32), Option<Arc<Vec<f32>>>>,
    order: VecDeque<(u32, u32)>,
    keep: usize,
    /// Positions read where no tile was available.
    pub missing: u64,
}

impl<'a> Dem<'a> {
    pub fn new(keep: usize, load: impl FnMut(u32, u32) -> Option<Vec<f32>> + 'a) -> Self {
        Dem { load: Box::new(load), tiles: HashMap::new(), order: VecDeque::new(), keep: keep.max(4), missing: 0 }
    }

    fn tile(&mut self, x: u32, y: u32) -> Option<Arc<Vec<f32>>> {
        if let Some(t) = self.tiles.get(&(x, y)) {
            return t.clone();
        }
        if self.order.len() >= self.keep
            && let Some(old) = self.order.pop_front()
        {
            self.tiles.remove(&old);
        }
        let t = (self.load)(x, y).filter(|t| t.len() == SIZE * SIZE).map(Arc::new);
        self.tiles.insert((x, y), t.clone());
        self.order.push_back((x, y));
        t
    }

    /// The pixel at global pixel coordinates (`ZOOM`).
    fn pixel(&mut self, gx: i64, gy: i64) -> Option<f32> {
        let max = (SIZE as i64) << ZOOM;
        if gx < 0 || gy < 0 || gx >= max || gy >= max {
            return None;
        }
        let t = self.tile((gx / SIZE as i64) as u32, (gy / SIZE as i64) as u32)?;
        Some(t[(gy as usize % SIZE) * SIZE + gx as usize % SIZE])
    }

    /// The altitude at a position, between the four nearest pixels (those that exist).
    pub fn at(&mut self, lat: f64, lon: f64) -> Option<f32> {
        let (x, y) = world(lat, lon, ZOOM as f64);
        // Pixel centres sit at +0.5.
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
        let (tx, ty) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
        let (mut sum, mut weight) = (0.0f32, 0.0f32);
        for (dx, dy, w) in [(0, 0, (1.0 - tx) * (1.0 - ty)), (1, 0, tx * (1.0 - ty)), (0, 1, (1.0 - tx) * ty), (1, 1, tx * ty)] {
            if let Some(v) = self.pixel(x0 + dx, y0 + dy) {
                sum += v * w;
                weight += w;
            }
        }
        if weight > 1e-6 {
            Some(sum / weight)
        } else {
            self.missing += 1;
            None
        }
    }
}

/// Climb and descent along altitudes, with a hysteresis of `THRESHOLD`: the level only moves
/// once it has risen or fallen that much. What is left at the end counts, so that climb minus
/// descent is always the last altitude minus the first: sums over edges that follow each other
/// lose nothing at their joints.
pub fn climb(eles: impl IntoIterator<Item = f32>) -> (f32, f32) {
    let (mut up, mut down) = (0.0, 0.0);
    let mut level: Option<f32> = None;
    let mut last = 0.0;
    for e in eles {
        let Some(l) = level else {
            level = Some(e);
            last = e;
            continue;
        };
        if e - l >= THRESHOLD {
            up += e - l;
            level = Some(e);
        } else if l - e >= THRESHOLD {
            down += l - e;
            level = Some(e);
        }
        last = e;
    }
    if let Some(l) = level {
        if last > l { up += last - l } else { down += l - last }
    }
    (up, down)
}

/// Positions every `step` metres along a line (its ends included), as (lat, lon).
pub fn resample(line: &[[f64; 2]], step: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let Some(first) = line.first() else { return out };
    out.push(*first);
    let mut carry = 0.0;
    for w in line.windows(2) {
        let d = crate::logic::fit::metres(w[0], w[1]);
        let mut at = step - carry;
        while at < d {
            let t = at / d;
            out.push([w[0][0] + (w[1][0] - w[0][0]) * t, w[0][1] + (w[1][1] - w[0][1]) * t]);
            at += step;
        }
        carry = (carry + d) % step;
    }
    if line.len() > 1 {
        out.push(line[line.len() - 1]);
    }
    out
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A Terrarium tile whose altitude is `f(column, row)`.
    pub fn terrarium(f: impl Fn(u32, u32) -> f32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(SIZE as u32, SIZE as u32, |x, y| {
            let v = f(x, y) + 32768.0;
            image::Rgb([(v / 256.0).floor() as u8, (v % 256.0).floor() as u8, ((v.fract()) * 256.0) as u8])
        });
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        png
    }

    #[test]
    fn decodes_terrarium_and_reads_between_pixels() {
        let t = decode(&terrarium(|x, _| x as f32 * 2.0 + 100.5)).unwrap();
        assert_eq!((t[0], t[1], t[256]), (100.5, 102.5, 100.5));
        assert!(decode(b"not a png").is_none());
        // One tile everywhere: altitude grows 2 m per pixel eastward.
        let tile = t.clone();
        let mut dem = Dem::new(4, move |_, _| Some(tile.clone()));
        let (lat, lon) = (43.7, 7.25);
        let a = dem.at(lat, lon).unwrap();
        // About 28 m a pixel: 280 m east is ~10 pixels, ~20 m higher (unless it wraps the tile).
        let b = dem.at(lat, lon + 280.0 / (111_320.0 * lat.to_radians().cos())).unwrap();
        assert!((b - a - 20.0).abs() < 1.5 || b < a, "{a} {b}");
        let mut none = Dem::new(4, |_, _| None);
        assert_eq!(none.at(lat, lon), None);
        assert_eq!(none.missing, 1);
    }

    #[test]
    fn climb_ignores_noise_and_counts_real_hills() {
        let (up, down) = climb([100.0, 101.0, 100.0, 101.5, 100.2]);
        assert!((up - 0.2).abs() < 1e-4 && down == 0.0);
        assert_eq!(climb([100.0, 105.0, 110.0, 104.0, 100.0]), (10.0, 10.0));
        assert_eq!(climb([]), (0.0, 0.0));
        assert_eq!(unpack(pack(4102.3)), Some(4102.3));
        assert_eq!(unpack(pack(-12.0)), Some(-12.0));
        assert_eq!(unpack(UNKNOWN), None);
    }

    #[test]
    fn resampling_keeps_the_ends_and_the_step() {
        let line = [[43.0, 7.0], [43.001, 7.0]];
        let pts = resample(&line, 10.0);
        assert_eq!(pts.first(), Some(&line[0]));
        assert_eq!(pts.last(), Some(&line[1]));
        // 111 m: 0, 10, … 110, then the end.
        assert_eq!(pts.len(), 13);
    }
}
