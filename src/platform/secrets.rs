//! The API key. Where the system has a secret store (Secret Service on Linux, Keychain on macOS,
//! Credential Manager on Windows) it goes there: encrypted at rest, unlocked with the session.
//! Where it has none, it is encrypted in a file (`vault`) with a random key kept in another one.
//! Never written in the clear, never logged by the app.

use crate::platform::{store, vault};
use std::fs;
use std::path::PathBuf;

/// Where the key lives on this machine.
#[derive(Clone, Debug, PartialEq)]
pub enum Store {
    System,
    /// No system secret store (why, as the system said it): the key is encrypted in a file.
    Vault(String),
}

/// The encrypted key, in the data folder, and the key that opens it, in the config folder:
/// copying or backing up one without the other gives nothing.
fn sealed_path() -> PathBuf {
    store::data_dir().join("key.enc")
}

fn master_path() -> PathBuf {
    store::config_dir().join("master.key")
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new("astrolabe", "garmin-personal-api-key").map_err(|e| keyring_error(&e))
}

fn keyring_error(e: &keyring::Error) -> String {
    format!("Trousseau du système indisponible ({e}).")
}

fn read_master() -> Option<[u8; 32]> {
    fs::read(master_path()).ok()?.try_into().ok()
}

fn load_sealed() -> Result<Option<String>, String> {
    let Ok(file) = fs::read(sealed_path()) else { return Ok(None) };
    read_master()
        .and_then(|master| vault::open(&master, &file))
        .map(Some)
        .ok_or_else(|| "La clé enregistrée dans l'app est illisible : retire-la et ajoute-la de nouveau.".into())
}

/// Looks for the key where it can be, and says which store this machine has.
pub fn load_key() -> (Store, Result<Option<String>, String>) {
    match entry().map(|e| e.get_password()) {
        Ok(Ok(k)) => (Store::System, Ok(Some(k))),
        Ok(Err(keyring::Error::NoEntry)) => (Store::System, Ok(None)),
        Ok(Err(e)) => (Store::Vault(keyring_error(&e)), load_sealed()),
        Err(e) => (Store::Vault(e), load_sealed()),
    }
}

/// Keeps the key: in the system store, or encrypted in a file where there is none.
pub fn store_key(key: &str, store: &Store) -> Result<(), String> {
    match store {
        Store::System => entry()?.set_password(key).map_err(|e| keyring_error(&e)),
        Store::Vault(_) => {
            let master = read_master().unwrap_or_else(vault::new_master);
            let sealed = vault::seal(&master, key)?;
            let io = |e: std::io::Error| format!("Impossible d'enregistrer la clé : {e}");
            // The key that opens first, so a cut between the two never leaves a key nothing opens.
            store::write_atomic(&master_path(), &master, true).map_err(io)?;
            store::write_atomic(&sealed_path(), &sealed, true).map_err(io)
        }
    }
}

/// Removes the encrypted key and its key, if any.
fn delete_sealed() -> Result<(), String> {
    for path in [sealed_path(), master_path()] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Impossible de retirer la clé : {e}")),
        }
    }
    Ok(())
}

/// Removes the key from where it is kept (both places: it may have moved between machines' setups).
pub fn delete_key() -> Result<(), String> {
    delete_sealed()?;
    match entry().map(|e| e.delete_credential()) {
        // With no system store there is nothing to remove from it.
        Ok(Ok(())) | Ok(Err(keyring::Error::NoEntry)) | Err(_) => Ok(()),
        Ok(Err(e)) => Err(keyring_error(&e)),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One test owns the environment: it points both folders at a fresh one. Only files are
    /// touched: tests never go near the real system store.
    #[test]
    fn the_encrypted_key_is_written_private_opened_and_removed() {
        let dir = std::env::temp_dir().join(format!("astrolabe-vault-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // SAFETY: no other test reads or writes the environment.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", dir.join("data"));
            std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
        }
        let vault = Store::Vault("no secret store".into());
        assert_eq!(load_sealed(), Ok(None));
        store_key("GCDC_secret", &vault).unwrap();
        assert_eq!(load_sealed(), Ok(Some("GCDC_secret".into())));
        // Replaced by a new key; the same master key keeps working.
        let master = fs::read(master_path()).unwrap();
        store_key("GCDC_other", &vault).unwrap();
        assert_eq!(fs::read(master_path()).unwrap(), master);
        assert_eq!(load_sealed(), Ok(Some("GCDC_other".into())));
        #[cfg(unix)]
        for path in [sealed_path(), master_path()] {
            assert_eq!(std::os::unix::fs::PermissionsExt::mode(&fs::metadata(path).unwrap().permissions()) & 0o777, 0o600);
        }
        assert!(!fs::read(sealed_path()).unwrap().windows(10).any(|w| w == b"GCDC_other"));
        // Master key lost, or file damaged: it says so, it never returns garbage.
        fs::remove_file(master_path()).unwrap();
        assert!(load_sealed().unwrap_err().contains("illisible"));
        store_key("GCDC_again", &vault).unwrap();
        assert_eq!(load_sealed(), Ok(Some("GCDC_again".into())));
        fs::write(sealed_path(), b"garbage").unwrap();
        assert!(load_sealed().is_err());
        delete_sealed().unwrap();
        assert!(!sealed_path().exists() && !master_path().exists());
        delete_sealed().unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

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
}
