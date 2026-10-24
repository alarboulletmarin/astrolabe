//! What a finished synchronisation says (pure).

use crate::logic::activity::group;

/// "3 nouvelles · 3 fichiers lus · 1 échec": what a synchronisation brought, with only what
/// is not zero, or "rien de nouveau" when nothing came.
pub fn summary(added: usize, files: usize, failed: usize) -> String {
    let many = |n: usize, one: &str, other: &str| format!("{} {}", group(n as i64), if n == 1 { one } else { other });
    let mut parts = Vec::new();
    if added > 0 {
        parts.push(many(added, "nouvelle", "nouvelles"));
    }
    if files > 0 {
        parts.push(many(files, "fichier lu", "fichiers lus"));
    }
    if failed > 0 {
        parts.push(many(failed, "échec", "échecs"));
    }
    if parts.is_empty() { "rien de nouveau".into() } else { parts.join(" · ") }
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
}
