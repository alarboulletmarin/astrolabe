//! Bref's visual language: seven colors per theme, one accent, warm neutrals. Other hues only
//! come from the recipe (a hue at 10 % over the background, text made legible on it).

use gpui_kit::{App, Hsla, WindowAppearance, hsla, rgb};
use std::borrow::Cow;

pub const SANS: &str = "IBM Plex Sans";
pub const MONO: &str = "IBM Plex Mono";

const FONT_FILES: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexSans-Bold.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/IBMPlexMono-Bold.ttf"),
];

pub fn init_fonts(cx: &mut App) {
    cx.text_system()
        .add_fonts(FONT_FILES.iter().map(|f| Cow::Borrowed(*f)).collect())
        .ok();
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub bg: Hsla,
    pub text: Hsla,
    /// Markers, what should step back.
    pub dim: Hsla,
    /// Secondary text, still readable.
    pub soft: Hsla,
    pub accent: Hsla,
    pub panel: Hsla,
    pub border: Hsla,
    pub selection: Hsla,
}

/// `top` laid over `under` at `share`.
pub fn blend(top: Hsla, under: Hsla, share: f32) -> Hsla {
    let mut c = top;
    c.a = share;
    under.blend(c)
}

impl Theme {
    /// Bref Dark and Bref Light.
    pub fn for_appearance(a: WindowAppearance) -> Theme {
        let dark = matches!(a, WindowAppearance::Dark | WindowAppearance::VibrantDark);
        let c = if dark {
            [0x1b1a19, 0xe4e0da, 0x77716a, 0xe0926b, 0x161514, 0x3a3735]
        } else {
            [0xfbfaf8, 0x26231f, 0xa8a29a, 0xb4532a, 0xf1eeea, 0xe2ddd6]
        };
        let h = |i: usize| -> Hsla { rgb(c[i]).into() };
        let (bg, text, dim, accent) = (h(0), h(1), h(2), h(3));
        Theme {
            dark,
            bg,
            text,
            dim,
            soft: blend(text, dim, 0.45),
            accent,
            panel: h(4),
            border: h(5),
            selection: blend(accent, bg, 0.2),
        }
    }

    /// The recipe: (background tint, legible text) for a hue.
    pub fn hue(&self, h: f32) -> (Hsla, Hsla) {
        (
            blend(hsla(h, 0.7, 0.5, 1.0), self.bg, 0.10),
            hsla(h, 0.55, if self.dark { 0.72 } else { 0.36 }, 1.0),
        )
    }

    pub fn good(&self) -> (Hsla, Hsla) {
        self.hue(0.38)
    }

    pub fn bad(&self) -> Hsla {
        hsla(0.0, 0.6, if self.dark { 0.68 } else { 0.45 }, 1.0)
    }

    /// Chart marks at rest: quiet, a step above the hairlines.
    pub fn mark(&self) -> Hsla {
        blend(self.dim, self.bg, 0.55)
    }
}
