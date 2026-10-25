mod app;
mod data;
mod lang;
mod logic;
mod platform;
mod ui;
mod views;

use gpui_kit::*;

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            ui::theme::init_fonts(cx);
            app::bind_keys(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1280.), px(840.)), cx)),
                titlebar: Some(TitlebarOptions { title: Some("Astrolabe".into()), ..Default::default() }),
                app_id: Some("dev.andrea.Astrolabe".into()),
                // Linux: the app draws its frame and buttons, the same on X11 and Wayland.
                #[cfg(target_os = "linux")]
                window_decorations: Some(WindowDecorations::Client),
                // The margin around the frame carries its shadow: it must stay see-through.
                window_background: WindowBackgroundAppearance::Transparent,
                window_min_size: Some(size(px(360.), px(240.))),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| app::Shell::new(window, cx)))
                .expect("failed to open window");
        });
}

#[cfg(test)]
mod tests {
    /// The layers only depend downwards (logic ← platform ← ui ← data ← views ← app): `logic` and
    /// `platform` never see gpui nor the layers above them, and the views never see the window.
    #[test]
    fn layers_only_depend_downwards() {
        use std::{fs, path::{Path, PathBuf}};
        let rules: &[(&str, &[&str])] = &[
            ("src/logic", &["gpui", "crate::platform", "crate::ui", "crate::data", "crate::views", "crate::app"]),
            ("src/platform", &["gpui", "crate::ui", "crate::data", "crate::views", "crate::app"]),
            ("src/ui", &["crate::platform", "crate::data", "crate::views", "crate::app"]),
            ("src/data", &["crate::views", "crate::app"]),
            ("src/views", &["crate::app"]),
        ];
        fn files(dir: &Path) -> Vec<PathBuf> {
            let entries = fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().path());
            entries.flat_map(|path| if path.is_dir() { files(&path) } else { vec![path] }).collect()
        }
        for (dir, banned) in rules {
            for path in files(Path::new(dir)) {
                let code = fs::read_to_string(&path).unwrap();
                // Comments may name anything; only code counts.
                let code: String = code.lines().map(|l| l.split("//").next().unwrap()).collect::<Vec<_>>().join("\n");
                for word in *banned {
                    assert!(!code.contains(word), "{} uses `{word}`", path.display());
                }
            }
        }
    }

    /// The text the interface shows is written with `tr(en, fr)` (or `trf`): no accented French
    /// letter may sit in a string literal outside those calls, so French text cannot slip in
    /// untranslated. Its limits: French without an accent ("Journal", "Allure") and French held in
    /// a constant or a table that does not go through `tr` are not seen; the files that hold French
    /// words on purpose (month names, the names devices give) are left out.
    #[test]
    fn no_french_text_outside_tr() {
        use std::{fs, path::{Path, PathBuf}};
        const ON_PURPOSE: [&str; 3] = ["src/lang.rs", "src/ui/format.rs", "src/logic/journal.rs"];
        const FRENCH_LETTERS: &str = "àâäéèêëîïôöùûüçœÀÂÉÈÊÎÔÙÛÇŒ«»";
        fn files(dir: &Path) -> Vec<PathBuf> {
            let entries = fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().path());
            entries.flat_map(|path| if path.is_dir() { files(&path) } else { vec![path] }).collect()
        }
        /// The string literals of `code` that are not inside a `tr(…)` or `trf(…)` call.
        fn loose_literals(code: &str) -> Vec<String> {
            let c: Vec<char> = code.chars().collect();
            let (mut i, mut found) = (0, Vec::new());
            // Whether a `tr(` call is being read, and the depth of its parentheses.
            let (mut depth, mut inside) = (0usize, false);
            while i < c.len() {
                match c[i] {
                    '/' if c.get(i + 1) == Some(&'/') => {
                        while i < c.len() && c[i] != '\n' {
                            i += 1;
                        }
                        continue;
                    }
                    '"' => {
                        let start = i + 1;
                        i += 1;
                        while i < c.len() && c[i] != '"' {
                            i += if c[i] == '\\' { 2 } else { 1 };
                        }
                        if !inside {
                            found.push(c[start..i.min(c.len())].iter().collect());
                        }
                    }
                    // A char literal ('é', '\n') or a lifetime ('a): only the first has a closing quote.
                    '\'' => {
                        if c.get(i + 1) == Some(&'\\') {
                            i += 3;
                        } else if c.get(i + 2) == Some(&'\'') {
                            i += 2;
                        }
                    }
                    '(' if inside => depth += 1,
                    ')' if inside => {
                        depth -= 1;
                        inside = depth > 0;
                    }
                    _ => {}
                }
                // `tr(` and `trf(` open a call; the check is on the `t`.
                if !inside && c[i] == 't' && (i == 0 || !(c[i - 1].is_alphanumeric() || c[i - 1] == '_')) {
                    for name in ["trf(", "tr("] {
                        if code_starts(&c, i, name) {
                            inside = true;
                            depth = 1;
                            i += name.len() - 1;
                            break;
                        }
                    }
                }
                i += 1;
            }
            found
        }
        fn code_starts(c: &[char], i: usize, word: &str) -> bool {
            word.chars().enumerate().all(|(k, w)| c.get(i + k) == Some(&w))
        }
        let mut hits = Vec::new();
        for path in files(Path::new("src")) {
            let name = path.to_string_lossy().replace('\\', "/");
            if ON_PURPOSE.contains(&name.as_str()) || name.ends_with("tests.rs") {
                continue;
            }
            let code = fs::read_to_string(&path).unwrap();
            let code = code.split("#[cfg(test)]\nmod tests").next().unwrap();
            for literal in loose_literals(code) {
                if literal.chars().any(|ch| FRENCH_LETTERS.contains(ch)) {
                    hits.push(format!("{name}: {literal}"));
                }
            }
        }
        assert!(hits.is_empty(), "French outside `tr`:\n{}", hits.join("\n"));
        // The scan itself sees what it should.
        assert_eq!(loose_literals(r#"a("é"); tr("e", "é"); trf(format!("x"), format!("é")); b("ç")"#), ["é", "ç"]);
        assert_eq!(loose_literals("x(\"a\\\"é\") // \"é\"\n'é'; 'a: \"b\""), ["a\\\"é", "b"]);
    }
}
