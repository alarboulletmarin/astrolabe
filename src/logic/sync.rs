//! What a finished synchronisation says (pure).

use crate::lang::{plural, tr};
use crate::logic::activity::group;

/// "3 nouvelles · 3 fichiers lus · 1 échec" ("3 new · 3 files read · 1 failure"): what a
/// synchronisation brought, with only what is not zero, or "rien de nouveau" ("nothing new") when
/// nothing came.
pub fn summary(added: usize, files: usize, failed: usize) -> String {
    let many = |n: usize, one: &'static str, other: &'static str| format!("{} {}", group(n as i64), plural(n, one, other));
    let mut parts = Vec::new();
    if added > 0 {
        parts.push(many(added, tr("new", "nouvelle"), tr("new", "nouvelles")));
    }
    if files > 0 {
        parts.push(many(files, tr("file read", "fichier lu"), tr("files read", "fichiers lus")));
    }
    if failed > 0 {
        parts.push(many(failed, tr("failure", "échec"), tr("failures", "échecs")));
    }
    if parts.is_empty() { tr("nothing new", "rien de nouveau").into() } else { parts.join(" · ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_only_what_is_not_zero() {
        assert_eq!(summary(0, 0, 0), "rien de nouveau");
        assert_eq!(summary(1, 1, 0), "1 nouvelle · 1 fichier lu");
        assert_eq!(summary(2, 2, 1), "2 nouvelles · 2 fichiers lus · 1 échec");
        assert_eq!(summary(0, 0, 2), "2 échecs");
        // Files alone (the "resume" button), and a large first sync.
        assert_eq!(summary(0, 9, 0), "9 fichiers lus");
        assert_eq!(summary(1_941, 1_900, 41), "1\u{a0}941 nouvelles · 1\u{a0}900 fichiers lus · 41 échecs");
    }

    #[test]
    fn says_it_in_english_too() {
        let _english = crate::lang::force(false);
        assert_eq!(summary(0, 0, 0), "nothing new");
        assert_eq!(summary(1, 1, 0), "1 new · 1 file read");
        assert_eq!(summary(2, 2, 1), "2 new · 2 files read · 1 failure");
        assert_eq!(summary(0, 0, 2), "2 failures");
        assert_eq!(summary(1_941, 1_900, 41), "1,941 new · 1,900 files read · 41 failures");
    }
}
