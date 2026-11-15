//! The loop planner's share of `Data`: the region's walking graph (downloaded, built, read back
//! from disk), the loops last planned, and the elevation tiles of the relief. The work runs on the
//! background executor; where it stands shows in the status line.

use super::{Data, Status};
use crate::lang::{tr, trf};
use crate::logic::elevation::{self, Dem};
use crate::logic::graph::{self, CANCELLED, Graph};
use crate::logic::loops::{self, Loop, NoLoop, Request};
use crate::logic::regions::{self, Region};
use crate::platform::{self, store};
use gpui_kit::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// A download or a build under way, and how far it is.
pub struct Job {
    /// 0: download, 1 to 3: passes over the extract, 4: elevation tiles, 5: altitudes and saving.
    pub stage: Arc<AtomicU64>,
    /// Bytes downloaded or tiles fetched, of `total`.
    pub done: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,
    /// The build's own progress (pass, bytes of the extract read in it).
    pub build: Arc<graph::Progress>,
    pub cancel: Arc<AtomicBool>,
}

impl Job {
    fn new(stage: u64) -> Job {
        Job {
            stage: Arc::new(AtomicU64::new(stage)),
            done: Arc::default(),
            total: Arc::default(),
            build: Arc::default(),
            cancel: Arc::default(),
        }
    }
}

pub struct Loops {
    pub region: &'static Region,
    pub graph: Option<Arc<Graph>>,
    /// The disk was looked at (once, when the page first shows).
    pub looked: bool,
    /// Size of the extract on disk.
    pub extract: Option<u64>,
    pub job: Option<Job>,
    /// What went wrong with the region, said on the page.
    pub error: Option<String>,
    /// The loops last planned, best first.
    pub found: Arc<Vec<Loop>>,
    /// Counts the sets of loops planned: a new one is fitted on the map, an edit is not.
    pub generation: u64,
    pub planning: bool,
    pub plan_error: Option<String>,
    plan_round: u64,
    /// Geofabrik's and the elevation tiles' addresses (local stand-ins in the tests).
    pub osm_base: String,
    pub elevation_base: String,
    relief: HashMap<(u8, u32, u32), Option<Arc<Vec<f32>>>>,
    relief_busy: usize,
}

impl Default for Loops {
    fn default() -> Self {
        Loops {
            region: &regions::REGIONS[0],
            graph: None,
            looked: false,
            extract: None,
            job: None,
            error: None,
            found: Arc::default(),
            generation: 0,
            planning: false,
            plan_error: None,
            plan_round: 0,
            osm_base: platform::osm::GEOFABRIK.into(),
            elevation_base: platform::elevation::TERRARIUM.into(),
            relief: HashMap::new(),
            relief_busy: 0,
        }
    }
}

/// Worker threads for the heavy work: all cores but one, which stays with the window.
fn threads() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).max(1)
}

impl Data {
    /// Looks at the disk for the region's graph, once: reads it, or builds it again from the
    /// extract when it is from another version or damaged (set aside, and said).
    pub fn open_loops(&mut self, cx: &mut Context<Self>) {
        if self.loops.looked {
            return;
        }
        self.loops.looked = true;
        self.loops.region = regions::find(self.settings.get("loops.region").map_or("", String::as_str));
        let id = self.loops.region.id;
        cx.spawn(async move |this, cx| {
            let (size, stored) = cx.background_executor().spawn(async move { (store::extract_size(id), store::load_graph(id)) }).await;
            this.update(cx, |d, cx| {
                d.loops.extract = size;
                match stored {
                    store::StoredGraph::Ready(g) => d.loops.graph = Some(Arc::new(g)),
                    store::StoredGraph::Absent | store::StoredGraph::Stale => {
                        if size.is_some() {
                            d.build_graph(cx);
                        }
                    }
                    store::StoredGraph::Failed(e) => {
                        d.loops.error = Some(e.clone());
                        d.status = Status::Failed(e);
                        if size.is_some() {
                            d.build_graph(cx);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Repaints while a job runs, so its progress moves.
    fn tick_loops(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(250)).await;
                let going = this.update(cx, |d, cx| {
                    cx.notify();
                    d.loops.job.is_some()
                });
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Downloads the region's extract, then builds its graph.
    pub fn download_region(&mut self, cx: &mut Context<Self>) {
        if self.loops.job.is_some() {
            return;
        }
        let job = Job::new(0);
        let (done, total, cancel) = (job.done.clone(), job.total.clone(), job.cancel.clone());
        self.loops.job = Some(job);
        self.loops.error = None;
        self.tick_loops(cx);
        let (base, region) = (self.loops.osm_base.clone(), self.loops.region);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = cx.background_executor().spawn(async move { platform::osm::download(&base, region, &done, &total, &cancel) }).await;
            this.update(cx, |d, cx| {
                d.loops.job = None;
                match res {
                    Ok(n) => {
                        d.loops.extract = Some(n);
                        d.build_graph(cx);
                    }
                    Err(e) => d.loops_failed(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn loops_failed(&mut self, e: String) {
        if e == CANCELLED {
            self.note = Some(tr("stopped", "arrêté").into());
        } else {
            self.loops.error = Some(e.clone());
            self.status = Status::Failed(e);
        }
    }

    /// Builds the region's graph from its extract, reads the altitudes (fetching the elevation
    /// tiles not cached yet), and keeps it on disk.
    pub fn build_graph(&mut self, cx: &mut Context<Self>) {
        if self.loops.job.is_some() {
            return;
        }
        let job = Job::new(1);
        let (stage, done, total, build, cancel) = (job.stage.clone(), job.done.clone(), job.total.clone(), job.build.clone(), job.cancel.clone());
        self.loops.job = Some(job);
        self.loops.error = None;
        self.tick_loops(cx);
        let (id, base) = (self.loops.region.id, self.loops.elevation_base.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move {
                    let size = store::extract_size(id).unwrap_or(0);
                    let progress = build.clone();
                    let mut read = |pass: u32, f: &mut dyn FnMut(&str, Vec<u8>) -> Result<(), String>| {
                        stage.store(pass as u64, Ordering::Relaxed);
                        platform::osm::read_extract(id, &progress.bytes, f)
                    };
                    let mut g = match graph::build(&mut read, size, threads(), &cancel, &build) {
                        Ok(g) => g,
                        Err(e) if e.starts_with("PBF:") => {
                            let kept = store::set_aside_extract(id);
                            let unreadable = tr("Extract unreadable", "Extrait illisible");
                            return Err(trf(
                                format!("{unreadable} ({e}), kept as {kept}: download it again."),
                                format!("{unreadable} ({e}), conservé sous {kept} : télécharge-le à nouveau."),
                            ));
                        }
                        Err(e) => return Err(e),
                    };
                    stage.store(4, Ordering::Relaxed);
                    let tiles = g.tiles();
                    let missing: Vec<(u32, u32)> = tiles.iter().copied().filter(|(x, y)| platform::elevation::cached(elevation::ZOOM, *x, *y).is_none()).collect();
                    total.store(tiles.len() as u64, Ordering::Relaxed);
                    done.store((tiles.len() - missing.len()) as u64, Ordering::Relaxed);
                    let (failed, last) = platform::elevation::fetch_all(&base, elevation::ZOOM, &missing, &done, &cancel);
                    if cancel.load(Ordering::Relaxed) {
                        return Err(CANCELLED.to_string());
                    }
                    stage.store(5, Ordering::Relaxed);
                    let mut dem = Dem::new(64, |x, y| platform::elevation::cached(elevation::ZOOM, x, y).and_then(|b| elevation::decode(&b)));
                    g.set_elevation(&mut dem);
                    store::save_graph(id, &g.encode())?;
                    Ok((g, failed, last))
                })
                .await;
            this.update(cx, |d, cx| {
                d.loops.job = None;
                match res {
                    Ok((g, failed, last)) => {
                        if failed > 0 {
                            let why = last.unwrap_or_default();
                            d.loops.error = Some(trf(
                                format!("Altitude incomplete: {failed} tiles could not be fetched ({why}). Build again once online."),
                                format!("Altitude incomplète : {failed} tuiles n'ont pas pu être téléchargées ({why}). Reconstruis une fois en ligne."),
                            ));
                        }
                        d.loops.graph = Some(Arc::new(g));
                        d.note = Some(tr("path network ready", "réseau de chemins prêt").into());
                    }
                    Err(e) => {
                        if e.contains(".damaged-") {
                            d.loops.extract = None;
                        }
                        d.loops_failed(e);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Asks the download or the build to stop.
    pub fn stop_loops_job(&mut self, cx: &mut Context<Self>) {
        if let Some(j) = &self.loops.job {
            j.cancel.store(true, Ordering::Relaxed);
            cx.notify();
        }
    }

    /// Where the download or the build stands, in words, and its share done.
    pub fn loops_line(&self) -> Option<(String, Option<f32>)> {
        let j = self.loops.job.as_ref()?;
        let share = |done: u64, total: u64| (total > 0).then(|| (done as f32 / total as f32).min(1.0));
        let pct = |s: Option<f32>| s.map(|s| format!(" : {:.0} %", s * 100.0)).unwrap_or_default();
        Some(match j.stage.load(Ordering::Relaxed) {
            0 => {
                let s = share(j.done.load(Ordering::Relaxed), j.total.load(Ordering::Relaxed));
                let mb = j.done.load(Ordering::Relaxed) / 1_000_000;
                (format!("{} · {mb} {}{}", tr("Loops: downloading the region", "Boucles : téléchargement de la région"), tr("MB", "Mo"), pct(s)), s)
            }
            p @ 1..=3 => {
                let s = share(j.build.bytes.load(Ordering::Relaxed), self.loops.extract.unwrap_or(0));
                (format!("{} {p}/3{}", tr("Loops: building the path network", "Boucles : construction du réseau"), pct(s)), s)
            }
            4 => {
                let (done, total) = (j.done.load(Ordering::Relaxed), j.total.load(Ordering::Relaxed));
                (format!("{} {done} / {total}", tr("Loops: elevation tiles", "Boucles : tuiles d'altitude")), share(done, total))
            }
            _ => (tr("Loops: reading the altitudes", "Boucles : lecture des altitudes").into(), None),
        })
    }

    /// Plans loops for `req` off the UI thread; a newer request makes an older one's result moot.
    pub fn plan_loops(&mut self, req: Request, cx: &mut Context<Self>) {
        let Some(g) = self.loops.graph.clone() else { return };
        self.loops.plan_round += 1;
        let round = self.loops.plan_round;
        self.loops.planning = true;
        self.loops.plan_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = cx.background_executor().spawn(async move { loops::plan(&g, &req, threads()) }).await;
            this.update(cx, |d, cx| {
                if d.loops.plan_round != round {
                    return;
                }
                d.loops.planning = false;
                match res {
                    Ok(found) => {
                        d.loops.found = Arc::new(found);
                        d.loops.generation += 1;
                    }
                    Err(e) => {
                        d.loops.found = Arc::default();
                        d.loops.plan_error = Some(
                            match e {
                                NoLoop::NoPath => tr("No path within 500 m of this point.", "Aucun chemin à moins de 500 m de ce point."),
                                NoLoop::NoLoop => tr("No loop of this length from here: try another length or start.", "Pas de boucle de cette longueur d'ici : essaie une autre longueur ou un autre départ."),
                            }
                            .into(),
                        );
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Forgets the loops (and the answer to a planning still under way).
    pub fn clear_loops(&mut self, cx: &mut Context<Self>) {
        self.loops.plan_round += 1;
        self.loops.planning = false;
        self.loops.plan_error = None;
        self.loops.found = Arc::default();
        self.loops.generation += 1;
        cx.notify();
    }

    /// Writes a loop as GPX where the user chose.
    pub fn export_loop(&mut self, l: &Loop, path: PathBuf, cx: &mut Context<Self>) {
        let name = format!("{} {:.1} km", tr("Loop", "Boucle"), l.dist / 1000.0);
        let text = crate::logic::gpx::write(&name, &l.line, &l.eles);
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        cx.spawn(async move |this, cx| {
            let res = cx.background_executor().spawn(async move { store::export(&path, text.as_bytes()) }).await;
            this.update(cx, |d, cx| {
                match res {
                    Ok(()) => d.note = Some(trf(format!("loop saved as {file}"), format!("boucle enregistrée sous {file}"))),
                    Err(e) => d.status = Status::Failed(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The altitudes of an elevation tile for the relief, if ready; otherwise asks for it (two at
    /// a time, from the cache or the server).
    pub fn relief_tile(&mut self, k: (u8, u32, u32), cx: &mut Context<Self>) -> Option<Arc<Vec<f32>>> {
        // The tests never go online.
        if cfg!(test) {
            return None;
        }
        if let Some(t) = self.loops.relief.get(&k) {
            return t.clone();
        }
        if self.loops.relief_busy >= 2 {
            return None;
        }
        // ponytail: drops every tile past 200, an LRU if panning around shows refetches.
        if self.loops.relief.len() > 200 {
            self.loops.relief.retain(|_, t| t.is_none());
        }
        self.loops.relief.insert(k, None);
        self.loops.relief_busy += 1;
        let base = self.loops.elevation_base.clone();
        cx.spawn(async move |this, cx| {
            let t = cx.background_executor().spawn(async move { platform::elevation::tile(&base, k.0, k.1, k.2).ok().and_then(|b| elevation::decode(&b)).map(Arc::new) }).await;
            this.update(cx, |d, cx| {
                d.loops.relief_busy -= 1;
                // A tile that can't be had stays empty, so it isn't asked again at every frame.
                if let Some(t) = t {
                    d.loops.relief.insert(k, Some(t));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        None
    }
}
