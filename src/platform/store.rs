//! The local store: what was synced, so the app opens instantly and offline.

use crate::logic::activity::Activity;
use crate::logic::fit::Derived;
use std::{
    collections::HashMap,
    fs,
    io::Write,
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

/// Tests write in a folder of their own, never in the user's.
fn dir(kind: Dir) -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("astrolabe-tests-{}", std::process::id())).join(["data", "config", "cache"][kind as usize]);
    }
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
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("tmp");
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, path)
}

pub fn load_cache() -> Vec<Activity> {
    fs::read(data_dir().join("activities.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
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
    write_atomic(&data_dir().join("settings"), text.as_bytes(), false).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Dir, dir_in};
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
}
