mod activity;
mod app;
mod chart;
mod detail;
mod fit;
mod garmin;
mod journal;
mod map;
mod mapview;
mod records;
mod heatmap;
mod settings;
mod stats;
mod theme;
mod ui;

use gpui_kit::*;

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            theme::init_fonts(cx);
            app::bind_keys(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1280.), px(840.)), cx)),
                titlebar: Some(TitlebarOptions { title: Some("Astrolabe".into()), ..Default::default() }),
                app_id: Some("dev.andrea.Astrolabe".into()),
                // Linux: the app draws its frame and buttons, the same on X11 and Wayland.
                #[cfg(target_os = "linux")]
                window_decorations: Some(WindowDecorations::Client),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| app::Home::new(window, cx)))
                .expect("failed to open window");
        });
}
