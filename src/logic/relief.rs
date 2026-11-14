//! A shaded relief for a map view, from Terrarium elevation tiles: slopes facing the light (from
//! the north-west, 45° up) lighter, the others darker, flat ground untouched. A raster drawn over
//! the map tiles, like the heat map's.

use crate::logic::map::{TILE, View};

/// The zoom of the elevation tiles read for a view: the view's own, within what the tiles hold.
pub fn zoom_for(view: &View) -> u8 {
    view.z.floor().clamp(5.0, 12.0) as u8
}

/// The elevation tiles (x, y at `zoom_for(view)`) a view of `w` × `h` pixels needs, a pixel's
/// margin included.
pub fn tiles(view: &View, w: f64, h: f64) -> Vec<(u32, u32)> {
    let z = zoom_for(view);
    let k = 2f64.powf(z as f64 - view.z);
    let n = 1i64 << z;
    let (x0, y0) = ((view.cx - w / 2.0) * k / TILE, (view.cy - h / 2.0) * k / TILE);
    let (x1, y1) = ((view.cx + w / 2.0) * k / TILE, (view.cy + h / 2.0) * k / TILE);
    let mut out = Vec::new();
    for y in (y0.floor() as i64 - 1).max(0)..=(y1.floor() as i64 + 1).min(n - 1) {
        for x in (x0.floor() as i64 - 1).max(0)..=(x1.floor() as i64 + 1).min(n - 1) {
            out.push((x as u32, y as u32));
        }
    }
    out
}

/// The shade of a view of `w` × `h` pixels, BGRA rows: black or white with the alpha of the slope
/// (stronger on `dark` themes, where the white reads less). `tile(x, y)` gives the altitudes of
/// a tile at `zoom_for(view)`, 256 × 256 rows; pixels without one stay clear.
pub fn shade(view: &View, w: usize, h: usize, dark: bool, tile: &dyn Fn(u32, u32) -> Option<std::sync::Arc<Vec<f32>>>) -> Vec<u8> {
    let z = zoom_for(view);
    let k = 2f64.powf(z as f64 - view.z);
    let size = TILE as i64;
    let max = size << z;
    let mut cache: std::collections::HashMap<(i64, i64), Option<std::sync::Arc<Vec<f32>>>> = std::collections::HashMap::new();
    let mut at = |gx: i64, gy: i64| -> Option<f32> {
        if gx < 0 || gy < 0 || gx >= max || gy >= max {
            return None;
        }
        let t = cache.entry((gx / size, gy / size)).or_insert_with(|| tile((gx / size) as u32, (gy / size) as u32));
        t.as_ref().map(|t| t[((gy % size) * size + gx % size) as usize])
    };
    // Metres per tile pixel at the view's centre latitude.
    let lat = (std::f64::consts::PI * (1.0 - 2.0 * view.cy / (TILE * 2f64.powf(view.z)))).sinh().atan();
    let metres = 40_075_016.7 * lat.cos() / (TILE * (1u64 << z) as f64);
    // Light from the north-west, 45° up; y grows southward.
    let (lx, ly, lz) = (-0.5f64, -0.5f64, std::f64::consts::FRAC_1_SQRT_2);
    let strength = if dark { 1.1 } else { 0.8 };
    let mut out = vec![0u8; w * h * 4];
    for py in 0..h {
        for px in 0..w {
            let gx = ((view.cx + px as f64 - w as f64 / 2.0) * k).floor() as i64;
            let gy = ((view.cy + py as f64 - h as f64 / 2.0) * k).floor() as i64;
            let (Some(e), Some(n), Some(s), Some(we)) = (at(gx + 1, gy), at(gx, gy - 1), at(gx, gy + 1), at(gx - 1, gy)) else { continue };
            // Slopes (exaggerated twice: 30 m pixels flatten the hills).
            let dx = 2.0 * (e - we) as f64 / (2.0 * metres);
            let dy = 2.0 * (s - n) as f64 / (2.0 * metres);
            let norm = (dx * dx + dy * dy + 1.0).sqrt();
            let lit = (-dx * lx - dy * ly + lz) / norm - lz;
            let alpha = (lit.abs() * strength * 255.0).min(150.0) as u8;
            let c = if lit > 0.0 { 255 } else { 0 };
            let i = (py * w + px) * 4;
            out[i..i + 4].copy_from_slice(&[c, c, c, alpha]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::map::world;
    use std::sync::Arc;

    #[test]
    fn slopes_facing_the_light_are_lighter() {
        let (cx, cy) = world(43.7, 7.25, 12.0);
        let view = View { cx, cy, z: 12.0 };
        // A ridge running north-south in every tile: rising east of x = 128, falling west of it.
        let ridge: Arc<Vec<f32>> = Arc::new((0..256 * 256).map(|i| 1000.0 - ((i % 256) as f32 - 128.0).abs() * 10.0).collect());
        let r = ridge.clone();
        let img = shade(&view, 64, 32, false, &move |_, _| Some(r.clone()));
        let px = |x: usize, y: usize| &img[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        // Find, along a row, a pixel on each side of the crest.
        let row: Vec<&[u8]> = (0..64).map(|x| px(x, 10)).collect();
        assert!(row.iter().any(|p| p[0] == 255 && p[3] > 20), "a lit slope");
        assert!(row.iter().any(|p| p[0] == 0 && p[3] > 20), "a slope in the shade");
        // Flat ground, or no tiles: clear.
        let flat: Arc<Vec<f32>> = Arc::new(vec![500.0; 256 * 256]);
        let img = shade(&view, 16, 16, true, &move |_, _| Some(flat.clone()));
        assert!(img.chunks(4).all(|p| p[3] == 0));
        assert!(shade(&view, 16, 16, true, &|_, _| None).chunks(4).all(|p| p[3] == 0));
        assert_eq!(zoom_for(&View { z: 15.5, ..view }), 12);
        assert!(tiles(&view, 64.0, 32.0).len() >= 4);
        let _ = ridge;
    }
}
