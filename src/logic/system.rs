//! What the settings say about the system the app runs on, by `std::env::consts::OS`.

use crate::lang::tr;

pub struct KeyStore {
    /// After "confiée" ("entrusted"): where the key goes.
    pub to: &'static str,
    /// The label next to the key's title.
    pub pill: &'static str,
    /// Start of the sentence that says there is none.
    pub missing: &'static str,
}

pub fn key_store(os: &str) -> KeyStore {
    match os {
        "linux" => KeyStore {
            to: tr("to the system key store (GNOME Keyring or KWallet)", "au trousseau du système (GNOME Keyring ou KWallet)"),
            pill: tr("in the key store", "dans le trousseau"),
            missing: tr("Without a system key store", "Faute de trousseau système"),
        },
        "macos" => KeyStore {
            to: tr("to the macOS Keychain", "au Trousseau d'accès de macOS"),
            pill: tr("in the Keychain", "dans le trousseau"),
            missing: tr("Without a Keychain", "Faute de Trousseau d'accès"),
        },
        "windows" => KeyStore {
            to: tr("to the Windows Credential Manager", "au Gestionnaire d'identifiants de Windows"),
            pill: tr("in the credential manager", "dans le gestionnaire"),
            missing: tr("Without a Credential Manager", "Faute de Gestionnaire d'identifiants"),
        },
        _ => KeyStore {
            to: tr("to the system key store", "au trousseau du système"),
            pill: tr("in the key store", "dans le trousseau"),
            missing: tr("Without a system key store", "Faute de trousseau système"),
        },
    }
}

/// `path` with the home folder written `~`, as a shell would; whole when it is elsewhere.
pub fn tilde(path: &str, home: &str) -> String {
    // "/home/a" must not shorten "/home/ab/x": the home ends at a separator.
    match path.strip_prefix(home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with(['/', '\\'])) => format!("~{rest}"),
        _ => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_system_names_its_own_store() {
        assert!(key_store("linux").to.contains("KWallet"));
        assert!(key_store("macos").to.contains("macOS") && !key_store("macos").to.contains("GNOME"));
        assert!(key_store("windows").to.contains("Windows") && key_store("windows").pill.contains("gestionnaire"));
        // A system we do not know: no store named, still a sentence.
        assert_eq!(key_store("freebsd").to, "au trousseau du système");
        assert!(key_store("windows").missing.contains("Gestionnaire") && !key_store("windows").missing.contains("trousseau"));
    }

    #[test]
    fn each_system_is_named_in_english_too() {
        let _english = crate::lang::force(false);
        assert!(key_store("linux").to.contains("KWallet") && key_store("linux").to.starts_with("to "));
        assert!(key_store("macos").to.contains("macOS") && !key_store("macos").to.contains("GNOME"));
        assert!(key_store("windows").to.contains("Windows") && key_store("windows").pill.contains("credential"));
        assert_eq!(key_store("freebsd").to, "to the system key store");
        assert!(key_store("windows").missing.contains("Credential Manager") && !key_store("windows").missing.contains("key store"));
    }

    #[test]
    fn the_home_folder_is_a_tilde_only_at_a_separator() {
        assert_eq!(tilde("/home/a/.local/share/astrolabe", "/home/a"), "~/.local/share/astrolabe");
        assert_eq!(tilde("/home/a", "/home/a"), "~");
        assert_eq!(tilde("/home/ab/x", "/home/a"), "/home/ab/x");
        assert_eq!(tilde("/srv/data", "/home/a"), "/srv/data");
        assert_eq!(tilde("/srv/data", ""), "/srv/data");
        assert_eq!(tilde(r"C:\Users\a\AppData", r"C:\Users\a"), r"~\AppData");
    }
}
