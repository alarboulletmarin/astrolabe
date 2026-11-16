//! The walking graph of a region, built from an OpenStreetMap extract: junctions and dead ends
//! as nodes, the ways between them as edges (nodes of degree 2 contracted, their geometry kept
//! on the edge), each with its length, climb and descent both ways, and class. Stored in a
//! versioned binary format. Pure: the bytes come from the caller.

use crate::logic::elevation::{self, Dem};
use crate::logic::fit::metres;
use crate::logic::osm::{self, Class};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, mpsc};

/// Raised when the filter, the classes, the contraction or the format change: an older graph is
/// built again from the extract kept on disk.
pub const VERSION: u32 = 3;
const MAGIC: &[u8; 8] = b"ASTRLOOP";
/// Edges are cut so that no straight stretch is longer: the altitude is read along it, and the
/// snapping grid finds every edge from its points.
const MAX_SEGMENT: f64 = 60.0;
/// The snapping grid's cells: 0.005° (about 550 × 400 m around Nice), in 1e-7 degrees.
const CELL: i64 = 50_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub a: u32,
    pub b: u32,
    /// Metres.
    pub len: f32,
    /// Climb and descent from `a` to `b`, metres (the other way swaps them).
    pub up: f32,
    pub down: f32,
    pub class: Class,
    /// Where its points between `a` and `b` start in `Graph::pts`.
    pub geom: u32,
}

/// Where the graph comes from, and how complete it is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meta {
    /// Size of the extract it was built from, in bytes.
    pub source_size: u64,
    /// When the extract was cut, in seconds since 1970 (0 if it doesn't say).
    pub source_time: i64,
    /// Positions where no altitude could be read (tiles missing): 0 when complete.
    pub missing: u64,
}

pub struct Graph {
    pub meta: Meta,
    /// Node positions, 1e-7 degrees.
    pub lat: Vec<i32>,
    pub lon: Vec<i32>,
    /// Node altitudes, `elevation::pack`ed.
    pub ele: Vec<u16>,
    /// Connected component of each node (0 is the first found).
    pub comp: Vec<u32>,
    /// The arcs leaving node `v`: `arcs[first[v]..first[v + 1]]`, each `edge << 1 | reversed`.
    pub first: Vec<u32>,
    pub arcs: Vec<u32>,
    pub edges: Vec<Edge>,
    /// The points inside edges, 1e-7 degrees, and their altitudes.
    pub pts: Vec<[i32; 2]>,
    pub pts_ele: Vec<u16>,
    grid: Grid,
    /// Nodes in each component (worked out, never stored).
    sizes: Vec<u32>,
}

/// The nearest place on the graph to a position: on edge `edge`, segment `seg` of its line
/// (from `line[seg]` to `line[seg + 1]`), at `t` along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub edge: u32,
    pub seg: u32,
    pub t: f32,
    /// Metres from the position asked.
    pub dist: f32,
}

impl Graph {
    pub fn nodes(&self) -> usize {
        self.lat.len()
    }

    pub fn arcs_of(&self, v: u32) -> &[u32] {
        &self.arcs[self.first[v as usize] as usize..self.first[v as usize + 1] as usize]
    }

    /// The edge of an arc and the node it leads to.
    pub fn head(&self, arc: u32) -> (u32, u32) {
        let e = &self.edges[(arc >> 1) as usize];
        (arc >> 1, if arc & 1 == 0 { e.b } else { e.a })
    }

    /// How many nodes the component of `v` has.
    pub fn component_size(&self, v: u32) -> u32 {
        self.sizes.get(self.comp[v as usize] as usize).copied().unwrap_or(0)
    }

    pub fn position(&self, v: u32) -> [f64; 2] {
        [self.lat[v as usize] as f64 * 1e-7, self.lon[v as usize] as f64 * 1e-7]
    }

    fn interior(&self, e: u32) -> std::ops::Range<usize> {
        let start = self.edges[e as usize].geom as usize;
        let end = self.edges.get(e as usize + 1).map_or(self.pts.len(), |n| n.geom as usize);
        start..end
    }

    /// An edge's line from `a` to `b`, 1e-7 degrees, with altitudes.
    pub fn line(&self, e: u32) -> Vec<([i32; 2], u16)> {
        let edge = &self.edges[e as usize];
        let (a, b) = (edge.a as usize, edge.b as usize);
        let r = self.interior(e);
        let mut out = Vec::with_capacity(r.len() + 2);
        out.push(([self.lat[a], self.lon[a]], self.ele[a]));
        out.extend(r.map(|i| (self.pts[i], self.pts_ele[i])));
        out.push(([self.lat[b], self.lon[b]], self.ele[b]));
        out
    }

    /// The edges that have a point in a cell (about 550 m a side) touching `bbox` `[south, west,
    /// north, east]`, degrees: every edge with a point in the box, and some around it. Each once.
    pub fn edges_in(&self, [s, w, n, e]: [f64; 4]) -> Vec<u32> {
        let g = &self.grid;
        let cell = |v: f64, v0: i32| ((v * 1e7) as i64 - v0 as i64).div_euclid(CELL);
        let (r0, r1, c0, c1) = (cell(s, g.lat0).max(0), cell(n, g.lat0).min(g.rows as i64 - 1), cell(w, g.lon0).max(0), cell(e, g.lon0).min(g.cols as i64 - 1));
        let mut out = Vec::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                let k = (r * g.cols as i64 + c) as usize;
                out.extend_from_slice(&g.items[g.start[k] as usize..g.start[k + 1] as usize]);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The nearest place on an edge that `accept` takes, within `max` metres.
    pub fn snap(&self, lat: f64, lon: f64, max: f64, accept: impl Fn(u32) -> bool) -> Option<Snap> {
        let g = &self.grid;
        if g.rows == 0 {
            return None;
        }
        let (qlat, qlon) = ((lat * 1e7) as i64, (lon * 1e7) as i64);
        let (row, col) = ((qlat - g.lat0 as i64).div_euclid(CELL), (qlon - g.lon0 as i64).div_euclid(CELL));
        let k = std::f64::consts::PI / 180.0 * 1e-7;
        let mx = 6_371_000.0 * k * (lat * std::f64::consts::PI / 180.0).cos();
        let my = 6_371_000.0 * k;
        // The smaller side of a cell, in metres: a ring of cells `r` away is at least this far times `r - 1`.
        let cell_m = (CELL as f64 * mx).min(CELL as f64 * my);
        let mut best: Option<Snap> = None;
        let mut seen = Vec::new();
        for r in 0..=((max / cell_m).ceil() as i64 + 1) {
            if best.is_some_and(|b| (b.dist as f64) < (r - 1).max(0) as f64 * cell_m) {
                break;
            }
            for dr in -r..=r {
                for dc in -r..=r {
                    if dr.abs() != r && dc.abs() != r {
                        continue;
                    }
                    let (rr, cc) = (row + dr, col + dc);
                    if rr < 0 || cc < 0 || rr >= g.rows as i64 || cc >= g.cols as i64 {
                        continue;
                    }
                    let cell = (rr * g.cols as i64 + cc) as usize;
                    for &e in &g.items[g.start[cell] as usize..g.start[cell + 1] as usize] {
                        if seen.contains(&e) || !accept(e) {
                            continue;
                        }
                        seen.push(e);
                        let line = self.line(e);
                        for (s, w) in line.windows(2).enumerate() {
                            let ax = (w[0].0[1] as i64 - qlon) as f64 * mx;
                            let ay = (w[0].0[0] as i64 - qlat) as f64 * my;
                            let bx = (w[1].0[1] as i64 - qlon) as f64 * mx;
                            let by = (w[1].0[0] as i64 - qlat) as f64 * my;
                            let (dx, dy) = (bx - ax, by - ay);
                            let len2 = dx * dx + dy * dy;
                            let t = if len2 > 0.0 { (-(ax * dx + ay * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                            let d = ((ax + t * dx).powi(2) + (ay + t * dy).powi(2)).sqrt();
                            if d <= max && best.is_none_or(|b| d < b.dist as f64) {
                                best = Some(Snap { edge: e, seg: s as u32, t: t as f32, dist: d as f32 });
                            }
                        }
                    }
                }
            }
        }
        best
    }

    /// The tiles of altitude (`elevation::ZOOM`) its points fall in.
    pub fn tiles(&self) -> Vec<(u32, u32)> {
        let mut t: Vec<(u32, u32)> = (0..self.nodes())
            .map(|v| (self.lat[v], self.lon[v]))
            .chain(self.pts.iter().map(|p| (p[0], p[1])))
            .map(|(la, lo)| elevation::tile_of(la as f64 * 1e-7, lo as f64 * 1e-7))
            .collect();
        t.sort_unstable();
        t.dedup();
        t
    }

    /// Reads the altitude of every node and point, and the climb and descent of every edge
    /// (every `elevation::STEP` metres, with its threshold). Edges are walked tile by tile, so
    /// that a few decoded tiles are enough.
    pub fn set_elevation(&mut self, dem: &mut Dem) {
        let start = dem.missing;
        let mut order: Vec<(u32, u32, u32)> = (0..self.edges.len() as u32)
            .map(|e| {
                let a = self.edges[e as usize].a;
                let (x, y) = elevation::tile_of(self.lat[a as usize] as f64 * 1e-7, self.lon[a as usize] as f64 * 1e-7);
                (y, x, e)
            })
            .collect();
        order.sort_unstable();
        let mut node_done = vec![false; self.nodes()];
        for (_, _, e) in order {
            let r = self.interior(e);
            let edge = self.edges[e as usize];
            for v in [edge.a, edge.b] {
                if !node_done[v as usize] {
                    node_done[v as usize] = true;
                    let [la, lo] = self.position(v);
                    self.ele[v as usize] = dem.at(la, lo).map_or(elevation::UNKNOWN, elevation::pack);
                }
            }
            for i in r.clone() {
                let p = self.pts[i];
                self.pts_ele[i] = dem.at(p[0] as f64 * 1e-7, p[1] as f64 * 1e-7).map_or(elevation::UNKNOWN, elevation::pack);
            }
            let line: Vec<[f64; 2]> = self.line(e).iter().map(|(p, _)| [p[0] as f64 * 1e-7, p[1] as f64 * 1e-7]).collect();
            let eles: Vec<f32> = elevation::resample(&line, elevation::STEP).into_iter().filter_map(|[la, lo]| dem.at(la, lo)).collect();
            let (up, down) = elevation::climb(eles);
            let edge = &mut self.edges[e as usize];
            edge.up = up;
            edge.down = down;
        }
        self.meta.missing = dem.missing - start;
    }

    /// The graph, in its file format.
    pub fn encode(&self) -> Vec<u8> {
        let (n, m, p) = (self.nodes(), self.edges.len(), self.pts.len());
        let mut b = Vec::with_capacity(64 + n * 22 + m * 36 + p * 10);
        b.extend_from_slice(MAGIC);
        put(&mut b, VERSION);
        b.extend_from_slice(&self.meta.source_size.to_le_bytes());
        b.extend_from_slice(&self.meta.source_time.to_le_bytes());
        b.extend_from_slice(&self.meta.missing.to_le_bytes());
        for c in [n, m, p] {
            put(&mut b, c as u32);
        }
        self.lat.iter().for_each(|v| put(&mut b, *v as u32));
        self.lon.iter().for_each(|v| put(&mut b, *v as u32));
        self.ele.iter().for_each(|v| b.extend_from_slice(&v.to_le_bytes()));
        self.comp.iter().for_each(|v| put(&mut b, *v));
        self.first.iter().for_each(|v| put(&mut b, *v));
        self.arcs.iter().for_each(|v| put(&mut b, *v));
        for e in &self.edges {
            for v in [e.a, e.b, e.len.to_bits(), e.up.to_bits(), e.down.to_bits(), e.class.0, e.geom] {
                put(&mut b, v);
            }
        }
        for q in &self.pts {
            put(&mut b, q[0] as u32);
            put(&mut b, q[1] as u32);
        }
        self.pts_ele.iter().for_each(|v| b.extend_from_slice(&v.to_le_bytes()));
        let sum = checksum(&b);
        b.extend_from_slice(&sum.to_le_bytes());
        b
    }

    /// Reads a graph back, checking everything: a damaged file is an error, never a panic.
    pub fn decode(b: &[u8]) -> Result<Graph, Unreadable> {
        if b.len() < 8 + 4 || &b[..8] != MAGIC {
            return Err(Unreadable::Damaged("not a graph".into()));
        }
        let version = u32::from_le_bytes(b[8..12].try_into().unwrap());
        if version != VERSION {
            return Err(Unreadable::Stale(version));
        }
        let damaged = |what: &str| Unreadable::Damaged(what.into());
        if b.len() < 20 {
            return Err(damaged("truncated"));
        }
        let (body, tail) = b.split_at(b.len() - 8);
        if checksum(body) != u64::from_le_bytes(tail.try_into().unwrap()) {
            return Err(damaged("checksum"));
        }
        let mut c = Cursor { b: body, i: 12 };
        let meta = Meta { source_size: c.u64()?, source_time: c.u64()? as i64, missing: c.u64()? };
        let (n, m, p) = (c.u32()? as usize, c.u32()? as usize, c.u32()? as usize);
        // Sizes announced must fit in what is there before anything is allocated.
        let need = n as u64 * 18 + 4 + m as u64 * 36 + p as u64 * 10;
        if need > body.len() as u64 {
            return Err(damaged("sizes"));
        }
        let lat = c.many(n, |c| c.u32().map(|v| v as i32))?;
        let lon = c.many(n, |c| c.u32().map(|v| v as i32))?;
        let ele = c.many(n, Cursor::u16)?;
        let comp = c.many(n, Cursor::u32)?;
        let first = c.many(n + 1, Cursor::u32)?;
        let arcs = c.many(2 * m, Cursor::u32)?;
        let edges = c.many(m, |c| {
            Ok(Edge {
                a: c.u32()?,
                b: c.u32()?,
                len: f32::from_bits(c.u32()?),
                up: f32::from_bits(c.u32()?),
                down: f32::from_bits(c.u32()?),
                class: Class(c.u32()?),
                geom: c.u32()?,
            })
        })?;
        let pts = c.many(p, |c| Ok([c.u32()? as i32, c.u32()? as i32]))?;
        let pts_ele = c.many(p, Cursor::u16)?;
        if c.i != body.len() {
            return Err(damaged("trailing bytes"));
        }
        // Every index points where it should.
        let ok = first.first() == Some(&0)
            && first.last() == Some(&(arcs.len() as u32))
            && first.windows(2).all(|w| w[0] <= w[1])
            && arcs.iter().all(|a| (*a >> 1) < m as u32)
            && edges.iter().all(|e| e.a < n as u32 && e.b < n as u32 && e.geom as usize <= p && e.len.is_finite() && e.len >= 0.0)
            && edges.windows(2).all(|w| w[0].geom <= w[1].geom)
            && comp.iter().all(|c| (*c as usize) < n.max(1));
        if !ok {
            return Err(damaged("indices"));
        }
        let mut g = Graph { meta, lat, lon, ele, comp, first, arcs, edges, pts, pts_ele, grid: Grid::default(), sizes: Vec::new() };
        g.grid = Grid::new(&g);
        g.count_components();
        Ok(g)
    }
}

/// Why a stored graph can't be used.
#[derive(Debug, PartialEq)]
pub enum Unreadable {
    /// Written by another version: build it again from the extract.
    Stale(u32),
    /// Not a graph, or damaged.
    Damaged(String),
}

fn put(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

/// FNV-1a over 8-byte words (the tail byte by byte): catches a truncated or altered file.
fn checksum(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut chunks = b.chunks_exact(8);
    for c in &mut chunks {
        h = (h ^ u64::from_le_bytes(c.try_into().unwrap())).wrapping_mul(0x100000001b3);
    }
    for x in chunks.remainder() {
        h = (h ^ *x as u64).wrapping_mul(0x100000001b3);
    }
    h
}

struct Cursor<'a> {
    b: &'a [u8],
    i: usize,
}

impl Cursor<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Unreadable> {
        let s = self.b.get(self.i..self.i + N).ok_or(Unreadable::Damaged("truncated".into()))?;
        self.i += N;
        Ok(s.try_into().unwrap())
    }
    fn u16(&mut self) -> Result<u16, Unreadable> {
        self.take().map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Result<u32, Unreadable> {
        self.take().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Result<u64, Unreadable> {
        self.take().map(u64::from_le_bytes)
    }
    fn many<T>(&mut self, n: usize, mut f: impl FnMut(&mut Self) -> Result<T, Unreadable>) -> Result<Vec<T>, Unreadable> {
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(f(self)?);
        }
        Ok(v)
    }
}

/// Edges by grid cell, for snapping: `items[start[cell]..start[cell + 1]]` are the edges with a
/// point in that cell. Built when the graph is, never stored.
#[derive(Default)]
struct Grid {
    lat0: i32,
    lon0: i32,
    rows: u32,
    cols: u32,
    start: Vec<u32>,
    items: Vec<u32>,
}

impl Grid {
    fn new(g: &Graph) -> Grid {
        let all = || g.lat.iter().zip(&g.lon).map(|(a, b)| [*a, *b]).chain(g.pts.iter().copied());
        let (Some(lat0), Some(lon0)) = (all().map(|p| p[0]).min(), all().map(|p| p[1]).min()) else {
            return Grid::default();
        };
        let lat1 = all().map(|p| p[0]).max().unwrap();
        let lon1 = all().map(|p| p[1]).max().unwrap();
        let rows = ((lat1 as i64 - lat0 as i64) / CELL + 1) as u32;
        let cols = ((lon1 as i64 - lon0 as i64) / CELL + 1) as u32;
        let cell = |p: [i32; 2]| (((p[0] as i64 - lat0 as i64) / CELL) * cols as i64 + (p[1] as i64 - lon0 as i64) / CELL) as u32;
        // Each edge once per cell it touches: counted, then placed.
        let cells_of = |e: usize, out: &mut Vec<u32>| {
            out.clear();
            let edge = &g.edges[e];
            let end = g.edges.get(e + 1).map_or(g.pts.len(), |n| n.geom as usize);
            let line = std::iter::once([g.lat[edge.a as usize], g.lon[edge.a as usize]])
                .chain(g.pts[edge.geom as usize..end].iter().copied())
                .chain(std::iter::once([g.lat[edge.b as usize], g.lon[edge.b as usize]]));
            for p in line {
                let c = cell(p);
                if !out.contains(&c) {
                    out.push(c);
                }
            }
        };
        let mut start = vec![0u32; rows as usize * cols as usize + 1];
        let mut scratch = Vec::new();
        for e in 0..g.edges.len() {
            cells_of(e, &mut scratch);
            for c in &scratch {
                start[*c as usize + 1] += 1;
            }
        }
        for i in 1..start.len() {
            start[i] += start[i - 1];
        }
        let mut fill = start.clone();
        let mut items = vec![0u32; *start.last().unwrap() as usize];
        for e in 0..g.edges.len() {
            cells_of(e, &mut scratch);
            for c in &scratch {
                items[fill[*c as usize] as usize] = e as u32;
                fill[*c as usize] += 1;
            }
        }
        Grid { lat0, lon0, rows, cols, start, items }
    }
}

/// How far a build has gone: the pass (1 to 3 over the extract), then the bytes of the extract
/// read in that pass (counted by the reader).
#[derive(Default)]
pub struct Progress {
    pub pass: AtomicU64,
    pub bytes: AtomicU64,
}

/// Reads the extract once more, for pass `pass`: hands each blob (its type and bytes) to the
/// callback, in the file's order.
pub type Reader<'a> = dyn FnMut(u32, &mut dyn FnMut(&str, Vec<u8>) -> Result<(), String>) -> Result<(), String> + 'a;

/// One pass over the extract: blobs are inflated and decoded by `threads` workers, each with its
/// own state. Also returns the file's header.
pub(crate) fn pass<S: Send>(
    read: &mut Reader,
    n: u32,
    threads: usize,
    cancel: &AtomicBool,
    progress: &Progress,
    init: impl Fn() -> S + Sync,
    work: impl Fn(&mut S, u64, &[u8]) -> Result<(), String> + Sync,
) -> Result<(osm::Header, Vec<S>), String> {
    progress.pass.store(n as u64, Ordering::Relaxed);
    progress.bytes.store(0, Ordering::Relaxed);
    let threads = threads.max(1);
    let failed: Mutex<Option<String>> = Mutex::new(None);
    let (tx, rx) = mpsc::sync_channel::<(u64, Vec<u8>)>(threads * 2);
    let rx = Mutex::new(rx);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut state = init();
                    loop {
                        let Ok((seq, blob)) = rx.lock().unwrap().recv() else { break };
                        // After a failure, keep draining so the reader never blocks on a full channel.
                        if failed.lock().unwrap().is_some() {
                            continue;
                        }
                        if let Err(e) = osm::blob_data(&blob).and_then(|d| work(&mut state, seq, &d)) {
                            failed.lock().unwrap().get_or_insert(e);
                        }
                    }
                    state
                })
            })
            .collect();
        let mut header = None;
        let mut seq = 0u64;
        let res = read(n, &mut |kind, blob| {
            if cancel.load(Ordering::Relaxed) {
                return Err(CANCELLED.into());
            }
            if let Some(e) = failed.lock().unwrap().clone() {
                return Err(e);
            }
            match kind {
                "OSMHeader" => header = Some(osm::header(&osm::blob_data(&blob)?)?),
                "OSMData" => {
                    tx.send((seq, blob)).map_err(|_| "workers stopped".to_string())?;
                    seq += 1;
                }
                _ => {}
            }
            Ok(())
        });
        drop(tx);
        let states: Vec<S> = workers.into_iter().map(|w| w.join().map_err(|_| "a worker panicked".to_string())).collect::<Result<_, _>>()?;
        res?;
        if let Some(e) = failed.lock().unwrap().take() {
            return Err(e);
        }
        Ok((header.ok_or("PBF: no header")?, states))
    })
}

/// The index of `id` in the sorted `ids`, searching forward from `cursor` (where the last one was
/// found): positions come sorted within a block, so most searches are a step or two.
pub(crate) fn gallop(ids: &[i64], cursor: &mut usize, id: i64) -> Option<usize> {
    let len = ids.len();
    if *cursor >= len || ids[*cursor] > id {
        *cursor = 0;
    }
    let (mut lo, mut hi, mut step) = (*cursor, *cursor, 1);
    while hi < len && ids[hi] < id {
        lo = hi;
        hi += step;
        step *= 2;
    }
    let hi = hi.min(len);
    let i = lo + ids[lo..hi].partition_point(|x| *x < id);
    *cursor = i;
    (i < len && ids[i] == id).then_some(i)
}

/// What a cancelled build answers.
pub const CANCELLED: &str = "cancelled";

/// Builds the graph of an extract, in three passes over it: the ways one can run on (which nodes
/// they use, which are junctions), the positions of those nodes, the ways again (cut at the
/// junctions). Altitudes are not read here (`set_elevation`).
pub fn build(read: &mut Reader, source_size: u64, threads: usize, cancel: &AtomicBool, progress: &Progress) -> Result<Graph, String> {
    // Pass 1: every node a kept way uses, and the ends of the ways.
    let (header, states) = pass(read, 1, threads, cancel, progress, || (Vec::<i64>::new(), Vec::<i64>::new(), Vec::<Vec<i64>>::new()), |(refs, ends, areas), _, d| {
        let b = osm::block(d, false, true)?;
        areas.extend(b.areas);
        for w in b.ways {
            ends.push(w.refs[0]);
            ends.push(*w.refs.last().unwrap());
            refs.extend_from_slice(&w.refs);
        }
        Ok(())
    })?;
    let mut ids: Vec<i64> = Vec::with_capacity(states.iter().map(|s| s.0.len()).sum());
    let mut ends = Vec::new();
    let mut areas: Vec<Vec<i64>> = Vec::new();
    for (r, e, a) in states {
        ids.extend_from_slice(&r);
        ends.extend_from_slice(&e);
        areas.extend(a);
    }
    // Nodes of the industrial areas, apart from `ids`: they are no junctions.
    let mut area_ids: Vec<i64> = areas.iter().flatten().copied().collect();
    area_ids.sort_unstable();
    area_ids.dedup();
    ids.sort_unstable();
    // A junction: used twice or more (by two ways, or twice by one), or the end of a way.
    let mut junction = Vec::with_capacity(ids.len() / 2);
    let mut w = 0;
    for r in 0..ids.len() {
        if w > 0 && ids[w - 1] == ids[r] {
            junction[w - 1] = true;
        } else {
            ids[w] = ids[r];
            junction.push(false);
            w += 1;
        }
    }
    ids.truncate(w);
    ids.shrink_to_fit();
    for e in ends {
        if let Ok(i) = ids.binary_search(&e) {
            junction[i] = true;
        }
    }

    // Pass 2: the positions of those nodes, packed (lat, lon) in a u64; MAX: not found.
    let coords: Vec<AtomicU64> = (0..ids.len()).map(|_| AtomicU64::new(u64::MAX)).collect();
    let area_coords: Vec<AtomicU64> = (0..area_ids.len()).map(|_| AtomicU64::new(u64::MAX)).collect();
    pass(read, 2, threads, cancel, progress, || (), |_, _, d| {
        let (mut cursor, mut area_cursor) = (0usize, 0usize);
        for (id, lat, lon) in osm::block(d, true, false)?.nodes {
            let packed = (lat as u32 as u64) << 32 | lon as u32 as u64;
            if let Some(at) = gallop(&ids, &mut cursor, id) {
                coords[at].store(packed, Ordering::Relaxed);
            }
            if let Some(at) = gallop(&area_ids, &mut area_cursor, id) {
                area_coords[at].store(packed, Ordering::Relaxed);
            }
        }
        Ok(())
    })?;
    let coords: Vec<u64> = coords.into_iter().map(AtomicU64::into_inner).collect();
    // The areas' rings, as positions; one with a node the extract lacks is left out.
    let rings: Vec<Vec<[i32; 2]>> = areas
        .iter()
        .filter_map(|a| {
            a.iter()
                .map(|r| {
                    let c = area_coords[area_ids.binary_search(r).ok()?].load(Ordering::Relaxed);
                    (c != u64::MAX).then_some([(c >> 32) as u32 as i32, c as u32 as i32])
                })
                .collect()
        })
        .collect();
    drop((areas, area_ids, area_coords));
    let pos = |i: u32| {
        let c = coords[i as usize];
        [(c >> 32) as u32 as i32, c as u32 as i32]
    };

    // Pass 3: the ways again, cut at junctions into pieces (node indices into `ids`). A way with
    // a node the extract lacks is left out.
    // ponytail: such a way is dropped whole; cutting it at the gap would keep the rest of it.
    let industrial = Industrial::new(&rings);
    type Pieces = Vec<(u64, Vec<u32>, Vec<(u32, u32, Class)>)>;
    let (_, states) = pass(read, 3, threads, cancel, progress, Pieces::new, |out, seq, d| {
        let (mut flat, mut pieces) = (Vec::new(), Vec::new());
        'way: for w in osm::block(d, false, true)?.ways {
            let mut idx = Vec::with_capacity(w.refs.len());
            for r in &w.refs {
                match ids.binary_search(r) {
                    Ok(i) if coords[i] != u64::MAX => idx.push(i as u32),
                    _ => continue 'way,
                }
            }
            // Industrial or not, segment by segment (`ind[k]`: between `idx[k]` and `idx[k + 1]`).
            // A piece is cut where that changes, so that edges stop at the area's border.
            // ponytail: the test is on a segment's middle, so a segment crossing the border is
            // wholly in or out; fine while way nodes are close (tens of metres), a cut at the
            // border itself if long straight segments turn out to matter.
            let ind: Vec<bool> = idx.windows(2).map(|s| industrial.segment(pos(s[0]), pos(s[1]))).collect();
            let class = |k: usize| if ind[k] { w.class.with_industrial() } else { w.class };
            let (mut start, mut first) = (flat.len() as u32, 0);
            flat.push(idx[0]);
            for (k, &i) in idx.iter().enumerate().skip(1) {
                flat.push(i);
                if junction[i as usize] || ind[k] != ind[k - 1] {
                    pieces.push((start, flat.len() as u32 - start, class(first)));
                    start = flat.len() as u32;
                    first = k;
                    flat.push(i);
                }
            }
            // The way's last node is a junction (an end), so the last piece is closed: drop the
            // node pushed to start a piece that never came.
            flat.pop();
        }
        out.push((seq, flat, pieces));
        Ok(())
    })?;
    let mut blocks: Pieces = states.into_iter().flatten().collect();
    blocks.sort_unstable_by_key(|b| b.0);
    let mut flat: Vec<u32> = Vec::new();
    let mut pieces: Vec<(u32, u32, Class)> = Vec::new();
    for (_, f, p) in blocks {
        let base = flat.len() as u32;
        flat.extend_from_slice(&f);
        pieces.extend(p.into_iter().map(|(s, n, c)| (s + base, n, c)));
    }
    drop(junction);
    drop(ids);
    let ends_of = |p: usize| {
        let (s, n, _) = pieces[p];
        (flat[s as usize], flat[(s + n - 1) as usize])
    };

    // Contraction: a node where exactly two pieces of the same class meet is not a junction.
    let mut incident: Vec<(u32, u32)> = (0..pieces.len()).flat_map(|p| {
        let (a, b) = ends_of(p);
        [(a, (p as u32) << 1), (b, (p as u32) << 1 | 1)]
    }).collect();
    incident.sort_unstable();
    let at = |v: u32| {
        let lo = incident.partition_point(|x| x.0 < v);
        let hi = incident.partition_point(|x| x.0 <= v);
        &incident[lo..hi]
    };
    let through = |v: u32| {
        let inc = at(v);
        inc.len() == 2 && inc[0].1 >> 1 != inc[1].1 >> 1 && pieces[(inc[0].1 >> 1) as usize].2 == pieces[(inc[1].1 >> 1) as usize].2
    };
    // Chains of pieces, each with its direction (`true`: as stored).
    let mut done = vec![false; pieces.len()];
    let mut chains: Vec<Vec<(u32, bool)>> = Vec::new();
    for p0 in 0..pieces.len() {
        if done[p0] {
            continue;
        }
        // Back to the chain's start: while the start node is passed through, step to the other piece.
        let (mut p, mut fwd) = (p0 as u32, true);
        loop {
            let (a, b) = ends_of(p as usize);
            let s = if fwd { a } else { b };
            if !through(s) {
                break;
            }
            let inc = at(s);
            let other = if inc[0].1 >> 1 == p { inc[1].1 } else { inc[0].1 };
            if other >> 1 == p0 as u32 {
                break; // a ring: start anywhere.
            }
            // Reaching `s` by the other piece: its end at `s` is where it finishes.
            p = other >> 1;
            fwd = other & 1 == 1;
        }
        let mut chain = Vec::new();
        loop {
            done[p as usize] = true;
            chain.push((p, fwd));
            let (a, b) = ends_of(p as usize);
            let t = if fwd { b } else { a };
            if !through(t) {
                break;
            }
            let inc = at(t);
            let other = if inc[0].1 >> 1 == p { inc[1].1 } else { inc[0].1 };
            if done[(other >> 1) as usize] {
                break;
            }
            p = other >> 1;
            fwd = other & 1 == 0;
        }
        chains.push(chain);
    }
    drop(incident);

    // Nodes: the ends of chains.
    let mut nodes: Vec<u32> = Vec::with_capacity(chains.len() * 2);
    let chain_line = |chain: &[(u32, bool)]| {
        let mut line: Vec<u32> = Vec::new();
        for &(p, fwd) in chain {
            let (s, n, _) = pieces[p as usize];
            let part = &flat[s as usize..(s + n) as usize];
            let skip = usize::from(!line.is_empty());
            if fwd {
                line.extend(part.iter().skip(skip));
            } else {
                line.extend(part.iter().rev().skip(skip));
            }
        }
        line
    };
    for chain in &chains {
        let line = chain_line(chain);
        nodes.push(line[0]);
        nodes.push(*line.last().unwrap());
    }
    nodes.sort_unstable();
    nodes.dedup();
    let rank = |i: u32| nodes.binary_search(&i).unwrap() as u32;

    let mut g = Graph {
        meta: Meta { source_size, source_time: header.timestamp, missing: 0 },
        lat: nodes.iter().map(|i| pos(*i)[0]).collect(),
        lon: nodes.iter().map(|i| pos(*i)[1]).collect(),
        ele: vec![elevation::UNKNOWN; nodes.len()],
        comp: Vec::new(),
        first: Vec::new(),
        arcs: Vec::new(),
        edges: Vec::with_capacity(chains.len()),
        pts: Vec::new(),
        pts_ele: Vec::new(),
        grid: Grid::default(),
        sizes: Vec::new(),
    };
    let deg = |p: [i32; 2]| [p[0] as f64 * 1e-7, p[1] as f64 * 1e-7];
    for chain in &chains {
        let line = chain_line(chain);
        let geom = g.pts.len() as u32;
        let mut len = 0.0;
        for k in 1..line.len() {
            let (a, b) = (pos(line[k - 1]), pos(line[k]));
            let d = metres(deg(a), deg(b));
            len += d;
            // Long stretches get points every `MAX_SEGMENT` or less.
            let cuts = (d / MAX_SEGMENT).ceil() as usize;
            for c in 1..cuts {
                let t = c as f64 / cuts as f64;
                g.pts.push([a[0] + ((b[0] - a[0]) as f64 * t) as i32, a[1] + ((b[1] - a[1]) as f64 * t) as i32]);
            }
            if k + 1 < line.len() {
                g.pts.push(b);
            }
        }
        g.edges.push(Edge { a: rank(line[0]), b: rank(*line.last().unwrap()), len: len as f32, up: 0.0, down: 0.0, class: pieces[chain[0].0 as usize].2, geom });
    }
    g.pts_ele = vec![elevation::UNKNOWN; g.pts.len()];
    g.link();
    g.grid = Grid::new(&g);
    Ok(g)
}

/// Whether `p` is inside the ring (even-odd rule; the ring is closed).
fn inside(ring: &[[i32; 2]], p: [i32; 2]) -> bool {
    let (y, x) = (p[0] as f64, p[1] as f64);
    let mut odd = false;
    for w in ring.windows(2) {
        let ([y0, x0], [y1, x1]) = ([w[0][0] as f64, w[0][1] as f64], [w[1][0] as f64, w[1][1] as f64]);
        if (y0 > y) != (y1 > y) && x < x0 + (y - y0) / (y1 - y0) * (x1 - x0) {
            odd = !odd;
        }
    }
    odd
}

/// The industrial areas' rings, indexed by the cells (of `CELL`) their bounding box touches.
struct Industrial<'a> {
    rings: &'a [Vec<[i32; 2]>],
    boxes: Vec<[i32; 4]>,
    index: std::collections::HashMap<(i64, i64), Vec<u32>>,
}

impl<'a> Industrial<'a> {
    fn new(rings: &'a [Vec<[i32; 2]>]) -> Self {
        let cell = |v: i32| (v as i64).div_euclid(CELL);
        let mut index: std::collections::HashMap<(i64, i64), Vec<u32>> = Default::default();
        let mut boxes = Vec::with_capacity(rings.len());
        for (i, r) in rings.iter().enumerate() {
            let (la0, la1) = (r.iter().map(|p| p[0]).min().unwrap(), r.iter().map(|p| p[0]).max().unwrap());
            let (lo0, lo1) = (r.iter().map(|p| p[1]).min().unwrap(), r.iter().map(|p| p[1]).max().unwrap());
            boxes.push([la0, la1, lo0, lo1]);
            for y in cell(la0)..=cell(la1) {
                for x in cell(lo0)..=cell(lo1) {
                    index.entry((y, x)).or_default().push(i as u32);
                }
            }
        }
        Industrial { rings, boxes, index }
    }

    /// Whether the middle of the segment `a`-`b` is inside one of the rings.
    fn segment(&self, a: [i32; 2], b: [i32; 2]) -> bool {
        let mid = [((a[0] as i64 + b[0] as i64) / 2) as i32, ((a[1] as i64 + b[1] as i64) / 2) as i32];
        let cell = |v: i32| (v as i64).div_euclid(CELL);
        let Some(cands) = self.index.get(&(cell(mid[0]), cell(mid[1]))) else { return false };
        cands.iter().any(|&i| {
            let b = self.boxes[i as usize];
            mid[0] >= b[0] && mid[0] <= b[1] && mid[1] >= b[2] && mid[1] <= b[3] && inside(&self.rings[i as usize], mid)
        })
    }
}

impl Graph {
    fn count_components(&mut self) {
        let mut sizes = vec![0u32; self.comp.iter().max().map_or(0, |m| *m as usize + 1)];
        for c in &self.comp {
            sizes[*c as usize] += 1;
        }
        self.sizes = sizes;
    }

    /// Adjacency and components, from the edges.
    fn link(&mut self) {
        let n = self.nodes();
        let mut first = vec![0u32; n + 1];
        for e in &self.edges {
            first[e.a as usize + 1] += 1;
            first[e.b as usize + 1] += 1;
        }
        for i in 1..=n {
            first[i] += first[i - 1];
        }
        let mut fill = first.clone();
        let mut arcs = vec![0u32; self.edges.len() * 2];
        for (i, e) in self.edges.iter().enumerate() {
            arcs[fill[e.a as usize] as usize] = (i as u32) << 1;
            fill[e.a as usize] += 1;
            arcs[fill[e.b as usize] as usize] = (i as u32) << 1 | 1;
            fill[e.b as usize] += 1;
        }
        self.first = first;
        self.arcs = arcs;
        // Components, numbered in the order their first node comes.
        let mut comp = vec![u32::MAX; n];
        let mut next = 0;
        let mut stack = Vec::new();
        for s in 0..n {
            if comp[s] != u32::MAX {
                continue;
            }
            comp[s] = next;
            stack.push(s as u32);
            while let Some(v) = stack.pop() {
                for &arc in self.arcs_of(v) {
                    let (_, w) = self.head(arc);
                    if comp[w as usize] == u32::MAX {
                        comp[w as usize] = next;
                        stack.push(w);
                    }
                }
            }
            next += 1;
        }
        self.comp = comp;
        self.count_components();
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::logic::osm::tests::{W, zz};

    fn delta(v: &[i64]) -> Vec<u64> {
        let mut prev = 0;
        v.iter().map(|x| { let d = x - prev; prev = *x; zz(d) }).collect()
    }

    /// An extract: its blobs (type, bytes), from nodes (id, lat, lon) and ways (id, tags, refs).
    pub fn extract(nodes: &[(i64, f64, f64)], ways: &[(i64, Vec<(&str, &str)>, Vec<i64>)]) -> Vec<(String, Vec<u8>)> {
        let blob = |raw: &[u8]| {
            let mut b = W::default();
            b.bytes(1, raw);
            b.0
        };
        let mut header = W::default();
        header.bytes(4, b"OsmSchema-V0.6").bytes(4, b"DenseNodes").uint(32, 1_760_000_000);
        let mut out = vec![("OSMHeader".to_string(), blob(&header.0))];
        // Nodes in one block, ways in another, as Geofabrik sorts them.
        let mut dense = W::default();
        let ids: Vec<i64> = nodes.iter().map(|n| n.0).collect();
        let lats: Vec<i64> = nodes.iter().map(|n| (n.1 * 1e7).round() as i64).collect();
        let lons: Vec<i64> = nodes.iter().map(|n| (n.2 * 1e7).round() as i64).collect();
        dense.packed(1, delta(&ids)).packed(8, delta(&lats)).packed(9, delta(&lons));
        let mut group = W::default();
        group.bytes(2, &dense.0);
        let mut block = W::default();
        block.bytes(1, &W::default().bytes(1, b"").0).bytes(2, &group.0).uint(17, 100);
        out.push(("OSMData".into(), blob(&block.0)));
        let mut strings = vec![String::new()];
        let mut group = W::default();
        for (id, tags, refs) in ways {
            let mut index = |s: &str| match strings.iter().position(|x| x == s) {
                Some(i) => i as u64,
                None => {
                    strings.push(s.into());
                    (strings.len() - 1) as u64
                }
            };
            let keys: Vec<u64> = tags.iter().map(|(k, _)| index(k)).collect();
            let vals: Vec<u64> = tags.iter().map(|(_, v)| index(v)).collect();
            let mut w = W::default();
            w.uint(1, *id as u64).packed(2, keys).packed(3, vals).packed(8, delta(refs));
            group.bytes(3, &w.0);
        }
        let mut table = W::default();
        for s in &strings {
            table.bytes(1, s.as_bytes());
        }
        let mut block = W::default();
        block.bytes(1, &table.0).bytes(2, &group.0).uint(17, 100);
        out.push(("OSMData".into(), blob(&block.0)));
        out
    }

    pub fn build_from(blobs: &[(String, Vec<u8>)]) -> Result<Graph, String> {
        let mut read = |_: u32, f: &mut dyn FnMut(&str, Vec<u8>) -> Result<(), String>| {
            for (k, b) in blobs {
                f(k, b.clone())?;
            }
            Ok(())
        };
        build(&mut read, 1234, 3, &AtomicBool::new(false), &Progress::default())
    }

    /// A small place: a square of residential streets (1-2-3-4) with a path through the middle
    /// (2 → 5 → 4, a bend at 5), a dead end (3 → 6), a private track (1 → 3), an island (7-8),
    /// and a street drawn as two ways meeting at 9 (contracted away).
    pub fn place() -> Vec<(String, Vec<u8>)> {
        let d = 0.001; // ~111 m north, ~80 m east at 43.7°
        let nodes = [
            (1, 43.7, 7.25),
            (2, 43.7, 7.25 + 2.0 * d),
            (3, 43.7 + 2.0 * d, 7.25 + 2.0 * d),
            (4, 43.7 + 2.0 * d, 7.25),
            (5, 43.7 + d, 7.25 + d),
            (6, 43.7 + 3.0 * d, 7.25 + 2.0 * d),
            (7, 43.8, 7.3),
            (8, 43.8, 7.301),
            (9, 43.7 + d, 7.25),
        ];
        let street = || vec![("highway", "residential")];
        let ways = [
            (100, street(), vec![1, 2, 3]),
            (101, street(), vec![3, 4]),
            (102, street(), vec![4, 9]),
            (103, street(), vec![9, 1]),
            (104, vec![("highway", "path"), ("surface", "dirt")], vec![2, 5, 4]),
            (105, vec![("highway", "footway")], vec![3, 6]),
            (106, vec![("highway", "track"), ("access", "private")], vec![1, 3]),
            (107, vec![("highway", "path")], vec![7, 8]),
            (108, vec![("highway", "motorway")], vec![1, 7]),
        ];
        extract(&nodes, &ways)
    }

    #[test]
    fn edges_in_a_box_are_those_with_a_point_in_it() {
        let g = town();
        let (lat, lon) = (g.lat.iter().map(|v| *v as f64 / 1e7), g.lon.iter().map(|v| *v as f64 / 1e7));
        let all = [lat.clone().fold(90.0, f64::min), lon.clone().fold(180.0, f64::min), lat.fold(-90.0, f64::max), lon.fold(-180.0, f64::max)];
        assert_eq!(g.edges_in(all), (0..g.edges.len() as u32).collect::<Vec<_>>(), "the whole town: every edge, once");
        // A small box in the middle: every edge with a point inside is there.
        let mid = [(all[0] + all[2]) / 2.0, (all[1] + all[3]) / 2.0];
        let small = [mid[0] - 0.002, mid[1] - 0.002, mid[0] + 0.002, mid[1] + 0.002];
        let got = g.edges_in(small);
        let inside = |p: [i32; 2]| (small[0]..=small[2]).contains(&(p[0] as f64 / 1e7)) && (small[1]..=small[3]).contains(&(p[1] as f64 / 1e7));
        for e in 0..g.edges.len() as u32 {
            if g.line(e).iter().any(|(p, _)| inside(*p)) {
                assert!(got.contains(&e), "edge {e}");
            }
        }
        assert!(got.len() < g.edges.len());
        assert!(g.edges_in([10.0, 10.0, 10.1, 10.1]).is_empty(), "far away: none");
        assert!(g.edges_in([small[2], small[3], small[0], small[1]]).is_empty(), "an upside-down box: none");
    }

    #[test]
    fn builds_junctions_and_contracts_the_rest() {
        let g = build_from(&place()).unwrap();
        // Nodes: 2, 3, 4, 6, 7, 8. 5 and 9 are passed through, and so is 1: the private track
        // and the motorway are left out, so two streets meet there.
        assert_eq!(g.nodes(), 6);
        // Edges: 4-2 (through 9 and 1), 2-3, 3-4, 2-4 (through 5), 3-6, 7-8.
        assert_eq!(g.edges.len(), 6);
        assert_eq!(g.meta.source_time, 1_760_000_000);
        let find = |lat: f64, lon: f64| (0..g.nodes() as u32).find(|v| (g.position(*v)[0] - lat).abs() < 1e-6 && (g.position(*v)[1] - lon).abs() < 1e-6).unwrap();
        let (n2, n4) = (find(43.7, 7.252), find(43.702, 7.25));
        let street = |e: &&Edge| !e.class.is_trail() && ((e.a == n2 && e.b == n4) || (e.a == n4 && e.b == n2));
        let e = g.edges.iter().position(|e| street(&e)).unwrap();
        // 111 + 111 + 161 m.
        assert!((g.edges[e].len - 383.3).abs() < 1.0, "{}", g.edges[e].len);
        // The corners at 9 and 1 stay in the line, with points every 60 m or less between them.
        let line = g.line(e as u32);
        assert!(line.len() == 8 && line.iter().any(|(p, _)| *p == [437_010_000, 72_500_000]) && line.iter().any(|(p, _)| *p == [437_000_000, 72_500_000]), "{line:?}");
        // The path through 5 is a trail; streets and the footway are not.
        assert_eq!(g.edges.iter().filter(|e| e.class.is_trail()).count(), 2);
        // Two components: the place, the island.
        let island = find(43.8, 7.3);
        assert_ne!(g.comp[island as usize], g.comp[n2 as usize]);
        assert_eq!(g.comp.iter().filter(|c| **c == g.comp[n2 as usize]).count(), 4);
        // Every arc leads back.
        for v in 0..g.nodes() as u32 {
            for &a in g.arcs_of(v) {
                let (e, w) = g.head(a);
                assert!(g.arcs_of(w).contains(&(e << 1 | (1 - (a & 1)))));
            }
        }
        // The same extract gives the same graph, whatever the threads did.
        assert_eq!(build_from(&place()).unwrap().encode(), g.encode());
    }

    #[test]
    fn marks_edges_inside_industrial_areas() {
        let d = 0.001;
        let (la, lo) = (43.7, 7.25);
        let nodes = [
            (1, la, lo),
            (2, la, lo + 2.0 * d),
            (3, la + 2.0 * d, lo + 2.0 * d),
            (4, la + 2.0 * d, lo),
            (6, la + 3.0 * d, lo + 2.0 * d),
            // A ring around the north: latitudes 43.7015 to 43.7035.
            (20, la + 1.5 * d, lo - d),
            (21, la + 3.5 * d, lo - d),
            (22, la + 3.5 * d, lo + 3.0 * d),
            (23, la + 1.5 * d, lo + 3.0 * d),
        ];
        let street = || vec![("highway", "residential")];
        let ring = |tag| (300, vec![("landuse", tag)], vec![20, 21, 22, 23, 20]);
        let ways = [
            (100, street(), vec![1, 2]),
            (101, street(), vec![2, 3]),
            (102, vec![("highway", "residential"), ("lit", "no")], vec![3, 4]),
            (103, street(), vec![4, 1]),
            (104, street(), vec![3, 6]),
            ring("industrial"),
        ];
        let g = build_from(&extract(&nodes, &ways)).unwrap();
        // 3-4 (unlit), 3-6, and the rest of the square contracted into one edge from 3 to 4.
        assert_eq!(g.edges.len(), 3);
        // The 606 m rest of the square lies below the area (its segments are outside, though both
        // its ends are on the border's inside): not marked. The two short edges are inside.
        for e in &g.edges {
            assert_eq!(e.class.industrial(), e.len < 500.0, "{e:?}");
        }
        assert!(g.edges.iter().any(|e| e.class.lit() == 2 && e.class.industrial()));
        assert_eq!(g.edges.iter().filter(|e| e.class.industrial()).count(), 2);
        // Other landuse: nothing marked.
        let mut forest = ways.clone();
        forest[5] = ring("forest");
        assert!(build_from(&extract(&nodes, &forest)).unwrap().edges.iter().all(|e| !e.class.industrial()));
        // The bits survive the file.
        let back = Graph::decode(&g.encode()).unwrap();
        assert_eq!(back.edges.iter().map(|e| e.class).collect::<Vec<_>>(), g.edges.iter().map(|e| e.class).collect::<Vec<_>>());
    }

    /// One street of `n + 1` nodes, 81 m apart along a parallel, and a ring over the nodes
    /// `from..=to` (in steps, widened by a quarter step); `open`: its last node is left out.
    fn street_and_ring(n: i64, from: f64, to: f64, open: bool) -> Graph {
        let d = 0.001;
        let mut nodes: Vec<(i64, f64, f64)> = (0..=n).map(|k| (k + 1, 43.7, 7.25 + k as f64 * d)).collect();
        let (x0, x1) = (7.25 + (from - 0.25) * d, 7.25 + (to + 0.25) * d);
        nodes.extend([(20, 43.699, x0), (21, 43.701, x0), (22, 43.701, x1), (23, 43.699, x1)]);
        let refs = if open { vec![20, 21, 22, 23] } else { vec![20, 21, 22, 23, 20] };
        let ways = [
            (100, vec![("highway", "residential")], (1..=n + 1).collect::<Vec<_>>()),
            (300, vec![("landuse", "industrial")], refs),
        ];
        build_from(&extract(&nodes, &ways)).unwrap()
    }

    #[test]
    fn cuts_edges_at_the_border_of_an_industrial_area() {
        // Eight segments (650 m) wholly inside: one edge, marked even though it is long.
        let g = street_and_ring(8, 0.0, 8.0, false);
        assert_eq!((g.edges.len(), g.edges[0].class.industrial()), (1, true));
        assert!(g.edges[0].len > 600.0, "{}", g.edges[0].len);
        // Nodes 3 to 6 inside: the street is cut where it enters and where it leaves.
        let g = street_and_ring(9, 3.0, 6.0, false);
        let mut e: Vec<_> = g.edges.iter().map(|e| (e.len.round(), e.class.industrial())).collect();
        e.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(e.iter().filter(|e| e.1).count(), 1, "{e:?}");
        assert_eq!(e.iter().map(|e| e.0).sum::<f32>().round(), 723.0, "{e:?}");
        let inside = g.edges.iter().find(|e| e.class.industrial()).unwrap();
        assert!((inside.len - 241.0).abs() < 2.0, "{}", inside.len);
        assert_eq!(g.edges.len(), 3);
        // A ring left open is no area.
        assert!(street_and_ring(8, 0.0, 8.0, true).edges.iter().all(|e| !e.class.industrial()));
    }

    #[test]
    fn snaps_onto_the_nearest_edge() {
        let g = build_from(&place()).unwrap();
        // Between 1 and 2, 10 m north of the street.
        let s = g.snap(43.7 + 0.00009, 7.2510, 500.0, |_| true).unwrap();
        let e = g.edges[s.edge as usize];
        assert!((s.dist - 10.0).abs() < 1.0, "{s:?}");
        assert!(!e.class.is_trail());
        assert!(g.snap(44.5, 8.0, 500.0, |_| true).is_none());
        // Only the island's edges: farther, but found.
        let island = g.comp[g.edges.iter().find(|e| e.class.highway() == osm::Highway::Path && e.len < 90.0).unwrap().a as usize];
        let s = g.snap(43.7, 7.25, 20_000.0, |e| g.comp[g.edges[e as usize].a as usize] == island).unwrap();
        assert!(s.dist > 10_000.0);
    }

    #[test]
    fn stores_and_reads_back_or_says_why_not() {
        let mut g = build_from(&place()).unwrap();
        // Altitude: rises 1 m per 10 m eastward... any tile does; a flat 100 m one here.
        let flat = crate::logic::elevation::decode(&crate::logic::elevation::tests::terrarium(|_, _| 100.0)).unwrap();
        g.set_elevation(&mut Dem::new(8, move |_, _| Some(flat.clone())));
        assert_eq!(g.meta.missing, 0);
        assert!(g.ele.iter().all(|e| elevation::unpack(*e) == Some(100.0)));
        assert!(g.edges.iter().all(|e| e.up < 1e-3 && e.down < 1e-3));
        let bytes = g.encode();
        let back = Graph::decode(&bytes).unwrap();
        assert_eq!(back.encode(), bytes);
        assert_eq!(back.meta, g.meta);
        // Another version: built again, not called damaged.
        let mut old = bytes.clone();
        old[8..12].copy_from_slice(&(VERSION - 1).to_le_bytes());
        assert_eq!(Graph::decode(&old).err(), Some(Unreadable::Stale(VERSION - 1)));
        // Truncated, altered, or not a graph at all.
        for bad in [&bytes[..bytes.len() - 3], &bytes[..40], b"not a graph at all"] {
            assert!(matches!(Graph::decode(bad), Err(Unreadable::Damaged(_))));
        }
        let mut flipped = bytes.clone();
        flipped[100] ^= 1;
        assert!(matches!(Graph::decode(&flipped), Err(Unreadable::Damaged(_))));
    }

    #[test]
    fn climb_is_read_along_each_edge() {
        let mut g = build_from(&place()).unwrap();
        // Altitude rises 1 m per pixel eastward (~28 m): edges going east climb, going west descend.
        let slope = crate::logic::elevation::decode(&crate::logic::elevation::tests::terrarium(|x, _| x as f32)).unwrap();
        g.set_elevation(&mut Dem::new(8, move |_, _| Some(slope.clone())));
        // 3-4 runs 160 m east-west: about 5.8 m one way, nothing the other.
        let e = g.edges.iter().find(|e| e.class.highway() == osm::Highway::Residential && g.lat[e.a as usize] == 437_020_000 && g.lat[e.b as usize] == 437_020_000).unwrap();
        let (east, west) = if g.lon[e.b as usize] > g.lon[e.a as usize] { (e.up, e.down) } else { (e.down, e.up) };
        assert!((east - 5.8).abs() < 1.0 && west == 0.0, "{e:?}");
        let mut none = build_from(&place()).unwrap();
        none.set_elevation(&mut Dem::new(8, |_, _| None));
        assert!(none.meta.missing > 0);
    }

    /// The blobs of a file in `tests/fixtures`, split as `platform::osm::read_extract` does.
    pub fn fixture(name: &str) -> Vec<(String, Vec<u8>)> {
        let b = std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let len = u32::from_be_bytes(b[i..i + 4].try_into().unwrap()) as usize;
            let (kind, size) = osm::blob_header(&b[i + 4..i + 4 + len]).unwrap();
            out.push((kind, b[i + 4 + len..i + 4 + len + size].to_vec()));
            i += 4 + len + size;
        }
        out
    }

    /// The made-up town of `scripts/osm-fixture.py`: a 3 km grid every 250 m, a diagonal path,
    /// a dead end with steps, an island; what is closed to walkers left out.
    pub fn town() -> Graph {
        build_from(&fixture("loops.osm.pbf")).unwrap()
    }

    #[test]
    fn builds_the_fixture_town() {
        let g = town();
        // 13 × 13 crossings, the dead end's two nodes, the island's two.
        assert_eq!(g.nodes(), 173);
        // 2 × 13 × 12 stretches of the grid, 6 of the diagonal, the dead end, the steps, the island.
        assert_eq!(g.edges.len(), 321);
        let total: f32 = g.edges.iter().map(|e| e.len).sum();
        let expected = 2.0 * 13.0 * 12.0 * 250.0 + 6.0 * 353.55 + 300.0 + 100.0 + 500.0;
        assert!((total - expected).abs() < 40.0, "{total} {expected}");
        assert_eq!(g.comp.iter().max(), Some(&1));
        assert_eq!(g.edges.iter().filter(|e| e.class.highway() == osm::Highway::Steps).count(), 1);
        // Every stretch of the grid is 250 m, give or take the earth's curve.
        assert!(g.edges.iter().filter(|e| e.class.highway() == osm::Highway::Residential).all(|e| (e.len - 250.0).abs() < 0.5));
    }

    #[test]
    fn gallop_finds_ids_in_any_order() {
        let ids: Vec<i64> = (0..1000).map(|i| i * 3).collect();
        let mut cursor = 0;
        for id in (0..3000).chain((0..3000).rev()).chain([5, 2997, 0, 1500]) {
            assert_eq!(gallop(&ids, &mut cursor, id), (id % 3 == 0).then_some((id / 3) as usize), "{id}");
        }
        assert_eq!(gallop(&ids, &mut cursor, 99_999), None);
        assert_eq!(gallop(&[], &mut 0, 1), None);
    }

    #[test]
    fn a_damaged_extract_is_an_error() {
        let mut blobs = place();
        blobs[2].1.truncate(10);
        assert!(build_from(&blobs).is_err());
        let cancel = AtomicBool::new(true);
        let mut read = |_: u32, f: &mut dyn FnMut(&str, Vec<u8>) -> Result<(), String>| {
            for (k, b) in place() {
                f(&k, b)?;
            }
            Ok(())
        };
        assert_eq!(build(&mut read, 0, 2, &cancel, &Progress::default()).err().as_deref(), Some(CANCELLED));
    }
}
