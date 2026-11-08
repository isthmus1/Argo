//! Floating chrome over a full-bleed canvas. Read-only: the UI visualizes agent work recorded in
//! SpacetimeDB; it offers navigation and inspection, never editing.

use crate::app::{Item, MARGIN, State};
use crate::describe::{self, Card};
use crate::graph::{self, RADIUS};
use crate::model::{Category, Evidence, Ty};
use crate::palette::Pal;
use rune_ui::{Color, Cursor, El, Family, LineCap, Path, Shape, TextStyle, canvas, col, el, row, text};

pub const MONO: TextStyle = TextStyle::new(11.0, 400.0).family(Family::Named("JetBrains Mono, DejaVu Sans Mono"));
const UI: TextStyle = TextStyle::new(11.5, 450.0);
const UI_STRONG: TextStyle = TextStyle::new(11.5, 600.0);
const LABEL: TextStyle = TextStyle::new(10.0, 650.0).tracking(0.4);

pub const W: f32 = 1600.0;
pub const H: f32 = 920.0;
const M: f32 = 16.0;
const DETAIL_W: f32 = 380.0;
const HOVER_W: f32 = 340.0;

const RUST_SOURCE: &[&str] = &[
    "pub fn step(acc: u32, i: u32) -> u32 {",
    "    normalize(acc.wrapping_add(i))",
    "}",
    "",
    "fn normalize(x: u32) -> u32 {",
    "    x & 0xff",
    "}",
];

/// Canvas area not covered by floating panels: x, y, w, h.
pub fn free_area(detail_open: bool) -> [f32; 4] {
    let right = if detail_open { DETAIL_W + 2.0 * M } else { M };
    [M, 72.0, W - M - right, H - 72.0 - 2.0 * M - LEGEND_H]
}

const LEGEND_H: f32 = 32.0;

/// What the marks mean, for the kinds actually on screen: wire and socket styles, header colours,
/// and the language chip.
fn legend(p: &Pal, s: &State) -> El {
    use crate::model::{Category, Lang, Ty};
    let item = |mark: El, what: &str| row().gap(6.0).items_center().child(mark).child(label(what, p.muted));
    let glyph = |g: &str, c: Color| text(g).font(TextStyle::new(12.0, 700.0)).color(c);
    let mut r = row().h(LEGEND_H).px(12.0).gap(16.0).items_center()
        .child(item(glyph("◆──▶", p.ty(Ty::Exec)), "runs next"))
        .child(item(glyph("◆┄┄▶", p.ty(Ty::Exec)), "back to loop"));
    if s.view.wires.iter().any(|w| s.view.wire_ty(w) != Ty::Exec)
        || s.view.nodes.iter().any(|n| n.ins.iter().chain(&n.outs).any(|p| p.ty != Ty::Exec))
    {
        r = r.child(item(glyph("●──▶", p.ty(Ty::U32)), "value"));
    }
    r = r.child(el().w(1.0).h(16.0).fill(p.panel_border));
    for (c, what) in [
        (Category::Interface, "function in / out"),
        (Category::Control, "control flow"),
        (Category::Statement, "statements"),
        (Category::Variable, "variable"),
        (Category::Function, "call"),
        (Category::Expression, "expression"),
        (Category::Type, "type"),
    ] {
        if s.view.nodes.iter().any(|n| n.kind.category() == c) {
            r = r.child(item(el().size(14.0, 10.0).radius(3.0).fill(p.header(c)), what));
        }
    }
    if s.view.nodes.iter().any(|n| n.lang == Lang::C) {
        r = r.child(el().w(1.0).h(16.0).fill(p.panel_border))
            .child(item(graph::lang_chip(p, Lang::C), "written in C"));
    }
    card(p).child(r)
}

fn label(s: &str, c: Color) -> El {
    text(s).font(UI).color(c).single_line()
}

fn mono(s: &str, c: Color) -> El {
    text(s).font(MONO).color(c)
}

fn card(p: &Pal) -> El {
    col().fill(p.panel).radius(RADIUS).border(1.0, p.panel_border)
        .shadow(8.0, 30.0, p.shadow.alpha(if p.dark { 0.5 } else { 0.12 }))
}

pub fn view(p: &Pal, s: &State) -> El {
    let mut root = el().size(W, H)
        .child(graph::view(p, s, W, H).absolute(0.0, 0.0))
        .child(breadcrumb(p, s).absolute(M, M))
        .child(status(p, s).pin(None, Some(M), Some(M), None))
        .child(legend(p, s).absolute(M, H - M - LEGEND_H));
    if let Some(item) = s.selected {
        root = root.child(info(p, s, item, true).0.w(DETAIL_W).absolute(W - M - DETAIL_W, 72.0));
    }
    if let Some(item) = s.hovered.filter(|h| Some(*h) != s.selected) {
        let (card, h) = info(p, s, item, false);
        let [x, y] = hover_anchor(s, item, h);
        root = root.child(card.w(HOVER_W).absolute(x, y));
    }
    root
}

fn breadcrumb(p: &Pal, s: &State) -> El {
    let mut r = row().h(40.0).px(14.0).gap(8.0).items_center()
        .child(text("◆").font(TextStyle::new(13.0, 700.0)).color(p.accent));
    let last = s.view.crumbs.len().saturating_sub(1);
    for (i, c) in s.view.crumbs.iter().enumerate() {
        if i > 0 {
            r = r.child(label("›", p.faint));
        }
        r = r.child(if i == last { text(c.clone()).font(UI_STRONG).color(p.text) } else { label(c, p.muted) });
    }
    card(p).child(r)
}

fn icon_button(p: &Pal, content: El, help: &str) -> El {
    el().size(32.0, 32.0).radius(8.0).center().hover(p.code_bg).cursor(Cursor::Pointer).help(help).child(content)
}

/// Connection plus worker presence, one row of icon buttons with tooltips.
fn status(p: &Pal, s: &State) -> El {
    let conn = {
        let c = if s.online { p.ok } else { p.faint };
        el().size(14.0, 14.0).radius(7.0).border(2.0, c.alpha(0.35)).center().child(el().size(6.0, 6.0).radius(3.0).fill(c))
    };
    let mut workers: Vec<(u8, bool, String)> = vec![];
    for n in &s.graph.nodes {
        if let Some(c) = n.claim {
            workers.push((c.worker, c.active, n.title.clone()));
        }
    }
    for g in &s.graph.groups {
        if let Some(c) = g.claim {
            workers.push((c.worker, c.active, g.title.clone()));
        }
    }
    workers.sort_by_key(|w| w.0);
    let mut r = row().h(40.0).px(6.0).gap(4.0).items_center()
        .child(icon_button(p, conn, if s.online { "SpacetimeDB · live" } else { "SpacetimeDB · offline" }));
    r = r.child(el().w(1.0).h(18.0).fill(p.panel_border).mx(3.0));
    for (worker, active, title) in &workers {
        let a = graph::avatar(p, *worker, true, 22.0).child(state_glyph(p, *active).absolute(12.0, 12.0));
        let tip = format!("worker {worker} · {} {title}", if *active { "translating" } else { "queued for" });
        r = r.child(icon_button(p, a, &tip));
    }
    r = r.child(el().w(1.0).h(18.0).fill(p.panel_border).mx(3.0))
        .child(icon_button(p, canvas(fit_icon(p)).size(14.0, 14.0), "Frame whole graph (F)").on_press(|s: &mut State| s.fit_all()));
    card(p).child(r)
}

/// A small spinner arc (translating) or pause bars (queued) on the avatar's corner.
fn state_glyph(p: &Pal, active: bool) -> El {
    let mut sh = Shape::new();
    if active {
        let pts: Vec<[f32; 2]> = (0..=12).map(|i| {
            let a = 0.4 + i as f32 * 0.42;
            [6.0 + 3.2 * a.cos(), 6.0 + 3.2 * a.sin()]
        }).collect();
        sh = sh.stroke(pts, 1.5, Color::WHITE);
    } else {
        sh = sh.line([4.5, 3.5], [4.5, 8.5], 1.5, LineCap::Round, Color::WHITE).line([7.5, 3.5], [7.5, 8.5], 1.5, LineCap::Round, Color::WHITE);
    }
    el().size(12.0, 12.0).radius(6.0).fill(if active { p.ok } else { p.faint }).border(2.0, p.panel).child(canvas(sh).size(12.0, 12.0))
}

fn fit_icon(p: &Pal) -> Shape {
    let c = p.muted;
    let mut sh = Shape::new();
    for (x, y, dx, dy) in [(1.0, 1.0, 1.0, 1.0), (13.0, 1.0, -1.0, 1.0), (1.0, 13.0, 1.0, -1.0), (13.0, 13.0, -1.0, -1.0)] {
        sh = sh.stroke_path(&Path::new().move_to([x, y + 4.0 * dy]).line_to([x, y]).line_to([x + 4.0 * dx, y]), 1.6, LineCap::Round, c);
    }
    sh
}

/// Screen rectangle of an item under the camera.
fn item_rect(s: &State, item: Item) -> [f32; 4] {
    match item {
        Item::Node(i) => s.screen_rect(i),
        Item::Group(g) => {
            let [bx, by, _, _] = s.placed.bounds;
            let [x, y, w, h] = s.placed.frames[g];
            let z = s.cam.zoom;
            [s.cam.pan[0] + (x - bx + MARGIN) * z, s.cam.pan[1] + (y - by + MARGIN) * z, w * z, h * z]
        }
    }
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> f32 {
    let w = (a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0]);
    let h = (a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1]);
    if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
}

/// Hover-card position: try right, left, below and above the item; keep inside the free
/// canvas, never over the item, and prefer the side covering the fewest nodes (direct
/// neighbours count triple).
fn hover_anchor(s: &State, item: Item, card_h: f32) -> [f32; 2] {
    let [x, y, w, h] = item_rect(s, item);
    let gap = 14.0;
    let [fx, fy, fw, fh] = free_area(s.selected.is_some());
    let clamp = |cx: f32, cy: f32| [cx.clamp(fx, (fx + fw - HOVER_W).max(fx)), cy.clamp(fy, (fy + fh - card_h).max(fy))];
    let candidates = [
        clamp(x + w + gap, y),
        clamp(x - gap - HOVER_W, y),
        clamp(x, y + h + gap),
        clamp(x, y - gap - card_h),
    ];
    let neighbours: Vec<usize> = match item {
        Item::Node(i) => s.view.wires.iter().filter_map(|wi| {
            if wi.from.0 == i { Some(wi.to.0) } else if wi.to.0 == i { Some(wi.from.0) } else { None }
        }).collect(),
        Item::Group(_) => vec![],
    };
    let me = [x, y, w, h];
    let cost = |c: [f32; 2]| {
        let r = [c[0], c[1], HOVER_W, card_h];
        let mut cost = overlap(r, me) * 100.0;
        for i in 0..s.view.nodes.len() {
            let o = overlap(r, s.screen_rect(i));
            cost += if neighbours.contains(&i) { 3.0 * o } else { o };
        }
        cost
    };
    candidates.into_iter().min_by(|a, b| cost(*a).total_cmp(&cost(*b))).unwrap_or([fx, fy])
}

/// Hover rows shown per port list before summarising.
const HOVER_PORTS: usize = 5;

/// Effect as a header icon (label in its tooltip); the docked panel spells it out.
fn effect_glyph(e: &str) -> &'static str {
    match e {
        "pure" => "∘",
        "memory read" | "memory read/write" => "⇄",
        "control flow" => "⑂",
        "returns" => "⇤",
        _ => "⚡",
    }
}

fn status_glyph(p: &Pal, e: Evidence) -> El {
    let (g, c, tip) = match e {
        Evidence::None => return el(),
        Evidence::Accepted => ("✓", p.ok, "Rust implementation accepted"),
        Evidence::Rejected => ("✕", p.bad, "Latest Rust candidate failed its checks"),
    };
    el().size(20.0, 20.0).radius(10.0).fill(c.alpha(0.18)).center().help(tip)
        .child(text(g).font(TextStyle::new(11.0, 800.0)).color(c))
}

/// One compact line of agent activity, and one for a failed candidate. No prose banners.
fn activity(p: &Pal, claim: Option<crate::model::Claim>, evidence: Evidence) -> Option<El> {
    let mut c = col().gap(6.0);
    let mut any = false;
    if let Some(cl) = claim {
        any = true;
        c = c.child(row().h(22.0).gap(8.0).items_center()
            .child(graph::avatar(p, cl.worker, true, 18.0))
            .child(text(format!("worker {}", cl.worker)).font(UI_STRONG).color(p.text))
            .child(label(if cl.active { "translating" } else { "queued" }, p.muted)));
    }
    if evidence == Evidence::Rejected {
        any = true;
        c = c.child(row().h(24.0).px(8.0).radius(6.0).fill(p.bad.alpha(0.12)).gap(7.0).items_center()
            .help("The latest Rust candidate failed its checks; the C implementation stays selected.")
            .child(text("✕").font(TextStyle::new(11.0, 800.0)).color(p.bad))
            .child(text("checks failed").font(UI_STRONG).color(p.bad))
            .child(label("· C kept", p.muted)));
    }
    any.then_some(c)
}

fn ports(p: &Pal, rows: &[(String, Ty, String, String)], tag: &str, arrow: &str, limit: usize) -> El {
    let mut t = col().gap(2.0);
    for (k, (name, ty, ty_text, other)) in rows.iter().take(limit).enumerate() {
        t = t.child(row().h(22.0).gap(8.0).items_center()
            .child(text(if k == 0 { tag } else { "" }).font(LABEL).color(p.faint).w(26.0))
            .child(el().size(8.0, 8.0).radius(4.0).fill(p.ty(*ty)))
            .child(mono(name, p.text).single_line().w(64.0))
            .child(mono(ty_text, p.ty_text(*ty)).single_line().w(64.0))
            .child(label(&format!("{arrow} {other}"), p.muted).truncate().grow(1.0)));
    }
    if rows.len() > limit {
        t = t.child(label(&format!("+{} more", rows.len() - limit), p.faint).ml(34.0));
    }
    t
}

fn divider(p: &Pal) -> El {
    el().h(1.0).w_full().fill(p.panel_border)
}

/// Rough card height for placement (the card itself sizes to its content).
fn estimate_h(c: &Card, claimed: bool, rejected: bool) -> f32 {
    let ports = c.inputs.len().min(HOVER_PORTS + 1) + c.outputs.len().min(HOVER_PORTS + 1);
    28.0 + 30.0 + ports as f32 * 24.0 + if claimed { 30.0 } else { 0.0 } + if rejected { 32.0 } else { 0.0 }
}

/// IDE-style card for a node or group. Docked as a full-height sidebar for the selection
/// (complete: signature, flow, effects, code); floating beside the hovered item (summary).
fn info(p: &Pal, s: &State, item: Item, docked: bool) -> (El, f32) {
    match item {
        Item::Node(i) => {
            let n = &s.view.nodes[i];
            let c = describe::card(&s.view, i);
            let limit = if docked { usize::MAX } else { HOVER_PORTS };
            let mut body = col().gap(12.0)
                .child(header(p, &n.title, graph::kind_glyph(n.kind), p.header(n.kind.category()), n.evidence, n.lang, &c.effects, (n.kind == crate::model::Kind::Group).then_some(n.members)));
            if let Some(a) = activity(p, n.claim, n.evidence) {
                body = body.child(a);
            }
            if docked {
                body = body.child(col().padding(9.0).radius(8.0).fill(p.code_bg).child(mono(&c.signature, p.text).wrap()));
            }
            if !c.inputs.is_empty() || !c.outputs.is_empty() {
                let mut t = col().gap(6.0);
                if !c.inputs.is_empty() {
                    t = t.child(ports(p, &c.inputs, "IN", "←", limit));
                }
                if !c.inputs.is_empty() && !c.outputs.is_empty() {
                    t = t.child(divider(p));
                }
                if !c.outputs.is_empty() {
                    t = t.child(ports(p, &c.outputs, "OUT", "→", limit));
                }
                body = body.child(t);
            }
            if docked {
                if let Some(flow) = &c.flow {
                    body = body.child(row().gap(8.0).items_center()
                        .child(text("FLOW").font(LABEL).color(p.faint).w(50.0))
                        .child(label(flow, p.text).truncate().grow(1.0)));
                }
                body = body.child(row().gap(8.0).items_center()
                    .child(text("EFFECT").font(LABEL).color(p.faint).w(50.0))
                    .child(label(&c.effects.join(" · "), p.text)));
            }
            let h = estimate_h(&c, n.claim.is_some(), n.evidence == Evidence::Rejected);
            (shell(p, body, docked, p.header(n.kind.category())), h)
        }
        Item::Group(g) => {
            let grp = &s.view.groups[g];
            let members: Vec<&crate::model::Node> = s.view.nodes.iter().filter(|n| n.group == Some(g)).collect();
            let mut body = col().gap(12.0)
                .child(header(p, &grp.title, "▣", p.header(Category::Function), grp.evidence, grp.lang, &[], Some(members.len())));
            if let Some(a) = activity(p, grp.claim, grp.evidence) {
                body = body.child(a);
            }
            let mut list = col().gap(2.0);
            for n in members.iter().take(if docked { 16 } else { 5 }) {
                list = list.child(row().h(22.0).gap(8.0).items_center()
                    .child(el().size(8.0, 8.0).radius(2.0).fill(p.header(n.kind.category())))
                    .child(mono(&n.title, p.text).single_line().grow(1.0))
                    .child(graph::lang_chip(p, n.lang)));
            }
            if !docked && members.len() > 5 {
                list = list.child(label(&format!("+{} more", members.len() - 5), p.faint).ml(16.0));
            }
            body = body.child(list);
            if docked && s.scene == "fixture" {
                let mut code = col().padding(10.0).radius(8.0).fill(p.code_bg);
                for line in RUST_SOURCE {
                    code = code.child(mono(if line.is_empty() { " " } else { line }, p.text));
                }
                body = body.child(code);
            }
            (shell(p, body, docked, p.header(Category::Function)), 80.0 + members.len().min(6) as f32 * 24.0)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn header(p: &Pal, title: &str, glyph: &str, tile: Color, evidence: Evidence, lang: crate::model::Lang, effects: &[&'static str], count: Option<usize>) -> El {
    let mut r = row().h(28.0).gap(9.0).items_center()
        .child(el().size(22.0, 22.0).radius(7.0).fill(tile).center()
            .child(text(glyph).font(TextStyle::new(11.5, 650.0)).color(Color::WHITE)))
        .child(text(title).font(TextStyle::new(14.5, 650.0)).color(p.text).single_line().truncate().grow(1.0));
    if let Some(c) = count {
        r = r.child(row().h(18.0).px(6.0).radius(4.0).fill(p.code_bg).items_center().help("nodes in this group")
            .child(text(format!("▣ {c}")).font(TextStyle::new(10.5, 650.0)).color(p.muted)));
    }
    if let Some(e) = effects.first() {
        r = r.child(el().size(20.0, 20.0).center().help(*e)
            .child(text(effect_glyph(e)).font(TextStyle::new(12.0, 650.0)).color(p.muted)));
    }
    r.child(status_glyph(p, evidence)).child(graph::lang_chip(p, lang))
}

fn shell(p: &Pal, body: El, docked: bool, family: Color) -> El {
    if docked {
        col().h(H - 72.0 - M).fill(p.panel).radius(RADIUS).border(1.0, p.panel_border).padding(16.0).child(body)
            .shadow(4.0, 16.0, p.shadow.alpha(if p.dark { 0.4 } else { 0.08 }))
    } else {
        col().fill(p.panel).radius(RADIUS).border(1.0, family.alpha(0.45)).padding(14.0).child(body)
            .shadow(8.0, 24.0, p.shadow.alpha(if p.dark { 0.6 } else { 0.18 }))
    }
}
