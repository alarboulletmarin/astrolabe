//! Routing on the walking graph: the part of it around a start (a local graph, in metres, with
//! the start inserted where it falls on its edge), what each way costs given what one prefers,
//! and A* between two of its nodes.

use crate::logic::elevation;
use crate::logic::fit::metres;
use crate::logic::graph::Graph;
use crate::logic::osm::{Class, Highway};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// What one prefers to run on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prefs {
    /// 0: streets rather than trails, 1: trails rather than streets, 0.5: either.
    pub trail: f32,
    pub avoid_paved: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { trail: 0.5, avoid_paved: false }
    }
}

/// How much a metre of this way costs, at least 1 (so the straight line is a lower bound); `None`
/// for a way not to run on (alpine scrambles).
pub fn factor(c: Class, p: &Prefs) -> Option<f32> {
    let mut f = match c.highway() {
        Highway::Path | Highway::Footway | Highway::Bridleway | Highway::Pedestrian | Highway::LivingStreet | Highway::Track => 1.0,
        Highway::Steps => 1.6,
        Highway::Residential | Highway::Unclassified | Highway::Service => 1.15,
        Highway::Tertiary | Highway::Cycleway => 1.4,
        Highway::Secondary => 1.8,
        Highway::Primary => 2.5,
    };
    f *= match c.sac_scale() {
        0..=2 => 1.0,
        3 => 1.5,
        4 => 3.0,
        _ => return None,
    };
    f *= match c.visibility() {
        4 => 1.5,
        5 | 6 => 3.0,
        _ => 1.0,
    };
    let lean = (p.trail.clamp(0.0, 1.0) - 0.5) * 2.0;
    f *= if c.is_trail() { 1.0 + (-lean).max(0.0) } else { 1.0 + lean.max(0.0) };
    if p.avoid_paved && c.is_paved() {
        f *= 2.5;
    }
    Some(f)
}

/// A stretch of the local graph: a whole edge of the graph, or the part of one between
/// `from` and `to` (fractions of its length, from its `a` end) when the start cuts it.
#[derive(Clone, Copy, Debug)]
pub struct Stretch {
    pub a: u32,
    pub b: u32,
    pub len: f32,
    pub up: f32,
    pub down: f32,
    pub class: Class,
    pub edge: u32,
    pub from: f32,
    pub to: f32,
    /// `len` times `factor`.
    pub cost: f32,
}

/// The graph within reach of a start: nodes in metres east (`x`) and north (`y`) of it, the
/// start being node 0.
pub struct Local<'g> {
    pub graph: &'g Graph,
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    pub first: Vec<u32>,
    pub arcs: Vec<u32>,
    pub stretches: Vec<Stretch>,
    grid: NodeGrid,
    /// Where the start fell (lat, lon), and metres per degree east and north there.
    pub start: [f64; 2],
    k: (f64, f64),
}

/// Where the start was put, and how far it was from what was asked.
#[derive(Debug, PartialEq)]
pub enum NoStart {
    /// No path near enough to start from.
    TooFar,
}

const START_REACH: f64 = 500.0;
/// A component of at least this many nodes is a network to start from.
const NETWORK: u32 = 200;

impl<'g> Local<'g> {
    /// The part of `g` reachable from the path nearest to (lat, lon) without going farther than
    /// `radius` metres from it, on ways `prefs` allows.
    pub fn around(g: &'g Graph, lat: f64, lon: f64, radius: f64, prefs: &Prefs) -> Result<Local<'g>, NoStart> {
        let usable = |e: u32| factor(g.edges[e as usize].class, prefs).is_some();
        // A path cut off from the rest (a yard's footway, streets boxed in by main roads, which
        // aren't kept) holds no loop: the start goes to the nearest part of a real network, if
        // there is one within reach.
        let joined = |e: u32| usable(e) && g.component_size(g.edges[e as usize].a) >= NETWORK;
        let snap = g.snap(lat, lon, START_REACH, joined).or_else(|| g.snap(lat, lon, START_REACH, usable)).ok_or(NoStart::TooFar)?;
        let line = g.line(snap.edge);
        let pts: Vec<[f64; 2]> = line.iter().map(|(p, _)| [p[0] as f64 * 1e-7, p[1] as f64 * 1e-7]).collect();
        let mut along = 0.0;
        for w in pts.windows(2).take(snap.seg as usize) {
            along += metres(w[0], w[1]);
        }
        let (s0, s1) = (pts[snap.seg as usize], pts[snap.seg as usize + 1]);
        along += metres(s0, s1) * snap.t as f64;
        let start = [s0[0] + (s1[0] - s0[0]) * snap.t as f64, s0[1] + (s1[1] - s0[1]) * snap.t as f64];
        let total: f64 = pts.windows(2).map(|w| metres(w[0], w[1])).sum();
        let frac = if total > 0.0 { (along / total) as f32 } else { 0.0 };
        let e = g.edges[snap.edge as usize];
        // The start is a node of its own, unless it falls on one of the edge's ends.
        let at_end = if frac <= 0.0 { Some(e.a) } else if frac >= 1.0 { Some(e.b) } else { None };

        let kx = 6_371_000.0 * std::f64::consts::PI / 180.0 * start[0].to_radians().cos();
        let ky = 6_371_000.0 * std::f64::consts::PI / 180.0;
        let xy = |p: [f64; 2]| (((p[1] - start[1]) * kx) as f32, ((p[0] - start[0]) * ky) as f32);
        let r2 = (radius * radius) as f32;

        // The nodes found so far, and those whose edges are still to look at.
        struct Grow {
            local_of: std::collections::HashMap<u32, u32>,
            x: Vec<f32>,
            y: Vec<f32>,
            queue: std::collections::VecDeque<u32>,
        }
        impl Grow {
            /// The local node of `v`, made if it is within reach.
            fn add(&mut self, v: u32, (px, py): (f32, f32), r2: f32) -> Option<u32> {
                if let Some(l) = self.local_of.get(&v) {
                    return Some(*l);
                }
                if px * px + py * py > r2 {
                    return None;
                }
                let l = self.x.len() as u32;
                self.local_of.insert(v, l);
                self.x.push(px);
                self.y.push(py);
                self.queue.push_back(v);
                Some(l)
            }
        }
        let mut grow = Grow { local_of: Default::default(), x: vec![0.0], y: vec![0.0], queue: Default::default() };
        let mut stretches = Vec::new();
        match at_end {
            Some(v) => {
                grow.local_of.insert(v, 0);
                grow.queue.push_back(v);
            }
            None => {
                let cost = factor(e.class, prefs).unwrap();
                for (end, from, to) in [(e.a, 0.0, frac), (e.b, frac, 1.0)] {
                    if let Some(l) = grow.add(end, xy(g.position(end)), r2) {
                        let part = to - from;
                        let (a, b) = if end == e.a { (l, 0) } else { (0, l) };
                        let len = e.len * part;
                        stretches.push(Stretch { a, b, len, up: e.up * part, down: e.down * part, class: e.class, edge: snap.edge, from, to, cost: len * cost });
                    }
                }
            }
        }
        // Breadth first from the start, every edge between two nodes kept once. The edge the start
        // cuts is there as its two parts only: whole, a loop could run it out and back unseen.
        let mut seen_edge = std::collections::HashSet::new();
        if at_end.is_none() {
            seen_edge.insert(snap.edge);
        }
        while let Some(v) = grow.queue.pop_front() {
            let lv = grow.local_of[&v];
            for &arc in g.arcs_of(v) {
                let (ei, w) = g.head(arc);
                if !usable(ei) || !seen_edge.insert(ei) {
                    continue;
                }
                let Some(lw) = grow.add(w, xy(g.position(w)), r2) else { continue };
                let ed = g.edges[ei as usize];
                let (a, b) = if arc & 1 == 0 { (lv, lw) } else { (lw, lv) };
                let cost = ed.len * factor(ed.class, prefs).unwrap();
                stretches.push(Stretch { a, b, len: ed.len, up: ed.up, down: ed.down, class: ed.class, edge: ei, from: 0.0, to: 1.0, cost });
            }
        }
        let Grow { x, y, .. } = grow;
        let n = x.len();
        let mut first = vec![0u32; n + 1];
        for s in &stretches {
            first[s.a as usize + 1] += 1;
            first[s.b as usize + 1] += 1;
        }
        for i in 1..=n {
            first[i] += first[i - 1];
        }
        let mut fill = first.clone();
        let mut arcs = vec![0u32; stretches.len() * 2];
        for (i, s) in stretches.iter().enumerate() {
            arcs[fill[s.a as usize] as usize] = (i as u32) << 1;
            fill[s.a as usize] += 1;
            arcs[fill[s.b as usize] as usize] = (i as u32) << 1 | 1;
            fill[s.b as usize] += 1;
        }
        let grid = NodeGrid::new(&x, &y);
        Ok(Local { graph: g, x, y, first, arcs, stretches, grid, start, k: (kx, ky) })
    }

    pub fn len(&self) -> usize {
        self.x.len()
    }

    pub fn arcs_of(&self, v: u32) -> &[u32] {
        &self.arcs[self.first[v as usize] as usize..self.first[v as usize + 1] as usize]
    }

    /// The stretch of an arc and the node it leads to.
    pub fn head(&self, arc: u32) -> (u32, u32) {
        let s = &self.stretches[(arc >> 1) as usize];
        (arc >> 1, if arc & 1 == 0 { s.b } else { s.a })
    }

    /// A position in metres east and north of the start.
    pub fn xy(&self, lat: f64, lon: f64) -> (f32, f32) {
        (((lon - self.start[1]) * self.k.0) as f32, ((lat - self.start[0]) * self.k.1) as f32)
    }

    /// The position of a local node, (lat, lon).
    pub fn position(&self, v: u32) -> [f64; 2] {
        [self.start[0] + self.y[v as usize] as f64 / self.k.1, self.start[1] + self.x[v as usize] as f64 / self.k.0]
    }

    /// The local node nearest to (x, y) metres, other than the start.
    pub fn nearest(&self, x: f32, y: f32) -> Option<u32> {
        self.grid.nearest(&self.x, &self.y, x, y)
    }

    /// An arc's line, from where it leaves to where it arrives: (lat, lon) and altitude.
    pub fn line(&self, arc: u32) -> Vec<([f64; 2], Option<f32>)> {
        let s = &self.stretches[(arc >> 1) as usize];
        let full: Vec<([f64; 2], Option<f32>)> =
            self.graph.line(s.edge).into_iter().map(|(p, e)| ([p[0] as f64 * 1e-7, p[1] as f64 * 1e-7], elevation::unpack(e))).collect();
        let mut part = if s.from <= 0.0 && s.to >= 1.0 { full } else { cut(&full, s.from as f64, s.to as f64) };
        // A stretch keeps its edge's direction (its `a` is the end at `from`): an arc taken the
        // other way reads it backwards.
        if arc & 1 == 1 {
            part.reverse();
        }
        part
    }
}

/// The part of a line between two fractions of its length.
fn cut(line: &[([f64; 2], Option<f32>)], from: f64, to: f64) -> Vec<([f64; 2], Option<f32>)> {
    let lens: Vec<f64> = line.windows(2).map(|w| metres(w[0].0, w[1].0)).collect();
    let total: f64 = lens.iter().sum();
    let (f, t) = (from * total, to * total);
    let lerp = |i: usize, at: f64, acc: f64| {
        let k = if lens[i] > 0.0 { (at - acc) / lens[i] } else { 0.0 };
        let (a, b) = (line[i], line[i + 1]);
        let ele = match (a.1, b.1) {
            (Some(x), Some(y)) => Some(x + (y - x) * k as f32),
            (x, y) => x.or(y),
        };
        ([a.0[0] + (b.0[0] - a.0[0]) * k, a.0[1] + (b.0[1] - a.0[1]) * k], ele)
    };
    let mut out = Vec::new();
    let mut acc = 0.0;
    for i in 0..lens.len() {
        let (s, e) = (acc, acc + lens[i]);
        if out.is_empty() && f <= e {
            out.push(lerp(i, f.max(s), s));
        }
        if !out.is_empty() {
            if t <= e {
                out.push(lerp(i, t, s));
                return out;
            }
            out.push(line[i + 1]);
        }
        acc = e;
    }
    if out.is_empty() {
        out.push(*line.last().unwrap());
    }
    out
}

/// Nodes by cell of 250 m, to find the one nearest to a point.
struct NodeGrid {
    x0: f32,
    y0: f32,
    cols: usize,
    rows: usize,
    start: Vec<u32>,
    items: Vec<u32>,
}

const NODE_CELL: f32 = 250.0;

impl NodeGrid {
    fn new(x: &[f32], y: &[f32]) -> NodeGrid {
        let fold = |v: &[f32], f: fn(f32, f32) -> f32, init: f32| v.iter().copied().fold(init, f);
        let (x0, y0) = (fold(x, f32::min, 0.0), fold(y, f32::min, 0.0));
        let (x1, y1) = (fold(x, f32::max, 0.0), fold(y, f32::max, 0.0));
        let cols = ((x1 - x0) / NODE_CELL) as usize + 1;
        let rows = ((y1 - y0) / NODE_CELL) as usize + 1;
        let cell = |i: usize| ((y[i] - y0) / NODE_CELL) as usize * cols + ((x[i] - x0) / NODE_CELL) as usize;
        let mut start = vec![0u32; cols * rows + 1];
        for i in 1..x.len() {
            start[cell(i) + 1] += 1;
        }
        for i in 1..start.len() {
            start[i] += start[i - 1];
        }
        let mut fill = start.clone();
        let mut items = vec![0u32; x.len().saturating_sub(1)];
        for i in 1..x.len() {
            let c = cell(i);
            items[fill[c] as usize] = i as u32;
            fill[c] += 1;
        }
        NodeGrid { x0, y0, cols, rows, start, items }
    }

    fn nearest(&self, xs: &[f32], ys: &[f32], x: f32, y: f32) -> Option<u32> {
        if self.items.is_empty() {
            return None;
        }
        let col = ((x - self.x0) / NODE_CELL).floor() as i64;
        let row = ((y - self.y0) / NODE_CELL).floor() as i64;
        let mut best: Option<(f32, u32)> = None;
        let span = self.cols.max(self.rows) as i64 + 1;
        for r in 0..=span {
            if best.is_some_and(|(d, _)| d.sqrt() < (r - 1).max(0) as f32 * NODE_CELL) {
                break;
            }
            for dr in -r..=r {
                for dc in -r..=r {
                    if dr.abs() != r && dc.abs() != r {
                        continue;
                    }
                    let (rr, cc) = (row + dr, col + dc);
                    if rr < 0 || cc < 0 || rr >= self.rows as i64 || cc >= self.cols as i64 {
                        continue;
                    }
                    let c = rr as usize * self.cols + cc as usize;
                    for &i in &self.items[self.start[c] as usize..self.start[c + 1] as usize] {
                        let d = (xs[i as usize] - x).powi(2) + (ys[i as usize] - y).powi(2);
                        if best.is_none_or(|(b, j)| d < b || (d == b && i < j)) {
                            best = Some((d, i));
                        }
                    }
                }
            }
        }
        best.map(|b| b.1)
    }
}

/// A* between local nodes, its arrays kept from one search to the next.
pub struct Search {
    cost: Vec<f32>,
    prev: Vec<u32>,
    stamp: Vec<u32>,
    round: u32,
    heap: BinaryHeap<Reverse<(u32, u32)>>,
}

impl Search {
    pub fn new(n: usize) -> Self {
        Search { cost: vec![0.0; n], prev: vec![0; n], stamp: vec![0; n], round: 0, heap: BinaryHeap::new() }
    }

    /// The cheapest arcs from `from` to `to`, a stretch costing `price(stretch)`, which must be
    /// at least its length (the straight line then never overestimates).
    pub fn route(&mut self, l: &Local, from: u32, to: u32, price: impl Fn(u32) -> f32) -> Option<Vec<u32>> {
        if from == to {
            return Some(Vec::new());
        }
        self.round += 1;
        self.heap.clear();
        let (tx, ty) = (l.x[to as usize], l.y[to as usize]);
        let h = |v: u32| ((l.x[v as usize] - tx).powi(2) + (l.y[v as usize] - ty).powi(2)).sqrt();
        self.stamp[from as usize] = self.round;
        self.cost[from as usize] = 0.0;
        self.prev[from as usize] = u32::MAX;
        // Non-negative floats order like their bits.
        self.heap.push(Reverse((h(from).to_bits(), from)));
        while let Some(Reverse((f, v))) = self.heap.pop() {
            let g = self.cost[v as usize];
            if f32::from_bits(f) > g + h(v) + 1e-3 {
                continue; // a stale entry
            }
            if v == to {
                let mut arcs = Vec::new();
                let mut at = to;
                while self.prev[at as usize] != u32::MAX {
                    let arc = self.prev[at as usize];
                    arcs.push(arc);
                    let s = &l.stretches[(arc >> 1) as usize];
                    at = if arc & 1 == 0 { s.a } else { s.b };
                }
                arcs.reverse();
                return Some(arcs);
            }
            for &arc in l.arcs_of(v) {
                let (s, w) = l.head(arc);
                let c = g + price(s);
                if self.stamp[w as usize] != self.round || c < self.cost[w as usize] {
                    self.stamp[w as usize] = self.round;
                    self.cost[w as usize] = c;
                    self.prev[w as usize] = arc;
                    self.heap.push(Reverse(((c + h(w)).to_bits(), w)));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::graph::tests::town;

    #[test]
    fn costs_follow_preferences() {
        let path = Class::new(Highway::Path, crate::logic::osm::Surface::Natural, 1, 0, 0);
        let street = Class::new(Highway::Residential, crate::logic::osm::Surface::Unknown, 0, 0, 0);
        let alpine = Class::new(Highway::Path, crate::logic::osm::Surface::Natural, 5, 0, 0);
        let neutral = Prefs::default();
        assert_eq!(factor(path, &neutral), Some(1.0));
        assert_eq!(factor(alpine, &neutral), None);
        let trails = Prefs { trail: 1.0, avoid_paved: true };
        assert!(factor(street, &trails).unwrap() > 4.0 * factor(path, &trails).unwrap());
        let streets = Prefs { trail: 0.0, avoid_paved: false };
        assert!(factor(path, &streets).unwrap() > factor(street, &streets).unwrap());
        for c in [path, street] {
            for p in [neutral, trails, streets] {
                assert!(factor(c, &p).unwrap() >= 1.0);
            }
        }
    }

    #[test]
    fn the_start_cuts_its_edge_and_a_star_finds_the_shortest_way() {
        let g = town();
        // 100 m east of the south-west corner, 5 m north of the street: on the first stretch.
        let (lat, lon) = (43.70 + 5.0 / 111_195.0, 7.25 + 100.0 / (111_195.0 * 0.7230));
        let l = Local::around(&g, lat, lon, 5_000.0, &Prefs::default()).unwrap();
        // The island is out of reach; everything else is in.
        assert_eq!(l.len(), 1 + 171);
        let parts: Vec<&Stretch> = l.stretches.iter().filter(|s| s.a == 0 || s.b == 0).collect();
        assert_eq!(parts.len(), 2);
        assert!((parts[0].len + parts[1].len - 250.0).abs() < 0.5);
        assert!(parts.iter().any(|s| (s.len - 100.0).abs() < 1.0));
        // To the north-east corner (3 km, 3 km): 6 km on the grid minus the 100 m already done,
        // or less by the diagonal path.
        let corner = l.nearest(2999.0 - 100.0, 2999.0).unwrap();
        let mut search = Search::new(l.len());
        let arcs = search.route(&l, 0, corner, |s| l.stretches[s as usize].len).unwrap();
        let d: f32 = arcs.iter().map(|a| l.stretches[(a >> 1) as usize].len).sum();
        assert!((d - (1500.0 * 2.0 - 100.0 + 2121.3)).abs() < 3.0, "{d}");
        // The line of the route joins up: each arc starts where the last ended.
        let mut prev: Option<[f64; 2]> = None;
        for a in &arcs {
            let line = l.line(*a);
            if let Some(p) = prev {
                assert!(metres(p, line[0].0) < 0.5);
            }
            prev = Some(line.last().unwrap().0);
        }
        // The first arc starts at the start.
        assert!(metres(l.line(arcs[0])[0].0, [43.70, lon]) < 0.5);
        assert!(Local::around(&g, 44.5, 7.0, 5_000.0, &Prefs::default()).is_err());
    }
}
