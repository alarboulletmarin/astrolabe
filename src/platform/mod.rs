//! Everything that leaves the process: the OS keyring, Garmin's Personal API, map tiles and the
//! local store. All functions block: call them from a background task.

pub mod garmin;
pub mod secrets;
pub mod store;
pub mod tiles;

/// Identifies the app to the services it calls (OpenStreetMap's tile policy asks for it).
pub const USER_AGENT: &str = concat!("astrolabe/", env!("CARGO_PKG_VERSION"), " (desktop activity journal)");

pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().user_agent(USER_AGENT).build().into()
}

pub(crate) fn http_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(401 | 403) => "Garmin refuse la clé : expirée ou révoquée ? Crée-en une nouvelle dans Garmin Connect.".into(),
        ureq::Error::StatusCode(429) => "Trop de requêtes : Garmin demande d'attendre un peu.".into(),
        ureq::Error::StatusCode(c) => format!("Garmin a répondu {c}."),
        e => format!("Réseau : {e}"),
    }
}
