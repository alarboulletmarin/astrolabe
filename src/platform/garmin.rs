//! Garmin's Personal API: the token, the activity list, the original files.

use super::{agent, http_error};
use crate::logic::activity::Activity;
use std::collections::HashSet;

const BASE: &str = "https://personal.connectapi.garmin.com";
const PAGE: usize = 100;

#[derive(Debug, PartialEq)]
pub enum Fail {
    /// Garmin asks to slow down: stop the queue, retry at the next sync.
    TooMany,
    Other(String),
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
