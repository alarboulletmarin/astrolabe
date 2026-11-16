//! Map tiles: OpenStreetMap's by default (attribution shown on every map), and the IGN's aerial
//! photos for the ground in 3D; cached on disk so a place is fetched once, as the tile usage
//! policy asks.

use super::store::{cache_dir, write_atomic};
use super::{USER_AGENT, agent, http_error, service_error};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

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

/// An aerial photo's tile (`PHOTO_TILES`); `None` where the IGN has none at all. One client per
/// wait, so that its connections serve again. Nine in ten come within 0.7 s, but the server
/// sometimes holds one, and a level of photos shows only once all its tiles are there: past
/// 1.2 s it is asked again at once (the second answer is quick), and given up after 10 s.
pub fn photo(z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, String> {
    ASKED.fetch_add(1, Ordering::SeqCst);
    let r = fetch_photo(z, x, y);
    ASKED.fetch_sub(1, Ordering::SeqCst);
    r
}

/// Photo tiles being fetched for the screen: those fetched ahead wait until there are none.
static ASKED: AtomicUsize = AtomicUsize::new(0);

/// Fetches the photo tiles `keys` (zoom, x, y) into the disk cache, in this order, `workers` at
/// a time, only while none is being fetched for the screen; stops once `cancel` is set. A tile
/// that fails is left for the screen to ask. Blocks until done.
pub fn photos_ahead(keys: &[(u8, u32, u32)], workers: usize, cancel: &AtomicBool) {
    let next = AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for _ in 0..workers {
            sc.spawn(|| {
                loop {
                    while ASKED.load(Ordering::SeqCst) > 0 && !cancel.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    let Some(&(z, x, y)) = keys.get(next.fetch_add(1, Ordering::Relaxed)).filter(|_| !cancel.load(Ordering::Relaxed)) else { break };
                    fetch_photo(z, x, y).ok();
                }
            });
        }
    });
}

fn fetch_photo(z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, String> {
    static QUICK: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    static PATIENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    let client = |cell: &'static std::sync::OnceLock<ureq::Agent>, ms| cell.get_or_init(|| ureq::Agent::config_builder().user_agent(USER_AGENT).timeout_global(Some(std::time::Duration::from_millis(ms))).build().into());
    let mut r = fetch_with(client(&QUICK, 1200), PHOTO_TILES, z, x, y);
    if matches!(r, Err(ureq::Error::Timeout(_))) {
        r = fetch_with(client(&PATIENT, 10_000), PHOTO_TILES, z, x, y);
    }
    match r {
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
    if let Some(b) = read_used(&path) {
        return Ok(b);
    }
    let bytes = client.get(&url).call()?.body_mut().read_to_vec()?;
    write_atomic(&path, &bytes, false).ok();
    Ok(bytes)
}

/// The most the map and photo tiles keep on disk. Past it, `trim_cache` takes away the least
/// lately used down to four fifths of it (a route's photos fetched ahead are some 125 MB for
/// 70 km: the last few routes' stay).
const CACHE_MAX: u64 = 500 << 20;

/// Reads a cached tile, marking it as used (its time of change, once a day at most: the cache
/// is trimmed by it).
fn read_used(path: &Path) -> Option<Vec<u8>> {
    let mut f = fs::File::open(path).ok()?;
    let mut b = Vec::new();
    f.read_to_end(&mut b).ok()?;
    let old = f.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|e| e > Duration::from_secs(86_400)));
    if old {
        // Windows only changes the date through a handle opened for writing (nothing is written).
        drop(f);
        fs::File::options().write(true).open(path).and_then(|w| w.set_modified(SystemTime::now())).ok();
    }
    Some(b)
}

/// Keeps the map and photo tiles on disk under `CACHE_MAX` (see there). Blocks a moment
/// (about 0.1 s for 30 000 tiles).
pub fn trim_cache() {
    trim(&cache_dir().join("tiles"), CACHE_MAX);
}

/// Past `max` bytes of files under `dir`, removes the least lately changed down to four fifths
/// of it. The folders stay.
fn trim(dir: &Path, max: u64) {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                dirs.push(e.path());
            } else {
                files.push((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len(), e.path()));
            }
        }
    }
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= max {
        return;
    }
    files.sort_unstable_by_key(|f| f.0);
    for (_, len, path) in files {
        if total <= max / 5 * 4 {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn the_cache_keeps_the_tiles_used_last() {
        let dir = std::env::temp_dir().join(format!("astrolabe-trim-{}", std::process::id()));
        let at = |name: &str| dir.join(name);
        let day = |n: u64| SystemTime::now() - Duration::from_secs(86_400 * n);
        for (name, age) in [("a/1", 5), ("a/2", 4), ("b/1", 3), ("b/2", 2), ("3", 1)] {
            write_atomic(&at(name), &[0; 100], false).unwrap();
            fs::File::options().write(true).open(at(name)).unwrap().set_modified(day(age)).unwrap();
        }
        // Read: used again, the newest now.
        assert_eq!(read_used(&at("a/1")).unwrap().len(), 100);
        assert!(read_used(&at("none")).is_none());
        // Under the limit: nothing goes.
        trim(&dir, 500);
        assert!(at("a/2").exists());
        // Over it: the least lately used, down to four fifths (here 320 bytes: three stay).
        trim(&dir, 400);
        let left: Vec<_> = ["a/1", "a/2", "b/1", "b/2", "3"].into_iter().filter(|n| at(n).exists()).collect();
        assert_eq!(left, ["a/1", "b/2", "3"]);
        // An empty folder, or none.
        trim(&dir.join("nowhere"), 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn photos_fetched_ahead_stop_once_cancelled() {
        // Cancelled before it starts: nothing is asked (the test would otherwise go online).
        let start = std::time::Instant::now();
        photos_ahead(&[(18, 132_000, 94_000); 50], 8, &AtomicBool::new(true));
        photos_ahead(&[], 8, &AtomicBool::new(false));
        assert!(start.elapsed() < std::time::Duration::from_millis(500));
    }

    #[test]
    fn tile_urls() {
        assert_eq!(tile_url(DEFAULT_TILES, 14, 8423, 5856), "https://tile.openstreetmap.org/14/8423/5856.png");
        assert_eq!(tile_url("https://x/{z}-{y}-{x}?k=1", 0, 0, 0), "https://x/0-0-0?k=1");
        // The whole world at zoom 0; at zoom 1, its north-east quarter.
        assert_eq!(tile_url("{bbox}", 0, 0, 0), "-20037508.34,-20037508.34,20037508.34,20037508.34");
        assert_eq!(tile_url("{bbox}", 1, 1, 0), "0.00,0.00,20037508.34,20037508.34");
    }
}
