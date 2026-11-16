//! Map tiles: OpenStreetMap's by default (attribution shown on every map), and the IGN's aerial
//! photos for the ground in 3D; cached on disk so a place is fetched once, as the tile usage
//! policy asks.

use super::store::{cache_dir, write_atomic};
use super::{agent, http_error};
use std::fs;

pub const DEFAULT_TILES: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";

/// The IGN's aerial photos of France (open licence, no key), as PNG: its tiled service only
/// gives them as JPEG, which the app doesn't read. Outside France, and out at sea, all white.
pub const PHOTO_TILES: &str = "https://data.geopf.fr/wms-r/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=ORTHOIMAGERY.ORTHOPHOTOS&STYLES=&CRS=EPSG:3857&BBOX={bbox}&WIDTH=256&HEIGHT=256&FORMAT=image/png";

/// A tile's address: `{z}`, `{x}`, `{y}`, or `{bbox}`, its box in web Mercator metres (west,
/// south, east, north).
pub fn tile_url(template: &str, z: u8, x: u32, y: u32) -> String {
    const HALF: f64 = 20_037_508.342_789_244;
    let side = 2.0 * HALF / (1u64 << z) as f64;
    let (west, north) = (-HALF + x as f64 * side, HALF - y as f64 * side);
    let bbox = format!("{west:.2},{:.2},{:.2},{north:.2}", north - side, west + side);
    template.replace("{z}", &z.to_string()).replace("{x}", &x.to_string()).replace("{y}", &y.to_string()).replace("{bbox}", &bbox)
}

pub fn tile(template: &str, z: u8, x: u32, y: u32) -> Result<Vec<u8>, String> {
    let url = tile_url(template, z, x, y);
    // One folder per server, so changing it never mixes two styles.
    let server: String = template.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let path = cache_dir().join("tiles").join(server).join(format!("{z}/{x}/{y}"));
    if let Ok(b) = fs::read(&path) {
        return Ok(b);
    }
    let bytes = agent()
        .get(&url)
        .call()
        .map_err(http_error)?
        .body_mut()
        .read_to_vec()
        .map_err(http_error)?;
    write_atomic(&path, &bytes, false).ok();
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_urls() {
        assert_eq!(tile_url(DEFAULT_TILES, 14, 8423, 5856), "https://tile.openstreetmap.org/14/8423/5856.png");
        assert_eq!(tile_url("https://x/{z}-{y}-{x}?k=1", 0, 0, 0), "https://x/0-0-0?k=1");
        // The whole world at zoom 0; at zoom 1, its north-east quarter.
        assert_eq!(tile_url("{bbox}", 0, 0, 0), "-20037508.34,-20037508.34,20037508.34,20037508.34");
        assert_eq!(tile_url("{bbox}", 1, 1, 0), "0.00,0.00,20037508.34,20037508.34");
    }
}
