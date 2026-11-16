//! Map tiles: OpenStreetMap's by default (attribution shown on every map), and the IGN's aerial
//! photos for the ground in 3D; cached on disk so a place is fetched once, as the tile usage
//! policy asks.

use super::store::{cache_dir, write_atomic};
use super::{USER_AGENT, agent, http_error, service_error};
use std::fs;

pub const DEFAULT_TILES: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";

/// The IGN's aerial photos of France (open licence, no key). Outside France, and out at sea,
/// all white; far from it, nothing (404).
pub const PHOTO_TILES: &str = "https://data.geopf.fr/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER=ORTHOIMAGERY.ORTHOPHOTOS&STYLE=normal&TILEMATRIXSET=PM&TILEMATRIX={z}&TILEROW={y}&TILECOL={x}&FORMAT=image/jpeg";

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
    fetch(template, z, x, y).map_err(http_error)
}

/// An aerial photo's tile (`PHOTO_TILES`); `None` where the IGN has none at all. One client for
/// all of them, so that its connections serve again; a tile not there within 10 s is given up
/// (the server sometimes holds one), to be asked again.
pub fn photo(z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, String> {
    static PHOTOS: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    let client = PHOTOS.get_or_init(|| ureq::Agent::config_builder().user_agent(USER_AGENT).timeout_global(Some(std::time::Duration::from_secs(10))).build().into());
    match fetch_with(client, PHOTO_TILES, z, x, y) {
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        r => r.map(Some).map_err(|e| service_error("IGN", e)),
    }
}

fn fetch(template: &str, z: u8, x: u32, y: u32) -> Result<Vec<u8>, ureq::Error> {
    fetch_with(&agent(), template, z, x, y)
}

fn fetch_with(client: &ureq::Agent, template: &str, z: u8, x: u32, y: u32) -> Result<Vec<u8>, ureq::Error> {
    let url = tile_url(template, z, x, y);
    // One folder per server, so changing it never mixes two styles.
    let server: String = template.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let path = cache_dir().join("tiles").join(server).join(format!("{z}/{x}/{y}"));
    if let Ok(b) = fs::read(&path) {
        return Ok(b);
    }
    let bytes = client.get(&url).call()?.body_mut().read_to_vec()?;
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
