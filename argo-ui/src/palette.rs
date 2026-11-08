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
    pub faint: Color,
    pub accent: Color,
    pub code_bg: Color,
    pub shadow: Color,
    pub exec: Color,
    /// Outline around exec wires and diamonds.
    pub exec_casing: Color,
    pub u32_: Color,
    pub bool_: Color,
    pub ptr: Color,
    pub ok: Color,
    /// An unjudged result: neither green (no report backs it) nor red (nothing failed).
    pub pending: Color,
    pub bad: Color,
}

fn h(s: &str) -> Color {
    rune_ui::color::hex(s).expect("valid hex literal")
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
            Category::Interface => ("#1d6b5b", "#2a8a74"),
            // Magenta, so red stays reserved for rejected/error state.
            Category::Control => ("#86377b", "#a54c98"),
            Category::Function => ("#2a5d82", "#3a75a3"),
            Category::Expression => ("#474c99", "#5a60b4"),
            Category::Variable => ("#6b5326", "#8f6e2f"),
            Category::Statement => ("#4a5160", "#5f6878"),
            Category::Type => ("#56622a", "#6e7d35"),
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

    /// Language chip (fill, text): neutral for C, rust orange for Rust, independent of header hue.
    pub fn chip(&self, lang: Lang) -> (Color, Color) {
        match (lang, self.dark) {
            (Lang::Rust, _) => (h("#e8703a"), Color::WHITE),
            (_, true) => (h("#3a3f47"), h("#c9d1d9")),
            (_, false) => (h("#e8eaed"), h("#3c4043")),
        }
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
        faint: h("#7a7f88"),
        accent: h("#5b8def"),
        code_bg: h("#1c1c1f"),
        shadow: Color::BLACK.alpha(0.5),
        exec: h("#e8e8e8"),
        exec_casing: h("#0d0d0f"),
        u32_: h("#3fb6a2"),
        bool_: h("#d6588a"),
        ptr: h("#6b8fd6"),
        ok: h("#6ee7a0"),
        pending: h("#f2c45a"),
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
        faint: h("#8a8f98"),
        accent: h("#3d6fc2"),
        code_bg: h("#f3f3f6"),
        shadow: Color::BLACK.alpha(0.12),
        exec: h("#3b404c"),
        exec_casing: h("#ffffff"),
        u32_: h("#118a78"),
        bool_: h("#b8406e"),
        ptr: h("#3f63b0"),
        ok: h("#067647"),
        pending: h("#9a6700"),
        bad: h("#b42318"),
    }
}
