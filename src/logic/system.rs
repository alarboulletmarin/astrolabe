//! What the settings say about the system the app runs on, by `std::env::consts::OS`.

pub struct KeyStore {
    /// After "confiée": where the key goes.
    pub to: &'static str,
    /// The label next to the key's title.
    pub pill: &'static str,
    /// Start of the sentence that says there is none.
    pub missing: &'static str,
}

pub fn key_store(os: &str) -> KeyStore {
    match os {
        "linux" => KeyStore { to: "au trousseau du système (GNOME Keyring ou KWallet)", pill: "dans le trousseau", missing: "Faute de trousseau système" },
        "macos" => KeyStore { to: "au Trousseau d'accès de macOS", pill: "dans le trousseau", missing: "Faute de Trousseau d'accès" },
        "windows" => KeyStore { to: "au Gestionnaire d'identifiants de Windows", pill: "dans le gestionnaire", missing: "Faute de Gestionnaire d'identifiants" },
        _ => KeyStore { to: "au trousseau du système", pill: "dans le trousseau", missing: "Faute de trousseau système" },
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
    fn the_home_folder_is_a_tilde_only_at_a_separator() {
        assert_eq!(tilde("/home/a/.local/share/astrolabe", "/home/a"), "~/.local/share/astrolabe");
        assert_eq!(tilde("/home/a", "/home/a"), "~");
        assert_eq!(tilde("/home/ab/x", "/home/a"), "/home/ab/x");
        assert_eq!(tilde("/srv/data", "/home/a"), "/srv/data");
        assert_eq!(tilde("/srv/data", ""), "/srv/data");
        assert_eq!(tilde(r"C:\Users\a\AppData", r"C:\Users\a"), r"~\AppData");
    }
}
