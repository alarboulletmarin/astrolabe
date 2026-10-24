//! The window: state, background work (keyring, sync, tracks, tiles, heat), the rail and the
//! status line. Each view is an `impl Home` of its own file.

use crate::logic::activity::{Activity, Group, Measure, Query, Sport, week_start};
use crate::logic::fit::{Derived, Split, Track};
use crate::logic::journal::Row;
use crate::logic::map;
use crate::platform::{garmin::{self, Fail}, secrets, store, tiles};
use crate::ui::icon;
use crate::ui::theme::{MONO, SANS, Theme};
use chrono::{Datelike, Days, Local, NaiveDate};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Theme as KitTheme, ThemeMode,
    input::{InputEvent, InputState},
    date_picker::{DatePickerEvent, DatePickerState},
    tooltip::Tooltip,
};
use gpui_kit::{prelude::FluentBuilder, *};
use std::{
    cell::Cell,
    collections::HashMap,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

actions!(astrolabe, [SelectNext, SelectPrev, OpenSelected, Back, Older, Newer]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, Some("Journal")),
        KeyBinding::new("up", SelectPrev, Some("Journal")),
        KeyBinding::new("enter", OpenSelected, Some("Journal")),
        KeyBinding::new("escape", Back, Some("Activity")),
        KeyBinding::new("left", Older, Some("Activity")),
        KeyBinding::new("right", Newer, Some("Activity")),
    ]);
}

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Journal,
    Activity(i64),
    Stats,
    Records,
    Heatmap,
    Settings,
}

pub enum Status {
    Idle,
    Busy(String),
    Failed(String),
}

/// A map's state: what it shows, where it is on screen, the drag in progress.
pub struct MapState {
    pub view: map::View,
    pub bounds: Rc<Cell<Bounds<Pixels>>>,
    pub drag: Option<(Point<Pixels>, map::View)>,
    /// What the view was last fitted to; a new subject fits again.
    pub fitted: Option<u64>,
}

impl MapState {
    fn new() -> Self {
        MapState { view: map::View::default(), bounds: Rc::new(Cell::new(Bounds::default())), drag: None, fitted: None }
    }
}

pub enum Tile {
    Loading,
    Ready(Arc<RenderImage>),
    Failed,
}

#[derive(Default)]
pub struct Heat {
    /// The raster and the view it was drawn for.
    pub image: Option<(Arc<RenderImage>, map::View, (f64, f64))>,
    pub busy: bool,
    pub drawn: Option<u64>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Chart {
    Bars,
    Cumul,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Compare {
    None,
    Previous,
    Year,
}

/// The menus of the stats sentence.
#[derive(Clone, Copy, PartialEq)]
pub enum Menu {
    Measure,
    Group,
    Period,
    Sports,
    Compare,
}

pub struct StatsState {
    pub query: Query,
    pub chart: Chart,
    pub compare: Compare,
    /// Index of the preset in use, `None` once the range was set by hand.
    pub preset: Option<usize>,
    pub hover: Option<usize>,
    pub pick: Option<usize>,
    pub cal_hover: Option<usize>,
    pub range: Entity<DatePickerState>,
    /// Dates picked by hand: the sentence shows the date picker instead of the period word.
    pub custom: bool,
    pub menu: Option<Menu>,
    /// Which menu a press outside just closed, and when.
    pub closed: Option<(Menu, Instant)>,
}

pub struct Loaded {
    pub id: i64,
    pub track: Arc<Track>,
    pub splits: Vec<Split>,
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

pub enum TrackState {
    None,
    Loading(i64),
    Failed(i64, String),
}

pub struct Home {
    pub view: View,
    pub acts: Vec<Activity>,
    pub key: Option<String>,
    pub key_loaded: bool,
    token: Option<(String, Instant)>,
    pub status: Status,
    pub note: Option<String>,
    pub key_input: Entity<InputState>,
    pub key_error: Option<String>,
    pub tiles_input: Entity<InputState>,
    pub settings: HashMap<String, String>,
    pub theme: Theme,
    pub focus: FocusHandle,

    // Journal
    pub sport: Option<Sport>,
    pub search: Entity<InputState>,
    pub rows: Vec<Row>,
    pub selected: Option<i64>,
    pub list: UniformListScrollHandle,

    // Tracks
    pub derived: Arc<HashMap<i64, Derived>>,
    pub fetch: Fetch,
    pub track: Option<Loaded>,
    pub track_state: TrackState,
    pub act_hover: Option<usize>,
    pub hist_hover: Option<i64>,
    pub act_map: MapState,
    /// A click (a press and release without moving) on a map, in map pixels.
    pub map_click: Option<(crate::views::map::Which, (f64, f64))>,
    /// When the wheel last tried to zoom a map without Ctrl: the hint shows a moment.
    pub map_hint: Option<Instant>,

    // Stats, records, heat map
    pub stats: StatsState,
    pub record_year: Option<i32>,
    pub heat_map: MapState,
    pub heat_sports: Vec<Sport>,
    pub heat_year: Option<i32>,
    pub heat: Heat,
    pub heat_pick: Option<(Point<Pixels>, Vec<i64>)>,

    pub tiles: HashMap<(u8, u32, u32), Tile>,
    tiles_busy: usize,
    /// Plot areas, by name, as laid out at the last frame: hover reads positions against them.
    pub plots: HashMap<&'static str, Rc<Cell<Bounds<Pixels>>>>,
    /// Width the views really get, measured at the last frame: the window's frame and shadow
    /// (drawn by gpui-kit on Linux) make the viewport a poor guess.
    content_w: Rc<Cell<f32>>,
    _subscriptions: Vec<Subscription>,
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

impl Home {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let key_input = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("GCDC_…"));
        let settings = store::load_settings();
        let tiles_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(tiles::DEFAULT_TILES)
                .default_value(settings.get("tiles").cloned().unwrap_or_default())
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Chercher une activité"));
        let today = today();
        let range = cx.new(|cx| {
            let mut s = DatePickerState::range(window, cx);
            s.set_date((today.with_ordinal(1).unwrap(), today), window, cx);
            s
        });
        let mut subs = vec![
            cx.observe_window_appearance(window, |this, window, cx| this.apply_appearance(window, cx)),
            cx.subscribe(&search, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.refresh(cx);
                    cx.notify();
                }
            }),
        ];
        subs.push(cx.subscribe(&range, |this, _, e: &DatePickerEvent, cx| {
            let DatePickerEvent::Change(v) = e;
            if let gpui_kit::component::calendar::Date::Range(Some(a), Some(b)) = v.date() {
                this.stats.query.from = a.min(b);
                this.stats.query.to = a.max(b);
                this.stats.preset = None;
                this.stats.pick = None;
                cx.notify();
            }
        }));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut home = Home {
            view: View::Journal,
            acts: Vec::new(),
            key: None,
            key_loaded: false,
            token: None,
            status: Status::Busy("Ouverture…".into()),
            note: None,
            key_input,
            key_error: None,
            tiles_input,
            settings,
            theme: Theme::for_appearance(window.appearance()),
            focus,
            sport: None,
            search,
            rows: Vec::new(),
            selected: None,
            list: UniformListScrollHandle::new(),
            derived: Arc::new(HashMap::new()),
            fetch: Fetch::default(),
            track: None,
            track_state: TrackState::None,
            act_hover: None,
            hist_hover: None,
            act_map: MapState::new(),
            map_click: None,
            map_hint: None,
            stats: StatsState {
                query: Query { measure: Measure::Distance, group: Group::Week, from: today.with_ordinal(1).unwrap(), to: today, sports: vec![] },
                chart: Chart::Bars,
                compare: Compare::None,
                preset: Some(4),
                hover: None,
                pick: None,
                cal_hover: None,
                range,
                custom: false,
                menu: None,
                closed: None,
            },
            record_year: None,
            heat_map: MapState::new(),
            heat_sports: vec![],
            heat_year: None,
            heat: Heat::default(),
            heat_pick: None,
            tiles: HashMap::new(),
            tiles_busy: 0,
            plots: HashMap::new(),
            content_w: Rc::new(Cell::new(0.0)),
            _subscriptions: subs,
        };
        home.apply_appearance(window, cx);
        home.start(cx);
        home
    }

    fn apply_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let theme = Theme::for_appearance(window.appearance());
        if theme.dark != self.theme.dark {
            // Tiles and heat are drawn in the theme's colors.
            self.tiles.clear();
            self.heat.drawn = None;
        }
        self.theme = theme;
        KitTheme::change(if theme.dark { ThemeMode::Dark } else { ThemeMode::Light }, Some(window), cx);
        cx.notify();
    }

    pub fn plot(&mut self, name: &'static str) -> Rc<Cell<Bounds<Pixels>>> {
        self.plots.entry(name).or_insert_with(|| Rc::new(Cell::new(Bounds::default()))).clone()
    }

    // --- Navigation

    pub fn set_view(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        self.view = view;
        if let View::Activity(id) = view {
            self.selected = Some(id);
            self.act_hover = None;
            self.load_track(id, cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn open(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.set_view(View::Activity(id), window, cx);
    }

    /// The activity before or after the shown one, in time.
    pub fn step_activity(&mut self, older: bool, window: &mut Window, cx: &mut Context<Self>) {
        let View::Activity(id) = self.view else { return };
        let Some(i) = self.acts.iter().position(|a| a.activity_id == id) else { return };
        let j = if older { i + 1 } else { i.wrapping_sub(1) };
        if let Some(a) = self.acts.get(j) {
            self.open(a.activity_id, window, cx);
        }
    }

    pub fn set_sport(&mut self, sport: Option<Sport>, cx: &mut Context<Self>) {
        self.sport = sport;
        self.refresh(cx);
    }

    pub fn index_of(&self, id: i64) -> Option<usize> {
        self.acts.iter().position(|a| a.activity_id == id)
    }

    pub fn select(&mut self, id: i64) {
        self.selected = Some(id);
        if let Some(row) = self.index_of(id).and_then(|ix| self.rows.iter().position(|r| *r == Row::Act(ix))) {
            self.list.scroll_to_item(row, ScrollStrategy::Center);
        }
    }

    pub fn activity(&self, id: i64) -> Option<&Activity> {
        self.acts.iter().find(|a| a.activity_id == id)
    }

    /// Rebuilds the journal rows after the data, the filter or the search changed.
    pub fn refresh(&mut self, cx: &App) {
        let needle = crate::logic::journal::fold(&self.search.read(cx).value());
        self.rows = crate::logic::journal::rows(&self.acts, self.sport, &needle);
    }

    pub fn select_step(&mut self, step: isize, cx: &mut Context<Self>) {
        let listed: Vec<usize> = self.rows.iter().filter_map(|r| if let Row::Act(i) = r { Some(*i) } else { None }).collect();
        let at = self.selected.and_then(|id| listed.iter().position(|i| self.acts[*i].activity_id == id));
        let next = match at {
            Some(p) => p.saturating_add_signed(step).min(listed.len().saturating_sub(1)),
            None => 0,
        };
        if let Some(i) = listed.get(next) {
            self.select(self.acts[*i].activity_id);
            cx.notify();
        }
    }

    // --- Background work

    /// Cache, derived tracks and keyring are read off the UI thread (the keyring may wait for an unlock prompt).
    fn start(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let (acts, derived, key) = cx
                .background_executor()
                .spawn(async { (store::load_cache(), store::load_derived(), secrets::load_key()) })
                .await;
            this.update(cx, |this, cx| {
                this.acts = acts;
                this.derived = Arc::new(derived);
                this.refresh(cx);
                this.key_loaded = true;
                this.status = Status::Idle;
                match key {
                    Ok(Some(k)) => {
                        this.key = Some(k);
                        this.sync(cx);
                    }
                    Ok(None) if this.acts.is_empty() => this.view = View::Settings,
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

    /// A bearer token, exchanged again after 12 h (they last 24).
    async fn token(this: &WeakEntity<Self>, cx: &mut AsyncApp) -> Result<String, String> {
        let (cached, key) = this.read_with(cx, |h, _| (h.token.clone(), h.key.clone())).map_err(|e| e.to_string())?;
        if let Some((t, at)) = cached.filter(|(_, at)| at.elapsed() < Duration::from_secs(12 * 3600)) {
            let _ = at;
            return Ok(t);
        }
        let key = key.ok_or("Aucune clé API : ajoute-la dans les réglages.")?;
        let t = cx.background_executor().spawn(async move { garmin::token(&key) }).await?;
        this.update(cx, |h, _| h.token = Some((t.clone(), Instant::now()))).ok();
        Ok(t)
    }

    pub fn sync(&mut self, cx: &mut Context<Self>) {
        if self.key.is_none() {
            self.view = View::Settings;
            cx.notify();
            return;
        }
        if self.busy() {
            return;
        }
        self.status = Status::Busy("Synchronisation : connexion à Garmin…".into());
        cx.notify();
        let known = self.acts.clone();
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // Live count while the pages arrive.
        let counter = seen.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(150)).await;
                let n = counter.load(std::sync::atomic::Ordering::Relaxed);
                let going = this
                    .update(cx, |h, cx| {
                        if !matches!(&h.status, Status::Busy(s) if s.starts_with("Synchronisation")) {
                            return false;
                        }
                        if n > 0 {
                            h.status = Status::Busy(format!("Synchronisation : {} activités reçues…", crate::logic::activity::group(n as i64)));
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
                        this.acts = all;
                        this.refresh(cx);
                        let at = Local::now().format("%H:%M");
                        this.note = Some(match added {
                            0 => format!("à jour à {at}"),
                            1 => format!("1 nouvelle à {at}"),
                            n => format!("{n} nouvelles à {at}"),
                        });
                        this.fetch_tracks(cx);
                        Status::Idle
                    }
                    Err(e) => Status::Failed(e),
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
                    .update(cx, |h, cx| {
                        h.fetch.current = Some(label.clone());
                        cx.notify();
                        !h.fetch.cancel && h.key.is_some()
                    })
                    .unwrap_or(false);
                if !go {
                    this.update(cx, |h, _| h.fetch.stopped = Some("Arrêté à ta demande.".into())).ok();
                    break;
                }
                let token = match Self::token(&this, cx).await {
                    Ok(t) => t,
                    Err(e) => {
                        this.update(cx, |h, _| h.fetch.stopped = Some(e)).ok();
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
                                let track = crate::logic::fit::unzip(&zip).and_then(|f| crate::logic::fit::parse(&f)).map_err(Fail::Other)?;
                                Ok(crate::logic::fit::derive(&track))
                            }
                        }
                    })
                    .await;
                let stop = this
                    .update(cx, |h, cx| {
                        h.fetch.done = n + 1;
                        let stop = match res {
                            Ok(d) => {
                                Arc::make_mut(&mut h.derived).insert(id, d);
                                in_a_row = 0;
                                false
                            }
                            Err(Fail::TooMany) => {
                                h.fetch.stopped = Some("Garmin demande de ralentir : la suite viendra à la prochaine synchronisation.".into());
                                true
                            }
                            Err(Fail::Other(e)) => {
                                h.fetch.failed += 1;
                                in_a_row += 1;
                                h.fetch.error = Some(format!("« {label} » : {e}"));
                                if in_a_row >= 5 {
                                    h.fetch.stopped = Some(format!("Arrêté après 5 échecs de suite. Dernier : {e}"));
                                }
                                in_a_row >= 5
                            }
                        };
                        if (n + 1) % 25 == 0 {
                            h.save_derived(cx);
                        }
                        h.heat.drawn = None;
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
            this.update(cx, |h, cx| {
                h.fetch.running = false;
                h.fetch.cancel = false;
                h.fetch.current = None;
                h.save_derived(cx);
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

    /// Reads the activity's file (downloading it first if needed) for the activity page.
    pub fn load_track(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.track.as_ref().is_some_and(|t| t.id == id) || matches!(self.track_state, TrackState::Loading(x) if x == id) {
            return;
        }
        self.track = None;
        self.track_state = TrackState::Loading(id);
        cx.spawn(async move |this, cx| {
            let local = cx.background_executor().spawn(async move { store::load_fit(id) }).await;
            let zip = match local {
                Some(z) => Ok(Some(z)),
                None => match Self::token(&this, cx).await {
                    Ok(t) => cx.background_executor().spawn(async move { garmin::download_fit(&t, id) }).await.map_err(|e| match e {
                        Fail::TooMany => "Garmin demande de ralentir : réessaie dans un moment.".to_string(),
                        Fail::Other(e) => e,
                    }),
                    Err(e) => Err(e),
                },
            };
            let res = match zip {
                Err(e) => Err(e),
                Ok(None) => Err("Garmin n'a pas de fichier pour cette activité (saisie à la main ?).".into()),
                Ok(Some(zip)) => {
                    cx.background_executor()
                        .spawn(async move {
                            store::save_fit(id, &zip)?;
                            let track = crate::logic::fit::parse(&crate::logic::fit::unzip(&zip)?)?;
                            let splits = track.splits(1000.0);
                            let derived = crate::logic::fit::derive(&track);
                            Ok::<_, String>((track, splits, derived))
                        })
                        .await
                }
            };
            this.update(cx, |h, cx| {
                if !matches!(h.track_state, TrackState::Loading(x) if x == id) {
                    return;
                }
                match res {
                    Ok((track, splits, derived)) => {
                        if !h.derived.contains_key(&id) {
                            Arc::make_mut(&mut h.derived).insert(id, derived);
                            h.save_derived(cx);
                        }
                        h.track = Some(Loaded { id, track: Arc::new(track), splits });
                        h.track_state = TrackState::None;
                    }
                    Err(e) => h.track_state = TrackState::Failed(id, e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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
        let rgb = |c: Hsla| {
            let r = c.to_rgb();
            [(r.r * 255.0) as u8, (r.g * 255.0) as u8, (r.b * 255.0) as u8]
        };
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
            this.update(cx, |h, cx| {
                h.tiles_busy -= 1;
                h.tiles.insert(k, img.map_or(Tile::Failed, Tile::Ready));
                cx.notify();
            })
            .ok();
        })
        .detach();
        None
    }

    /// Lines of the heat map, as filtered.
    pub fn heat_ids(&self) -> Vec<i64> {
        self.acts
            .iter()
            .filter(|a| self.heat_sports.is_empty() || self.heat_sports.contains(&a.sport()))
            .filter(|a| self.heat_year.is_none_or(|y| a.date().is_some_and(|d| d.year() == y)))
            .filter(|a| self.derived.get(&a.activity_id).is_some_and(|d| !d.line.is_empty()))
            .map(|a| a.activity_id)
            .collect()
    }

    /// Redraws the heat raster when what it shows changed; one drawing at a time, the latest wins.
    pub fn ensure_heat(&mut self, size: (f64, f64), scale: f32, cx: &mut Context<Self>) {
        let ids = self.heat_ids();
        let view = self.heat_map.view;
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (view.cx.to_bits(), view.cy.to_bits(), view.z.to_bits(), size.0.to_bits(), size.1.to_bits(), self.theme.dark).hash(&mut h);
            ids.hash(&mut h);
            h.finish()
        };
        if self.heat.drawn == Some(key) || self.heat.busy || size.0 < 8.0 || size.1 < 8.0 {
            return;
        }
        self.heat.busy = true;
        let derived = self.derived.clone();
        let (w, h) = ((size.0 * scale as f64) as usize, (size.1 * scale as f64) as usize);
        let scaled = map::View { cx: view.cx * scale as f64, cy: view.cy * scale as f64, z: view.z + (scale as f64).log2() };
        let dark = self.theme.dark;
        let accent = self.theme.accent.to_rgb();
        let a = [(accent.r * 255.0) as u8, (accent.g * 255.0) as u8, (accent.b * 255.0) as u8];
        // Light theme: pale where one passage, deep where the most; dark theme: the reverse reads.
        let shade = |k: f32| a.map(|c| (c as f32 * k).min(255.0) as u8);
        let lift = |k: f32| a.map(|c| (c as f32 + (255.0 - c as f32) * k) as u8);
        let (light, deep) = if dark { (shade(0.55), lift(0.55)) } else { (lift(0.55), shade(0.45)) };
        cx.spawn(async move |this, cx| {
            let img = cx
                .background_executor()
                .spawn(async move {
                    let lines: Vec<&[[f32; 2]]> = ids.iter().filter_map(|id| derived.get(id)).map(|d| d.line.as_slice()).collect();
                    let counts = map::heat(&lines, &scaled, w, h);
                    render_image(w as u32, h as u32, map::colorize(&counts, light, deep)).map(Arc::new)
                })
                .await;
            this.update(cx, |h, cx| {
                h.heat.busy = false;
                h.heat.drawn = Some(key);
                if let Some(img) = img {
                    h.heat.image = Some((img, view, size));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn save_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let key = match secrets::clean_key(&self.key_input.read(cx).value()) {
            Ok(k) => k,
            Err(e) => {
                self.key_error = Some(e.into());
                cx.notify();
                return;
            }
        };
        self.key_error = None;
        self.key_input.update(cx, |s, cx| s.set_value("", window, cx));
        self.status = Status::Busy("Vérification de la clé…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn({
                    let key = key.clone();
                    async move {
                        let t = garmin::token(&key)?;
                        secrets::store_key(&key)?;
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
                        this.view = View::Journal;
                        this.sync(cx);
                    }
                    Err(e) => this.key_error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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
                        this.note = Some("clé retirée du trousseau".into());
                    }
                    Err(e) => this.key_error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn save_tiles(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        let value = value.trim().to_string();
        if !value.is_empty() && !(value.starts_with("https://") && value.contains("{z}") && value.contains("{x}") && value.contains("{y}")) {
            self.note = Some("Adresse de tuiles refusée : https://…/{z}/{x}/{y}…".into());
            cx.notify();
            return;
        }
        if value.is_empty() {
            self.tiles_input.update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.settings.insert("tiles".into(), value);
        self.tiles.clear();
        let s = self.settings.clone();
        cx.background_executor().spawn(async move { store::save_settings(&s) }).detach();
        self.note = Some("serveur de tuiles enregistré".into());
        cx.notify();
    }

    // --- Chrome

    fn rail(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let current = match self.view {
            View::Activity(_) => View::Journal,
            v => v,
        };
        let item = |id: &'static str, view: View, name: IconName, tip: &'static str, cx: &mut Context<Self>| {
            let on = current == view;
            div()
                .id(id)
                .size(px(30.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(on, |d| d.bg(t.selection))
                .hover(|d| d.bg(t.border))
                .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                .on_click(cx.listener(move |this, _, window, cx| this.set_view(view, window, cx)))
                .child(icon(name, if on { t.accent } else { t.soft }, 16.))
        };
        let syncing = self.syncing();
        let spin = icon(IconName::RefreshCw, if syncing { t.accent } else if self.key.is_some() { t.soft } else { t.dim }, 15.);
        let spin = if syncing {
            spin.with_animation("sync-spin", Animation::new(Duration::from_millis(1100)).repeat(), |i, d| {
                i.transform(Transformation::rotate(percentage(d)))
            })
            .into_any_element()
        } else {
            spin.into_any_element()
        };
        let tip: SharedString = self.sync_line().unwrap_or_else(|| "Synchroniser avec Garmin".into()).into();
        let percent = self.sync_share().map(|f| format!("{:.0}%", f * 100.0));
        div()
            .w(px(46.))
            .flex_none()
            .h_full()
            .bg(t.panel)
            .border_r_1()
            .border_color(t.border)
            .flex()
            .flex_col()
            .items_center()
            .py(px(12.))
            .gap(px(6.))
            .child(crate::ui::drag_window(div().id("rail-logo").w_full().flex().justify_center().pb(px(10.)).font_family(MONO).font_weight(FontWeight::BOLD).text_size(px(15.)).text_color(t.accent).child("m.")))
            .child(item("rail-journal", View::Journal, IconName::NotebookText, "Journal", cx))
            .child(item("rail-stats", View::Stats, IconName::ChartColumn, "Statistiques", cx))
            .child(item("rail-records", View::Records, IconName::Trophy, "Records", cx))
            .child(item("rail-map", View::Heatmap, IconName::Map, "Carte de chaleur", cx))
            // The empty part of the rail moves the window, as in Bref.
            .child(crate::ui::drag_window(div().id("rail-drag").flex_1().w_full()))
            .child(
                div()
                    .id("rail-sync")
                    .w(px(34.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(3.))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.border))
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    // While it runs, a click shows where it is; otherwise it starts one.
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.syncing() {
                            this.set_view(View::Settings, window, cx);
                        } else {
                            this.sync(cx);
                        }
                    }))
                    .child(spin)
                    .when_some(percent, |d, p| d.child(div().font_family(MONO).text_size(px(9.)).text_color(t.accent).child(p))),
            )
            .child(item("rail-settings", View::Settings, IconName::Settings, "Réglages", cx))
    }

    /// One synchronisation, two steps: the list of activities, then their files.
    pub fn syncing(&self) -> bool {
        matches!(&self.status, Status::Busy(s) if s.starts_with("Synchronisation")) || self.fetch.running
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

    /// Bottom-right, like Bref's word count: what the app is doing, errors in red. A click shows
    /// the synchronisation in the settings.
    fn status_line(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = self.theme;
        let n = crate::logic::activity::group(self.acts.len() as i64);
        let s = if self.acts.len() > 1 { "s" } else { "" };
        let (text, color) = if let Status::Failed(e) = &self.status {
            (e.clone(), t.bad())
        } else if let Some(line) = self.sync_line() {
            (format!("Synchronisation {line}"), t.soft)
        } else if let Status::Busy(s) = &self.status {
            (s.clone(), t.soft)
        } else if let Some(why) = &self.fetch.stopped {
            (format!("{n} activité{s} · synchronisation interrompue : {why}"), t.bad())
        } else {
            (format!("{n} activité{s}{}", self.note.as_ref().map(|x| format!(" · {x}")).unwrap_or_default()), t.dim)
        };
        let share = self.sync_share();
        div()
            .id("status")
            .absolute()
            .bottom(px(6.))
            .right(px(14.))
            .max_w(px(620.))
            .px(px(6.))
            .py(px(1.))
            .rounded(px(6.))
            .bg(t.bg)
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(color)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| {
                if matches!(this.status, Status::Failed(_)) {
                    this.status = Status::Idle;
                }
                this.set_view(View::Settings, window, cx);
            }))
            .when_some(share, |d, f| {
                d.child(div().w(px(70.)).h(px(4.)).rounded(px(2.)).bg(t.border).child(div().h_full().rounded(px(2.)).bg(t.accent).w(relative(f))))
            })
            .child(div().min_w_0().child(text))
    }
}

/// BGRA pixels into an image GPUI can draw.
pub fn render_image(w: u32, h: u32, bgra: Vec<u8>) -> Option<RenderImage> {
    let buf = image::RgbaImage::from_raw(w, h, bgra)?;
    Some(RenderImage::new(vec![image::Frame::new(buf)]))
}

/// Monday of this week, and the presets of the stats range.
pub fn presets(today: NaiveDate) -> [(&'static str, NaiveDate, NaiveDate); 6] {
    let months = |n: u32| today.checked_sub_months(chrono::Months::new(n)).unwrap() + Days::new(1);
    [
        ("Les 4 dernières semaines", week_start(today) - Days::new(21), today),
        ("Les 3 derniers mois", months(3), today),
        ("Les 6 derniers mois", months(6), today),
        ("Les 12 derniers mois", months(12), today),
        ("Cette année", today.with_ordinal(1).unwrap(), today),
        ("Depuis le début", NaiveDate::from_ymd_opt(2000, 1, 1).unwrap(), today),
    ]
}

impl Home {
    /// Minimise, maximise, close: a floating pill at the top right, which also moves the window.
    fn window_controls(&self, window: &Window) -> Div {
        let t = self.theme;
        let controls = window.window_controls();
        let button = |id: &'static str, name: IconName| {
            div()
                .id(id)
                .size(px(24.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|d| d.bg(t.border))
                .child(icon(name, t.soft, 14.))
        };
        crate::ui::drag_window(
            div()
                .absolute()
                .top(px(8.))
                .right(px(8.))
                .flex()
                .items_center()
                .gap(px(2.))
                .p(px(3.))
                .rounded(px(10.))
                .bg(t.panel)
                .border_1()
                .border_color(t.border)
                .shadow(vec![BoxShadow { color: hsla(0., 0., 0., 0.18), offset: point(px(0.), px(1.)), blur_radius: px(6.), spread_radius: px(0.), inset: false }])
                .occlude(),
        )
        .when(controls.minimize, |d| d.child(button("win-min", IconName::Minus).on_click(|_, window, _| window.minimize_window())))
        .when(controls.maximize, |d| {
            let name = if window.is_maximized() { IconName::Copy } else { IconName::Square };
            d.child(button("win-max", name).on_click(|_, window, _| window.zoom_window()))
        })
        .child(button("win-close", IconName::X).on_click(|_, window, _| window.remove_window()))
    }
}

impl Render for Home {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        // On Linux gpui-kit draws the frame (shadow, border, resize edges) and the app adds the
        // window buttons. Elsewhere the system does it all.
        let client = matches!(window.window_decorations(), Decorations::Client { .. });
        crate::ui::set_top(if client { 52.0 } else { 28.0 });
        let measured = self.content_w.get();
        let width = if measured > 0.0 { measured } else { window.viewport_size().width / px(1.) - 46.0 };
        let body = match self.view {
            View::Journal => self.journal(width, cx).into_any_element(),
            View::Activity(id) => self.activity_page(id, width, window, cx).into_any_element(),
            View::Stats => self.stats(width, window, cx).into_any_element(),
            View::Records => self.records(width, cx).into_any_element(),
            View::Heatmap => self.heatmap(window, cx).into_any_element(),
            View::Settings => self.settings(width, cx).into_any_element(),
        };
        div()
            .size_full()
            .relative()
            .flex()
            .overflow_hidden()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(SANS)
            .text_size(px(13.))
            .track_focus(&self.focus)
            .key_context(match self.view {
                View::Journal => "Journal",
                View::Activity(_) => "Activity",
                _ => "Home",
            })
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.select_step(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.select_step(-1, cx)))
            .on_action(cx.listener(|this, _: &OpenSelected, window, cx| {
                if let Some(id) = this.selected {
                    this.open(id, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Back, window, cx| this.set_view(View::Journal, window, cx)))
            .on_action(cx.listener(|this, _: &Older, window, cx| this.step_activity(true, window, cx)))
            .on_action(cx.listener(|this, _: &Newer, window, cx| this.step_activity(false, window, cx)))
            .child(self.rail(cx))
            .child({
                let cell = self.content_w.clone();
                div()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .relative()
                    .child(
                        canvas(
                            move |b, window, _| {
                                let w = b.size.width / px(1.);
                                // A new width lays the views out again at the next frame.
                                if (cell.get() - w).abs() > 0.5 {
                                    cell.set(w);
                                    window.refresh();
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(body)
            })
            .child(self.status_line(cx))
            .when(client, |d| d.child(self.window_controls(window)))
            // Alt + drag, anywhere: moves the window.
            .when(client, |d| {
                d.capture_any_mouse_down(|e, window, cx| {
                    if e.button == MouseButton::Left && e.modifiers.alt {
                        window.start_window_move();
                        cx.stop_propagation();
                    }
                })
            })
    }
}
