//! Everything that leaves the process: the OS keyring (API key), Garmin's Personal API, map tiles,
//! and the local store. All functions block: call them from a background task.

use crate::activity::Activity;
use crate::fit::Derived;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const BASE: &str = "https://personal.connectapi.garmin.com";
const PAGE: usize = 100;
/// Identifies the app to the services it calls (OpenStreetMap's tile policy asks for it).
pub const USER_AGENT: &str = concat!("astrolabe/", env!("CARGO_PKG_VERSION"), " (desktop activity journal)");

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().user_agent(USER_AGENT).build().into()
}

// --- API key: stored by the OS secret store (Secret Service on Linux, Keychain on macOS,
// Credential Manager on Windows), encrypted at rest and unlocked with the session.
// Never written to disk or logged by the app.

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new("astrolabe", "garmin-personal-api-key").map_err(|e| keyring_error(&e))
}

fn keyring_error(e: &keyring::Error) -> String {
    format!("Trousseau du système indisponible ({e}). Sous Linux, il faut un service Secret Service (GNOME Keyring, KWallet, KeePassXC).")
}

pub fn load_key() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(keyring_error(&e)),
    }
}

pub fn store_key(key: &str) -> Result<(), String> {
    entry()?.set_password(key).map_err(|e| keyring_error(&e))
}

pub fn delete_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(keyring_error(&e)),
    }
}

/// Trims what a paste brings along and refuses what can't be a key, before it reaches a header.
pub fn clean_key(raw: &str) -> Result<String, &'static str> {
    let key = raw.trim();
    if key.is_empty() {
        return Err("Colle ta clé API d'abord.");
    }
    if key.len() > 1024 || key.chars().any(|c| c.is_whitespace() || c.is_control() || !c.is_ascii()) {
        return Err("Ce texte ne ressemble pas à une clé API Garmin.");
    }
    Ok(key.to_string())
}

// --- Personal API

#[derive(Debug, PartialEq)]
pub enum Fail {
    /// Garmin asks to slow down: stop the queue, retry at the next sync.
    TooMany,
    Other(String),
}

fn http_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(401 | 403) => "Garmin refuse la clé : expirée ou révoquée ? Crée-en une nouvelle dans Garmin Connect.".into(),
        ureq::Error::StatusCode(429) => "Trop de requêtes : Garmin demande d'attendre un peu.".into(),
        ureq::Error::StatusCode(c) => format!("Garmin a répondu {c}."),
        e => format!("Réseau : {e}"),
    }
}

/// Exchanges the API key for a bearer token (a JWT valid 24 h), kept in memory by the caller.
pub fn token(key: &str) -> Result<String, String> {
    let mut res = agent()
        .post(format!("{BASE}/personalapitoken-service/personal-api/personal-api-key/token"))
        .header("Authorization", format!("ApiKey {key}"))
        .header("Accept", "*/*")
        .send_empty()
        .map_err(http_error)?;
    let token = res.body_mut().read_to_string().map_err(http_error)?;
    Ok(token.trim().to_string())
}

fn page(token: &str, start: usize) -> Result<Vec<Activity>, String> {
    agent()
        .get(format!("{BASE}/activitylist-service/activities/search/activities"))
        .query("start", start.to_string())
        .query("limit", PAGE.to_string())
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .call()
        .map_err(http_error)?
        .body_mut()
        .read_json()
        .map_err(|e| format!("Réponse inattendue de Garmin : {e}"))
}

/// Fetches activities newer than the ones already known (newest first) and returns the merged list.
// ponytail: stops at the first known activity, so edits made on Garmin to old activities aren't
// picked up; add a "full resync" button if that matters.
/// `seen` counts the activities received so far, for the progress shown while it runs.
pub fn sync(token: &str, known: &[Activity], seen: &std::sync::atomic::AtomicUsize) -> Result<Vec<Activity>, String> {
    let ids: HashSet<i64> = known.iter().map(|a| a.activity_id).collect();
    let mut fresh = Vec::new();
    let mut start = 0;
    loop {
        let batch = page(token, start)?;
        seen.fetch_add(batch.len(), std::sync::atomic::Ordering::Relaxed);
        let full = batch.len() == PAGE;
        let before = fresh.len();
        fresh.extend(batch.into_iter().filter(|a| !ids.contains(&a.activity_id)));
        if !full || fresh.len() - before < PAGE {
            break;
        }
        start += PAGE;
    }
    Ok(merge(fresh, known))
}

fn merge(fresh: Vec<Activity>, known: &[Activity]) -> Vec<Activity> {
    let mut all = fresh;
    all.extend_from_slice(known);
    // "YYYY-MM-DD HH:MM:SS" sorts like the date it is.
    all.sort_by(|a, b| b.start_time_local.cmp(&a.start_time_local));
    all.dedup_by_key(|a| a.activity_id);
    all
}

/// The original file (a ZIP holding the FIT). `None`: the activity has none (entered by hand).
pub fn download_fit(token: &str, id: i64) -> Result<Option<Vec<u8>>, Fail> {
    let res = agent()
        .get(format!("{BASE}/download-service/files/activity/{id}"))
        .header("Authorization", format!("Bearer {token}"))
        // The official client's list (Garmin answers 406 without x-zip-compressed); anything else
        // is still checked when the ZIP is read.
        .header("Accept", "application/zip, application/x-zip-compressed, application/octet-stream, */*;q=0.1")
        .call();
    match res {
        Ok(mut r) => r
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_vec()
            .map(Some)
            .map_err(|e| Fail::Other(http_error(e))),
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(ureq::Error::StatusCode(429)) => Err(Fail::TooMany),
        Err(e) => Err(Fail::Other(http_error(e))),
    }
}

// --- Map tiles: OpenStreetMap's by default (attribution shown on every map), cached on disk
// so a place is fetched once, as the tile usage policy asks.

pub const DEFAULT_TILES: &str = "https://tile.openstreetmap.org/{z}/{x}/{y}.png";

pub fn tile_url(template: &str, z: u8, x: u32, y: u32) -> String {
    template.replace("{z}", &z.to_string()).replace("{x}", &x.to_string()).replace("{y}", &y.to_string())
}

pub fn tile(template: &str, z: u8, x: u32, y: u32) -> Result<Vec<u8>, String> {
    let url = tile_url(template, z, x, y);
    // One folder per server, so changing it never mixes two styles.
    let server: String = template.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let path = cache_dir().join("tiles").join(server).join(format!("{z}/{x}/{y}"));
    if let Ok(b) = fs::read(&path) {
        return Ok(b);
    }
    let bytes = agent()
        .get(&url)
        .call()
        .map_err(http_error)?
        .body_mut()
        .read_to_vec()
        .map_err(http_error)?;
    write_atomic(&path, &bytes, false).ok();
    Ok(bytes)
}

// --- Local store: what was synced, so the app opens instantly and offline.

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share")).join("astrolabe")
}

fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".cache")).join("astrolabe")
}

/// Atomic write (temp file + rename); `private`: readable by the user only (health data).
fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_key_rejects_garbage() {
        assert_eq!(clean_key("  GCDC_abc.def+/=\n").unwrap(), "GCDC_abc.def+/=");
        assert!(clean_key("").is_err());
        assert!(clean_key("   \n").is_err());
        assert!(clean_key("API_GARMIN_KEY=a b").is_err());
        assert!(clean_key("clé").is_err());
        assert!(clean_key("a\u{0}b").is_err());
        assert!(clean_key(&"a".repeat(1025)).is_err());
        assert!(clean_key(&"a".repeat(1024)).is_ok());
    }

    #[test]
    fn merge_dedups_and_sorts_newest_first() {
        let a = |id, t: &str| -> Activity {
            serde_json::from_value(serde_json::json!({
                "activityId": id, "startTimeLocal": t, "activityType": {"typeKey": "running"}
            }))
            .unwrap()
        };
        let known = vec![a(2, "2026-10-20 07:00:00"), a(1, "2026-10-19 07:00:00")];
        let fresh = vec![a(3, "2026-10-21 07:00:00"), a(2, "2026-10-20 07:00:00")];
        let ids: Vec<_> = merge(fresh, &known).iter().map(|x| x.activity_id).collect();
        assert_eq!(ids, [3, 2, 1]);
        assert!(merge(vec![], &[]).is_empty());
    }

    #[test]
    fn tile_urls() {
        assert_eq!(tile_url(DEFAULT_TILES, 14, 8423, 5856), "https://tile.openstreetmap.org/14/8423/5856.png");
        assert_eq!(tile_url("https://x/{z}-{y}-{x}?k=1", 0, 0, 0), "https://x/0-0-0?k=1");
    }
}
