//! The local store: what was synced, so the app opens instantly and offline.

use crate::logic::activity::Activity;
use crate::logic::fit::Derived;
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share")).join("astrolabe")
}

pub(super) fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".cache")).join("astrolabe")
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
    write_atomic(&data_dir().join("activities.json"), &bytes, true).map_err(|e| format!("Cache non enregistré : {e}"))
}

fn fit_path(id: i64) -> PathBuf {
    data_dir().join("fit").join(format!("{id}.zip"))
}

pub fn load_fit(id: i64) -> Option<Vec<u8>> {
    fs::read(fit_path(id)).ok()
}

pub fn save_fit(id: i64, bytes: &[u8]) -> Result<(), String> {
    write_atomic(&fit_path(id), bytes, true).map_err(|e| format!("Trace non enregistrée : {e}"))
}

/// Per activity: the simplified line and the best efforts. An entry means the file was handled
/// (an empty one: the activity has no file).
pub fn load_derived() -> HashMap<i64, Derived> {
    fs::read(data_dir().join("derived.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_derived(d: &HashMap<i64, Derived>) -> Result<(), String> {
    let bytes = serde_json::to_vec(d).map_err(|e| e.to_string())?;
    write_atomic(&data_dir().join("derived.json"), &bytes, true).map_err(|e| format!("Traces non enregistrées : {e}"))
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
