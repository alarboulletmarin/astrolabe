//! OpenStreetMap extracts: downloaded once from Geofabrik (streamed to disk, never held in
//! memory), then read back frame by frame for each pass of the graph build.

use super::store::{cache_dir, extract_path, extract_size, write_atomic, write_atomic_from};
use crate::logic::buildings::{self, Building};
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
    read_frames(&extract_path(region), done, f)
}

/// The buildings of `tiles` (x, y at `buildings::ZOOM`) of a region: from the cache, else read
/// from the extract (two passes over it, on `threads` workers) and kept, tile by tile, empty ones
/// too, so the extract is read once for a place. The cache is the extract's own: another
/// download, another folder.
pub fn buildings(region: &str, tiles: &[(u32, u32)], threads: usize) -> Result<std::collections::HashMap<(u32, u32), Vec<Building>>, String> {
    let size = extract_size(region).ok_or("no extract")?;
    let path = |t: &(u32, u32)| cache_dir().join("buildings").join(format!("{region}-{size}")).join(format!("{}/{}", t.0, t.1));
    let mut out = std::collections::HashMap::new();
    let mut missing = Vec::new();
    for t in tiles {
        match std::fs::read(path(t)).ok().and_then(|b| buildings::decode(&b)) {
            Some(b) => drop(out.insert(*t, b)),
            None => missing.push(*t),
        }
    }
    if !missing.is_empty() {
        let done = AtomicU64::new(0);
        let mut read = |_: u32, f: &mut dyn FnMut(&str, Vec<u8>) -> Result<(), String>| read_extract(region, &done, f);
        for (t, b) in buildings::read(&mut read, &missing, threads, &AtomicBool::new(false))? {
            write_atomic(&path(&t), &buildings::encode(&b), false).ok();
            out.insert(t, b);
        }
    }
    Ok(out)
}

fn read_frames(path: &std::path::Path, done: &AtomicU64, f: &mut dyn FnMut(&str, Vec<u8>) -> Result<(), String>) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("{} : {e}", tr("Extract unreadable", "Extrait illisible")))?;
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

    /// The highest resident memory of this process so far, in MB (Linux only).
    fn peak_mb() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
        line.split_whitespace().nth(1)?.parse::<u64>().ok().map(|kb| kb / 1024)
    }

    /// Builds the graph of a real extract and says how long each step took and the memory at
    /// its peak. Run by hand, in release, with the extract's path:
    ///
    /// ASTROLABE_PBF=paca.osm.pbf cargo test --release -- --ignored --nocapture measure_graph_build
    ///
    /// Elevation tiles go to `target/measure/cache` (fetched once, the network needed then), the
    /// graph to `target/measure/region.graph` (or `ASTROLABE_GRAPH`), for `measure_loops_around_nice`.
    #[test]
    #[ignore]
    fn measure_graph_build() {
        use crate::logic::{elevation, graph};
        use std::time::Instant;
        let pbf = std::path::PathBuf::from(std::env::var("ASTROLABE_PBF").expect("ASTROLABE_PBF: the extract to build"));
        let out = std::env::var("ASTROLABE_GRAPH").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/target/measure/region.graph")));
        crate::platform::store::use_root(std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/target/measure")));
        let size = std::fs::metadata(&pbf).unwrap().len();
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).max(1);
        let progress = graph::Progress::default();
        let mut passes: Vec<(Instant, u32)> = Vec::new();
        let t0 = Instant::now();
        let mut read = |pass: u32, f: &mut dyn FnMut(&str, Vec<u8>) -> Result<(), String>| {
            passes.push((Instant::now(), pass));
            read_frames(&pbf, &progress.bytes, f)
        };
        let mut g = graph::build(&mut read, size, threads, &AtomicBool::new(false), &progress).unwrap();
        let built = t0.elapsed();
        let tiles = g.tiles();
        let t1 = Instant::now();
        let (failed, last) = crate::platform::elevation::fetch_all(crate::platform::elevation::TERRARIUM, elevation::ZOOM, &tiles, &AtomicU64::new(0), &AtomicBool::new(false));
        let fetched = t1.elapsed();
        let t2 = Instant::now();
        let mut dem = elevation::Dem::new(64, |x, y| crate::platform::elevation::cached(elevation::ZOOM, x, y).and_then(|b| elevation::decode(&b)));
        g.set_elevation(&mut dem);
        let altitudes = t2.elapsed();
        let t3 = Instant::now();
        let bytes = g.encode();
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, &bytes).unwrap();
        let saved = t3.elapsed();
        let t4 = Instant::now();
        let back = graph::Graph::decode(&std::fs::read(&out).unwrap()).unwrap();
        let read_back = t4.elapsed();
        let p = |i: usize| passes.get(i + 1).map_or(t0 + built, |x| x.0) - passes[i].0;
        println!("extract: {} MB, {threads} threads", size / 1_000_000);
        println!("passes: {:.1} s, {:.1} s, {:.1} s; graph: {:.1} s", p(0).as_secs_f64(), p(1).as_secs_f64(), p(2).as_secs_f64(), built.as_secs_f64());
        println!("nodes {}, edges {}, points {}", back.nodes(), back.edges.len(), back.pts.len());
        println!("elevation tiles: {} ({failed} failed {last:?}) in {:.1} s; altitudes {:.1} s", tiles.len(), fetched.as_secs_f64(), altitudes.as_secs_f64());
        println!("file {} MB written in {:.1} s, read back in {:.2} s", bytes.len() / 1_000_000, saved.as_secs_f64(), read_back.as_secs_f64());
        println!("peak memory: {:?} MB", peak_mb());
    }
}
