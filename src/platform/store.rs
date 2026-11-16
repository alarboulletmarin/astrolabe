//! The local store: what was synced, so the app opens instantly.

use crate::logic::activity::Activity;
use crate::logic::fit::Derived;
use crate::logic::graph::{Graph, Unreadable};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
enum Dir {
    Data,
    Config,
    Cache,
}

/// Where `kind` lives on `os`, given the environment. The XDG variables win everywhere (the
/// tests and a portable setup point them elsewhere); then the system's own convention: the
/// folders of the user's profile on Windows, `~/Library` on macOS, the XDG defaults elsewhere.
fn dir_in(kind: Dir, os: &str, var: impl Fn(&str) -> Option<std::ffi::OsString>) -> PathBuf {
    let xdg = ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"][kind as usize];
    if let Some(dir) = var(xdg).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir).join("astrolabe");
    }
    let home = PathBuf::from(var(if os == "windows" { "USERPROFILE" } else { "HOME" }).unwrap_or_default());
    match os {
        // Local for what is big or private to this machine; roaming for the setting that is not.
        "windows" => match kind {
            Dir::Config => var("APPDATA").map(PathBuf::from).unwrap_or_else(|| home.join("AppData/Roaming")),
            _ => var("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home.join("AppData/Local")),
        }
        .join("astrolabe"),
        "macos" => match kind {
            Dir::Cache => home.join("Library/Caches"),
            _ => home.join("Library/Application Support"),
        }
        .join("astrolabe"),
        _ => home.join([".local/share", ".config", ".cache"][kind as usize]).join("astrolabe"),
    }
}

#[cfg(test)]
thread_local! {
    static ROOT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Points this test's store at `root` (each test runs on its own thread, and GPUI's test
/// executor runs the background tasks on it too).
#[cfg(test)]
pub fn use_root(root: PathBuf) {
    ROOT.with(|r| *r.borrow_mut() = Some(root));
}

/// Tests write in a folder of their own, never in the user's.
fn dir(kind: Dir) -> PathBuf {
    #[cfg(test)]
    {
        let root = ROOT.with(|r| r.borrow().clone()).unwrap_or_else(|| std::env::temp_dir().join(format!("astrolabe-tests-{}", std::process::id())));
        return root.join(["data", "config", "cache"][kind as usize]);
    }
    #[cfg(not(test))]
    dir_in(kind, std::env::consts::OS, |name| std::env::var_os(name))
}

pub fn data_dir() -> PathBuf {
    dir(Dir::Data)
}

pub(super) fn config_dir() -> PathBuf {
    dir(Dir::Config)
}

pub(super) fn cache_dir() -> PathBuf {
    dir(Dir::Cache)
}

/// Atomic write (temp file + rename); `private`: readable by the user only (health data).
pub(super) fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    write_atomic_from(path, &mut &bytes[..], private).map(drop)
}

/// The same, from a reader, for what is too big to hold in memory (an extract of a few hundred
/// MB). Returns the bytes written. If the reader fails, the temporary file is left behind (the
/// next write takes it over) and the file in place is untouched.
pub(super) fn write_atomic_from(path: &Path, from: &mut dyn std::io::Read, private: bool) -> std::io::Result<u64> {
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("tmp");
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    }
    // Elsewhere the user's profile folder is already theirs alone.
    #[cfg(not(unix))]
    let _ = private;
    let mut f = std::io::BufWriter::new(opts.open(&tmp)?);
    let n = std::io::copy(from, &mut f)?;
    let f = f.into_inner().map_err(|e| e.into_error())?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)?;
    Ok(n)
}

/// Seconds since 1970, for the names of files set aside.
fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Renames a file that can't be read to `<stem>.damaged-<seconds>.<ext>`, beside it, so that
/// nothing writes over it; returns the name it is kept under.
fn set_aside(path: &Path, ext: &str) -> String {
    let stem = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().trim_end_matches(&format!(".{ext}")).to_string());
    let aside = path.with_file_name(format!("{stem}.damaged-{}.{ext}", now_secs()));
    let kept = if fs::rename(path, &aside).is_ok() { &aside } else { path };
    kept.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// The synced activities. No file is a first launch (empty); a file that can't be read is set
/// aside under another name, so the next sync can't write over it, and reported.
pub fn load_cache() -> Result<Vec<Activity>, String> {
    read_cache(&data_dir().join("activities.json"))
}

fn read_cache(path: &Path) -> Result<Vec<Activity>, String> {
    let unreadable = crate::lang::tr("Activity cache unreadable", "Cache des activités illisible");
    let err = match fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        // The folder itself can't be read: nothing to set aside.
        Err(e) => return Err(format!("{unreadable} : {e}")),
        Ok(b) => match serde_json::from_slice(&b) {
            Ok(acts) => return Ok(acts),
            Err(e) => e.to_string(),
        },
    };
    let kept = set_aside(path, "json");
    Err(crate::lang::trf(format!("{unreadable} ({err}), kept as {kept}"), format!("{unreadable} ({err}), conservé sous {kept}")))
}

pub fn save_cache(acts: &[Activity]) -> Result<(), String> {
    let bytes = serde_json::to_vec(acts).map_err(|e| e.to_string())?;
    write_atomic(&data_dir().join("activities.json"), &bytes, true).map_err(|e| format!("{} : {e}", crate::lang::tr("Cache not saved", "Cache non enregistré")))
}

fn fit_path(id: i64) -> PathBuf {
    data_dir().join("fit").join(format!("{id}.zip"))
}

pub fn load_fit(id: i64) -> Option<Vec<u8>> {
    fs::read(fit_path(id)).ok()
}

pub fn save_fit(id: i64, bytes: &[u8]) -> Result<(), String> {
    write_atomic(&fit_path(id), bytes, true).map_err(|e| format!("{} : {e}", crate::lang::tr("Track not saved", "Trace non enregistrée")))
}

/// Per activity: the simplified line and the best efforts. An entry means the file was handled
/// (an empty one: the activity has no file).
pub fn load_derived() -> HashMap<i64, Derived> {
    fs::read(data_dir().join("derived.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_derived(d: &HashMap<i64, Derived>) -> Result<(), String> {
    let bytes = serde_json::to_vec(d).map_err(|e| e.to_string())?;
    write_atomic(&data_dir().join("derived.json"), &bytes, true).map_err(|e| format!("{} : {e}", crate::lang::tr("Tracks not saved", "Traces non enregistrées")))
}

/// Settings that aren't secrets (the tile server), one `key=value` per line.
pub fn load_settings() -> HashMap<String, String> {
    fs::read_to_string(data_dir().join("settings"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
        .collect()
}

pub fn save_settings(s: &HashMap<String, String>) -> Result<(), String> {
    let mut keys: Vec<_> = s.iter().collect();
    keys.sort();
    let text: String = keys.into_iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    write_atomic(&data_dir().join("settings"), text.as_bytes(), false).map_err(|e| format!("{} : {e}", crate::lang::tr("Settings not saved", "Réglages non enregistrés")))
}

/// The routes kept on the Loops page. No file: none yet; a file that can't be read is set aside,
/// so that the next save can't write over it, and reported.
pub fn load_routes() -> Result<Vec<crate::logic::loops::Route>, String> {
    let path = data_dir().join("routes.json");
    let unreadable = crate::lang::tr("Routes unreadable", "Parcours illisibles");
    let err = match fs::read(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{unreadable} : {e}")),
        Ok(b) => match serde_json::from_slice::<Vec<crate::logic::loops::Route>>(&b) {
            Ok(mut r) => {
                // Tracks imported before they were marked: no waypoints, no seed, never planned.
                for x in &mut r {
                    x.shape.track |= x.shape.waypoints.is_empty() && x.shape.seed == 0;
                }
                return Ok(r);
            }
            Err(e) => e.to_string(),
        },
    };
    let kept = set_aside(&path, "json");
    Err(crate::lang::trf(format!("{unreadable} ({err}), kept as {kept}"), format!("{unreadable} ({err}), conservés sous {kept}")))
}

pub fn save_routes(r: &[crate::logic::loops::Route]) -> Result<(), String> {
    let bytes = serde_json::to_vec(r).map_err(|e| e.to_string())?;
    write_atomic(&data_dir().join("routes.json"), &bytes, true).map_err(|e| format!("{} : {e}", crate::lang::tr("Routes not saved", "Parcours non enregistrés")))
}

/// The folder of the OpenStreetMap extracts and the graphs built from them.
pub fn osm_dir() -> PathBuf {
    data_dir().join("osm")
}

pub fn extract_path(region: &str) -> PathBuf {
    osm_dir().join(format!("{region}.osm.pbf"))
}

fn graph_path(region: &str) -> PathBuf {
    osm_dir().join(format!("{region}.graph"))
}

/// The size of a region's extract, when it is on disk.
pub fn extract_size(region: &str) -> Option<u64> {
    fs::metadata(extract_path(region)).ok().map(|m| m.len()).filter(|n| *n > 0)
}

/// A region's extract that can't be read is set aside (it can be downloaded again).
pub fn set_aside_extract(region: &str) -> String {
    set_aside(&extract_path(region), "osm.pbf")
}

/// What is on disk of a region's graph.
pub enum StoredGraph {
    Absent,
    Ready(Graph),
    /// Written by another version: build it again from the extract.
    Stale,
    /// Damaged and set aside, or the folder can't be read: said plainly.
    Failed(String),
}

pub fn load_graph(region: &str) -> StoredGraph {
    read_graph(&graph_path(region))
}

fn read_graph(path: &Path) -> StoredGraph {
    let unreadable = crate::lang::tr("Path network unreadable", "Réseau de chemins illisible");
    let bytes = match fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return StoredGraph::Absent,
        Err(e) => return StoredGraph::Failed(format!("{unreadable} : {e}")),
        Ok(b) => b,
    };
    match Graph::decode(&bytes) {
        Ok(g) => StoredGraph::Ready(g),
        Err(Unreadable::Stale(_)) => StoredGraph::Stale,
        Err(Unreadable::Damaged(why)) => {
            let kept = set_aside(path, "graph");
            StoredGraph::Failed(crate::lang::trf(format!("{unreadable} ({why}), kept as {kept}"), format!("{unreadable} ({why}), conservé sous {kept}")))
        }
    }
}

pub fn save_graph(region: &str, bytes: &[u8]) -> Result<(), String> {
    write_atomic(&graph_path(region), bytes, false).map_err(|e| format!("{} : {e}", crate::lang::tr("Path network not saved", "Réseau de chemins non enregistré")))
}

/// The regions downloaded, in the order they were added; `None` before any list was kept (the
/// first region may be there from before the list).
pub fn load_regions() -> Result<Option<Vec<crate::logic::regions::Region>>, String> {
    match fs::read(data_dir().join("regions.json")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
        Ok(b) => serde_json::from_slice(&b).map(Some).map_err(|e| e.to_string()),
    }
    .map_err(|e| format!("{} : {e}", crate::lang::tr("Regions unreadable", "Régions illisibles")))
}

pub fn save_regions(r: &[crate::logic::regions::Region]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(r).map_err(|e| e.to_string())?;
    write_atomic(&data_dir().join("regions.json"), &bytes, true).map_err(|e| format!("{} : {e}", crate::lang::tr("Regions not saved", "Régions non enregistrées")))
}

/// What is on disk of a region: its extract's size and when it was downloaded, and whether its
/// path network is built.
pub fn region_files(region: &str) -> (Option<u64>, Option<std::time::SystemTime>, bool) {
    let m = fs::metadata(extract_path(region)).ok().filter(|m| m.len() > 0);
    (m.as_ref().map(|m| m.len()), m.and_then(|m| m.modified().ok()), graph_path(region).is_file())
}

/// Deletes a region's extract, path network and buildings read from it (asked, and confirmed,
/// by the user). The elevation tiles stay: they are shared.
pub fn delete_region(region: &str) -> Result<(), String> {
    let failed = |e: std::io::Error| format!("{} : {e}", crate::lang::tr("Region not removed", "Région non retirée"));
    // The extract, a download cut midway (its temporary file), and the network.
    for p in [extract_path(region), extract_path(region).with_extension("tmp"), graph_path(region)] {
        match fs::remove_file(&p) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(failed(e)),
            _ => {}
        }
    }
    let buildings = cache_dir().join("buildings");
    for e in fs::read_dir(&buildings).into_iter().flatten().flatten() {
        // One folder per region and extract size: `{region}-{size}`.
        let name = e.file_name().to_string_lossy().to_string();
        if name.rsplit_once('-').is_some_and(|(r, size)| r == region && size.parse::<u64>().is_ok()) {
            fs::remove_dir_all(e.path()).map_err(failed)?;
        }
    }
    Ok(())
}

/// Where the save dialog opens: the Downloads folder, else the home folder.
pub fn export_dir() -> PathBuf {
    #[cfg(test)]
    return dir(Dir::Data);
    #[cfg(not(test))]
    {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_default();
        let downloads = home.join("Downloads");
        if downloads.is_dir() { downloads } else { home }
    }
}

/// Writes a file the user chose (an exported loop), whole or not at all.
pub fn export(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_atomic(path, bytes, false).map_err(|e| format!("{} : {e}", crate::lang::tr("File not saved", "Fichier non enregistré")))
}

#[cfg(test)]
mod tests {
    use super::{Dir, StoredGraph, dir_in, read_cache, read_graph};
    use std::{collections::HashMap, ffi::OsString, path::PathBuf};

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs.iter().map(|(k, v)| (k.to_string(), OsString::from(v))).collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn each_system_keeps_its_files_where_it_expects_them() {
        let p = |s: &str| PathBuf::from(s);
        // Linux: the XDG defaults under the home folder, or the variables when set.
        assert_eq!(dir_in(Dir::Data, "linux", env(&[("HOME", "/home/a")])), p("/home/a/.local/share/astrolabe"));
        assert_eq!(dir_in(Dir::Config, "linux", env(&[("HOME", "/home/a")])), p("/home/a/.config/astrolabe"));
        assert_eq!(dir_in(Dir::Cache, "linux", env(&[("HOME", "/home/a")])), p("/home/a/.cache/astrolabe"));
        assert_eq!(dir_in(Dir::Data, "linux", env(&[("HOME", "/home/a"), ("XDG_DATA_HOME", "/srv/d")])), p("/srv/d/astrolabe"));
        // An empty variable is not a folder.
        assert_eq!(dir_in(Dir::Data, "linux", env(&[("HOME", "/home/a"), ("XDG_DATA_HOME", "")])), p("/home/a/.local/share/astrolabe"));
        // macOS: under ~/Library.
        assert_eq!(dir_in(Dir::Data, "macos", env(&[("HOME", "/Users/a")])), p("/Users/a/Library/Application Support/astrolabe"));
        assert_eq!(dir_in(Dir::Config, "macos", env(&[("HOME", "/Users/a")])), p("/Users/a/Library/Application Support/astrolabe"));
        assert_eq!(dir_in(Dir::Cache, "macos", env(&[("HOME", "/Users/a")])), p("/Users/a/Library/Caches/astrolabe"));
        // Windows: no HOME there; the profile's folders, local for data and cache, roaming for config.
        let win = [("USERPROFILE", "C:/Users/a"), ("LOCALAPPDATA", "C:/Users/a/AppData/Local"), ("APPDATA", "C:/Users/a/AppData/Roaming")];
        assert_eq!(dir_in(Dir::Data, "windows", env(&win)), p("C:/Users/a/AppData/Local/astrolabe"));
        assert_eq!(dir_in(Dir::Cache, "windows", env(&win)), p("C:/Users/a/AppData/Local/astrolabe"));
        assert_eq!(dir_in(Dir::Config, "windows", env(&win)), p("C:/Users/a/AppData/Roaming/astrolabe"));
        // The variables missing: built from the profile.
        assert_eq!(dir_in(Dir::Data, "windows", env(&[("USERPROFILE", "C:/Users/a")])), p("C:/Users/a/AppData/Local/astrolabe"));
        assert_eq!(dir_in(Dir::Config, "windows", env(&[("USERPROFILE", "C:/Users/a")])), p("C:/Users/a/AppData/Roaming/astrolabe"));
        // Nothing at all: a relative folder, as before, rather than a panic.
        assert_eq!(dir_in(Dir::Data, "linux", env(&[])), p(".local/share/astrolabe"));
    }

    #[test]
    fn a_missing_cache_is_a_first_launch_and_a_damaged_one_is_kept_aside() {
        let dir = std::env::temp_dir().join(format!("astrolabe-cache-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("activities.json");
        assert_eq!(read_cache(&path).unwrap().len(), 0);
        std::fs::write(&path, b"[]").unwrap();
        assert_eq!(read_cache(&path).unwrap().len(), 0);
        for bad in [&b"{not json"[..], b"", b"\xff\xfe"] {
            std::fs::write(&path, bad).unwrap();
            let e = read_cache(&path).unwrap_err();
            assert!(!path.exists(), "{e}");
            let aside: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|f| f.file_name().to_string_lossy().contains("damaged")).collect();
            assert_eq!(std::fs::read(aside[0].path()).unwrap(), bad);
            std::fs::remove_file(aside[0].path()).unwrap();
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_damaged_graph_is_kept_aside_and_an_old_one_is_stale() {
        let dir = std::env::temp_dir().join(format!("astrolabe-graph-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("r.graph");
        assert!(matches!(read_graph(&path), StoredGraph::Absent));
        let good = crate::logic::graph::tests::build_from(&crate::logic::graph::tests::place()).unwrap().encode();
        std::fs::write(&path, &good).unwrap();
        assert!(matches!(read_graph(&path), StoredGraph::Ready(_)));
        let mut old = good.clone();
        old[8] = old[8].wrapping_sub(1);
        std::fs::write(&path, &old).unwrap();
        assert!(matches!(read_graph(&path), StoredGraph::Stale));
        assert!(path.exists());
        std::fs::write(&path, &good[..good.len() / 2]).unwrap();
        let StoredGraph::Failed(e) = read_graph(&path) else { panic!() };
        assert!(e.contains("r.damaged-") && e.ends_with(".graph"), "{e}");
        assert!(!path.exists());
        let aside: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(aside.len(), 1);
        assert_eq!(std::fs::read(aside[0].path()).unwrap(), &good[..good.len() / 2]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_stream_is_written_whole_or_not_at_all() {
        let dir = std::env::temp_dir().join(format!("astrolabe-stream-test-{}", std::process::id()));
        let path = dir.join("big.osm.pbf");
        super::write_atomic_from(&path, &mut &b"first"[..], false).unwrap();
        struct Cut(usize);
        impl std::io::Read for Cut {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.0 == 0 {
                    return Err(std::io::Error::other("connection reset"));
                }
                self.0 -= 1;
                buf[0] = b'x';
                Ok(1)
            }
        }
        assert!(super::write_atomic_from(&path, &mut Cut(1000), false).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        assert_eq!(super::write_atomic_from(&path, &mut &b"second"[..], false).unwrap(), 6);
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
