//! Floating chrome over a full-bleed canvas. Read-only: the UI visualizes agent work recorded in
//! SpacetimeDB; it offers navigation and inspection, never editing.

use crate::graph;
use crate::mock::{Lang, RUST_SOURCE};
use crate::palette::Pal;
use rune_ui::{Color, El, Family, TextStyle, col, el, row, text};

pub const MONO: TextStyle = TextStyle::new(11.0, 400.0).family(Family::Named("JetBrains Mono, DejaVu Sans Mono"));
const UI: TextStyle = TextStyle::new(11.5, 450.0);
const UI_STRONG: TextStyle = TextStyle::new(11.5, 600.0);

pub const W: f32 = 1600.0;
pub const H: f32 = 920.0;
const M: f32 = 16.0;
const DETAIL_W: f32 = 300.0;

fn label(s: &str, c: Color) -> El {
    text(s).font(UI).color(c).single_line()
}

fn mono(s: &str, c: Color) -> El {
    text(s).font(MONO).color(c)
}

fn dot(c: Color, d: f32) -> El {
    el().size(d, d).radius(d / 2.0).fill(c)
}

fn card(p: &Pal) -> El {
    col().fill(p.panel).radius(12.0).border(1.0, p.panel_border)
        .shadow(8.0, 30.0, p.shadow.alpha(if p.dark { 0.5 } else { 0.12 }))
}

pub fn view(p: &Pal) -> El {
    el().size(W, H)
        .child(graph::view(p, W, H, [110.0, 380.0]).absolute(0.0, 0.0))
        .child(breadcrumb(p).absolute(M, M))
        .child(live(p).absolute(W - M - 76.0, M))
        .child(detail(p).absolute(W - M - DETAIL_W, 68.0))
        .child(legend(p).absolute(M, H - M - 40.0))
}

fn breadcrumb(p: &Pal) -> El {
    card(p).child(row().h(40.0).px(14.0).gap(8.0).items_center()
        .child(text("◆").font(TextStyle::new(13.0, 700.0)).color(p.accent))
        .child(label("fixture", p.muted))
        .child(label("›", p.faint))
        .child(text("compute").font(UI_STRONG).color(p.text)))
}

fn live(p: &Pal) -> El {
    card(p).w(76.0).child(row().h(40.0).px(14.0).gap(7.0).items_center()
        .child(dot(p.ok, 7.0))
        .child(label("live", p.text)))
}

/// Detail for the selected item: what it is, its evidence, and its current implementation.
fn detail(p: &Pal) -> El {
    let stat = |big: &str, small: &str, c: Color| {
        col().grow(1.0).padding(10.0).gap(2.0).radius(8.0).fill(p.code_bg)
            .child(text(big).font(TextStyle::new(15.0, 650.0)).color(c))
            .child(label(small, p.muted))
    };
    let mut code = col().padding(10.0).radius(8.0).fill(p.code_bg);
    for line in RUST_SOURCE {
        code = code.child(mono(if line.is_empty() { " " } else { line }, p.text));
    }
    card(p).w(DETAIL_W).padding(14.0).gap(12.0)
        .child(row().gap(8.0).items_center()
            .child(dot(p.lang(Lang::Rust), 9.0))
            .child(text("step ∘ normalize").font(TextStyle::new(14.0, 650.0)).color(p.text)))
        .child(row().gap(8.0)
            .child(stat("33/33", "cases", p.ok))
            .child(stat("✓", "mixed build", p.ok)))
        .child(code)
}

/// Colour key and worker presence. One word each.
fn legend(p: &Pal) -> El {
    let key = |c: Color, s: &str| row().gap(6.0).items_center().child(dot(c, 8.0)).child(label(s, p.muted));
    let worker = |n: u8, s: &str, active: bool| {
        let a = el().size(18.0, 18.0).radius(9.0).fill(p.worker(n)).center()
            .child(text(n.to_string()).font(TextStyle::new(10.0, 700.0)).color(Color::WHITE));
        row().gap(6.0).items_center()
            .child(if active { a } else { a.opacity(0.6) })
            .child(label(s, p.muted))
    };
    card(p).child(row().h(40.0).px(14.0).gap(14.0).items_center()
        .child(key(p.lang(Lang::C), "C"))
        .child(key(p.lang(Lang::Rust), "Rust"))
        .child(el().w(1.0).h(18.0).fill(p.panel_border))
        .child(worker(1, "translating", true))
        .child(worker(2, "queued", false)))
}
