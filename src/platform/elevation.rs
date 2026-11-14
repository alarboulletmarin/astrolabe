//! Elevation tiles (Terrarium PNG, from the AWS open data bucket), cached on disk like the map
//! tiles: a place is fetched once, then read offline.

use super::store::{cache_dir, write_atomic};
use super::{agent, service_error};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const TERRARIUM: &str = "https://s3.amazonaws.com/elevation-tiles-prod";

fn path(z: u8, x: u32, y: u32) -> PathBuf {
    cache_dir().join("elevation").join("terrarium").join(format!("{z}/{x}/{y}.png"))
}

/// The tile from the cache, without the network.
pub fn cached(z: u8, x: u32, y: u32) -> Option<Vec<u8>> {
    fs::read(path(z, x, y)).ok()
}

/// The tile, from the cache or else the server (then kept).
pub fn tile(base: &str, z: u8, x: u32, y: u32) -> Result<Vec<u8>, String> {
    fetch(base, z, x, y, &path(z, x, y))
}

/// The tile from `at` on disk, or else from the server, then kept there.
fn fetch(base: &str, z: u8, x: u32, y: u32, at: &std::path::Path) -> Result<Vec<u8>, String> {
    if let Ok(b) = fs::read(at) {
        return Ok(b);
    }
    let url = format!("{}/terrarium/{z}/{x}/{y}.png", base.trim_end_matches('/'));
    let who = crate::lang::tr("The elevation server", "Le serveur d'altitude");
    let bytes = agent().get(&url).call().map_err(|e| service_error(who, e))?.body_mut().read_to_vec().map_err(|e| service_error(who, e))?;
    write_atomic(at, &bytes, false).ok();
    Ok(bytes)
}

/// Fetches the tiles not cached yet, four at a time; `done` counts the tiles handled. Returns how
/// many failed, and the last failure.
pub fn fetch_all(base: &str, z: u8, tiles: &[(u32, u32)], done: &AtomicU64, cancel: &AtomicBool) -> (usize, Option<String>) {
    // Where each goes, decided here: the store's folder belongs to this thread (in the tests,
    // each has its own).
    let paths: Vec<PathBuf> = tiles.iter().map(|(x, y)| path(z, *x, *y)).collect();
    let next = AtomicU64::new(0);
    let failed = AtomicU64::new(0);
    let last = std::sync::Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed) as usize;
                    if i >= tiles.len() || cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let (x, y) = tiles[i];
                    if let Err(e) = fetch(base, z, x, y, &paths[i]) {
                        failed.fetch_add(1, Ordering::Relaxed);
                        *last.lock().unwrap() = Some(e);
                    }
                    done.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    (failed.load(Ordering::Relaxed) as usize, last.into_inner().unwrap())
}
