//! Loops of a given length from a start: waypoints on a polygon around it, joined by A*, its size
//! adjusted until the length fits; several of them, as different as possible, best first. The
//! same request and seed on the same graph give the same loops.

use crate::logic::graph::Graph;
use crate::logic::route::{Local, NoStart, Prefs, Search};
use std::f64::consts::PI;

/// How much longer the paths are than the polygon of waypoints, on average: the first guess of
/// its size, before it is measured and adjusted.
// ponytail: 1.3 is a common figure for walking networks; the benchmark around Nice
// (`measure_loops_around_nice`) is what tells whether another start converges faster.
pub const DETOUR: f64 = 1.3;
/// Exact mode: how close the length must come.
pub const TOLERANCE: f64 = 0.02;
/// Times the polygon is resized before keeping the best try.
const RESIZES: usize = 7;
/// Moves of single waypoints to a nearby node, in exact mode, after the resizing; and the nodes
/// tried at each.
const NUDGES: usize = 12;
const NUDGE_OPTIONS: usize = 6;
/// A stretch already taken costs this many times more for each time it was taken.
const REUSE: f32 = 4.0;
/// Two loops sharing more than this share of the shorter one: only the better is kept.
pub const OVERLAP: f32 = 0.3;
/// A loop that runs more than this share of its length twice is not proposed.
pub const MAX_BACK: f32 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// This many metres, within `TOLERANCE`.
    Exact(f64),
    /// Between these, in metres.
    Range(f64, f64),
}

impl Target {
    /// The length aimed at.
    pub fn aim(self) -> f64 {
        match self {
            Target::Exact(l) => l,
            Target::Range(a, b) => (a + b) / 2.0,
        }
    }

    pub fn longest(self) -> f64 {
        match self {
            Target::Exact(l) => l * (1.0 + TOLERANCE),
            Target::Range(a, b) => a.max(b),
        }
    }

    /// How far a length falls outside what was asked, as a share of the aim (0 inside).
    pub fn miss(self, d: f64) -> f64 {
        match self {
            Target::Exact(l) => (((d - l).abs() / l) - TOLERANCE).max(0.0) + (d - l).abs() / l * 0.1,
            Target::Range(a, b) => (if d < a.min(b) { a.min(b) - d } else if d > a.max(b) { d - a.max(b) } else { 0.0 }) / self.aim(),
        }
    }

    pub fn holds(self, d: f64) -> bool {
        match self {
            Target::Exact(l) => (d - l).abs() <= l * TOLERANCE,
            Target::Range(a, b) => (a.min(b)..=a.max(b)).contains(&d),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request {
    pub lat: f64,
    pub lon: f64,
    pub target: Target,
    pub prefs: Prefs,
    /// Climb wished for, metres: favours the loops nearest to it.
    pub climb: Option<f64>,
    pub seed: u64,
    /// How many loops to propose.
    pub count: usize,
}

/// A proposed loop.
#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    /// (lat, lon), from the start back to it.
    pub line: Vec<[f64; 2]>,
    /// Altitude at each point of the line, metres (`None` where no tile was read).
    pub eles: Vec<Option<f32>>,
    /// Metres from the start at each point of the line.
    pub along: Vec<f32>,
    pub dist: f64,
    pub up: f64,
    pub down: f64,
    /// Shares of the length on trails, on asphalt, and run twice.
    pub trail: f32,
    pub paved: f32,
    pub back: f32,
    /// Asking with this seed gives this loop first.
    pub seed: u64,
    /// Lower is better.
    pub score: f64,
    /// The graph's edges it uses, with the length used on each, for comparing loops.
    edges: Vec<(u32, f32)>,
}

#[derive(Debug, PartialEq)]
pub enum NoLoop {
    /// No path near the start.
    NoPath,
    /// Paths, but none that make a loop of that length.
    NoLoop,
}

/// SplitMix64: small, and the same everywhere.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    /// In [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The seed of the `i`th attempt of a request: the first is the request's own, so a loop's seed
/// asked again gives that loop first.
fn attempt_seed(seed: u64, i: usize) -> u64 {
    if i == 0 { seed } else { Rng::new(seed ^ (i as u64).wrapping_mul(0x2545f4914f6cdd1d)).next() }
}

/// Loops for `req`, at most `req.count`, as different from each other as `OVERLAP` allows: first
/// the one drawn from the request's own seed (so a loop's seed gives that loop back), then the
/// best of the others. Attempts run on `threads` threads; the result doesn't depend on how many.
pub fn plan(g: &Graph, req: &Request, threads: usize) -> Result<Vec<Loop>, NoLoop> {
    let radius = req.target.longest() / 2.0 + 100.0;
    let local = Local::around(g, req.lat, req.lon, radius, &req.prefs).map_err(|NoStart::TooFar| NoLoop::NoPath)?;
    if local.stretches.is_empty() {
        return Err(NoLoop::NoPath);
    }
    let attempts = (req.count * 2).max(4);
    let threads = threads.clamp(1, attempts);
    let mut found: Vec<(usize, Loop)> = std::thread::scope(|s| {
        let local = &local;
        let workers: Vec<_> = (0..threads)
            .map(|t| {
                s.spawn(move || {
                    let mut search = Search::new(local.len());
                    (t..attempts).step_by(threads).filter_map(|i| attempt(local, req, attempt_seed(req.seed, i), &mut search).map(|l| (i, l))).collect::<Vec<_>>()
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
    });
    // Exact mode: loops off the length are dropped, if any is on it.
    if let Target::Exact(_) = req.target
        && found.iter().any(|(_, l)| req.target.holds(l.dist))
    {
        found.retain(|(_, l)| req.target.holds(l.dist));
    }
    // The seed's own loop, then the best; ties by attempt, so the order never depends on the threads.
    found.sort_by(|a, b| (a.0 != 0).cmp(&(b.0 != 0)).then(a.1.score.total_cmp(&b.1.score)).then(a.0.cmp(&b.0)));
    let mut kept: Vec<Loop> = Vec::new();
    for (_, l) in found {
        if l.back > MAX_BACK {
            continue;
        }
        if kept.iter().all(|k| overlap(k, &l) <= OVERLAP) {
            kept.push(l);
        }
        if kept.len() == req.count {
            break;
        }
    }
    if kept.is_empty() { Err(NoLoop::NoLoop) } else { Ok(kept) }
}

/// The share of the shorter loop that the other also runs.
pub fn overlap(a: &Loop, b: &Loop) -> f32 {
    let (mut i, mut j, mut shared) = (0, 0, 0.0);
    while i < a.edges.len() && j < b.edges.len() {
        match a.edges[i].0.cmp(&b.edges[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                shared += a.edges[i].1.min(b.edges[j].1);
                i += 1;
                j += 1;
            }
        }
    }
    let total = |l: &Loop| l.edges.iter().map(|e| e.1).sum::<f32>();
    shared / total(a).min(total(b)).max(1.0)
}

/// One loop: its waypoints' places, then the routes through them.
struct Try {
    waypoints: Vec<u32>,
    /// Start → first waypoint, …, last waypoint → start, as routed.
    legs: Vec<Vec<u32>>,
    /// The legs joined, spurs cut.
    arcs: Vec<u32>,
    dist: f64,
}

fn attempt(l: &Local, req: &Request, seed: u64, search: &mut Search) -> Option<Loop> {
    let mut rng = Rng::new(seed);
    let n = 2 + (rng.next() % 3) as usize;
    let bearing = rng.unit() * 2.0 * PI;
    // The polygon: the start and `n` waypoints on a circle through the start, jittered.
    let sides = (n + 1) as f64;
    let shape: Vec<(f64, f64)> = (1..=n)
        .map(|k| {
            let angle = bearing + PI + 2.0 * PI * k as f64 / sides + (rng.unit() - 0.5) * 0.5 * 2.0 * PI / sides;
            (angle, 1.0 + (rng.unit() - 0.5) * 0.3)
        })
        .collect();
    let aim = req.target.aim();
    let perimeter = 2.0 * sides * (PI / sides).sin();
    let place = |scale: f64| -> Vec<u32> {
        let r = aim / (DETOUR * perimeter) * scale;
        let (cx, cy) = (r * bearing.cos(), r * bearing.sin());
        let mut w: Vec<u32> = shape.iter().filter_map(|(a, k)| l.nearest((cx + r * k * a.cos()) as f32, (cy + r * k * a.sin()) as f32)).collect();
        w.dedup();
        w
    };
    let mut best: Option<Try> = None;
    let keep = |t: Try, best: &mut Option<Try>| {
        let better = best.as_ref().is_none_or(|b| req.target.miss(t.dist) < req.target.miss(b.dist));
        if better {
            *best = Some(t);
        }
    };
    // Resize by secant steps on the scale, from what each try measured.
    let (mut s0, mut d0) = (1.0, None::<f64>);
    let mut scale = 1.0;
    for _ in 0..RESIZES {
        let w = place(scale);
        let Some(t) = route_all(l, &w, search) else {
            scale *= 0.8;
            continue;
        };
        let d = t.dist;
        let done = req.target.holds(d);
        keep(t, &mut best);
        if done {
            break;
        }
        let next = match d0 {
            Some(p) if (d - p).abs() > 1.0 => scale + (aim - d) * (scale - s0) / (d - p),
            _ => scale * aim / d.max(1.0),
        };
        (s0, d0) = (scale, Some(d));
        scale = next.clamp(scale * 0.4, scale * 2.5).max(0.05);
    }
    let mut best = best?;
    // Exact mode: move one waypoint at a time to a nearby node, while it brings the length
    // closer; the routes stay loops, never an out-and-back added for length.
    if matches!(req.target, Target::Exact(_)) {
        // First coarse: a waypoint slid along its line from the start by half what is missing
        // (a loop through it gains or loses about twice the slide).
        for k in 0..2 * best.waypoints.len() {
            if req.target.holds(best.dist) {
                break;
            }
            let i = k % best.waypoints.len();
            let w = best.waypoints[i] as usize;
            let (x, y) = (l.x[w] as f64, l.y[w] as f64);
            let r = x.hypot(y).max(1.0);
            let slide = (aim - best.dist) / 2.0;
            let k = ((r + slide) / r).max(0.1);
            if let Some(v) = l.nearest((x * k) as f32, (y * k) as f32)
                && v != 0
                && !best.waypoints.contains(&v)
                && let Some(t) = nudge(l, &best, i, v, search)
                && (t.dist - aim).abs() < (best.dist - aim).abs()
            {
                best = t;
            }
        }
        // Then fine: a waypoint moved to a node nearby.
        let mut stuck = 0;
        for k in 0..NUDGES {
            let n = best.waypoints.len();
            if req.target.holds(best.dist) || n == 0 || stuck >= n {
                break;
            }
            let i = k % n;
            // Too short: try the nodes farther from the start first; too long, the nearer.
            let longer = best.dist < aim;
            let far = |v: u32| l.x[v as usize].hypot(l.y[v as usize]);
            let mut options: Vec<u32> = neighbours(l, best.waypoints[i]).into_iter().filter(|v| *v != 0 && !best.waypoints.contains(v)).collect();
            options.sort_by(|a, b| if longer { far(*b).total_cmp(&far(*a)) } else { far(*a).total_cmp(&far(*b)) });
            let mut moved = false;
            for v in options.into_iter().take(NUDGE_OPTIONS) {
                if let Some(t) = nudge(l, &best, i, v, search)
                    && (t.dist - aim).abs() < (best.dist - aim).abs()
                {
                    best = t;
                    moved = true;
                }
            }
            stuck = if moved { 0 } else { stuck + 1 };
        }
    }
    Some(measure(l, req, seed, &best))
}

/// Nodes one and two stretches away.
fn neighbours(l: &Local, v: u32) -> Vec<u32> {
    let mut out = Vec::new();
    for &a in l.arcs_of(v) {
        let (_, w) = l.head(a);
        if !out.contains(&w) && w != v {
            out.push(w);
        }
        for &b in l.arcs_of(w) {
            let (_, x) = l.head(b);
            if !out.contains(&x) && x != v {
                out.push(x);
            }
        }
    }
    out
}

/// Start → waypoints → start, each leg avoiding what the previous ones took.
fn route_all(l: &Local, waypoints: &[u32], search: &mut Search) -> Option<Try> {
    let mut used = vec![0u8; l.stretches.len()];
    let mut legs = Vec::with_capacity(waypoints.len() + 1);
    let stops: Vec<u32> = std::iter::once(0).chain(waypoints.iter().copied()).chain(std::iter::once(0)).collect();
    for w in stops.windows(2) {
        let leg = leg(l, w[0], w[1], &used, search)?;
        take(&mut used, &leg, 1);
        legs.push(leg);
    }
    join(l, waypoints.to_vec(), legs)
}

/// A leg from `a` to `b`, what was already taken costing more.
fn leg(l: &Local, a: u32, b: u32, used: &[u8], search: &mut Search) -> Option<Vec<u32>> {
    search.route(l, a, b, |s| l.stretches[s as usize].cost * (1.0 + REUSE * used[s as usize] as f32))
}

/// Counts the stretches of `arcs` as taken (`by` 1) or no more (`by` −1).
fn take(used: &mut [u8], arcs: &[u32], by: i8) {
    for a in arcs {
        let u = &mut used[(a >> 1) as usize];
        *u = if by > 0 { u.saturating_add(1) } else { u.saturating_sub(1) };
    }
}

/// The same loop with waypoint `i` moved to `v`: only its two legs are routed again, avoiding
/// what the others take.
fn nudge(l: &Local, t: &Try, i: usize, v: u32, search: &mut Search) -> Option<Try> {
    let mut used = vec![0u8; l.stretches.len()];
    for (k, leg) in t.legs.iter().enumerate() {
        if k != i && k != i + 1 {
            take(&mut used, leg, 1);
        }
    }
    let before = if i == 0 { 0 } else { t.waypoints[i - 1] };
    let after = t.waypoints.get(i + 1).copied().unwrap_or(0);
    let mut legs = t.legs.clone();
    legs[i] = leg(l, before, v, &used, search)?;
    take(&mut used, &legs[i], 1);
    legs[i + 1] = leg(l, v, after, &used, search)?;
    let mut waypoints = t.waypoints.clone();
    waypoints[i] = v;
    join(l, waypoints, legs)
}

/// The legs as one loop: in by a stretch and straight back out by it is cut, again and again.
fn join(l: &Local, waypoints: Vec<u32>, legs: Vec<Vec<u32>>) -> Option<Try> {
    let mut arcs: Vec<u32> = Vec::with_capacity(legs.iter().map(Vec::len).sum());
    for &a in legs.iter().flatten() {
        if arcs.last().is_some_and(|p| p >> 1 == a >> 1 && p & 1 != a & 1) {
            arcs.pop();
        } else {
            arcs.push(a);
        }
    }
    if arcs.is_empty() {
        return None;
    }
    let dist = arcs.iter().map(|a| l.stretches[(a >> 1) as usize].len as f64).sum();
    Some(Try { waypoints, legs, arcs, dist })
}

/// What a loop is like: its line, length, climb, surfaces, the share run twice, its score.
fn measure(l: &Local, req: &Request, seed: u64, t: &Try) -> Loop {
    let arcs = &t.arcs;
    let (mut up, mut down, mut trail, mut paved) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut count: std::collections::HashMap<u32, u8> = std::collections::HashMap::new();
    let mut edges: std::collections::BTreeMap<u32, f32> = std::collections::BTreeMap::new();
    let (mut line, mut eles, mut along) = (Vec::new(), Vec::new(), Vec::new());
    let mut at = 0.0f32;
    for &a in arcs {
        let s = &l.stretches[(a >> 1) as usize];
        let (u, d) = if a & 1 == 0 { (s.up, s.down) } else { (s.down, s.up) };
        up += u as f64;
        down += d as f64;
        if s.class.is_trail() {
            trail += s.len as f64;
        }
        if s.class.is_paved() {
            paved += s.len as f64;
        }
        *count.entry(a >> 1).or_default() += 1;
        *edges.entry(s.edge).or_default() += s.len;
        let pts = l.line(a);
        for (k, (p, e)) in pts.iter().enumerate() {
            if k == 0 && !line.is_empty() {
                continue;
            }
            if let Some(prev) = line.last() {
                at += crate::logic::fit::metres(*prev, *p) as f32;
            }
            line.push(*p);
            eles.push(*e);
            along.push(at);
        }
    }
    let back: f64 = count.iter().filter(|(_, n)| **n > 1).map(|(s, n)| l.stretches[*s as usize].len as f64 * *n as f64).sum();
    let dist = t.dist.max(1.0);
    let (trail, paved, back) = ((trail / dist) as f32, (paved / dist) as f32, (back / dist) as f32);
    let p = &req.prefs;
    let mut score = 10.0 * req.target.miss(t.dist) + 2.0 * back as f64;
    if (p.trail - 0.5).abs() > 0.05 {
        // Leaning to trails: the share of trail; to streets: the share of the rest.
        let wanted = if p.trail > 0.5 { trail } else { 1.0 - trail };
        score += (1.0 - wanted as f64) * (p.trail - 0.5).abs() as f64 * 2.0;
    }
    if p.avoid_paved {
        score += paved as f64;
    }
    if let Some(c) = req.climb {
        score += (up - c).abs() / c.max(100.0);
    }
    Loop { line, eles, along, dist: t.dist, up, down, trail, paved, back, seed, score, edges: edges.into_iter().collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::graph::tests::town;

    fn req(target: Target, seed: u64) -> Request {
        // On the south street, 100 m from the corner.
        Request { lat: 43.70, lon: 7.25 + 100.0 / (111_195.0 * 0.7230), target, prefs: Prefs::default(), climb: None, seed, count: 3 }
    }

    #[test]
    fn exact_loops_come_within_two_percent_and_differ() {
        let g = town();
        for (l, seed) in [(3000.0, 1), (5000.0, 2), (7000.0, 3), (9000.0, 4)] {
            let loops = plan(&g, &req(Target::Exact(l), seed), 4).unwrap();
            assert!(!loops.is_empty());
            assert!(loops.iter().all(|b| Target::Exact(l).holds(b.dist)), "{l}: {:?}", loops.iter().map(|b| b.dist).collect::<Vec<_>>());
            for (i, a) in loops.iter().enumerate() {
                for b in &loops[i + 1..] {
                    assert!(overlap(a, b) <= OVERLAP);
                }
                assert!(a.back <= MAX_BACK);
                // A loop: it ends where it starts, and its line measures what it says.
                assert!(crate::logic::fit::metres(a.line[0], *a.line.last().unwrap()) < 1.0);
                assert!((*a.along.last().unwrap() as f64 - a.dist).abs() < a.dist * 0.01);
                assert_eq!(a.line.len(), a.eles.len());
            }
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_loops_whatever_the_threads() {
        let g = town();
        let r = req(Target::Exact(6000.0), 42);
        let one = plan(&g, &r, 1).unwrap();
        let many = plan(&g, &r, 8).unwrap();
        assert_eq!(one, many);
        // A loop's own seed gives it first.
        let again = plan(&g, &Request { seed: one[1].seed, ..r }, 2).unwrap();
        assert_eq!(again[0].line, one[1].line);
        assert_ne!(plan(&g, &Request { seed: 43, ..r }, 2).unwrap()[0].line, one[0].line);
    }

    #[test]
    fn ranges_and_preferences_shape_the_loops() {
        let g = town();
        let loops = plan(&g, &req(Target::Range(4000.0, 6000.0), 7), 4).unwrap();
        assert!(loops.iter().take(2).all(|l| (4000.0..=6000.0).contains(&l.dist)));
        // Leaning to trails: more of them than leaning to streets.
        let trails = plan(&g, &Request { prefs: Prefs { trail: 1.0, avoid_paved: true }, ..req(Target::Exact(5000.0), 9) }, 4).unwrap();
        let streets = plan(&g, &Request { prefs: Prefs { trail: 0.0, avoid_paved: false }, ..req(Target::Exact(5000.0), 9) }, 4).unwrap();
        let mean = |ls: &[Loop], f: fn(&Loop) -> f32| ls.iter().map(f).sum::<f32>() / ls.len() as f32;
        assert!(mean(&trails, |l| l.trail) > mean(&streets, |l| l.trail) + 0.2, "{} {}", mean(&trails, |l| l.trail), mean(&streets, |l| l.trail));
        assert!(mean(&trails, |l| l.paved) < mean(&streets, |l| l.paved));
    }

    #[test]
    fn no_path_near_the_start_says_so() {
        let g = town();
        assert_eq!(plan(&g, &Request { lat: 44.5, ..req(Target::Exact(5000.0), 1) }, 2).err(), Some(NoLoop::NoPath));
        // From the island, 500 m of path: no loop of 5 km.
        let island = Request { lat: 43.70 + 200.0 / 111_195.0, lon: 7.25 + 4000.0 / (111_195.0 * 0.7230), ..req(Target::Exact(5000.0), 1) };
        assert_eq!(plan(&g, &island, 2).err(), Some(NoLoop::NoLoop));
    }

    #[test]
    fn rng_is_stable() {
        let mut r = Rng::new(0);
        assert_eq!(r.next(), 0xe220a8397b1dcdaf);
        assert!((0..1000).map(|_| r.unit()).all(|u| (0.0..1.0).contains(&u)));
    }

    /// 200 requests of 5 to 40 km in exact mode, from starts drawn within 12 km of Nice: how many
    /// get a loop within 2 %, and how long planning takes. Run by hand, in release, on a graph
    /// built by `measure_graph_build`:
    ///
    /// ASTROLABE_GRAPH=target/measure/region.graph cargo test --release -- --ignored --nocapture measure_loops_around_nice
    #[test]
    #[ignore]
    fn measure_loops_around_nice() {
        use std::time::Instant;
        let path = std::env::var("ASTROLABE_GRAPH").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/target/measure/region.graph").into());
        let t = Instant::now();
        let g = crate::logic::graph::Graph::decode(&std::fs::read(&path).expect("a graph: see measure_graph_build")).unwrap();
        println!("graph read in {:.2} s", t.elapsed().as_secs_f64());
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).max(1);
        let mut rng = Rng::new(2026);
        let (mut times, mut twenty, mut fit, mut asked, mut none) = (Vec::new(), Vec::new(), 0, 0, 0);
        let mut first_fit = 0;
        while asked < 200 {
            let (r, a) = (12_000.0 * rng.unit().sqrt(), rng.unit() * 2.0 * PI);
            let lat = 43.7034 + r * a.sin() / 111_195.0;
            let lon = 7.2663 + r * a.cos() / (111_195.0 * 43.7034f64.to_radians().cos());
            let km = 5.0 + rng.unit() * 35.0;
            let req = Request { lat, lon, target: Target::Exact(km * 1000.0), prefs: Prefs::default(), climb: None, seed: rng.next(), count: 5 };
            let t = Instant::now();
            let res = plan(&g, &req, threads);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            match res {
                // In the sea or beyond the paths: not a request one would make.
                Err(NoLoop::NoPath) => continue,
                Err(NoLoop::NoLoop) => none += 1,
                Ok(loops) => {
                    fit += loops.iter().any(|l| req.target.holds(l.dist)) as usize;
                    first_fit += req.target.holds(loops[0].dist) as usize;
                }
            }
            asked += 1;
            times.push(ms);
            if (18.0..=22.0).contains(&km) {
                twenty.push(ms);
            }
        }
        let q = |v: &mut Vec<f64>, p: f64| {
            v.sort_by(f64::total_cmp);
            v.get(((v.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(f64::NAN)
        };
        println!("{asked} requests, {threads} threads: {fit} with a loop within 2 % ({first_fit} the first one), {none} without a loop");
        println!("planning: p50 {:.0} ms, p95 {:.0} ms, max {:.0} ms", q(&mut times, 0.5), q(&mut times, 0.95), q(&mut times, 1.0));
        println!("18 to 22 km ({}): p50 {:.0} ms, p95 {:.0} ms", twenty.len(), q(&mut twenty, 0.5), q(&mut twenty, 0.95));
    }

    /// The same on a made-up network, denser than a city (a jittered grid every 120 m over
    /// 40 × 40 km, every fifth street a path): for tuning without a real extract. Its figures
    /// are not Nice's.
    ///
    /// cargo test --release -- --ignored --nocapture measure_loops_on_a_made_up_network
    #[test]
    #[ignore]
    fn measure_loops_on_a_made_up_network() {
        use std::time::Instant;
        let (n, step) = (333i64, 120.0);
        let mut rng = Rng::new(1);
        let (dlat, dlon) = (1.0 / 111_195.0, 1.0 / (111_195.0 * 0.7230));
        let id = |r: i64, c: i64| 1 + r * 1000 + c;
        let nodes: Vec<(i64, f64, f64)> = (0..n)
            .flat_map(|r| (0..n).map(move |c| (r, c)))
            .map(|(r, c)| (id(r, c), 43.5 + (r as f64 * step) * dlat, 7.0 + (c as f64 * step) * dlon))
            .map(|(i, la, lo)| (i, la + (rng.unit() - 0.5) * 40.0 * dlat, lo + (rng.unit() - 0.5) * 40.0 * dlon))
            .collect();
        let mut ways = Vec::new();
        for r in 0..n {
            let tags = if r % 5 == 0 { vec![("highway", "path")] } else { vec![("highway", "residential")] };
            ways.push((10_000_000 + r, tags.clone(), (0..n).map(|c| id(r, c)).collect::<Vec<_>>()));
            ways.push((20_000_000 + r, tags, (0..n).map(|c| id(c, r)).collect::<Vec<_>>()));
        }
        let t = Instant::now();
        let g = crate::logic::graph::tests::build_from(&crate::logic::graph::tests::extract(&nodes, &ways)).unwrap();
        println!("made up: {} nodes, {} edges, built in {:.1} s", g.nodes(), g.edges.len(), t.elapsed().as_secs_f64());
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).max(1);
        let mut rng = Rng::new(7);
        let (mut times, mut fit) = (Vec::new(), 0);
        for _ in 0..60 {
            let km = 5.0 + rng.unit() * 35.0;
            let req = Request { lat: 43.68 + rng.unit() * 0.01, lon: 7.25 + rng.unit() * 0.01, target: Target::Exact(km * 1000.0), prefs: Prefs::default(), climb: None, seed: rng.next(), count: 5 };
            let t = Instant::now();
            let res = plan(&g, &req, threads);
            times.push((km, t.elapsed().as_secs_f64() * 1000.0));
            fit += res.is_ok_and(|l| req.target.holds(l[0].dist)) as usize;
        }
        times.sort_by(|a, b| a.1.total_cmp(&b.1));
        println!("60 requests, {threads} threads, {fit} within 2 %: p50 {:.0} ms, p95 {:.0} ms, max {:.0} ms at {:.0} km", times[30].1, times[57].1, times[59].1, times[59].0);
    }
}
