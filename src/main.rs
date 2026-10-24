mod app;
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
            gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| app::Home::new(window, cx)))
                .expect("failed to open window");
        });
}

#[cfg(test)]
mod tests {
    /// The layers only depend downwards: `logic` and `platform` never see gpui nor the layers above
    /// them, and `ui` never sees the views nor the window.
    #[test]
    fn layers_only_depend_downwards() {
        use std::{fs, path::{Path, PathBuf}};
        let rules: &[(&str, &[&str])] = &[
            ("src/logic", &["gpui", "crate::platform", "crate::ui", "crate::views", "crate::app"]),
            ("src/platform", &["gpui", "crate::ui", "crate::views", "crate::app"]),
            ("src/ui", &["crate::platform", "crate::views", "crate::app", "impl Home"]),
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
}
