//! Light and dark palettes. Every colour the views use comes from here; nothing hard-codes a mode.

use crate::model::{Lang, Ty};
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
    pub node_muted: Color,
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
    pub u32_: Color,
    pub bool_: Color,
    pub ptr: Color,
    pub structural: Color,
    pub lang_c: Color,
    pub lang_rust: Color,
    pub ok: Color,
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

    /// Node stripe and legend colour: the implementation language, neutral for structure.
    pub fn lang(&self, lang: Lang) -> Color {
        match lang {
            Lang::None => self.structural,
            Lang::C => self.lang_c,
            Lang::Rust => self.lang_rust,
        }
    }

    /// Worker identity colour, stable per worker number.
    pub fn worker(&self, worker: u8) -> Color {
        let hues = if self.dark { ["#4aa3df", "#b48ef0", "#e0b84a"] } else { ["#2a7fc0", "#8a5fd6", "#b8901c"] };
        h(hues[(worker as usize).saturating_sub(1) % hues.len()])
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
        node_muted: h("#8e8e96"),
        frame_fill: h("#34343e").alpha(0.40),
        frame_text: h("#8e8e9e"),
        panel: h("#242428").alpha(0.94),
        panel_border: h("#38383e"),
        text: h("#e4e4e7"),
        muted: h("#9a9aa2"),
        faint: h("#5e5e66"),
        accent: h("#5b8def"),
        code_bg: h("#1c1c1f"),
        shadow: Color::BLACK.alpha(0.5),
        exec: h("#e8e8e8"),
        u32_: h("#3fb6a2"),
        bool_: h("#d6588a"),
        ptr: h("#6b8fd6"),
        structural: h("#5a5a63"),
        lang_c: h("#7f97b3"),
        lang_rust: h("#e8844a"),
        ok: h("#5dbb63"),
        bad: h("#e5534b"),
    }
}

fn light_pal() -> Pal {
    Pal {
        dark: false,
        canvas: h("#f1f1f3"),
        grid_minor: h("#e8e8ec"),
        grid_major: h("#dcdce2"),
        node_body: h("#ffffff"),
        node_border: h("#d4d4da"),
        node_text: h("#202024"),
        node_muted: h("#6c6c75"),
        frame_fill: h("#d6d6e4").alpha(0.40),
        frame_text: h("#70708a"),
        panel: h("#ffffff").alpha(0.94),
        panel_border: h("#dadae0"),
        text: h("#1f1f23"),
        muted: h("#62626c"),
        faint: h("#a6a6ae"),
        accent: h("#3d6fc2"),
        code_bg: h("#f3f3f6"),
        shadow: Color::BLACK.alpha(0.12),
        exec: h("#2a2a2e"),
        u32_: h("#118a78"),
        bool_: h("#b8406e"),
        ptr: h("#3f63b0"),
        structural: h("#b4b4bc"),
        lang_c: h("#4f6886"),
        lang_rust: h("#c2581f"),
        ok: h("#2f8f3a"),
        bad: h("#c8372f"),
    }
}
