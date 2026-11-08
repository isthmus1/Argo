//! Light and dark palettes. Every colour the views use comes from here; nothing hard-codes a mode.

use crate::model::{Category, Lang, Ty};
use rune_ui::Color;

#[derive(Clone, Copy)]
pub struct Pal {
    pub dark: bool,
    pub canvas: Color,
    pub grid_minor: Color,
    pub grid_major: Color,
    pub node_body: Color,
    pub node_border: Color,
    pub node_text: Color,
    pub frame_fill: Color,
    pub frame_text: Color,
    pub panel: Color,
    pub panel_border: Color,
    pub text: Color,
    pub muted: Color,
    /// The quietest text; still 4.5:1 on panels and the canvas.
    pub faint: Color,
    /// Selection and primary-button fill; white text on it stays 4.5:1.
    pub accent: Color,
    pub code_bg: Color,
    pub shadow: Color,
    pub exec: Color,
    /// Outline around exec wires and diamonds.
    pub exec_casing: Color,
    /// What wires away from the hovered or selected item fade toward. Dark mode fades into the
    /// canvas; light mode into a grey. Either stops at `WIRE_FLOOR` (see [`Pal::dim_wire`]).
    pub wire_dim: Color,
    pub u32_: Color,
    pub bool_: Color,
    pub ptr: Color,
    pub ok: Color,
    pub bad: Color,
}

fn h(s: &str) -> Color {
    rune_ui::color::hex(s).expect("valid hex literal")
}

/// Least contrast a faded wire keeps against what it is drawn over (WCAG non-text, 3:1).
pub const WIRE_FLOOR: f32 = 3.0;

/// WCAG 2 contrast ratio. `Color` holds linear components, so its luminance is relative
/// luminance directly.
pub fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

impl Pal {
    pub fn new(dark: bool) -> Pal {
        if dark { dark_pal() } else { light_pal() }
    }

    pub fn ty(&self, ty: Ty) -> Color {
        match ty {
            Ty::Exec => self.exec,
            Ty::U32 => self.u32_,
            Ty::Bool => self.bool_,
            Ty::Ptr => self.ptr,
        }
    }

    /// Blender-style full header fill per node family.
    pub fn header(&self, c: Category) -> Color {
        let (dark, light) = match c {
            Category::Interface => ("#1d6b5b", "#26816c"),
            // Magenta, so red stays reserved for rejected/error state.
            Category::Control => ("#86377b", "#a54c98"),
            Category::Function => ("#2a5d82", "#3a75a3"),
            Category::Expression => ("#474c99", "#5a60b4"),
            Category::Variable => ("#6b5326", "#8f6e2f"),
            Category::Statement => ("#4a5160", "#5f6878"),
            Category::Type => ("#56622a", "#6b7a33"),
        };
        h(if self.dark { dark } else { light })
    }

    /// Worker identity colour, stable per worker number; never the selection accent.
    pub fn worker(&self, worker: u8) -> Color {
        // One agent-activity colour no node family, pin type or language uses; workers are
        // told apart by number.
        let _ = worker;
        h(if self.dark { "#b9a3ff" } else { "#7c5cd6" })
    }

    /// Text drawn on a worker colour: dark on the light amber/cyan, white otherwise.
    pub fn on_worker(&self, worker: u8) -> Color {
        let _ = worker;
        if self.dark { h("#17131f") } else { Color::WHITE }
    }

    /// Language chip (fill, text, edge): solid slate for C, rust orange for Rust, independent of
    /// header hue and of equal weight. Both fills are dark enough for their small white label
    /// (4.5:1); the C chip's lighter edge keeps it apart from the slate Statement header.
    pub fn chip(&self, lang: Lang) -> (Color, Color, Color) {
        match (lang, self.dark) {
            (Lang::Rust, _) => (h("#c4521f"), Color::WHITE, Color::BLACK.alpha(0.2)),
            (_, true) => (h("#5b6678"), Color::WHITE, h("#8b95a7")),
            (_, false) => (h("#475569"), Color::WHITE, h("#8592a6")),
        }
    }

    /// An expanded frame's fill over the canvas, as one opaque colour: what wires inside a frame
    /// are seen against.
    pub fn framed(&self) -> Color {
        self.canvas.mix(self.frame_fill.alpha(1.0), self.frame_fill.a)
    }

    /// A wire colour faded for wires away from the hovered or selected item. Mixed toward an
    /// opaque colour, never by alpha: translucent polylines double-blend at their joints. It goes
    /// up to `DIM_MIX` of the way to `wire_dim`, so dense graphs stay calm, but stops where it
    /// would fall under `WIRE_FLOOR` contrast against the canvas or a frame's fill, so every
    /// faded wire stays traceable.
    pub fn dim_wire(&self, c: Color) -> Color {
        let backs = [self.canvas, self.framed()];
        let ok = |t: f32| backs.iter().all(|&b| contrast(c.mix(self.wire_dim, t), b) >= WIRE_FLOOR);
        let most = if self.dark { 0.6 } else { 0.75 };
        if ok(most) {
            return c.mix(self.wire_dim, most);
        }
        // Contrast falls monotonically along the mix while the colour stays on its side of the
        // backdrops, so bisect for the furthest mix that still clears the floor.
        let (mut lo, mut hi) = (0.0f32, most);
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            if ok(mid) { lo = mid } else { hi = mid }
        }
        c.mix(self.wire_dim, lo)
    }

    /// Small type-name text: darker than the pin colour in light mode for contrast.
    pub fn ty_text(&self, ty: Ty) -> Color {
        if self.dark {
            return self.ty(ty);
        }
        match ty {
            Ty::U32 => h("#007d70"),
            Ty::Bool => h("#a52f68"),
            Ty::Ptr => h("#315da8"),
            Ty::Exec => self.text,
        }
    }
}

fn dark_pal() -> Pal {
    Pal {
        dark: true,
        canvas: h("#18181a"),
        grid_minor: h("#1e1e21"),
        grid_major: h("#26262a"),
        node_body: h("#2b2b2f"),
        node_border: h("#3a3a40"),
        node_text: h("#e4e4e7"),
        frame_fill: h("#34343e").alpha(0.40),
        frame_text: h("#d4d4dc"),
        panel: h("#242428"),
        panel_border: h("#38383e"),
        text: h("#e4e4e7"),
        muted: h("#aeb4be"),
        faint: h("#888d96"),
        accent: h("#3b6fd9"),
        code_bg: h("#1c1c1f"),
        shadow: Color::BLACK.alpha(0.5),
        // About 75 % of white's lightness, so exec reads first without drowning the data wires.
        exec: h("#b8b8bc"),
        exec_casing: h("#0d0d0f"),
        wire_dim: h("#18181a"),
        u32_: h("#3fb6a2"),
        bool_: h("#d6588a"),
        ptr: h("#6b8fd6"),
        ok: h("#6ee7a0"),
        bad: h("#ff8a80"),
    }
}

fn light_pal() -> Pal {
    Pal {
        dark: false,
        canvas: h("#f1f1f3"),
        grid_minor: h("#e6e7eb"),
        grid_major: h("#dcdee3"),
        node_body: h("#ffffff"),
        node_border: h("#d9dbe0"),
        node_text: h("#202024"),
        frame_fill: h("#d6d6e4").alpha(0.40),
        frame_text: h("#2c2c34"),
        panel: h("#ffffff"),
        panel_border: h("#dadae0"),
        text: h("#1f1f23"),
        muted: h("#59616c"),
        faint: h("#686d76"),
        accent: h("#3d6fc2"),
        code_bg: h("#f3f3f6"),
        shadow: Color::BLACK.alpha(0.12),
        exec: h("#3b404c"),
        exec_casing: h("#ffffff"),
        wire_dim: h("#86868c"),
        u32_: h("#118a78"),
        bool_: h("#b8406e"),
        ptr: h("#3f63b0"),
        ok: h("#067647"),
        bad: h("#b42318"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_text_meets_wcag_aa() {
        let (light, dark) = (Pal::new(false), Pal::new(true));
        for p in [light, dark] {
            // Both language chips' labels.
            for lang in [Lang::Rust, Lang::C] {
                let (fill, fg, _) = p.chip(lang);
                assert!(contrast(fg, fill) >= 4.5, "{lang:?} chip {}", contrast(fg, fill));
            }
            // White primary-button labels on the accent.
            assert!(contrast(Color::WHITE, p.accent) >= 4.5, "white/accent {}", contrast(Color::WHITE, p.accent));
        }
        // Faint text on panels and on the canvas, in both modes.
        for (mode, p) in [("light", light), ("dark", dark)] {
            assert!(contrast(p.faint, p.panel) >= 4.5, "{mode} faint/panel {}", contrast(p.faint, p.panel));
            assert!(contrast(p.faint, p.canvas) >= 4.5, "{mode} faint/canvas {}", contrast(p.faint, p.canvas));
            // White header titles on every node family's header.
            for c in [Category::Interface, Category::Control, Category::Function, Category::Expression, Category::Variable, Category::Statement, Category::Type] {
                let r = contrast(Color::WHITE, p.header(c));
                assert!(r >= 4.5, "{mode} white on {c:?} header {r}");
            }
        }
        // The darker dark-mode accent still marks a selection on the canvas (3:1, non-text).
        assert!(contrast(dark.accent, dark.canvas) >= 3.0, "accent/canvas {}", contrast(dark.accent, dark.canvas));
    }

    #[test]
    fn faded_wires_stay_traceable_on_canvas_and_frames_but_weaker() {
        for p in [Pal::new(false), Pal::new(true)] {
            let mode = if p.dark { "dark" } else { "light" };
            for ty in [Ty::Exec, Ty::U32, Ty::Bool, Ty::Ptr] {
                let (full, dim) = (p.ty(ty), p.dim_wire(p.ty(ty)));
                for (on, back) in [("canvas", p.canvas), ("frame", p.framed())] {
                    let r = contrast(dim, back);
                    assert!(r >= WIRE_FLOOR - 1e-3, "{mode} {ty:?} faded on {on}: {r}");
                    // Weaker: visibly less contrast than the full colour where that has room.
                    assert!(r < contrast(full, back) - 0.4 || contrast(full, back) < WIRE_FLOOR + 0.5, "{mode} {ty:?} on {on}");
                }
            }
        }
    }

    #[test]
    fn the_c_chip_stands_apart_from_slate_headers() {
        for p in [Pal::new(false), Pal::new(true)] {
            let (fill, _, edge) = p.chip(Lang::C);
            let head = p.header(Category::Statement);
            // A solid chip, with an edge lighter than both it and the header around it.
            assert_eq!(fill.a, 1.0);
            assert!(edge.luminance() > fill.luminance() && edge.luminance() > head.luminance(), "dark: {}", p.dark);
            assert!(contrast(edge, head) >= 1.5, "dark: {} edge/header {}", p.dark, contrast(edge, head));
        }
    }
}
