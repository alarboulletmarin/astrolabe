//! The buildings of a region's extract, for the ground in 3D: footprints and heights, read tile
//! by tile and kept on disk, so the extract is read once for a place. Pure: the caller reads
//! and writes the files.

use crate::logic::graph::{Progress, Reader, gallop, pass};
use crate::logic::map::world;
use crate::logic::osm;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

/// The zoom of the tiles the buildings are kept by: about 3.5 km across at 45°.
pub const ZOOM: u8 = 13;

/// A building: its outline (latitude and longitude in 1e-7 degrees, not closed) and its height in
/// metres.
#[derive(Clone, Debug, PartialEq)]
pub struct Building {
    pub ring: Vec<[i32; 2]>,
    pub height: f32,
}

/// The tile (x, y at `ZOOM`) a position is in.
pub fn tile_of(lat: f64, lon: f64) -> (u32, u32) {
    let (x, y) = world(lat, lon, ZOOM as f64);
    let max = (1u32 << ZOOM) - 1;
    (((x / 256.0) as u32).min(max), ((y / 256.0) as u32).min(max))
}

/// The tiles within `margin` metres of a line of (lat, lon): where its buildings are.
pub fn tiles_along(line: &[[f64; 2]], margin: f64) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for p in line {
        let (dy, dx) = (margin / 110_540.0, margin / (111_320.0 * p[0].to_radians().cos().max(0.01)));
        for (a, b) in [(-dy, -dx), (-dy, dx), (dy, -dx), (dy, dx)] {
            let t = tile_of(p[0] + a, p[1] + b);
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

/// The buildings of `tiles` (each goes to the tile of its first point) from an extract, in two
/// passes over it: the positions around the tiles, then the buildings made of them. Every tile
/// asked for is in the answer, empty or not.
pub fn read(read: &mut Reader, tiles: &[(u32, u32)], threads: usize, cancel: &AtomicBool) -> Result<HashMap<(u32, u32), Vec<Building>>, String> {
    let mut out: HashMap<(u32, u32), Vec<Building>> = tiles.iter().map(|t| (*t, Vec::new())).collect();
    if tiles.is_empty() {
        return Ok(out);
    }
    // The box of the tiles, in 1e-7 degrees, a tile's tenth wider: a building's first point
    // says its tile, the others may stand a little outside.
    let n = (1u64 << ZOOM) as f64;
    let lat = |y: f64| (std::f64::consts::PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees();
    let lon = |x: f64| x / n * 360.0 - 180.0;
    let (x0, x1) = (tiles.iter().map(|t| t.0).min().unwrap() as f64 - 0.1, tiles.iter().map(|t| t.0).max().unwrap() as f64 + 1.1);
    let (y0, y1) = (tiles.iter().map(|t| t.1).min().unwrap() as f64 - 0.1, tiles.iter().map(|t| t.1).max().unwrap() as f64 + 1.1);
    let e7 = |v: f64| (v * 1e7) as i32;
    let (south, north, west, east) = (e7(lat(y1)), e7(lat(y0)), e7(lon(x0)), e7(lon(x1)));
    let progress = Progress::default();

    // Pass 1: the positions in the box.
    let (_, states) = pass(read, 1, threads, cancel, &progress, Vec::<(i64, [i32; 2])>::new, |nodes, _, d| {
        nodes.extend(osm::block(d, true, false)?.nodes.into_iter().filter(|(_, la, lo)| (south..=north).contains(la) && (west..=east).contains(lo)).map(|(id, la, lo)| (id, [la, lo])));
        Ok(())
    })?;
    let mut nodes: Vec<(i64, [i32; 2])> = states.into_iter().flatten().collect();
    nodes.sort_unstable_by_key(|n| n.0);
    let ids: Vec<i64> = nodes.iter().map(|n| n.0).collect();

    // Pass 2: the buildings whose points are all there, and whose first is in a tile asked for.
    let (_, states) = pass(read, 2, threads, cancel, &progress, Vec::<((u32, u32), Building)>::new, |found, _, d| {
        let mut cursor = 0;
        for (refs, height) in osm::block_of(d, false, false, true)?.buildings {
            let Some(first) = gallop(&ids, &mut cursor, refs[0]).map(|i| nodes[i].1) else { continue };
            let tile = tile_of(first[0] as f64 / 1e7, first[1] as f64 / 1e7);
            if !tiles.contains(&tile) {
                continue;
            }
            // Closed: the last point is the first again.
            let ring: Option<Vec<[i32; 2]>> = refs[..refs.len() - 1].iter().map(|r| ids.binary_search(r).ok().map(|i| nodes[i].1)).collect();
            if let Some(ring) = ring {
                found.push((tile, Building { ring, height }));
            }
        }
        Ok(())
    })?;
    for (tile, b) in states.into_iter().flatten() {
        out.entry(tile).or_default().push(b);
    }
    Ok(out)
}

/// A tile's buildings as bytes: a count, then for each its height, its number of points and its
/// points (little-endian).
pub fn encode(buildings: &[Building]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + buildings.iter().map(|b| 6 + b.ring.len() * 8).sum::<usize>());
    out.extend_from_slice(&(buildings.len() as u32).to_le_bytes());
    for b in buildings {
        out.extend_from_slice(&b.height.to_le_bytes());
        out.extend_from_slice(&(b.ring.len().min(u16::MAX as usize) as u16).to_le_bytes());
        for p in b.ring.iter().take(u16::MAX as usize) {
            out.extend_from_slice(&p[0].to_le_bytes());
            out.extend_from_slice(&p[1].to_le_bytes());
        }
    }
    out
}

/// Reads what `encode` wrote; `None` when the bytes are cut short or are something else.
pub fn decode(bytes: &[u8]) -> Option<Vec<Building>> {
    let mut at = 0usize;
    let mut take = |n: usize| {
        let s = bytes.get(at..at.checked_add(n)?)?;
        at += n;
        Some(s)
    };
    let count = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
    // A building takes 6 bytes at least: a count beyond what the bytes can hold is damage.
    if count > bytes.len() / 6 {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let height = f32::from_le_bytes(take(4)?.try_into().ok()?);
        let n = u16::from_le_bytes(take(2)?.try_into().ok()?) as usize;
        let pts = take(n * 8)?;
        let ring = pts.chunks_exact(8).map(|c| [i32::from_le_bytes(c[..4].try_into().unwrap()), i32::from_le_bytes(c[4..].try_into().unwrap())]).collect();
        out.push(Building { ring, height: if height.is_finite() { height.clamp(2.0, 400.0) } else { 8.0 } });
    }
    (at == bytes.len()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::graph::tests::extract;

    fn read_from(blobs: &[(String, Vec<u8>)], tiles: &[(u32, u32)]) -> HashMap<(u32, u32), Vec<Building>> {
        let mut reader = |_: u32, f: &mut dyn FnMut(&str, Vec<u8>) -> Result<(), String>| {
            for (k, b) in blobs {
                f(k, b.clone())?;
            }
            Ok(())
        };
        read(&mut reader, tiles, 3, &AtomicBool::new(false)).unwrap()
    }

    #[test]
    fn reads_the_buildings_of_the_tiles_asked_for() {
        // Two houses in Nice, a shed in Monaco, an open way, a house with a point that is not in
        // the extract, a street.
        let d = 0.0001;
        let nodes = [
            (1, 43.70, 7.25), (2, 43.70, 7.25 + d), (3, 43.70 + d, 7.25 + d), (4, 43.70 + d, 7.25),
            (5, 43.7003, 7.2503), (6, 43.7003, 7.2503 + d), (7, 43.7003 + d, 7.2503),
            (8, 43.74, 7.42), (9, 43.74, 7.42 + d), (10, 43.74 + d, 7.42),
        ];
        let ways = [
            (100, vec![("building", "yes"), ("height", "14")], vec![1, 2, 3, 4, 1]),
            (101, vec![("building", "house")], vec![5, 6, 7, 5]),
            (102, vec![("building", "shed")], vec![8, 9, 10, 8]),
            (103, vec![("building", "yes")], vec![1, 2, 3]),
            (104, vec![("building", "yes")], vec![1, 2, 99, 1]),
            (105, vec![("highway", "residential")], vec![1, 2]),
        ];
        let blobs = extract(&nodes, &ways);
        let (nice, monaco) = (tile_of(43.70, 7.25), tile_of(43.74, 7.42));
        assert_ne!(nice, monaco);
        let got = read_from(&blobs, &[nice]);
        assert_eq!(got.len(), 1);
        let mut houses = got[&nice].clone();
        houses.sort_by(|a, b| a.height.total_cmp(&b.height));
        assert_eq!(houses.len(), 2, "{houses:?}");
        assert_eq!((houses[0].height, houses[0].ring.len()), (8.0, 3));
        assert_eq!(houses[1], Building { height: 14.0, ring: vec![[437_000_000, 72_500_000], [437_000_000, 72_501_000], [437_001_000, 72_501_000], [437_001_000, 72_500_000]] });
        // Both tiles; a tile with nothing is there, empty; no tile, nothing read.
        let empty = tile_of(44.5, 6.0);
        let got = read_from(&blobs, &[nice, monaco, empty]);
        assert_eq!((got[&nice].len(), got[&monaco].len(), got[&empty].len()), (2, 1, 0));
        assert!(read_from(&blobs, &[]).is_empty());
    }

    #[test]
    fn tiles_go_to_bytes_and_back_and_damage_is_none() {
        let some = vec![Building { ring: vec![[437_000_000, 72_500_000], [-1, i32::MIN], [i32::MAX, 0]], height: 14.5 }, Building { ring: vec![], height: 2.0 }];
        let bytes = encode(&some);
        assert_eq!(decode(&bytes), Some(some));
        assert_eq!(decode(&encode(&[])), Some(vec![]));
        // Cut short, grown, or garbage: never a panic, never half a tile.
        for cut in 0..bytes.len() {
            assert_eq!(decode(&bytes[..cut]), None, "cut at {cut}");
        }
        assert_eq!(decode(&[bytes.clone(), vec![0]].concat()), None);
        assert_eq!(decode(&[255; 64]), None);
        assert_eq!(decode(&[]), None);
    }

    #[test]
    fn a_line_asks_for_the_tiles_beside_it_once() {
        let t = tiles_along(&[[43.70, 7.25], [43.7001, 7.2501], [43.74, 7.42]], 300.0);
        assert!(t.contains(&tile_of(43.70, 7.25)) && t.contains(&tile_of(43.74, 7.42)));
        let mut u = t.clone();
        u.sort();
        u.dedup();
        assert_eq!(u.len(), t.len());
        assert!(t.len() <= 8, "{t:?}");
        assert!(tiles_along(&[], 300.0).is_empty());
    }

    /// The real thing: `ASTRO_PBF=<extract> cargo test --release reading_buildings -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn timing_reading_buildings_from_an_extract() {
        let path = std::env::var("ASTRO_PBF").expect("ASTRO_PBF");
        let mut reader = |_: u32, f: &mut dyn FnMut(&str, Vec<u8>) -> Result<(), String>| {
            use std::io::Read;
            let mut r = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(&path).map_err(|e| e.to_string())?);
            loop {
                let mut len = [0u8; 4];
                if r.read_exact(&mut len).is_err() {
                    return Ok(());
                }
                let mut head = vec![0u8; u32::from_be_bytes(len) as usize];
                r.read_exact(&mut head).map_err(|e| e.to_string())?;
                let (kind, size) = osm::blob_header(&head)?;
                let mut blob = vec![0u8; size];
                r.read_exact(&mut blob).map_err(|e| e.to_string())?;
                f(&kind, blob)?;
            }
        };
        // A loop of 24 km along the bay of Nice.
        let line: Vec<[f64; 2]> = (0..200).map(|k| [43.66 + 0.05 * (k as f64 / 200.0), 7.20 + 0.14 * (k as f64 / 200.0)]).collect();
        let tiles = tiles_along(&line, 300.0);
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).max(1);
        let start = std::time::Instant::now();
        let got = read(&mut reader, &tiles, threads, &AtomicBool::new(false)).unwrap();
        let s = start.elapsed().as_secs_f64();
        let (count, points): (usize, usize) = got.values().flatten().fold((0, 0), |(c, p), b| (c + 1, p + b.ring.len()));
        let bytes: usize = got.values().map(|v| encode(v).len()).sum();
        println!("buildings: {} tiles, {count} buildings, {points} points, {bytes} bytes on disk, read in {s:.1} s on {threads} threads", tiles.len());
    }
}
