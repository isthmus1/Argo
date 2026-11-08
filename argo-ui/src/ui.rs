//! Floating chrome over a full-bleed canvas. Read-only: the UI visualizes agent work recorded in
//! SpacetimeDB; it offers navigation and inspection, never editing. Opening a project and
//! requesting a rewrite live in `dialogs`.

use crate::app::{Item, MARGIN, State};
use crate::describe;
use crate::graph::{self, RADIUS};
use crate::model::{Category, Evidence, Ty};
use crate::palette::Pal;
use rune_ui::{Color, Cursor, El, Family, LineCap, Path, Shape, TextStyle, canvas, col, el, row, text};

pub const MONO: TextStyle = TextStyle::new(11.0, 400.0).family(Family::Named("JetBrains Mono, DejaVu Sans Mono"));
pub(crate) const UI: TextStyle = TextStyle::new(11.5, 450.0);
pub(crate) const UI_STRONG: TextStyle = TextStyle::new(11.5, 600.0);
pub(crate) const LABEL: TextStyle = TextStyle::new(10.0, 650.0).tracking(0.4);

/// Default window size; the layout follows the actual window (see [`viewport`]).
pub const W: f32 = 1440.0;
pub const H: f32 = 900.0;
/// The smallest window the layout is designed for.
pub const MIN_W: f32 = 760.0;
pub const MIN_H: f32 = 520.0;
pub(crate) const M: f32 = 16.0;
/// Top of the canvas area below the floating top bar.
pub(crate) const TOP: f32 = 72.0;

thread_local! {
    /// The window's size in points, as last laid out (the default size until the first layout).
    static VIEWPORT: std::cell::Cell<[f32; 2]> = const { std::cell::Cell::new([W, H]) };
    /// Light/dark override chosen in the app; `None` follows the system.
    static THEME: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// The window size in points.
pub fn viewport() -> [f32; 2] {
    VIEWPORT.with(|v| v.get())
}

/// Record a new window size; returns the previous one.
pub fn set_viewport(size: [f32; 2]) -> [f32; 2] {
    VIEWPORT.with(|v| v.replace(size))
}

/// The chosen appearance: the in-app override, else the system's.
pub fn dark(system_dark: bool) -> bool {
    THEME.with(|t| t.get()).unwrap_or(system_dark)
}

/// Set (or clear, with `None`) the in-app light/dark override.
pub fn set_theme(dark: Option<bool>) {
    THEME.with(|t| t.set(dark));
}
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
pub fn free_area(detail_open: bool, explorer: bool) -> [f32; 4] {
    let [w, h] = viewport();
    let right = if detail_open { detail_w() + 2.0 * M } else { M };
    let left = if explorer { M + explorer_w() + M } else { M };
    [left, TOP, (w - left - right).max(120.0), (h - TOP - M).max(120.0)]
}

/// Docked details width: narrower in small windows so the canvas keeps most of the width.
pub fn detail_w() -> f32 {
    (viewport()[0] * 0.3).clamp(280.0, DETAIL_W)
}

/// Explorer width, narrowing with the window like the details panel.
pub fn explorer_w() -> f32 {
    (viewport()[0] * 0.22).clamp(220.0, EXPLORER_W)
}

/// Width of the project explorer docked on the left.
pub const EXPLORER_W: f32 = 290.0;

pub(crate) fn label(s: &str, c: Color) -> El {
    text(s).font(UI).color(c).single_line()
}

fn mono(s: &str, c: Color) -> El {
    text(s).font(MONO).color(c)
}

pub(crate) fn card(p: &Pal) -> El {
    col().fill(p.panel).radius(RADIUS).border(1.0, p.panel_border)
        .shadow(8.0, 30.0, p.shadow.alpha(if p.dark { 0.5 } else { 0.12 }))
}

/// The whole window. `inset` is the width macOS's traffic lights take at the top-left (0
/// elsewhere); the breadcrumb starts after them.
pub fn view(p: &Pal, s: &State, inset: f32) -> El {
    let [w, h] = viewport();
    let left = if inset > 0.0 { inset + 12.0 } else { M };
    let mut root = el().key("root").w_full().h_full().fill(p.canvas)
        .on_layout(|s: &mut State, r| s.resized([r[2], r[3]]))
        .child(graph::view(p, s, w, h).absolute(0.0, 0.0))
        .child(breadcrumb(p, s).absolute(left, M))
        .child(row().gap(8.0).child(crate::dialogs::toolbar(p, s)).child(status(p, s)).pin(None, Some(M), Some(M), None));
    if s.project.is_some() {
        root = root.child(crate::dialogs::explorer(p, s).absolute(M, TOP));
    }
    if let Some(item) = s.selected {
        root = root.child(info(p, s, item, true).0.w(detail_w()).absolute(w - M - detail_w(), TOP));
    }
    if let Some(item) = s.hovered.filter(|h| Some(*h) != s.selected) {
        let (card, estimate) = info(p, s, item, false);
        let w = hover_w(s, item);
        let [x, y] = hover_anchor(s, item, w, hover_h(item, w, estimate));
        // Keyed per item so a newly hovered card always reports its own first layout.
        root = root.child(card.w(w).key(format!("hover-{item:?}"))
            .on_layout(move |_: &mut State, r| HOVER_SIZE.with(|c| c.set(Some((item, [r[2], r[3]])))))
            .absolute(x, y));
    }
    if s.modal.is_some() {
        root = root.child(crate::dialogs::modal(p, s).absolute(0.0, 0.0));
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

/// Hover-card width: a card with port rows uses the full width; one with only its kind and title
/// fits them, so no empty band trails the title.
fn hover_w(s: &State, item: Item) -> f32 {
    let Item::Node(i) = item else { return HOVER_W };
    let n = &s.view.nodes[i];
    let c = describe::card(&s.view, i);
    if !c.inputs.is_empty() || !c.outputs.is_empty() || n.claim.is_some() || n.evidence == Evidence::Rejected {
        return HOVER_W;
    }
    let chars = |t: &str| t.chars().count() as f32;
    // Padding, kind tile and gap, title; then whichever header marks are present.
    let mut title = 14.0 * 2.0 + 22.0 + 9.0 + chars(&n.title) * 8.2;
    if c.effects.iter().any(|e| *e != "control flow") {
        title += 9.0 + 20.0;
    }
    if n.evidence != Evidence::None {
        title += 9.0 + 20.0;
    }
    if n.lang != crate::model::Lang::None {
        title += 9.0 + 26.0;
    }
    let kicker = 14.0 * 2.0 + 8.0 + 6.0 + chars(n.kind.category().name()) * 7.6;
    title.max(kicker).clamp(160.0, HOVER_W)
}

/// Hover-card position for `item`: its screen rect, header height and the other nodes on the
/// canvas (direct neighbours weigh triple) feed [`place_card`].
fn hover_anchor(s: &State, item: Item, card_w: f32, card_h: f32) -> [f32; 2] {
    let rect = item_rect(s, item);
    let head = match item {
        Item::Node(_) => crate::model::HEADER,
        Item::Group(_) => FRAME_HEAD,
    } * s.cam.zoom;
    let me = match item {
        Item::Node(i) => Some(i),
        Item::Group(_) => None,
    };
    let neighbour = |i: usize| s.view.wires.iter().any(|wi| Some(wi.from.0) == me && wi.to.0 == i || Some(wi.to.0) == me && wi.from.0 == i);
    let others: Vec<([f32; 4], f32)> = (0..s.view.nodes.len()).filter(|i| Some(*i) != me)
        .map(|i| (s.screen_rect(i), if neighbour(i) { 3.0 } else { 1.0 }))
        .collect();
    place_card(rect, head, [card_w, card_h], free_area(s.selected.is_some(), s.project.is_some()), &others)
}

/// Height of a frame's title row in world units (as `graph::frame` draws it).
const FRAME_HEAD: f32 = 32.0;
/// Space between a hover card and the item it describes.
const CARD_GAP: f32 = 14.0;

/// Where a floating card of size `card` goes beside an item at `item` (`[x, y, w, h]`, window
/// points) whose header row is `head` tall, inside the `free` canvas rect. Candidates sit right,
/// left, below and above the item (each also aligned to the item's far edge), clamped into the
/// free area. Clamping can push a candidate back over the item, so the ranking is: candidates
/// clear of the item first; failing that, ones that leave its header row (its name) visible;
/// then the rest. Within a rank the card covers the least area, of the item plus the weighted
/// area of `others` (rect, weight); ties keep the earlier candidate (right, left, below, above).
pub(crate) fn place_card(item: [f32; 4], head: f32, card: [f32; 2], free: [f32; 4], others: &[([f32; 4], f32)]) -> [f32; 2] {
    let [x, y, w, h] = item;
    let [cw, ch] = card;
    let [fx, fy, fw, fh] = free;
    let clamp = |cx: f32, cy: f32| [cx.clamp(fx, (fx + fw - cw).max(fx)), cy.clamp(fy, (fy + fh - ch).max(fy))];
    let gap = CARD_GAP;
    let candidates = [
        clamp(x + w + gap, y),
        clamp(x - gap - cw, y),
        clamp(x, y + h + gap),
        clamp(x, y - gap - ch),
        clamp(x + w + gap, y + h - ch),
        clamp(x - gap - cw, y + h - ch),
        clamp(x + w - cw, y + h + gap),
        clamp(x + w - cw, y - gap - ch),
        // Last resort for an item too big to sit beside: over its body, under its header.
        clamp(x + w - cw, y + head + gap),
        clamp(x, y + head + gap),
    ];
    let header = [x, y, w, head.min(h)];
    let rank = |c: [f32; 2]| {
        let r = [c[0], c[1], cw, ch];
        let on_item = overlap(r, item);
        let tier = if on_item <= 0.0 { 0 } else if overlap(r, header) <= 0.0 { 1 } else { 2 };
        let covered: f32 = others.iter().map(|(o, wt)| overlap(r, *o) * wt).sum();
        (tier, on_item + covered)
    };
    candidates.into_iter().min_by(|a, b| {
        let (a, b) = (rank(*a), rank(*b));
        a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))
    }).unwrap_or([fx, fy])
}

thread_local! {
    /// The hover card's laid-out size, with the item it described: the next frame places the
    /// card by its real size rather than the estimate.
    static HOVER_SIZE: std::cell::Cell<Option<(Item, [f32; 2])>> = const { std::cell::Cell::new(None) };
}

/// Hover card height for `item` at width `w`: as last laid out when that was this card at this
/// width, else `estimate`.
fn hover_h(item: Item, w: f32, estimate: f32) -> f32 {
    match HOVER_SIZE.with(|c| c.get()) {
        Some((i, [mw, mh])) if i == item && (mw - w).abs() < 0.5 && mh > 0.0 => mh,
        _ => estimate,
    }
}

/// Hover rows shown per port list before summarising.
const HOVER_PORTS: usize = 5;

/// Effect as a header icon (label in its tooltip); the docked panel spells it out.
fn effect_glyph(e: &str) -> &'static str {
    match e {
        "pure" => "∘",
        "memory read" | "memory read/write" => "⇄",
        "control flow" => "◇",
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

/// Line heights of the `LABEL` and `UI` text styles: Rune lays a line out 1.25 font sizes tall.
const LABEL_LINE: f32 = 10.0 * 1.25;
const UI_LINE: f32 = 11.5 * 1.25;

/// Height of a list of `n` 22-point rows 2 points apart, cut at `limit` with a "+N more" line.
fn rows_h(n: usize, limit: usize) -> f32 {
    let shown = n.min(limit);
    let mut h = shown as f32 * 22.0 + shown.saturating_sub(1) as f32 * 2.0;
    if n > limit {
        h += 2.0 + UI_LINE;
    }
    h
}

/// Hover card height before it is laid out, following `info` and `shell` piece by piece: the
/// padding (the border draws inside it), kicker over header, then each section 12 points down.
fn estimate_h(sections: &[f32]) -> f32 {
    let head = LABEL_LINE + 4.0 + 28.0;
    2.0 * 14.0 + head + sections.iter().map(|s| 12.0 + s).sum::<f32>()
}

/// Height of the activity section (claim row, failed-checks row), if any.
fn activity_h(claimed: bool, rejected: bool) -> Option<f32> {
    match (claimed, rejected) {
        (false, false) => None,
        (true, false) => Some(22.0),
        (false, true) => Some(24.0),
        (true, true) => Some(22.0 + 6.0 + 24.0),
    }
}

/// Height of the hover card's port section, if the node has ports.
fn ports_h(inputs: usize, outputs: usize) -> Option<f32> {
    match (inputs, outputs) {
        (0, 0) => None,
        (n, 0) | (0, n) => Some(rows_h(n, HOVER_PORTS)),
        (i, o) => Some(rows_h(i, HOVER_PORTS) + 6.0 + 1.0 + 6.0 + rows_h(o, HOVER_PORTS)),
    }
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
                .child(col().gap(4.0)
                    .child(kicker(p, n.kind.category().name(), p.header(n.kind.category())))
                    .child(header(p, &n.title, graph::kind_glyph(n.kind), p.header(n.kind.category()), n.evidence, n.lang, &c.effects, (n.kind == crate::model::Kind::Group).then_some(n.members))));
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
            let sections: Vec<f32> = [activity_h(n.claim.is_some(), n.evidence == Evidence::Rejected), ports_h(c.inputs.len(), c.outputs.len())]
                .into_iter().flatten().collect();
            (shell(p, body, docked, p.header(n.kind.category())), estimate_h(&sections))
        }
        Item::Group(g) => {
            let grp = &s.view.groups[g];
            let members: Vec<&crate::model::Node> = s.view.nodes.iter().filter(|n| n.group == Some(g)).collect();
            let mut body = col().gap(12.0)
                .child(col().gap(4.0)
                    .child(kicker(p, "node group", p.header(Category::Function)))
                    .child(header(p, &grp.title, "▣", p.header(Category::Function), grp.evidence, grp.lang, &[], Some(members.len()))));
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
            let sections: Vec<f32> = [activity_h(grp.claim.is_some(), grp.evidence == Evidence::Rejected), Some(rows_h(members.len(), 5))]
                .into_iter().flatten().collect();
            (shell(p, body, docked, p.header(Category::Function)), estimate_h(&sections))
        }
    }
}

/// What kind of node this is, in words, above the title: the header colour's meaning.
fn kicker(p: &Pal, what: &str, tile: Color) -> El {
    row().gap(6.0).items_center()
        .child(el().size(8.0, 8.0).radius(2.0).fill(tile))
        .child(text(what.to_uppercase()).font(LABEL).color(p.muted))
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
    // Control flow is already named above the title; other effects keep their icon.
    if let Some(e) = effects.iter().find(|e| **e != "control flow") {
        r = r.child(el().size(20.0, 20.0).center().help(*e)
            .child(text(effect_glyph(e)).font(TextStyle::new(12.0, 650.0)).color(p.muted)));
    }
    // Absent marks add nothing, not an empty slot plus its gap.
    if evidence != Evidence::None {
        r = r.child(status_glyph(p, evidence));
    }
    if lang != crate::model::Lang::None {
        r = r.child(graph::lang_chip(p, lang));
    }
    r
}

fn shell(p: &Pal, body: El, docked: bool, family: Color) -> El {
    if docked {
        col().h(viewport()[1] - TOP - M).fill(p.panel).radius(RADIUS).border(1.0, p.panel_border).padding(16.0).child(body)
            .shadow(4.0, 16.0, p.shadow.alpha(if p.dark { 0.4 } else { 0.08 }))
    } else {
        col().fill(p.panel).radius(RADIUS).border(1.0, family.alpha(0.45)).padding(14.0).child(body)
            .shadow(8.0, 24.0, p.shadow.alpha(if p.dark { 0.6 } else { 0.18 }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREE: [f32; 4] = [16.0, 72.0, 1000.0, 800.0];
    const CARD: [f32; 2] = [340.0, 166.0];

    fn place(item: [f32; 4], others: &[([f32; 4], f32)]) -> [f32; 4] {
        let [x, y] = place_card(item, 30.0, CARD, FREE, others);
        [x, y, CARD[0], CARD[1]]
    }

    fn inside(r: [f32; 4], f: [f32; 4]) -> bool {
        r[0] >= f[0] && r[1] >= f[1] && r[0] + r[2] <= f[0] + f[2] + 0.01 && r[1] + r[3] <= f[1] + f[3] + 0.01
    }

    #[test]
    fn open_canvas_puts_the_card_right_of_the_item() {
        let item = [400.0, 400.0, 140.0, 90.0];
        let r = place(item, &[]);
        assert_eq!([r[0], r[1]], [400.0 + 140.0 + CARD_GAP, 400.0]);
    }

    #[test]
    fn the_side_covering_fewest_nodes_wins() {
        let item = [400.0, 400.0, 140.0, 90.0];
        // A neighbour to the right: the card goes left instead.
        let right = ([560.0, 380.0, 140.0, 120.0], 3.0);
        let r = place(item, &[right]);
        assert_eq!([r[0], r[1]], [400.0 - CARD_GAP - CARD[0], 400.0]);
        assert_eq!(overlap(r, right.0), 0.0);
    }

    #[test]
    fn a_clamped_candidate_over_the_item_loses_to_any_clear_one() {
        // The reported case: an item near the top of the canvas with busy sides. "Above" only
        // fits once clamped down onto the item's header (one point of it here), where it covers
        // no other node; the clear candidates beside and below all cover nodes, yet one of them
        // must win. (Weighing item overlap as just another cost picked "above".)
        let item = [400.0, 237.0, 140.0, 90.0];
        let others = [
            ([560.0, 240.0, 300.0, 300.0], 3.0),
            ([80.0, 240.0, 300.0, 300.0], 3.0),
            ([380.0, 340.0, 400.0, 200.0], 1.0),
        ];
        let r = place(item, &others);
        assert_eq!(overlap(r, item), 0.0, "card {r:?} covers the item");
        assert!(inside(r, FREE));
    }

    #[test]
    fn near_the_right_edge_the_card_flips_left() {
        let item = [860.0, 300.0, 140.0, 90.0];
        let r = place(item, &[]);
        assert_eq!(overlap(r, item), 0.0);
        assert_eq!(r[0], 860.0 - CARD_GAP - CARD[0]);
    }

    #[test]
    fn near_each_corner_the_card_stays_clear_and_inside() {
        for item in [[16.0, 72.0, 140.0, 90.0], [16.0, 782.0, 140.0, 90.0], [876.0, 72.0, 140.0, 90.0], [876.0, 782.0, 140.0, 90.0]] {
            let r = place(item, &[]);
            assert_eq!(overlap(r, item), 0.0, "item {item:?} card {r:?}");
            assert!(inside(r, FREE), "item {item:?} card {r:?}");
        }
    }

    #[test]
    fn with_no_clear_spot_the_items_header_stays_visible() {
        // A frame filling the canvas: the card has to sit on it, but not on its title row.
        let item = [10.0, 60.0, 1020.0, 820.0];
        let head = 32.0;
        let [x, y] = place_card(item, head, CARD, FREE, &[]);
        let r = [x, y, CARD[0], CARD[1]];
        assert_eq!(overlap(r, [item[0], item[1], item[2], head]), 0.0, "card {r:?} covers the title");
        assert!(inside(r, FREE));
    }

    #[test]
    fn with_no_clear_spot_the_card_avoids_other_nodes_below_the_header() {
        let item = [10.0, 60.0, 1020.0, 820.0];
        // Nodes along the top-left under the header; the right side is free.
        let others = [([16.0, 110.0, 600.0, 200.0], 1.0)];
        let [x, y] = place_card(item, 32.0, CARD, FREE, &others);
        assert_eq!(overlap([x, y, CARD[0], CARD[1]], others[0].0), 0.0);
    }

    #[test]
    fn over_a_zoomed_frame_the_card_prefers_empty_frame_to_member_nodes() {
        // A frame at zoom 3 filling a 1024x640 window past its right edge. The card has to sit
        // on the frame; a spot hanging off its left edge covers less frame but a member node,
        // and must lose to an empty stretch of frame under the title.
        let free = [16.0, 72.0, 992.0, 552.0];
        let item = [104.0, 110.0, 1200.0, 700.0];
        let card = [340.0, 234.0];
        let head = 96.0;
        let others = [([157.0, 207.0, 388.0, 310.0], 1.0), ([786.0, 537.0, 300.0, 100.0], 1.0), ([157.0, 603.0, 360.0, 100.0], 1.0)];
        let [x, y] = place_card(item, head, card, free, &others);
        let r = [x, y, card[0], card[1]];
        assert_eq!(overlap(r, [item[0], item[1], item[2], head]), 0.0, "card {r:?} covers the title");
        for (o, _) in others {
            assert_eq!(overlap(r, o), 0.0, "card {r:?} covers {o:?}");
        }
    }

    #[test]
    fn estimate_matches_measured_cards() {
        // Heights measured from headless captures of the real cards.
        // `acc = 0`: one output.
        assert_eq!(estimate_h(&[ports_h(0, 1).unwrap()]), 106.5);
        // `acc + i`: two inputs, one output.
        assert_eq!(estimate_h(&[ports_h(2, 1).unwrap()]), 165.5);
        // `report`: claimed, failed checks, one data input.
        assert_eq!(estimate_h(&[activity_h(true, true).unwrap(), ports_h(1, 0).unwrap()]), 170.5);
        // Cut lists add a "+N more" line (14.375 points measured, plus the row gap).
        assert_eq!(rows_h(7, 5), rows_h(5, 5) + 2.0 + 14.375);
        assert_eq!(ports_h(0, 0), None);
        assert_eq!(activity_h(false, false), None);
    }

    #[test]
    fn measured_size_is_used_only_for_the_same_card() {
        HOVER_SIZE.with(|c| c.set(Some((Item::Node(3), [340.0, 200.0]))));
        assert_eq!(hover_h(Item::Node(3), 340.0, 150.0), 200.0);
        assert_eq!(hover_h(Item::Node(4), 340.0, 150.0), 150.0);
        assert_eq!(hover_h(Item::Node(3), 220.0, 150.0), 150.0);
        HOVER_SIZE.with(|c| c.set(None));
        assert_eq!(hover_h(Item::Node(3), 340.0, 150.0), 150.0);
    }
}
