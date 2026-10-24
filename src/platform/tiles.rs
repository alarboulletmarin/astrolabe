//! Map tiles: OpenStreetMap's by default (attribution shown on every map), cached on disk
//! so a place is fetched once, as the tile usage policy asks.

use super::store::{cache_dir, write_atomic};
use super::{agent, http_error};
use std::fs;

pub const DEFAULT_TILES: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";

pub fn tile_url(template: &str, z: u8, x: u32, y: u32) -> String {
    template.replace("{z}", &z.to_string()).replace("{x}", &x.to_string()).replace("{y}", &y.to_string())
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
    }
}
