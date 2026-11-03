//! The whole chain, network → disk → reopening, against a local stand-in for Garmin: a plain
//! HTTP server on 127.0.0.1 that answers what each test scripts. Made-up activities, the real
//! code from the HTTP client to the files; each test has a folder of its own and never sees the
//! key store.

use super::{Data, Status};
use crate::lang::tr;
use crate::logic::fit;
use crate::platform::store;
use crate::ui::theme::Theme;
use gpui::{AppContext as _, Entity, TestAppContext};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Answer = Box<dyn FnMut(&str) -> (u16, Vec<u8>) + Send>;

/// The stand-in: answers each request with the scripted function of its path, and logs the paths.
struct Garmin {
    url: String,
    answer: Arc<Mutex<Answer>>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Garmin {
    fn new(answer: impl FnMut(&str) -> (u16, Vec<u8>) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let answer: Arc<Mutex<Answer>> = Arc::new(Mutex::new(Box::new(answer)));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let (a, log) = (answer.clone(), asked.clone());
        std::thread::spawn(move || {
            for mut conn in listener.incoming().flatten() {
                let mut head = Vec::new();
                let mut byte = [0u8];
                while !head.ends_with(b"\r\n\r\n") && conn.read(&mut byte).unwrap_or(0) == 1 {
                    head.push(byte[0]);
                }
                let line = String::from_utf8_lossy(&head);
                let path = line.split(' ').nth(1).unwrap_or("").to_string();
                log.lock().unwrap().push(path.clone());
                let (code, body) = (a.lock().unwrap())(&path);
                let _ = conn.write_all(format!("HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes());
                let _ = conn.write_all(&body);
            }
        });
        Garmin { url, answer, asked }
    }

    fn answer(&self, answer: impl FnMut(&str) -> (u16, Vec<u8>) + Send + 'static) {
        *self.answer.lock().unwrap() = Box::new(answer);
    }

    fn asked(&self, part: &str) -> usize {
        self.asked.lock().unwrap().iter().filter(|p| p.contains(part)).count()
    }
}

/// An address where nothing listens: the network is down.
fn offline() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    format!("http://{}", l.local_addr().unwrap())
}

fn list(ids: &[i64]) -> Vec<u8> {
    let acts: Vec<_> = ids
        .iter()
        .map(|id| serde_json::json!({"activityId": id, "startTimeLocal": format!("2026-10-{:02} 07:00:00", id), "activityType": {"typeKey": "running"}}))
        .collect();
    serde_json::to_vec(&acts).unwrap()
}

fn fit_file() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/garmin-fenix-5-bike.fit")).unwrap()
}

/// Garmin with `ids` (newest last), each with its file except those in `no_file`.
fn garmin(ids: &'static [i64], no_file: &'static [i64]) -> impl FnMut(&str) -> (u16, Vec<u8>) + Send + 'static {
    let fit = fit_file();
    move |path| match path {
        p if p.contains("/token") => (200, b"tok".to_vec()),
        p if p.contains("start=0") => (200, list(ids)),
        p if p.contains("/activities/search") => (200, list(&[])),
        p if no_file.iter().any(|id| p.ends_with(&format!("/{id}"))) => (404, vec![]),
        p if p.contains("/files/activity/") => (200, fit.clone()),
        _ => (500, vec![]),
    }
}

/// A folder of the test's own, empty, for the store.
fn folder(name: &str) -> PathBuf {
    // The same folder from one run to the next, emptied first: they don't pile up.
    let root = std::env::temp_dir().join(format!("astrolabe-flow-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    store::use_root(root.clone());
    root
}

fn open(cx: &mut TestAppContext, api: &str, key: Option<&str>) -> Entity<Data> {
    let (api, key) = (api.to_string(), key.map(String::from));
    let data = cx.new(|cx| Data::open_with(Theme::for_appearance(gpui::WindowAppearance::Dark), api, key, cx));
    settle(cx);
    data
}

/// Runs everything there is to run, the timers included (the pause between two files).
fn settle(cx: &mut TestAppContext) {
    for _ in 0..30 {
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(1));
    }
    cx.run_until_parked();
}

fn ids(data: &Entity<Data>, cx: &mut TestAppContext) -> Vec<i64> {
    data.read_with(cx, |d, _| d.acts.iter().map(|a| a.activity_id).collect())
}

fn derived(data: &Entity<Data>, cx: &mut TestAppContext) -> Vec<i64> {
    data.read_with(cx, |d, _| {
        let mut v: Vec<_> = d.derived.keys().copied().collect();
        v.sort();
        v
    })
}

fn failed(data: &Entity<Data>, cx: &mut TestAppContext) -> Option<String> {
    data.read_with(cx, |d, _| if let Status::Failed(e) = &d.status { Some(e.clone()) } else { None })
}

/// Synced, closed, reopened without the network: everything is still there, and the failed
/// sync says why without taking anything away. Without a key, the app doesn't even try.
#[gpui::test]
fn a_sync_is_still_there_when_reopened_offline(cx: &mut TestAppContext) {
    let root = folder("offline");
    let g = Garmin::new(garmin(&[2, 1], &[2]));
    let data = open(cx, &g.url, Some("key"));
    assert_eq!(ids(&data, cx), [2, 1]);
    assert_eq!(derived(&data, cx), [1, 2]);
    assert_eq!(failed(&data, cx), None);
    data.read_with(cx, |d, _| {
        assert_eq!((d.fetch.done, d.fetch.failed, d.fetch.stopped.clone()), (2, 0, None));
        assert!(!d.derived[&1].line.is_empty(), "the file's track was derived");
        assert!(d.derived[&2].line.is_empty(), "no file: handled, empty");
    });
    assert!(root.join("data/fit/1.zip").exists() && !root.join("data/fit/2.zip").exists());
    assert_eq!(g.asked("/token"), 1, "one token for the whole sync");

    let again = open(cx, &offline(), Some("key"));
    assert_eq!(ids(&again, cx), [2, 1]);
    assert_eq!(derived(&again, cx), [1, 2]);
    assert!(failed(&again, cx).is_some_and(|e| e.starts_with(tr("Network", "Réseau"))), "{:?}", failed(&again, cx));

    let no_key = open(cx, &offline(), None);
    assert_eq!(ids(&no_key, cx), [2, 1]);
    assert_eq!(failed(&no_key, cx), None);
}

/// Garmin's "too many requests" on the list: said plainly, nothing written; the next sync works.
#[gpui::test]
fn a_refused_list_is_said_and_the_next_sync_goes_through(cx: &mut TestAppContext) {
    let root = folder("429");
    let g = Garmin::new(|p| if p.contains("/token") { (200, b"tok".to_vec()) } else { (429, vec![]) });
    let data = open(cx, &g.url, Some("key"));
    assert_eq!(failed(&data, cx).as_deref(), Some(tr("Too many requests: Garmin asks to wait a little.", "Trop de requêtes : Garmin demande d'attendre un peu.")));
    assert!(ids(&data, cx).is_empty());
    assert!(!root.join("data/activities.json").exists());
    assert_eq!(data.read_with(cx, |d, _| d.sync_outcome()), Some(false));

    g.answer(garmin(&[1], &[]));
    data.update(cx, |d, cx| d.sync(cx));
    settle(cx);
    assert_eq!(ids(&data, cx), [1]);
    assert_eq!(derived(&data, cx), [1]);
    assert_eq!(failed(&data, cx), None);
}

/// The network gone in the middle of the list: nothing half-written, the cache as it was.
#[gpui::test]
fn a_cut_in_the_list_leaves_the_cache_as_it_was(cx: &mut TestAppContext) {
    folder("cut");
    let g = Garmin::new(garmin(&[1], &[1]));
    let data = open(cx, &g.url, Some("key"));
    assert_eq!(ids(&data, cx), [1]);
    // A full first page, then the connection drops (an empty, broken answer).
    let page: Vec<i64> = (100..200).collect();
    g.answer(move |p| match p {
        p if p.contains("start=0") => (200, list(&page)),
        p if p.contains("/token") => (200, b"tok".to_vec()),
        _ => (200, b"[{\"activityId\":".to_vec()),
    });
    data.update(cx, |d, cx| d.sync(cx));
    settle(cx);
    assert!(failed(&data, cx).is_some());
    assert_eq!(ids(&data, cx), [1]);
    assert_eq!(ids(&open(cx, &offline(), None), cx), [1], "the file on disk too");
}

/// "Too many requests" on the files: the queue stops, says so, and the next sync picks up the rest.
#[gpui::test]
fn a_refused_file_stops_the_queue_and_the_next_sync_resumes(cx: &mut TestAppContext) {
    folder("resume");
    let mut first = garmin(&[1, 2, 3], &[]);
    let g = Garmin::new(move |p| if p.ends_with("/activity/2") { (429, vec![]) } else { first(p) });
    let data = open(cx, &g.url, Some("key"));
    assert_eq!(derived(&data, cx), [3], "newest first, stopped at 2");
    data.read_with(cx, |d, _| assert!(d.fetch.stopped.is_some() && !d.fetch.running));
    assert_eq!(data.read_with(cx, |d, _| d.sync_outcome()), Some(false));
    assert_eq!(g.asked("/activity/1"), 0);

    g.answer(garmin(&[1, 2, 3], &[]));
    data.update(cx, |d, cx| d.sync(cx));
    settle(cx);
    assert_eq!(derived(&data, cx), [1, 2, 3]);
    data.read_with(cx, |d, _| assert_eq!((d.fetch.stopped.clone(), d.fetch.failed), (None, 0)));
    assert_eq!(g.asked("/activity/3"), 1, "a file already read isn't asked again");
}

/// A damaged file is counted and named; the others go on, and it is tried again next time.
#[gpui::test]
fn a_damaged_file_is_counted_and_the_others_go_on(cx: &mut TestAppContext) {
    folder("damaged");
    let mut good = garmin(&[1, 2, 3], &[]);
    let g = Garmin::new(move |p| if p.ends_with("/activity/2") { (200, b"not a file".to_vec()) } else { good(p) });
    let data = open(cx, &g.url, Some("key"));
    assert_eq!(derived(&data, cx), [1, 3]);
    data.read_with(cx, |d, _| {
        assert_eq!(d.fetch.failed, 1);
        assert!(d.fetch.error.as_deref().is_some_and(|e| e.contains("ZIP")), "{:?}", d.fetch.error);
    });
    g.answer(garmin(&[1, 2, 3], &[]));
    data.update(cx, |d, cx| d.sync(cx));
    settle(cx);
    assert_eq!(derived(&data, cx), [1, 2, 3]);
}

/// A folder that can't be written: the sync says the cache wasn't saved and keeps nothing it
/// couldn't keep.
#[gpui::test]
fn a_disk_that_refuses_writes_says_so(cx: &mut TestAppContext) {
    let root = folder("readonly");
    // The temporary file the cache is written through is taken by a folder: the write fails,
    // whoever runs the tests (permissions don't stop root) and on every system.
    std::fs::create_dir_all(root.join("data/activities.tmp")).unwrap();
    let g = Garmin::new(garmin(&[1], &[]));
    let data = open(cx, &g.url, Some("key"));
    assert!(failed(&data, cx).is_some_and(|e| e.starts_with(tr("Cache not saved", "Cache non enregistré"))), "{:?}", failed(&data, cx));
    assert!(ids(&data, cx).is_empty());
    assert!(!root.join("data/activities.json").exists());
}

/// A store folder that can't be read is reported as such, with nothing set aside, and no sync
/// writes over it.
#[gpui::test]
fn a_folder_that_cant_be_read_is_reported(cx: &mut TestAppContext) {
    let root = folder("unreadable");
    // The data folder is a file: reading under it fails with an error that isn't "not found".
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("data"), b"").unwrap();
    let g = Garmin::new(garmin(&[1], &[]));
    let data = open(cx, &g.url, Some("key"));
    let e = failed(&data, cx).unwrap_or_default();
    assert!(e.starts_with(tr("Activity cache unreadable", "Cache des activités illisible")) && !e.contains("activities"), "{e}");
    assert_eq!(g.asked("/activities/search"), 0);
}

/// Files written by an older version open: unknown fields are ignored, missing ones take their
/// default, and tracks derived by an older version are derived again from their file, offline.
#[gpui::test]
fn an_older_version_s_files_still_open(cx: &mut TestAppContext) {
    let root = folder("old");
    let data_dir = root.join("data");
    std::fs::create_dir_all(data_dir.join("fit")).unwrap();
    std::fs::write(
        data_dir.join("activities.json"),
        br#"[{"activityId":1,"startTimeLocal":"2026-10-01 07:00:00","activityType":{"typeKey":"cycling"},"gone":true}]"#,
    )
    .unwrap();
    std::fs::write(data_dir.join("derived.json"), br#"{"1":{"line":[],"efforts":[]}}"#).unwrap();
    std::fs::write(data_dir.join("fit/1.zip"), fit_file()).unwrap();
    let data = open(cx, &offline(), None);
    assert_eq!(ids(&data, cx), [1]);
    assert_eq!(failed(&data, cx), None);
    data.read_with(cx, |d, _| {
        assert_eq!(d.derived[&1].version, fit::VERSION);
        assert!(!d.derived[&1].line.is_empty());
    });
}

/// A write that fails reaches the status line; during a sync, which owns that line, its note.
#[gpui::test]
fn a_write_that_fails_is_shown(cx: &mut TestAppContext) {
    let data = cx.new(|_| Data::with_activities(Theme::for_appearance(gpui::WindowAppearance::Dark), vec![]));
    data.update(cx, |d, cx| d.persist(cx, || Err("disk full".into())));
    cx.run_until_parked();
    assert_eq!(failed(&data, cx).as_deref(), Some("disk full"));

    data.update(cx, |d, cx| {
        d.status = Status::Listing("…".into());
        d.persist(cx, || Err("read-only".into()));
        d.persist(cx, || Ok(()));
    });
    cx.run_until_parked();
    data.read_with(cx, |d, _| {
        assert!(matches!(d.status, Status::Listing(_)));
        assert_eq!(d.note.as_deref(), Some("read-only"));
    });
}
