//! The API key, stored by the OS secret store (Secret Service on Linux, Keychain on macOS,
//! Credential Manager on Windows), encrypted at rest and unlocked with the session.
//! Never written to disk or logged by the app.

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
}
