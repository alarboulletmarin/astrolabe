# Architecture

Astrolabe is one Rust crate and one binary, built on GPUI through
[gpui-kit](https://crates.io/crates/gpui-kit) (pinned at 0.8). This page is the map: where things
live, which way the dependencies go, how the data moves, and the rules the code keeps.

## Layers

```
src/
  main.rs      opens the window (app id dev.andrea.Astrolabe, client-side decorations)
  lang.rs      tr("English", "Français"), trf for built strings, the language setting
  logic/       pure: no gpui, no disk, no network
  platform/    what leaves the process, all blocking: Garmin, the store, the key, the tiles,
               OpenStreetMap extracts and elevation tiles
  ui/          the look shared by every view: theme, fonts, widgets, charts, date formats
  data/        `Data`, the one entity that holds the activities (and the loop planner's graph)
               and talks to platform
  views/       one entity per page, each with its own state
  app/         `Shell`: the window, the rail, the page shown, the status line, the shortcuts
```

Each layer only uses the layers above it in this list:

```
app ──► views ──► data ──► ui ──► platform ──► logic        lang: usable everywhere
```

The test `layers_only_depend_downwards` (`src/main.rs`) fails when a file breaks this rule:
`logic` and `platform` never name `gpui`, `ui` never reaches the disk, a view never sees the window.

## How the data moves

`Data` (`data/mod.rs`) is the only owner of what was synced: the activities (newest first), the
tracks derived from their files, the key, the token, the sync's progress, the settings and the
map tiles in memory. Everything else reads it.

- **Views** observe `Data` and keep a `Snapshot` (`Arc`s of the activities and derived tracks,
  and the theme), so drawing never locks or copies the list. A view asks for another page by
  emitting it (`cx.emit(Page::…)`); the shell listens, keeps the history (`logic/history.rs`)
  and shows it.
- **`Data` never changes the page.** It emits `DataEvent::NeedKey` (no key to sync with) and
  `DataEvent::KeyReady`; the shell decides what to show.
- **Blocking work** (`platform`) runs on GPUI's background executor; `Data` gets the result back
  on the UI thread and updates itself. The status line reads `Data::status`
  (`Idle`, `Busy`, `Listing`, `Failed`) and `Data::note`.

### Opening

`Data::start` reads the activity cache, the derived tracks and the key off the UI thread (the
keyring may wait for an unlock prompt). With a key, a sync starts. Tracks derived by an older
version of `logic/fit.rs` (`Derived::version < fit::VERSION`) are derived again from their files
on disk, without the network.

A missing cache is a first launch. A cache that can't be parsed is renamed
`activities.damaged-<seconds>.json`, reported in the status line, and the sync waits to be started
by hand, so the error stays visible and nothing writes over the file.

### Synchronising

1. **Token**: the API key is exchanged for a bearer token (`platform/garmin.rs`), kept in memory
   and exchanged again after 12 hours.
2. **List**: pages of 100 activities, newest first, until a page is short or reaches a known one. The merged
   list is written to the cache before it replaces the one in memory: if the write fails, the
   sync fails and nothing changes.
3. **Files**: `Data::fetch_tracks` downloads, one at a time and newest first, the file of every
   activity not derived yet, keeps it on disk (`fit/<id>.zip`), parses it and derives the track,
   best efforts, altitude profile and time per heart rate band (`logic/fit.rs`). It stops on
   Garmin's *too many requests*, after five failures in a row, or when asked; the next sync picks
   up the rest. An activity without a file (entered by hand) gets an empty entry, so it isn't
   asked again.

The activity page reads a file from disk, or downloads it if the queue hasn't reached it yet
(`Data::track`).

### Planning loops

`Data::loops` (`data/loops.rs`) holds the loop planner's state: the region, its walking graph once
loaded, the job under way, and the loops last planned.

1. **Opening the page** (`Data::open_loops`, once): the graph is read from disk off the UI thread.
   - A graph from another `graph::VERSION` is built again from the extract kept on disk.
   - A damaged graph is set aside as `<region>.damaged-<seconds>.graph`, reported, and built again.
2. **Download** (`Data::download_region`): the Geofabrik extract is streamed straight to disk
   (`platform/osm.rs`, `store::write_atomic_from`). A cut download leaves the previous file
   untouched.
3. **Build** (`Data::build_graph`), on the background executor, with all cores but one:
   - three passes over the extract (`logic/osm.rs` decodes, `logic/graph.rs` builds): the ways one
     can run on and their nodes, those nodes' positions, then the ways cut at the junctions, with
     nodes of degree 2 contracted;
   - the elevation tiles not cached yet are fetched (`platform/elevation.rs`);
   - climb and descent are read along each edge (`logic/elevation.rs`);
   - the graph is written with `store::save_graph`.

   The status line reads `Data::loops_line`. An extract that can't be decoded is set aside as
   `<region>.damaged-<seconds>.osm.pbf`.
4. **Planning** (`Data::plan_loops`, `logic/loops.rs`):
   - a local graph around the start (`logic/route.rs`): the part reachable within half the longest
     loop, the start cut into its edge;
   - attempts on worker threads: waypoints on a polygon, A* legs, the polygon resized until the
     length fits;
   - the loops kept: different from each other, best first, the seed's own loop first.

   A newer request makes an older one's result moot.

## On disk

| Folder (Linux) | File | What |
|---|---|---|
| `~/.local/share/astrolabe` | `activities.json` | the activity list, as Garmin gives it |
| | `derived.json` | per activity: simplified track, best efforts, profile, heart rate bands, version |
| | `fit/<id>.zip` | the original files |
| | `settings` | `key=value` lines: tiles, theme, language, journal columns |
| | `key.enc` | the API key, sealed, when the system has no keyring |
| `~/.config/astrolabe` | `master.key` | the key that opens `key.enc` |
| | `osm/<region>.osm.pbf` | the region's OpenStreetMap extract, as Geofabrik gives it |
| | `osm/<region>.graph` | its walking graph: nodes, edges with length, climb, descent and class, geometry; `graph::VERSION` |
| `~/.cache/astrolabe` | `tiles/` | map tiles, by server |
| | `elevation/terrarium/<z>/<x>/<y>.png` | elevation tiles (Terrarium) |

macOS uses `~/Library/Application Support` and `~/Library/Caches`, Windows the local and roaming
`AppData` folders; the `XDG_*` variables win everywhere (`platform/store.rs`).

Every write goes through `store::write_atomic` (a temporary file, synced, then renamed), or
`store::write_atomic_from` for what is streamed (an extract); health data are created readable by the
user alone. Nothing is deleted.

The graph file is little-endian:

- `ASTRLOOP`, `VERSION`;
- the extract's size and cut time, and how many positions lack an altitude;
- the counts of nodes, edges and points;
- the arrays: node positions (1e-7°), altitudes (u16 decimetres from −500 m), components, adjacency,
  edges, points;
- an FNV-1a checksum over 8-byte words.

`Graph::decode` checks every size and index, and tells a file from another version (`Stale`) from a
damaged one. A write made in the background
(`Data::persist`) reports its failure in the status line.

The API key goes to the system keyring (`platform/secrets.rs`); without one, it is sealed with
XChaCha20-Poly1305 in `key.enc`, opened by `master.key` (`platform/vault.rs`), both `0600`. It is
never written in the clear or logged.

## Tests

`cargo test` needs no display, no network and no Garmin account.

| Kind | Where | What it proves |
|---|---|---|
| Pure | `#[cfg(test)]` at the bottom of each `logic/` file | figures, FIT parsing on `tests/fixtures`, records, columns, dates; PBF decoding (damaged input included), the graph built from `tests/fixtures/loops.osm.pbf` (a made-up town written by `scripts/osm-fixture.py`), A*, loops |
| Window | `app/tests.rs` | the whole window on GPUI's test platform: clicks and keys in, the page shown out, with activities made up in memory; a loop planned from a click and saved through the simulated save dialog |
| Flows | `data/tests.rs` | the real chain, HTTP client to files and back, against a local stand-in for Garmin that each test scripts: sync then reopen offline, 429 on the list or the files and resuming, a cut connection, a damaged file, a disk that refuses writes, an older version's files; the same for Geofabrik and the elevation tiles: download, build, plan, reopen offline, a damaged or older graph, a damaged extract, no elevation server |
| Rules | `main.rs` | the layers; no French text outside `tr` |

Each flow test points the store at a folder of its own (`store::use_root`) and gives `Data` its
key directly (`Data::open_with`): the tests never touch the user's files or keyring.

## Where to look

| To change… | Go to |
|---|---|
| the activity model, sports, totals, the statistics' queries | `logic/activity.rs` |
| the journal's rows, search, columns, gap to the usual pace, heart rate zones | `logic/journal.rs`, `logic/columns.rs` |
| the home page's figures (median week, streak, year to date) | `logic/dashboard.rs` |
| reading FIT files and what is derived from them | `logic/fit.rs` (bump `VERSION` when a derived figure changes) |
| records and podiums | `logic/records.rs` |
| form: load, fatigue, freshness | `logic/form.rs` |
| map projection, tiles, heat raster | `logic/map.rs`; the widget: `views/map.rs` |
| reading OpenStreetMap extracts, which ways are kept | `logic/osm.rs` |
| the walking graph, its file format | `logic/graph.rs` (bump `VERSION` when what it keeps changes) |
| altitude, climb, the relief | `logic/elevation.rs`, `logic/relief.rs` |
| the cost of a way, A* | `logic/route.rs` |
| loop generation, its constants (detour, tolerance, overlap) | `logic/loops.rs` |
| regions offered | `logic/regions.rs` |
| downloading extracts and elevation tiles | `platform/osm.rs`, `platform/elevation.rs` |
| the loop planner's jobs | `data/loops.rs`; the page: `views/loops.rs` |
| what the end of a sync says | `logic/sync.rs` |
| Garmin's endpoints, HTTP errors | `platform/garmin.rs`, `platform/mod.rs` |
| where files go, atomic writes | `platform/store.rs` |
| the sync, the download queue, settings | `data/mod.rs` |
| colors, fonts, shared widgets | `ui/theme.rs`, `ui/mod.rs` |
| a page | `views/<page>.rs` |
| the rail, the status line, shortcuts, back and forward | `app/mod.rs` |

## Rules

- **gpui-kit is pinned** and its API moves: check a signature in its source for that version.
  On Linux, `gpui_kit::open_window` already draws the window frame (shadow, border, resizing).
- **No new dependency** without agreement in an issue.
- **Text**: code, comments and docs in English; the interface goes through `tr`.
- **A deliberate shortcut** carries a `// ponytail:` comment naming its limit and what comes next.
