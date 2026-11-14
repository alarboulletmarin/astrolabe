//! OpenStreetMap extracts: downloaded once from Geofabrik (streamed to disk, never held in
//! memory), then read back frame by frame for each pass of the graph build.

use super::store::{extract_path, write_atomic_from};
use super::{agent, service_error};
use crate::lang::tr;
use crate::logic::osm::{MAX_HEADER, blob_header};
use crate::logic::regions::Region;
use std::fs::File;
use std::io::{BufReader, Read};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const GEOFABRIK: &str = "https://download.geofabrik.de";

/// Counts what goes through, and stops when asked.
struct Counted<'a, R> {
    inner: R,
    done: &'a AtomicU64,
    cancel: &'a AtomicBool,
}

impl<R: Read> Read for Counted<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::other(crate::logic::graph::CANCELLED));
        }
        let n = self.inner.read(buf)?;
        self.done.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// Downloads a region's extract to the data folder: `total` is set once the server says the
/// size, `done` counts the bytes. A cut download leaves the file that was there untouched.
pub fn download(base: &str, region: &Region, done: &AtomicU64, total: &AtomicU64, cancel: &AtomicBool) -> Result<u64, String> {
    let url = format!("{}/{}", base.trim_end_matches('/'), region.path);
    let mut res = agent().get(&url).call().map_err(|e| service_error("Geofabrik", e))?;
    let expected = res.body().content_length();
    total.store(expected.unwrap_or(0), Ordering::Relaxed);
    let mut from = Counted { inner: res.body_mut().as_reader(), done, cancel };
    let cut = tr("Download cut", "Téléchargement interrompu");
    let n = write_atomic_from(&extract_path(region.id), &mut from, false).map_err(|e| {
        if e.to_string() == crate::logic::graph::CANCELLED { e.to_string() } else { format!("{cut} : {e}") }
    })?;
    if expected.is_some_and(|x| x != n) {
        return Err(format!("{cut} : {n} / {}", expected.unwrap_or(0)));
    }
    Ok(n)
}

/// Reads a region's extract, handing each blob (its type and bytes) to `f`; `done` counts the
/// bytes read.
pub fn read_extract(region: &str, done: &AtomicU64, f: &mut dyn FnMut(&str, Vec<u8>) -> Result<(), String>) -> Result<(), String> {
    let file = File::open(extract_path(region)).map_err(|e| format!("{} : {e}", tr("Extract unreadable", "Extrait illisible")))?;
    let mut r = BufReader::with_capacity(1 << 20, file);
    loop {
        let mut len = [0u8; 4];
        // A clean end is between frames.
        match r.read(&mut len[..1]) {
            Ok(0) => return Ok(()),
            Ok(_) => r.read_exact(&mut len[1..]).map_err(|_| "PBF: truncated frame")?,
            Err(e) => return Err(e.to_string()),
        }
        let len = u32::from_be_bytes(len) as usize;
        if len > MAX_HEADER {
            return Err("PBF: frame header too large".into());
        }
        let mut head = vec![0u8; len];
        r.read_exact(&mut head).map_err(|_| "PBF: truncated frame")?;
        let (kind, size) = blob_header(&head)?;
        let mut blob = vec![0u8; size];
        r.read_exact(&mut blob).map_err(|_| "PBF: truncated blob")?;
        done.fetch_add((4 + len + size) as u64, Ordering::Relaxed);
        f(&kind, blob)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_extract_frame_by_frame_and_stops_at_damage() {
        let root = std::env::temp_dir().join(format!("astrolabe-extract-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        crate::platform::store::use_root(root.clone());
        let fixture = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/loops.osm.pbf")).unwrap();
        let path = extract_path("town");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &fixture).unwrap();
        let done = AtomicU64::new(0);
        let mut kinds = Vec::new();
        read_extract("town", &done, &mut |k, _| {
            kinds.push(k.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(kinds[0], "OSMHeader");
        assert!(kinds.len() > 3 && kinds[1..].iter().all(|k| k == "OSMData"));
        assert_eq!(done.load(Ordering::Relaxed), fixture.len() as u64);
        std::fs::write(&path, &fixture[..fixture.len() - 7]).unwrap();
        assert!(read_extract("town", &done, &mut |_, _| Ok(())).unwrap_err().starts_with("PBF:"));
        assert_eq!(crate::platform::store::extract_size("missing"), None);
        assert!(read_extract("missing", &done, &mut |_, _| Ok(())).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
