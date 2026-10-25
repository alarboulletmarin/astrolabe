//! Language: interface text in English or French (`tr`), how the system's language is detected,
//! and how numbers are written in each. Used by every layer.

use std::sync::OnceLock;

/// Interface text: French if the system's language is, English otherwise.
pub fn tr(en: &'static str, fr: &'static str) -> &'static str {
    if is_french() { fr } else { en }
}

/// Whether the interface speaks French. Tests run in French, to stay deterministic, unless a test
/// asks otherwise with `force`.
pub fn is_french() -> bool {
    #[cfg(test)]
    if let Some(forced) = FORCED.with(|f| f.get()) {
        return forced;
    }
    static FRENCH: OnceLock<bool> = OnceLock::new();
    *FRENCH.get_or_init(|| cfg!(test) || system_is_french())
}

#[cfg(test)]
thread_local! {
    static FORCED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Puts the interface in French or English on this thread, until the guard is dropped: a test
/// reads both without touching the process's environment.
#[cfg(test)]
pub fn force(french: bool) -> Forced {
    Forced(FORCED.with(|f| f.replace(Some(french))))
}

#[cfg(test)]
pub struct Forced(Option<bool>);

#[cfg(test)]
impl Drop for Forced {
    fn drop(&mut self) {
        FORCED.with(|f| f.set(self.0));
    }
}

/// Like `tr` for a text built at run time: both versions are written out, the right one is kept.
pub fn trf(en: String, fr: String) -> String {
    if is_french() { fr } else { en }
}

/// `one` for a single thing, `many` otherwise: French counts zero as one thing ("0 sortie"),
/// English does not ("0 outings").
pub fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    let single = if is_french() { n <= 1 } else { n == 1 };
    if single { one } else { many }
}

/// What goes between a label and what it says: French sets a space before the colon.
pub fn colon() -> &'static str {
    tr(": ", " : ")
}

/// The percent sign: French sets a space before it ("12 %"), English does not ("12%").
pub fn pct() -> &'static str {
    tr("%", " %")
}

/// Separates the thousands: a no-break space in French, a comma in English.
pub fn thousands() -> char {
    if is_french() { '\u{a0}' } else { ',' }
}

/// A number written with `.` as its decimal mark, as the language writes it.
pub fn decimal(number: String) -> String {
    if is_french() { number.replace('.', ",") } else { number }
}

/// `1 080` or `1,080`: a whole number with its thousands separated.
pub fn group(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(thousands());
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// `1st`, `2nd`, `11th`; `1er`, `2e`.
pub fn ordinal(n: usize) -> String {
    if is_french() {
        return if n == 1 { "1er".into() } else { format!("{n}e") };
    }
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// Locale variables first (launched from a terminal), otherwise the system:
/// an application launched from the Finder or the Start menu gets none.
fn system_is_french() -> bool {
    match ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty())) {
        Some(lang) => lang.starts_with("fr"),
        None => os_language_is_french(),
    }
}

#[cfg(windows)]
fn os_language_is_french() -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // A LANGID: the 10 low bits give the language (0x0c: French).
    (unsafe { GetUserDefaultUILanguage() } & 0x3ff) == 0x0c
}

#[cfg(target_os = "macos")]
fn os_language_is_french() -> bool {
    std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .ok()
        .and_then(|out| first_language(&String::from_utf8_lossy(&out.stdout)).map(|l| l.starts_with("fr")))
        .unwrap_or(false)
}

// ponytail: on Linux, only environment variables count (the system
// offers nothing else); on macOS the language comes from a call to `defaults` at startup.
#[cfg(not(any(windows, target_os = "macos")))]
fn os_language_is_french() -> bool {
    false
}

/// First language of the list that `defaults read -g AppleLanguages` prints:
/// `(\n    "fr-FR",\n    "en-US"\n)`, with or without quotes.
#[cfg(any(target_os = "macos", test))]
fn first_language(list: &str) -> Option<&str> {
    list.lines().map(str::trim).find(|l| !l.is_empty() && *l != "(" && *l != ")").map(|l| l.trim_end_matches(',').trim_matches('"'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_first_system_language() {
        // What `defaults read -g AppleLanguages` prints, with and without quotes.
        assert_eq!(first_language("(\n    \"fr-FR\",\n    \"en-US\"\n)\n"), Some("fr-FR"));
        assert_eq!(first_language("(\n    en,\n    fr\n)"), Some("en"));
        assert_eq!(first_language("(\n)"), None);
        assert_eq!(first_language(""), None);
    }

    #[test]
    fn the_override_holds_on_its_thread_and_lets_go() {
        assert_eq!(tr("yes", "oui"), "oui");
        {
            let _english = force(false);
            assert_eq!(tr("yes", "oui"), "yes");
            {
                let _french = force(true);
                assert_eq!(tr("yes", "oui"), "oui");
            }
            assert_eq!(tr("yes", "oui"), "yes");
        }
        assert_eq!(tr("yes", "oui"), "oui");
    }

    #[test]
    fn numbers_are_written_as_each_language_does() {
        let cases = [(0, "0", "0"), (999, "999", "999"), (1000, "1\u{a0}000", "1,000"), (100_000, "100\u{a0}000", "100,000"), (-1234, "-1\u{a0}234", "-1,234"), (1_234_567, "1\u{a0}234\u{a0}567", "1,234,567")];
        for (n, fr, en) in cases {
            assert_eq!(group(n), fr);
            let _english = force(false);
            assert_eq!(group(n), en);
        }
        assert_eq!(decimal("4.0".into()), "4,0");
        let _english = force(false);
        assert_eq!(decimal("4.0".into()), "4.0");
    }

    #[test]
    fn counts_and_ranks_in_both_languages() {
        // French: zero is singular; English: only one is.
        let words = |n| plural(n, "sortie", "sorties");
        assert_eq!((words(0), words(1), words(2)), ("sortie", "sortie", "sorties"));
        assert_eq!([1, 2, 12].map(ordinal), ["1er", "2e", "12e"]);
        let _english = force(false);
        let words = |n| plural(n, "outing", "outings");
        assert_eq!((words(0), words(1), words(2)), ("outings", "outing", "outings"));
        assert_eq!([1, 2, 3, 4, 11, 12, 13, 21, 22, 101, 111].map(ordinal), ["1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "101st", "111th"]);
    }
}
