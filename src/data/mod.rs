//! The data every view reads, and the background work that fills it: the local store, the
//! keyring, the synchronisation with Garmin, the activity files, the map tiles. Views observe it;
//! it never decides what the window shows, it says so with a `DataEvent`.

use crate::logic::activity::Activity;
use crate::logic::fit::{self, Derived, Split, Track};
use crate::logic::map;
use crate::platform::{
    garmin::{self, Fail},
    secrets, store, tiles,
};
use crate::ui::render_image;
use crate::ui::theme::Theme;
use chrono::Local;
use gpui_kit::*;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// How long the rail says "done" after a clean synchronisation.
const RESULT_SHOWN: Duration = Duration::from_secs(6);

pub enum Status {
    Idle,
    Busy(String),
    Failed(String),
}

/// The track downloads, as they go: what the status line, the settings and the records show.
#[derive(Default)]
pub struct Fetch {
    pub running: bool,
    /// Asked to stop after the file in progress.
    pub cancel: bool,
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    /// The activity being downloaded.
    pub current: Option<String>,
    /// The last failure, said plainly.
    pub error: Option<String>,
    /// Why the queue stopped before the end.
    pub stopped: Option<String>,
}

pub enum Tile {
    Loading,
    Ready(Arc<RenderImage>),
    Failed,
}

/// What the window should do about the data.
pub enum DataEvent {
    /// No key to sync with: the settings are where one is added.
    NeedKey,
    /// A key was checked and saved: back to the activities.
    KeyReady,
}

pub struct Data {
    /// Newest first.
    pub acts: Arc<Vec<Activity>>,
    /// Per activity whose file was handled: its simplified line and best efforts.
    pub derived: Arc<HashMap<i64, Derived>>,
    pub theme: Theme,
    pub key: Option<String>,
    pub key_loaded: bool,
    /// Where the key is kept on this machine (known once `key_loaded`).
    pub key_store: secrets::Store,
    /// Why the last key was refused.
    pub key_error: Option<String>,
    token: Option<(String, Instant)>,
    pub status: Status,
    /// The last thing worth saying, after the activity count.
    pub note: Option<String>,
    /// New activities of the synchronisation under way, said once its files are done.
    sync_added: Option<usize>,
    /// When the last synchronisation ended, and whether it went through without a failure.
    finished: Option<(Instant, bool)>,
    /// Settings that aren't secrets (the tile server).
    pub settings: HashMap<String, String>,
    pub fetch: Fetch,
    tiles: HashMap<(u8, u32, u32), Tile>,
    tiles_busy: usize,
}

impl EventEmitter<DataEvent> for Data {}

/// The data a view draws from, as it was at the last change.
#[derive(Clone)]
pub struct Snapshot {
    pub acts: Arc<Vec<Activity>>,
    pub derived: Arc<HashMap<i64, Derived>>,
    pub theme: Theme,
}

impl Snapshot {
    pub fn activity(&self, id: i64) -> Option<&Activity> {
        self.acts.iter().find(|a| a.activity_id == id)
    }

    pub fn index_of(&self, id: i64) -> Option<usize> {
        self.acts.iter().position(|a| a.activity_id == id)
    }
}

impl Data {
    pub fn new(theme: Theme, cx: &mut Context<Self>) -> Self {
        let mut data = Self::blank(theme);
        data.start(cx);
        data
    }

    /// Data that never reads the disk nor the key store, never goes online: for the tests of the window.
    #[cfg(test)]
    pub fn with_activities(theme: Theme, acts: Vec<Activity>) -> Self {
        Data { acts: Arc::new(acts), key_loaded: true, status: Status::Idle, ..Self::blank(theme) }
    }

    fn blank(theme: Theme) -> Self {
        Data {
            acts: Arc::new(Vec::new()),
            derived: Arc::new(HashMap::new()),
            theme,
            key: None,
            key_loaded: false,
            key_store: secrets::Store::System,
            key_error: None,
            token: None,
            status: Status::Busy("Ouverture…".into()),
            note: None,
            sync_added: None,
            finished: None,
            settings: store::load_settings(),
            fetch: Fetch::default(),
            tiles: HashMap::new(),
            tiles_busy: 0,
        }
    }

    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        if theme.dark != self.theme.dark {
            // Tiles are drawn in the theme's colors.
            self.tiles.clear();
        }
        self.theme = theme;
        cx.notify();
    }

    /// What the views read on every frame, copied when the data changes (cheap: shared pointers).
    pub fn snapshot(&self) -> Snapshot {
        Snapshot { acts: self.acts.clone(), derived: self.derived.clone(), theme: self.theme }
    }

    /// Cache, derived tracks and keyring are read off the UI thread (the keyring may wait for an unlock prompt).
    fn start(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let (acts, derived, (key_store, key)) = cx
                .background_executor()
                .spawn(async { (store::load_cache(), store::load_derived(), secrets::load_key()) })
                .await;
            this.update(cx, |this, cx| {
                this.acts = Arc::new(acts);
                this.derived = Arc::new(derived);
                this.upgrade_derived(cx);
                this.key_loaded = true;
                this.status = Status::Idle;
                this.key_store = key_store;
                match key {
                    Ok(Some(k)) => {
                        this.key = Some(k);
                        this.sync(cx);
                    }
                    Ok(None) if this.acts.is_empty() => cx.emit(DataEvent::NeedKey),
                    Ok(None) => {}
                    Err(e) => this.status = Status::Failed(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn busy(&self) -> bool {
        matches!(self.status, Status::Busy(_))
    }

    /// Clears an error once it was seen.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if matches!(self.status, Status::Failed(_)) {
            self.status = Status::Idle;
            cx.notify();
        }
    }

    /// A bearer token, exchanged again after 12 h (they last 24).
    async fn token(this: &WeakEntity<Self>, cx: &mut AsyncApp) -> Result<String, String> {
        let (cached, key) = this.read_with(cx, |d, _| (d.token.clone(), d.key.clone())).map_err(|e| e.to_string())?;
        if let Some((t, _)) = cached.filter(|(_, at)| at.elapsed() < Duration::from_secs(12 * 3600)) {
            return Ok(t);
        }
        let key = key.ok_or("Aucune clé API : ajoute-la dans les réglages.")?;
        let t = cx.background_executor().spawn(async move { garmin::token(&key) }).await?;
        this.update(cx, |d, _| d.token = Some((t.clone(), Instant::now()))).ok();
        Ok(t)
    }

    pub fn sync(&mut self, cx: &mut Context<Self>) {
        if self.key.is_none() {
            cx.emit(DataEvent::NeedKey);
            return;
        }
        if self.busy() {
            return;
        }
        self.status = Status::Busy("Synchronisation : connexion à Garmin…".into());
        self.finished = None;
        cx.notify();
        let known = self.acts.clone();
        let seen = Arc::new(AtomicUsize::new(0));
        // Live count while the pages arrive.
        let counter = seen.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(150)).await;
                let n = counter.load(Ordering::Relaxed);
                let going = this
                    .update(cx, |d, cx| {
                        if !matches!(&d.status, Status::Busy(s) if s.starts_with("Synchronisation")) {
                            return false;
                        }
                        if n > 0 {
                            d.status = Status::Busy(format!("Synchronisation : {} activités reçues…", crate::logic::activity::group(n as i64)));
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !going {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let res = match Self::token(&this, cx).await {
                Ok(token) => {
                    cx.background_executor()
                        .spawn(async move {
                            let all = garmin::sync(&token, &known, &seen)?;
                            let added = all.len() - known.len();
                            store::save_cache(&all)?;
                            Ok::<_, String>((all, added))
                        })
                        .await
                }
                Err(e) => Err(e),
            };
            this.update(cx, |this, cx| {
                this.status = match res {
                    Ok((all, added)) => {
                        this.acts = Arc::new(all);
                        let at = Local::now().format("%H:%M");
                        this.note = Some(match added {
                            0 => format!("à jour à {at}"),
                            1 => format!("1 nouvelle à {at}"),
                            n => format!("{n} nouvelles à {at}"),
                        });
                        this.sync_added = Some(added);
                        this.fetch_tracks(cx);
                        if !this.fetch.running {
                            // Nothing left to download: an old stop no longer holds.
                            this.fetch.stopped = None;
                            this.conclude(0, 0, cx);
                        }
                        Status::Idle
                    }
                    Err(e) => {
                        this.finished = Some((Instant::now(), false));
                        Status::Failed(e)
                    }
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Downloads, one at a time, the files of the activities not handled yet, newest first.
    /// Every step is shown; it stops on Garmin's "too many requests", after 5 failures in a row,
    /// or when asked, and picks up at the next sync (or the "Reprendre" button).
    pub fn fetch_tracks(&mut self, cx: &mut Context<Self>) {
        if self.fetch.running || self.key.is_none() {
            return;
        }
        let todo: Vec<(i64, String)> =
            self.acts.iter().filter(|a| !self.derived.contains_key(&a.activity_id)).map(|a| (a.activity_id, crate::logic::journal::name(a))).collect();
        if todo.is_empty() {
            return;
        }
        self.fetch = Fetch { running: true, total: todo.len(), ..Fetch::default() };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut in_a_row = 0;
            for (n, (id, label)) in todo.iter().cloned().enumerate() {
                let go = this
                    .update(cx, |d, cx| {
                        d.fetch.current = Some(label.clone());
                        cx.notify();
                        !d.fetch.cancel && d.key.is_some()
                    })
                    .unwrap_or(false);
                if !go {
                    this.update(cx, |d, _| d.fetch.stopped = Some("Arrêté à ta demande.".into())).ok();
                    break;
                }
                let token = match Self::token(&this, cx).await {
                    Ok(t) => t,
                    Err(e) => {
                        this.update(cx, |d, _| d.fetch.stopped = Some(e)).ok();
                        break;
                    }
                };
                let res = cx
                    .background_executor()
                    .spawn(async move {
                        match garmin::download_fit(&token, id)? {
                            None => Ok(Derived::default()),
                            Some(zip) => {
                                store::save_fit(id, &zip).map_err(Fail::Other)?;
                                let track = fit::unzip(&zip).and_then(|f| fit::parse(&f)).map_err(Fail::Other)?;
                                Ok(fit::derive(&track))
                            }
                        }
                    })
                    .await;
                let stop = this
                    .update(cx, |d, cx| {
                        d.fetch.done = n + 1;
                        let stop = match res {
                            Ok(derived) => {
                                Arc::make_mut(&mut d.derived).insert(id, derived);
                                in_a_row = 0;
                                false
                            }
                            Err(Fail::TooMany) => {
                                d.fetch.stopped = Some("Garmin demande de ralentir : la suite viendra à la prochaine synchronisation.".into());
                                true
                            }
                            Err(Fail::Other(e)) => {
                                d.fetch.failed += 1;
                                in_a_row += 1;
                                d.fetch.error = Some(format!("« {label} » : {e}"));
                                if in_a_row >= 5 {
                                    d.fetch.stopped = Some(format!("Arrêté après 5 échecs de suite. Dernier : {e}"));
                                }
                                in_a_row >= 5
                            }
                        };
                        if (n + 1) % 25 == 0 {
                            d.save_derived(cx);
                        }
                        cx.notify();
                        stop
                    })
                    .unwrap_or(true);
                if stop {
                    break;
                }
                // Gentle with the API: unknown limits.
                cx.background_executor().timer(Duration::from_millis(300)).await;
            }
            this.update(cx, |d, cx| {
                d.fetch.running = false;
                d.fetch.cancel = false;
                d.fetch.current = None;
                d.save_derived(cx);
                let failed = d.fetch.failed;
                d.conclude(d.fetch.done.saturating_sub(failed), failed, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The synchronisation is over: says what it brought, and for a moment how it went.
    fn conclude(&mut self, files: usize, failed: usize, cx: &mut Context<Self>) {
        let added = self.sync_added.take();
        if added.is_some() || files + failed > 0 {
            let at = Local::now().format("%H:%M");
            self.note = Some(format!("synchronisé à {at} · {}", crate::logic::sync::summary(added.unwrap_or(0), files, failed)));
        }
        self.finished = Some((Instant::now(), failed == 0 && self.fetch.stopped.is_none()));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RESULT_SHOWN).await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    /// How the last synchronisation went: `Some(true)` for a few seconds after a clean one,
    /// `Some(false)` after one that failed or stopped short, until the next one starts.
    pub fn sync_outcome(&self) -> Option<bool> {
        match self.finished {
            Some((_, false)) => Some(false),
            _ if matches!(self.status, Status::Failed(_)) => Some(false),
            Some((at, true)) if at.elapsed() < RESULT_SHOWN && !self.syncing() => Some(true),
            _ => None,
        }
    }

    /// Asks the track downloads to stop after the file in progress.
    pub fn stop_fetch(&mut self, cx: &mut Context<Self>) {
        self.fetch.cancel = true;
        cx.notify();
    }

    /// Entries kept by an older version are derived again from the files on disk, off the UI
    /// thread and without the network. One without a file stays as it is (and is tried again at
    /// the next launch); nothing is removed.
    fn upgrade_derived(&mut self, cx: &mut Context<Self>) {
        let stale: Vec<i64> = self.derived.iter().filter(|(_, d)| d.version < fit::VERSION).map(|(id, _)| *id).collect();
        if stale.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let fresh: Vec<(i64, Derived)> = cx
                .background_executor()
                .spawn(async move {
                    stale
                        .into_iter()
                        .filter_map(|id| {
                            let track = fit::unzip(&store::load_fit(id)?).and_then(|f| fit::parse(&f)).ok()?;
                            Some((id, fit::derive(&track)))
                        })
                        .collect()
                })
                .await;
            this.update(cx, |d, cx| {
                if fresh.is_empty() {
                    return;
                }
                let map = Arc::make_mut(&mut d.derived);
                for (id, derived) in fresh {
                    map.insert(id, derived);
                }
                d.save_derived(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn save_derived(&self, cx: &mut Context<Self>) {
        let d = self.derived.clone();
        cx.background_executor().spawn(async move { store::save_derived(&d) }).detach();
    }

    /// Reads an activity's file, downloading it first if needed, and keeps what it derives.
    pub fn track(&mut self, id: i64, cx: &mut Context<Self>) -> Task<Result<(Track, Vec<Split>), String>> {
        cx.spawn(async move |this, cx| {
            let local = cx.background_executor().spawn(async move { store::load_fit(id) }).await;
            let zip = match local {
                Some(z) => Some(z),
                None => {
                    let t = Self::token(&this, cx).await?;
                    cx.background_executor().spawn(async move { garmin::download_fit(&t, id) }).await.map_err(|e| match e {
                        Fail::TooMany => "Garmin demande de ralentir : réessaie dans un moment.".to_string(),
                        Fail::Other(e) => e,
                    })?
                }
            };
            let zip = zip.ok_or("Garmin n'a pas de fichier pour cette activité (saisie à la main ?).")?;
            let (track, splits, derived) = cx
                .background_executor()
                .spawn(async move {
                    store::save_fit(id, &zip)?;
                    let track = fit::parse(&fit::unzip(&zip)?)?;
                    let splits = track.splits(1000.0);
                    let derived = fit::derive(&track);
                    Ok::<_, String>((track, splits, derived))
                })
                .await?;
            this.update(cx, |d, cx| {
                if !d.derived.contains_key(&id) {
                    Arc::make_mut(&mut d.derived).insert(id, derived);
                    d.save_derived(cx);
                    cx.notify();
                }
            })
            .ok();
            Ok((track, splits))
        })
    }

    /// The tile if it's ready; otherwise asks for it (two downloads at a time, as the tile policy asks).
    pub fn tile(&mut self, k: (u8, u32, u32), cx: &mut Context<Self>) -> Option<Arc<RenderImage>> {
        match self.tiles.get(&k) {
            Some(Tile::Ready(img)) => return Some(img.clone()),
            Some(_) => return None,
            None if self.tiles_busy >= 2 => return None,
            None => {}
        }
        // ponytail: drops every tile past 600, an LRU if memory or refetches show up.
        if self.tiles.len() > 600 {
            self.tiles.retain(|_, t| matches!(t, Tile::Loading));
        }
        self.tiles.insert(k, Tile::Loading);
        self.tiles_busy += 1;
        let template = self.settings.get("tiles").filter(|s| !s.is_empty()).cloned().unwrap_or(tiles::DEFAULT_TILES.into());
        let (bg, fg) = (rgb(self.theme.bg), rgb(self.theme.text));
        cx.spawn(async move |this, cx| {
            let img = cx
                .background_executor()
                .spawn(async move {
                    let png = tiles::tile(&template, k.0, k.1, k.2).ok()?;
                    let (w, h, px) = map::tint(&png, bg, fg)?;
                    Some(Arc::new(render_image(w, h, px)?))
                })
                .await;
            this.update(cx, |d, cx| {
                d.tiles_busy -= 1;
                d.tiles.insert(k, img.map_or(Tile::Failed, Tile::Ready));
                cx.notify();
            })
            .ok();
        })
        .detach();
        None
    }

    /// Checks the key with Garmin, then hands it to the keyring.
    pub fn save_key(&mut self, raw: &str, cx: &mut Context<Self>) -> bool {
        if self.busy() {
            return false;
        }
        let key = match secrets::clean_key(raw) {
            Ok(k) => k,
            Err(e) => {
                self.key_error = Some(e.into());
                cx.notify();
                return false;
            }
        };
        self.key_error = None;
        let key_store = self.key_store.clone();
        self.status = Status::Busy("Vérification de la clé…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn({
                    let key = key.clone();
                    async move {
                        let t = garmin::token(&key)?;
                        secrets::store_key(&key, &key_store)?;
                        Ok::<_, String>(t)
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.status = Status::Idle;
                match res {
                    Ok(t) => {
                        this.key = Some(key);
                        this.token = Some((t, Instant::now()));
                        cx.emit(DataEvent::KeyReady);
                        this.sync(cx);
                    }
                    Err(e) => this.key_error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        true
    }

    pub fn delete_key(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let res = cx.background_executor().spawn(async { secrets::delete_key() }).await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(()) => {
                        this.key = None;
                        this.token = None;
                        this.note = Some("clé retirée".into());
                    }
                    Err(e) => this.key_error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Another tile server (empty: the default one). `false` when the address can't be one.
    /// Keeps a setting on disk (a word or a list of them), for the views to read back at their start.
    pub fn set_setting(&mut self, key: &str, value: String, cx: &mut Context<Self>) {
        self.settings.insert(key.into(), value);
        let s = self.settings.clone();
        cx.background_executor().spawn(async move { store::save_settings(&s) }).detach();
        cx.notify();
    }

    pub fn save_tiles(&mut self, value: &str, cx: &mut Context<Self>) -> bool {
        let value = value.trim().to_string();
        if !value.is_empty() && !(value.starts_with("https://") && value.contains("{z}") && value.contains("{x}") && value.contains("{y}")) {
            self.note = Some("Adresse de tuiles refusée : https://…/{z}/{x}/{y}…".into());
            cx.notify();
            return false;
        }
        self.settings.insert("tiles".into(), value);
        self.tiles.clear();
        let s = self.settings.clone();
        cx.background_executor().spawn(async move { store::save_settings(&s) }).detach();
        self.note = Some("serveur de tuiles enregistré".into());
        cx.notify();
        true
    }

    /// One synchronisation, two steps: the list of activities, then their files.
    pub fn syncing(&self) -> bool {
        self.listing() || self.fetch.running
    }

    /// The first step of a synchronisation runs: the list of activities.
    pub fn listing(&self) -> bool {
        matches!(&self.status, Status::Busy(s) if s.starts_with("Synchronisation"))
    }

    /// Where the synchronisation stands, in words; `None` when nothing runs.
    pub fn sync_line(&self) -> Option<String> {
        let g = |n: usize| crate::logic::activity::group(n as i64);
        match &self.status {
            Status::Busy(s) if s.starts_with("Synchronisation") => Some(format!("1/2 · {}", s.trim_start_matches("Synchronisation : "))),
            _ if self.fetch.running => Some(format!("2/2 · fichiers des activités : {} / {}", g(self.fetch.done), g(self.fetch.total))),
            _ => None,
        }
    }

    /// Share of the files done, during the second step.
    pub fn sync_share(&self) -> Option<f32> {
        (self.fetch.running && self.fetch.total > 0).then(|| self.fetch.done as f32 / self.fetch.total as f32)
    }
}

fn rgb(c: Hsla) -> [u8; 3] {
    let r = c.to_rgb();
    [(r.r * 255.0) as u8, (r.g * 255.0) as u8, (r.b * 255.0) as u8]
}
