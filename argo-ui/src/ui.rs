//! Floating chrome over a full-bleed canvas. Read-only: the UI visualizes agent work (mock
//! scenes, protocol replays, snapshots and opened projects); it offers navigation and
//! inspection, never editing. Opening a project and requesting a rewrite live in `dialogs`.
//!
//! Controls inside a floating card sit `INSET` from every edge with a radius concentric with
//! the card's corner (`CONTROL_R = RADIUS - INSET`).

use crate::app::{Item, MARGIN, State};
use crate::describe;
use crate::graph::{self, RADIUS};
use crate::describe::{Mark, Port};
use crate::model::{Category, Evidence, Kind, Rejection};
use crate::palette::Pal;
use argo_protocol::NodeId;
use rune_ui::{Color, Cursor, El, Family, LineCap, Path, ScrollSpec, Shape, TextStyle, canvas, col, el, row, text};

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
/// Docked details width in large and small windows (see [`detail_w`]).
const DETAIL_W: f32 = 420.0;
const DETAIL_MIN_W: f32 = 260.0;
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

/// Docked details width, growing with the window from [`DETAIL_MIN_W`] at 1024 points wide to
/// [`DETAIL_W`] at 2560: small windows keep more canvas, large ones a proportionate panel.
pub fn detail_w() -> f32 {
    let t = (viewport()[0] - 1024.0) / (2560.0 - 1024.0);
    (DETAIL_MIN_W + t * (DETAIL_W - DETAIL_MIN_W)).clamp(DETAIL_MIN_W, DETAIL_W)
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

/// Height of the floating top-bar cards (breadcrumb, toolbar, status).
pub(crate) const BAR_H: f32 = 40.0;
/// How far a control sits inside a bar card, the same on every side.
pub(crate) const INSET: f32 = 4.0;
/// A control inside a bar card: its height and its radius, concentric with the card's corner.
pub(crate) const CONTROL_H: f32 = BAR_H - 2.0 * INSET;
pub(crate) const CONTROL_R: f32 = RADIUS - INSET;

/// A floating top-bar card holding one row of controls, each `INSET` from every edge.
pub(crate) fn bar(p: &Pal, controls: El) -> El {
    card(p).child(controls.h(BAR_H).padding(INSET).gap(INSET).items_center())
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
        // No card for an item hidden under the floating panels.
        let h = hover_h(item, w, estimate);
        if let Some([x, y]) = hover_anchor(s, item, w, h) {
            // Keyed per item so a newly hovered card always reports its own first layout.
            root = root.child(card.w(w).key(format!("hover-{item:?}"))
                .on_layout(move |_: &mut State, r| HOVER_SIZE.with(|c| c.set(Some((item, [r[2], r[3]])))))
                .absolute(x, y));
        }
    }
    if s.modal.is_some() {
        root = root.child(crate::dialogs::modal(p, s).absolute(0.0, 0.0));
    }
    root
}

/// The protocol containers behind the breadcrumb, root first, when the crumbs are a real
/// hierarchy (protocol scenes, snapshots and projects); `None` for mock scenes.
pub(crate) fn crumb_path(s: &State) -> Option<Vec<NodeId>> {
    let live = s.live.as_ref()?;
    let nodes = &live.snap.graph.nodes;
    let mut path = vec![];
    let mut cur = live.container;
    while let Some(id) = cur {
        let n = nodes.iter().find(|n| n.id == id)?;
        path.push(id);
        cur = n.parent;
    }
    path.reverse();
    (path.len() == s.view.crumbs.len()).then_some(path)
}

/// Show an ancestor's level: it becomes the container with its body opened (as the explorer
/// opens a function), relaid out and framed.
pub(crate) fn go_to_crumb(s: &mut State, id: NodeId) {
    crate::project::open_function(s, id);
    // A file crumb keeps that file expanded in the explorer.
    if s.project.as_ref().is_some_and(|p| p.modules.iter().any(|m| m.id == id)) {
        s.forms.expanded = Some(id);
    }
}

/// One breadcrumb: ancestors are buttons that navigate up; the current level, and every crumb
/// of a mock scene, is plain text.
fn crumb(p: &Pal, name: &str, current: bool, target: Option<NodeId>) -> El {
    let t = if current { text(name.to_string()).font(UI_STRONG).color(p.text) } else { label(name, p.muted) };
    let c = row().h(CONTROL_H).px(8.0).radius(CONTROL_R).items_center().child(t);
    match target {
        Some(id) if !current => c.hover(p.code_bg).cursor(Cursor::Pointer).help(format!("Show {name}"))
            .on_press(move |s: &mut State| go_to_crumb(s, id)),
        _ => c,
    }
}

/// Width of the breadcrumb's logo slot: the ◆ glyph plus a little air, so the first crumb sits
/// about as far from it as crumbs sit from each other.
const LOGO_W: f32 = 16.0;

fn breadcrumb(p: &Pal, s: &State) -> El {
    let path = crumb_path(s);
    let mut r = row().gap(0.0)
        .child(el().w(LOGO_W).h(CONTROL_H).center().child(text("◆").font(TextStyle::new(13.0, 700.0)).color(p.accent)));
    let last = s.view.crumbs.len().saturating_sub(1);
    for (i, c) in s.view.crumbs.iter().enumerate() {
        if i > 0 {
            r = r.child(label("›", p.faint));
        }
        r = r.child(crumb(p, c, i == last, path.as_ref().map(|p| p[i])).key(format!("crumb-{i}")));
    }
    bar(p, r.child(source_chip(p, &source(s)).ml(2.0).mr(4.0))).key("breadcrumb")
}

/// Where the graph comes from, as a passive chip after the crumbs: a small muted glyph and one
/// word. Not a control: no border, no hover fill, no accent; the details are in its tooltip.
fn source_chip(p: &Pal, src: &Source) -> El {
    row().key("source").h(20.0).px(4.0).gap(5.0).items_center().help(src.tip())
        .child(canvas(source_glyph(p, src)).size(10.0, 10.0))
        .child(text(src.label()).font(TextStyle::new(10.5, 550.0)).color(p.muted).single_line())
}

fn icon_button(p: &Pal, content: El, help: &str) -> El {
    sized_icon_button(p, content, help, CONTROL_H, CONTROL_R)
}

/// [`icon_button`] at another size, for controls outside the bars.
fn sized_icon_button(p: &Pal, content: El, help: &str, d: f32, r: f32) -> El {
    el().size(d, d).min_w(d).radius(r).center().hover(p.code_bg).cursor(Cursor::Pointer).help(help).child(content)
}

/// Where the graph on screen comes from. Nothing here is a live backend connection.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Source {
    /// A seeded mock scene with simulated agent activity.
    Mock(&'static str),
    /// The `argo-protocol` fixture, replaying its recorded updates (`left` still to come).
    Fixture { left: usize },
    /// A snapshot file (`ARGO_SNAPSHOT`).
    Snapshot(String),
    /// A C project opened from a git URL or directory.
    Project(String),
}

pub(crate) fn source(s: &State) -> Source {
    if let Some(proj) = &s.project {
        return Source::Project(proj.source.clone());
    }
    match &s.live {
        None => Source::Mock(s.scene),
        Some(_) if s.scene == "snapshot" => Source::Snapshot(std::env::var("ARGO_SNAPSHOT").unwrap_or_else(|_| "snapshot".into())),
        Some(live) => Source::Fixture { left: live.pending() },
    }
}

impl Source {
    fn tip(&self) -> String {
        match self {
            Source::Mock(scene) => format!("Mock scene “{scene}” · simulated agents"),
            Source::Fixture { left: 0 } => "Protocol fixture · replay finished".into(),
            Source::Fixture { left } => format!("Protocol fixture · replaying, {left} updates left"),
            Source::Snapshot(path) => format!("Snapshot file · {path}"),
            Source::Project(src) => format!("C project · {src}"),
        }
    }

    /// For a replay: recorded updates applied, of all.
    fn progress(&self) -> Option<(usize, usize)> {
        let Source::Fixture { left } = self else { return None };
        static TOTAL: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let total = *TOTAL.get_or_init(|| argo_protocol::fixture::transcript().len()).max(left);
        Some((total - left, total))
    }

    /// The chip's one word: `mock`, `replay 3/12` (updates applied of all), `snapshot`, `project`.
    fn label(&self) -> String {
        match self {
            Source::Mock(_) => "mock".into(),
            Source::Fixture { .. } => {
                let (done, total) = self.progress().unwrap_or_default();
                format!("replay {done}/{total}")
            }
            Source::Snapshot(_) => "snapshot".into(),
            Source::Project(_) => "project".into(),
        }
    }
}

/// The data source as a small neutral outline, 10 points square: a stack of cards (seeded
/// scene), a timeline with its playhead (recorded replay), a page (snapshot file), a folder
/// (project). Nothing here is a control.
fn source_glyph(p: &Pal, src: &Source) -> Shape {
    let c = p.muted;
    let w = 1.2;
    match src {
        Source::Mock(_) => {
            let back = Path::new().move_to([3.0, 1.0]).line_to([9.0, 1.0]).line_to([9.0, 6.5]);
            let front = Path::new().move_to([1.0, 3.5]).line_to([6.5, 3.5]).line_to([6.5, 9.0]).line_to([1.0, 9.0]).close();
            Shape::new().stroke_path(&back, w, LineCap::Round, c).stroke_path(&front, w, LineCap::Round, c)
        }
        Source::Fixture { .. } => {
            let (done, total) = src.progress().unwrap_or((1, 1));
            let x = 2.0 + 6.0 * done as f32 / total.max(1) as f32;
            Shape::new().line([1.0, 5.0], [9.0, 5.0], w, LineCap::Round, p.faint).line([1.0, 5.0], [x, 5.0], w, LineCap::Round, c)
                .fill(ring([x, 5.0], 2.0, 16), c)
        }
        Source::Snapshot(_) => {
            let outline = Path::new().move_to([2.0, 0.8]).line_to([6.0, 0.8]).line_to([8.5, 3.3]).line_to([8.5, 9.2]).line_to([2.0, 9.2]).close();
            Shape::new().stroke_path(&outline, w, LineCap::Round, c).line([3.8, 5.0], [6.7, 5.0], 1.0, LineCap::Round, c)
                .line([3.8, 7.0], [6.7, 7.0], 1.0, LineCap::Round, c)
        }
        Source::Project(_) => {
            let outline = Path::new().move_to([0.8, 2.0]).line_to([3.8, 2.0]).line_to([5.0, 3.2]).line_to([9.2, 3.2]).line_to([9.2, 8.6]).line_to([0.8, 8.6]).close();
            Shape::new().stroke_path(&outline, w, LineCap::Round, c)
        }
    }
}

/// Every claimed item on screen: worker, whether it is translating, title, and the item.
pub(crate) fn workers(s: &State) -> Vec<(u8, bool, String, Item)> {
    let mut out = vec![];
    for (i, n) in s.view.nodes.iter().enumerate() {
        if let Some(c) = n.claim {
            out.push((c.worker, c.active, n.title.clone(), Item::Node(i)));
        }
    }
    for (g, grp) in s.view.groups.iter().enumerate() {
        let drawn = !s.collapsed.get(g).copied().unwrap_or(false) && s.placed.frames.get(g).is_some_and(|f| f[2] > 0.0);
        if let Some(c) = grp.claim.filter(|_| drawn) {
            out.push((c.worker, c.active, grp.title.clone(), Item::Group(g)));
        }
    }
    out.sort_by_key(|w| (w.0, !w.1));
    out
}

/// Select `item` and pan it to the middle of the canvas left free beside the details panel
/// (its top-left corner when it is larger than that area).
pub(crate) fn reveal(s: &mut State, item: Item) {
    s.select(item);
    let [x, y, w, h] = item_rect(s, item);
    let [fx, fy, fw, fh] = free_area(true, s.project.is_some());
    let place = |at: f32, len: f32, from: f32, room: f32| if len <= room { from + (room - len) / 2.0 - at } else { from + 24.0 - at };
    s.cam.pan[0] += place(x, w, fx, fw);
    s.cam.pan[1] += place(y, h, fy, fh);
}

/// Worker presence (click one to go to its work), framing and appearance. The data source is a
/// passive chip in the breadcrumb, not here among the buttons.
fn status(p: &Pal, s: &State) -> El {
    let sep = || el().w(1.0).h(18.0).fill(p.panel_border).mx(3.0);
    let mut r = row();
    let workers = workers(s);
    let any_worker = !workers.is_empty();
    for (k, (worker, active, title, item)) in workers.into_iter().enumerate() {
        let a = graph::avatar(p, worker, true, AVATAR).child(state_glyph(p, active).absolute(BADGE_AT, BADGE_AT));
        let tip = format!("worker {worker} · {} {title}", if active { "translating" } else { "queued for" });
        r = r.child(icon_button(p, a, &tip).key(format!("worker-{k}")).on_press(move |s: &mut State| reveal(s, item)));
    }
    let dark = p.dark;
    if any_worker {
        r = r.child(sep());
    }
    r = r
        .child(icon_button(p, canvas(fit_icon(p)).size(14.0, 14.0), "Frame whole graph (F)").on_press(|s: &mut State| s.fit_all()))
        .child(icon_button(p, canvas(theme_icon(p, dark)).size(14.0, 14.0), if dark { "Switch to light mode" } else { "Switch to dark mode" })
            .key("theme").on_press(move |_: &mut State| set_theme(Some(!dark))));
    bar(p, r).key("status")
}

/// The appearance a click switches to: a moon in light mode, a sun in dark mode.
fn theme_icon(p: &Pal, dark: bool) -> Shape {
    let c = p.muted;
    let circle = |cx: f32, cy: f32, r: f32, from: f32, to: f32, n: usize| -> Vec<[f32; 2]> {
        (0..=n).map(|k| {
            let a = from + (to - from) * k as f32 / n as f32;
            [cx + r * a.cos(), cy + r * a.sin()]
        }).collect()
    };
    if dark {
        let mut sh = Shape::new().fill(circle(7.0, 7.0, 3.0, 0.0, std::f32::consts::TAU, 24), c);
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::TAU / 8.0;
            sh = sh.line([7.0 + 4.9 * a.cos(), 7.0 + 4.9 * a.sin()], [7.0 + 6.3 * a.cos(), 7.0 + 6.3 * a.sin()], 1.4, LineCap::Round, c);
        }
        sh
    } else {
        Shape::new().fill(crescent([7.0, 7.0], 6.0, [10.0, 4.0], 4.8), c)
    }
}

/// The part of the disc at `c0` (radius `r0`) outside the disc at `c1` (radius `r1`), as one
/// simple polygon: the outer arc, then the bite's arc back. The circles must intersect.
fn crescent(c0: [f32; 2], r0: f32, c1: [f32; 2], r1: f32) -> Vec<[f32; 2]> {
    let (dx, dy) = (c1[0] - c0[0], c1[1] - c0[1]);
    let d = (dx * dx + dy * dy).sqrt();
    let phi = dy.atan2(dx);
    // Half-angles of the intersection points, seen from each centre.
    let alpha = ((r0 * r0 + d * d - r1 * r1) / (2.0 * r0 * d)).clamp(-1.0, 1.0).acos();
    let beta = ((r1 * r1 + d * d - r0 * r0) / (2.0 * r1 * d)).clamp(-1.0, 1.0).acos();
    let n = 20;
    let mut pts = vec![];
    // Outer arc, the long way round away from the bite.
    for k in 0..=n {
        let a = phi + alpha + (std::f32::consts::TAU - 2.0 * alpha) * k as f32 / n as f32;
        pts.push([c0[0] + r0 * a.cos(), c0[1] + r0 * a.sin()]);
    }
    // The bite's arc inside the disc, facing back toward `c0`, from where the outer arc ended
    // (`back + beta` seen from `c1`) to where it began.
    let back = phi + std::f32::consts::PI;
    for k in 1..n {
        let a = back + beta - 2.0 * beta * k as f32 / n as f32;
        pts.push([c1[0] + r1 * a.cos(), c1[1] + r1 * a.sin()]);
    }
    pts
}

/// The toolbar's worker avatar diameter.
const AVATAR: f32 = 22.0;
/// The avatar's state badge: its dot, and the ring in the bar's colour around it.
const BADGE_DOT: f32 = 8.0;
const BADGE_RING: f32 = 1.5;
/// Where the badge's box sits in the avatar: its centre 10 points right of and below the
/// avatar's (on the rim's bottom-right, 14 out along the diagonal), so the dot stays clear of
/// the digit in the avatar's middle.
const BADGE_AT: f32 = AVATAR / 2.0 + 10.0 - (BADGE_DOT / 2.0 + BADGE_RING);

/// A small spinner arc (translating) or pause bars (queued) on the avatar's bottom-right,
/// ringed in the bar's colour so it reads as separate from the avatar.
fn state_glyph(p: &Pal, active: bool) -> El {
    let d = BADGE_DOT;
    let c = d / 2.0;
    let mut sh = Shape::new();
    if active {
        let pts: Vec<[f32; 2]> = (0..=12).map(|i| {
            let a = 0.4 + i as f32 * 0.42;
            [c + 2.2 * a.cos(), c + 2.2 * a.sin()]
        }).collect();
        sh = sh.stroke(pts, 1.2, Color::WHITE);
    } else {
        sh = sh.line([c - 1.3, c - 1.8], [c - 1.3, c + 1.8], 1.2, LineCap::Round, Color::WHITE)
            .line([c + 1.3, c - 1.8], [c + 1.3, c + 1.8], 1.2, LineCap::Round, Color::WHITE);
    }
    let dot = el().size(d, d).radius(c).fill(if active { p.ok } else { p.faint }).child(canvas(sh).size(d, d));
    let o = d + 2.0 * BADGE_RING;
    el().size(o, o).radius(o / 2.0).fill(p.panel).center().child(dot)
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
    if c.badge() {
        title += 9.0 + 20.0;
    }
    if n.evidence == Evidence::Accepted {
        title += 9.0 + 20.0;
    }
    if n.lang != crate::model::Lang::None {
        title += 9.0 + 26.0;
    }
    let kicker = 14.0 * 2.0 + 8.0 + 6.0 + chars(&kicker_text(n)) * 7.6;
    title.max(kicker).clamp(160.0, HOVER_W)
}

/// The part of `r` inside `clip`, or `None` when they don't overlap.
fn clip_rect(r: [f32; 4], clip: [f32; 4]) -> Option<[f32; 4]> {
    let (x0, y0) = (r[0].max(clip[0]), r[1].max(clip[1]));
    let (x1, y1) = ((r[0] + r[2]).min(clip[0] + clip[2]), (r[1] + r[3]).min(clip[1] + clip[3]));
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
}

/// The part of an item (`rect`, header row `head` tall) left visible in the `free` canvas, with
/// the header height still showing in it; `None` when less than about a header's worth shows,
/// so no card describes something the user can't see (an item under the docked panel).
fn visible_part(rect: [f32; 4], head: f32, free: [f32; 4]) -> Option<([f32; 4], f32)> {
    let vis = clip_rect(rect, free)?;
    let head = head.min(rect[3]);
    if vis[2] * vis[3] < rect[2] * head * 0.9 {
        return None;
    }
    Some((vis, (rect[1] + head - vis[1]).max(0.0)))
}

/// Weight of another node's or frame's title band in [`place_card`]: a card never hides a
/// neighbour's name if anywhere else will do.
const TITLE_WEIGHT: f32 = 4.0;

/// Hover-card position for `item`, or `None` when the item is hidden under the floating panels.
/// Its visible rect, header height and what else is on the canvas feed [`place_card`]: other nodes (direct neighbours weigh triple), every on-screen
/// node's header band and every other frame's title row, weighted by [`TITLE_WEIGHT`] on top,
/// and wire ends (see [`wire_end`]).
fn hover_anchor(s: &State, item: Item, card_w: f32, card_h: f32) -> Option<[f32; 2]> {
    let free = free_area(s.selected.is_some(), s.project.is_some());
    let z = s.cam.zoom;
    let head = match item {
        Item::Node(_) => crate::model::HEADER,
        Item::Group(_) => FRAME_HEAD,
    } * z;
    let (rect, head) = visible_part(item_rect(s, item), head, free)?;
    let me = match item {
        Item::Node(i) => Some(i),
        Item::Group(_) => None,
    };
    let [vw, vh] = viewport();
    let on_screen = |r: [f32; 4]| overlap(r, [0.0, 0.0, vw, vh]) > 0.0;
    let neighbour = |i: usize| s.view.wires.iter().any(|wi| Some(wi.from.0) == me && wi.to.0 == i || Some(wi.to.0) == me && wi.from.0 == i);
    let mut others: Vec<([f32; 4], f32)> = vec![];
    for i in (0..s.view.nodes.len()).filter(|i| Some(*i) != me) {
        let r = s.screen_rect(i);
        if !on_screen(r) {
            continue;
        }
        others.push((r, if neighbour(i) { 3.0 } else { 1.0 }));
        others.push(([r[0], r[1], r[2], (crate::model::HEADER * z).min(r[3])], TITLE_WEIGHT));
    }
    for g in 0..s.view.groups.len() {
        let drawn = !s.collapsed.get(g).copied().unwrap_or(false) && s.placed.frames.get(g).is_some_and(|f| f[2] > 0.0);
        if !drawn || item == Item::Group(g) {
            continue;
        }
        let r = item_rect(s, Item::Group(g));
        if on_screen(r) {
            others.push(([r[0], r[1], r[2], (FRAME_HEAD * z).min(r[3])], TITLE_WEIGHT));
        }
    }
    // Wire ends: each socket and the first stretch of its wire, so a card doesn't cut where
    // wires join nodes. The hovered item's own wires are what its card describes: their ends,
    // and the pin names at their far ends, weigh most.
    let inside = |n: usize| match item {
        Item::Node(i) => n == i,
        Item::Group(g) => s.view.nodes[n].group == Some(g),
    };
    for w in &s.view.wires {
        let hot = inside(w.from.0) || inside(w.to.0);
        for (n, output, idx) in [(w.from.0, true, w.from.1), (w.to.0, false, w.to.1)] {
            let r = s.screen_rect(n);
            if !on_screen(r) {
                continue;
            }
            let node = &s.view.nodes[n];
            let [lx, ly] = crate::model::socket_local(node, s.placed.size[n][0], output, idx);
            let at = [r[0] + lx * z, r[1] + ly * z];
            others.push((wire_end(at, output), if hot { HOT_WEIGHT } else { WIRE_WEIGHT }));
            if hot && !inside(n) {
                let pin = if output { &node.outs[idx].label } else { &node.ins[idx].label };
                others.push((pin_label(at, output, pin.chars().count(), z), HOT_WEIGHT));
            }
        }
    }
    Some(place_card(rect, head, [card_w, card_h], free, &others))
}

/// Weight of a socket and the start of its wire in [`place_card`], and of the hovered item's
/// own wire ends and the pins they reach.
const WIRE_WEIGHT: f32 = 2.0;
const HOT_WEIGHT: f32 = 10.0;
/// How far along its wire a socket's end region reaches, in window points.
const WIRE_STUB: f32 = 40.0;

/// A socket at `at` and the first [`WIRE_STUB`] points of its wire, which leaves an output to
/// the right and enters an input from the left.
fn wire_end(at: [f32; 2], output: bool) -> [f32; 4] {
    let (pad, h) = (8.0, 14.0);
    let x = if output { at[0] - pad } else { at[0] - WIRE_STUB };
    [x, at[1] - h / 2.0, WIRE_STUB + pad, h]
}

/// A socket's pin name inside its node (`chars` long at zoom `z`): left of an output's socket,
/// right of an input's.
fn pin_label(at: [f32; 2], output: bool, chars: usize, z: f32) -> [f32; 4] {
    let w = (chars as f32 * 6.5 + 16.0) * z;
    let h = crate::model::ROW * z;
    [if output { at[0] - w } else { at[0] }, at[1] - h / 2.0, w, h]
}

/// Height of a frame's title row in world units (as `graph::frame` draws it).
const FRAME_HEAD: f32 = 32.0;
/// Space between a hover card and the item it describes.
const CARD_GAP: f32 = 14.0;

/// Cost, in covered area, of each point a card sits farther from its item than [`CARD_GAP`]:
/// moving 100 points away is worth it to keep about a quarter of a node, or a few wire ends,
/// in view.
const DISTANCE_COST: f32 = 40.0;
/// Roughly how many spots of the free canvas [`place_card`] tries besides the item's sides.
const SCAN_SPOTS: f32 = 600.0;

/// Gap between two rects (0 when they touch or overlap).
fn rect_gap(a: [f32; 4], b: [f32; 4]) -> f32 {
    let dx = (b[0] - (a[0] + a[2])).max(a[0] - (b[0] + b[2])).max(0.0);
    let dy = (b[1] - (a[1] + a[3])).max(a[1] - (b[1] + b[3])).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

/// Where a floating card of size `card` goes beside an item at `item` (`[x, y, w, h]`, window
/// points) whose header row is `head` tall, inside the `free` canvas rect. Candidates sit right,
/// left, below and above the item (each also aligned to the item's far edge), clamped into the
/// free area; then spots across the free canvas, for when everything beside the item is busy.
/// Clamping can push a candidate back over the item, so the ranking is: candidates clear of the
/// item first; failing that, ones that leave its header row (its name) visible; then the rest.
/// Within a rank the card covers the least area, of the item plus the weighted area of
/// `others` (rect, weight), plus [`DISTANCE_COST`] for each point it strays beyond
/// [`CARD_GAP`]; ties keep the earlier candidate (right, left, below, above).
pub(crate) fn place_card(item: [f32; 4], head: f32, card: [f32; 2], free: [f32; 4], others: &[([f32; 4], f32)]) -> [f32; 2] {
    let [x, y, w, h] = item;
    let [cw, ch] = card;
    let [fx, fy, fw, fh] = free;
    let clamp = |cx: f32, cy: f32| [cx.clamp(fx, (fx + fw - cw).max(fx)), cy.clamp(fy, (fy + fh - ch).max(fy))];
    let gap = CARD_GAP;
    let mut candidates = vec![
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
    // A grid over the free canvas, about `SCAN_SPOTS` spots, its last row and column on the
    // far edges.
    let (room_x, room_y) = ((fw - cw).max(0.0), (fh - ch).max(0.0));
    let step = (room_x * room_y / SCAN_SPOTS).sqrt().max(12.0);
    let (nx, ny) = ((room_x / step).ceil() as usize, (room_y / step).ceil() as usize);
    for j in 0..=ny {
        for i in 0..=nx {
            candidates.push(clamp(fx + room_x * i as f32 / nx.max(1) as f32, fy + room_y * j as f32 / ny.max(1) as f32));
        }
    }
    let header = [x, y, w, head.min(h)];
    let rank = |c: [f32; 2]| {
        let r = [c[0], c[1], cw, ch];
        let on_item = overlap(r, item);
        let tier = if on_item <= 0.0 { 0 } else if overlap(r, header) <= 0.0 { 1 } else { 2 };
        let covered: f32 = others.iter().map(|(o, wt)| overlap(r, *o) * wt).sum();
        let away = (rect_gap(r, item) - gap).max(0.0) * DISTANCE_COST;
        (tier, on_item + covered + away)
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
/// Hover rows shown of a node group's members before summarising.
const HOVER_MEMBERS: usize = 5;

/// Points around a circle, closed.
fn ring(c: [f32; 2], r: f32, n: usize) -> Vec<[f32; 2]> {
    (0..=n).map(|k| {
        let a = k as f32 * std::f32::consts::TAU / n as f32;
        [c[0] + r * a.cos(), c[1] + r * a.sin()]
    }).collect()
}

/// The effect mark drawn 10 points square: a filled dot (has effects), a hollow dot (pure) or a
/// dotted ring (unknown, or only inferred).
fn effect_shape(color: Color, mark: Mark) -> Shape {
    let c = [5.0, 5.0];
    match mark {
        Mark::Effects => Shape::new().fill(ring(c, 4.0, 24), color),
        Mark::Pure => Shape::new().stroke(ring(c, 3.5, 24), 1.5, color),
        Mark::Unknown => (0..8).fold(Shape::new(), |sh, k| {
            let a = k as f32 * std::f32::consts::TAU / 8.0;
            sh.fill(ring([c[0] + 3.8 * a.cos(), c[1] + 3.8 * a.sin()], 0.85, 8), color)
        }),
    }
}

/// The effect badge in a card's header; what it means is in its tooltip.
fn effect_badge(p: &Pal, mark: Mark, tip: String) -> El {
    el().size(20.0, 20.0).center().help(tip).child(canvas(effect_shape(p.muted, mark)).size(10.0, 10.0))
}

/// The word for an effect mark.
fn mark_word(mark: Mark) -> &'static str {
    match mark {
        Mark::Effects => "effects",
        Mark::Pure => "pure",
        Mark::Unknown => "unknown",
    }
}

/// The docked panel's EFFECTS row: this item's mark and its word. The key to all three marks
/// is in the tooltip, after what the effects are.
fn effect_legend(p: &Pal, mark: Mark, tip: String) -> El {
    row().gap(6.0).items_center().help(effect_key(&tip))
        .child(text("EFFECTS").font(LABEL).color(p.faint).w(58.0))
        .child(canvas(effect_shape(p.muted, mark)).size(10.0, 10.0))
        .child(label(mark_word(mark), p.text))
}

/// An effect tooltip followed by the key to the three marks.
fn effect_key(tip: &str) -> String {
    format!("{tip}\n● effects · ○ pure · ◌ unknown or inferred")
}

/// The header's mark for an accepted Rust implementation.
fn accepted_glyph(p: &Pal) -> El {
    el().size(20.0, 20.0).radius(10.0).fill(p.ok.alpha(0.18)).center().help("Rust implementation accepted")
        .child(text("✓").font(TextStyle::new(11.0, 800.0)).color(p.ok))
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

/// The docked panel's validation evidence for a rejected candidate: its failure, case count
/// and counterexample, monospace. Nothing is shown that the report didn't carry.
fn validation(p: &Pal, r: &Rejection) -> El {
    let mut head = row().gap(8.0).items_center()
        .child(text("VALIDATION").font(LABEL).color(p.faint).w(64.0))
        .child(text(r.failure).font(UI_STRONG).color(p.bad));
    if let Some((passed, total)) = r.cases {
        head = head.child(label(&format!("{passed}/{total} cases"), p.muted).help(format!("{passed} of {total} cases passed")));
    }
    let mut c = col().gap(4.0).padding(9.0).radius(8.0).fill(p.code_bg).child(head);
    if let Some(cx) = &r.counterexample {
        for (k, v, color) in [("input", &cx.input, p.text), ("expected", &cx.expected, p.ok), ("actual", &cx.actual, p.bad)] {
            c = c.child(row().gap(8.0)
                .child(text(k.to_uppercase()).font(LABEL).color(p.faint).w(64.0).mt(1.0))
                .child(mono(v, color).wrap().grow(1.0)));
        }
    }
    c
}

/// Width of a port row's columns before the other end: tag, type dot, pin and type, each
/// followed by the row's 8-point gap.
const PORT_COLUMNS: f32 = 26.0 + 8.0 + 8.0 + 8.0 + 64.0 + 8.0 + 64.0 + 8.0;
/// Average advance of a `UI` character, for fitting an end into its column before layout.
const UI_CHAR: f32 = 6.4;
/// Where a docked port's other end starts on its own line: under the pin name.
const END_INDENT: f32 = 26.0 + 8.0 + 8.0 + 8.0;

/// Port rows: tag, type dot, pin, type and the other end, the whole of it in a tooltip.
/// Floating (`width` is the card's), the end sits beside the type, its producer shortened so
/// the pin it reaches stays readable ([`describe::fit_end`]). Docked, every end has its own
/// line under its pin, so rows are even and nothing is cut.
fn ports(p: &Pal, rows: &[Port], tag: &str, arrow: &str, limit: usize, docked: bool, width: f32) -> El {
    // Characters that fit beside the type, less the arrow and its space.
    let fits = (((width - 2.0 * 14.0 - PORT_COLUMNS) / UI_CHAR).floor() as usize).saturating_sub(2).max(4);
    let mut t = col().gap(2.0);
    for (k, port) in rows.iter().take(limit).enumerate() {
        // The arrow stays joined to the name it points at.
        let end = if docked { port.other.clone() } else { describe::fit_end(&port.other, fits) };
        let other = label(&format!("{arrow}\u{a0}{end}"), p.muted).help(port.full.clone());
        let pin = row().h(22.0).gap(8.0).items_center()
            .child(text(if k == 0 { tag } else { "" }).font(LABEL).color(p.faint).w(26.0).min_w(26.0))
            .child(el().size(8.0, 8.0).min_w(8.0).radius(4.0).fill(p.ty(port.ty)))
            .child(mono(&port.name, p.text).single_line().truncate().help_when_truncated(port.name.clone()).w(64.0).min_w(64.0))
            .child(mono(&port.ty_text, p.ty_text(port.ty)).single_line().truncate().help_when_truncated(port.ty_text.clone()).w(64.0).min_w(64.0));
        t = t.child(if docked {
            col().child(pin).child(other.wrap_anywhere().ml(END_INDENT).mb(3.0))
        } else {
            pin.child(other.truncate().grow(1.0))
        });
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
            let group = n.kind == Kind::Group;
            let tile = p.header(n.kind.category());
            let badge = c.badge().then(|| (c.mark, c.effect_tip()));
            let mut tools = vec![];
            if docked && group && s.can_expand(i) {
                tools.push(group_toggle(p, true).on_press(move |s: &mut State| s.expand_node(i)));
            }
            if docked {
                tools.push(close_button(p));
            }
            let head = header(p, &n.title, graph::kind_glyph(n.kind), tile, n.evidence, n.lang, badge, group.then_some(n.members), docked, tools);
            let width = if docked { detail_w() } else { hover_w(s, item) };
            let mut body = col().gap(12.0)
                .child(col().gap(4.0).child(kicker(p, &kicker_text(n), tile)).child(head));
            if let Some(a) = activity(p, n.claim, n.evidence) {
                body = body.child(a);
            }
            if docked && let Some(r) = n.rejection.as_ref().filter(|_| n.evidence == Evidence::Rejected) {
                body = body.child(validation(p, r));
            }
            if docked {
                body = body.child(col().padding(9.0).radius(8.0).fill(p.code_bg).child(mono(&c.signature, p.text).wrap()));
            }
            if !c.inputs.is_empty() || !c.outputs.is_empty() {
                let mut t = col().gap(6.0);
                if !c.inputs.is_empty() {
                    t = t.child(ports(p, &c.inputs, "IN", "←", limit, docked, width));
                }
                if !c.inputs.is_empty() && !c.outputs.is_empty() {
                    t = t.child(divider(p));
                }
                if !c.outputs.is_empty() {
                    t = t.child(ports(p, &c.outputs, "OUT", "→", limit, docked, width));
                }
                body = body.child(t);
            }
            if docked {
                if let Some((flow, full)) = &c.flow {
                    body = body.child(row().gap(6.0)
                        .child(text("FLOW").font(LABEL).color(p.faint).w(58.0).mt(1.0))
                        .child(label(flow, p.text).wrap().grow(1.0).help(full.clone())));
                }
                body = body.child(effect_legend(p, c.mark, c.effect_tip()));
            }
            let sections: Vec<f32> = [activity_h(n.claim.is_some(), n.evidence == Evidence::Rejected), ports_h(c.inputs.len(), c.outputs.len())]
                .into_iter().flatten().collect();
            (shell(p, body, docked, p.header(n.kind.category())), estimate_h(&sections))
        }
        Item::Group(g) => {
            let grp = &s.view.groups[g];
            let members: Vec<&crate::model::Node> = s.view.nodes.iter().filter(|n| n.group == Some(g)).collect();
            let declared = grp.effects.as_deref().map(describe::declared);
            let badge = declared.as_ref().map(|(m, list)| (*m, format!("Effects: {}", list.join(", "))));
            let tools = if docked { vec![group_toggle(p, false).on_press(move |s: &mut State| s.toggle_group(g)), close_button(p)] } else { vec![] };
            let head = header(p, &grp.title, "▣", p.header(Category::Function), grp.evidence, grp.lang, badge.clone(), Some(members.len()), docked, tools);
            let mut body = col().gap(12.0)
                .child(col().gap(4.0).child(kicker(p, "node group", p.header(Category::Function))).child(head));
            if let Some(a) = activity(p, grp.claim, grp.evidence) {
                body = body.child(a);
            }
            if docked && let Some(r) = grp.rejection.as_ref().filter(|_| grp.evidence == Evidence::Rejected) {
                body = body.child(validation(p, r));
            }
            let mut list = col().gap(2.0);
            // Docked lists every member (the panel scrolls); the hover card summarises.
            let shown = if docked { members.len() } else { HOVER_MEMBERS };
            for n in members.iter().take(shown) {
                list = list.child(row().h(22.0).gap(8.0).items_center()
                    .child(el().size(8.0, 8.0).min_w(8.0).radius(2.0).fill(p.header(n.kind.category())))
                    .child(mono(&n.title, p.text).single_line().truncate().help_when_truncated(n.title.clone()).grow(1.0))
                    .child(graph::lang_chip(p, n.lang)));
            }
            if members.len() > shown {
                list = list.child(label(&format!("+{} more", members.len() - shown), p.faint).ml(16.0));
            }
            body = body.child(list);
            if docked && s.scene == "fixture" {
                let mut code = col().padding(10.0).radius(8.0).fill(p.code_bg);
                for line in RUST_SOURCE {
                    code = code.child(mono(if line.is_empty() { " " } else { line }, p.text).wrap());
                }
                body = body.child(code);
            }
            if docked && let Some((m, tip)) = badge {
                body = body.child(effect_legend(p, m, tip));
            }
            let sections: Vec<f32> = [activity_h(grp.claim.is_some(), grp.evidence == Evidence::Rejected), Some(rows_h(members.len(), HOVER_MEMBERS))]
                .into_iter().flatten().collect();
            (shell(p, body, docked, p.header(Category::Function)), estimate_h(&sections))
        }
    }
}

/// The kicker's words for a node: its category, except that a collapsed node group stays a
/// node group whether it is drawn as a frame or as one node.
fn kicker_text(n: &crate::model::Node) -> String {
    if n.kind == Kind::Group { "node group · collapsed".into() } else { n.kind.category().name().into() }
}

/// What kind of node this is, in words, above the title: the header colour's meaning.
fn kicker(p: &Pal, what: &str, tile: Color) -> El {
    row().gap(6.0).items_center()
        .child(el().size(8.0, 8.0).radius(2.0).fill(tile))
        .child(text(what.to_uppercase()).font(LABEL).color(p.muted))
}

/// The docked panel's toggle between a node group's two views: arrows pointing out (expand
/// the collapsed node into its frame) or in (collapse the frame to one node), drawn as on the
/// canvas's own group button.
fn group_toggle(p: &Pal, expand: bool) -> El {
    let (d, c) = (14.0_f32, 7.0_f32);
    let m = d * 0.15;
    let fg = p.muted;
    let mut sh = Shape::new();
    for (sx, sy) in [(1.0_f32, -1.0_f32), (-1.0, 1.0)] {
        let outer = [c + sx * (c - m), c + sy * (c - m)];
        let inner = [c + sx * 1.5, c + sy * 1.5];
        let tip = if expand { outer } else { inner };
        let k = 3.5;
        let (hx, hy) = if expand { (-sx * k, -sy * k) } else { (sx * k, sy * k) };
        sh = sh.line(outer, inner, 1.5, LineCap::Round, fg)
            .stroke_path(&Path::new().move_to([tip[0] + hx, tip[1]]).line_to(tip).line_to([tip[0], tip[1] + hy]), 1.5, LineCap::Round, fg);
    }
    let tip = if expand { "Expand group" } else { "Collapse to node" };
    el().size(26.0, 26.0).radius(7.0).center().hover(p.code_bg).cursor(Cursor::Pointer).help(tip)
        .key(if expand { "expand-group" } else { "collapse-group" })
        .child(canvas(sh).size(d, d))
}

/// The docked panel's close control: a small ✕, as the bar's icon buttons are drawn.
fn close_button(p: &Pal) -> El {
    let c = p.muted;
    let x = Shape::new().line([1.0, 1.0], [9.0, 9.0], 1.5, LineCap::Round, c).line([9.0, 1.0], [1.0, 9.0], 1.5, LineCap::Round, c);
    sized_icon_button(p, canvas(x).size(10.0, 10.0), "Close (Esc)", 26.0, 7.0).key("close-details")
        .on_press(|s: &mut State| s.selected = None)
}

/// Size of the kind tile left of a card's title, and the gap after it.
const TILE: f32 = 22.0;
const TILE_GAP: f32 = 9.0;

/// A card's header. Floating, one row: kind tile, title (cut, whole in its tooltip) and the
/// chips. Docked, the title has its row to itself and wraps rather than being cut, and the
/// chips, then `tools` (group toggle, close) at the far end, sit on a row under it.
#[allow(clippy::too_many_arguments)]
fn header(p: &Pal, title: &str, glyph: &str, tile: Color, evidence: Evidence, lang: crate::model::Lang, badge: Option<(Mark, String)>, count: Option<usize>, docked: bool, tools: Vec<El>) -> El {
    let tile = el().size(TILE, TILE).min_w(TILE).radius(7.0).fill(tile).center()
        .child(text(glyph).font(TextStyle::new(11.5, 650.0)).color(Color::WHITE));
    let name = text(title).font(TextStyle::new(14.5, 650.0)).color(p.text);
    let mut chips: Vec<El> = vec![];
    if let Some(c) = count {
        chips.push(row().h(18.0).px(6.0).radius(4.0).fill(p.code_bg).items_center().help("nodes in this group")
            .child(text(format!("▣ {c}")).font(TextStyle::new(10.5, 650.0)).color(p.muted)));
    }
    if let Some((mark, tip)) = badge {
        chips.push(effect_badge(p, mark, tip));
    }
    // Absent marks add nothing, not an empty slot plus its gap. A failed candidate is told by
    // the "checks failed" line under the header, so only an accepted one is marked here.
    if evidence == Evidence::Accepted {
        chips.push(accepted_glyph(p));
    }
    if lang != crate::model::Lang::None {
        chips.push(graph::lang_chip(p, lang));
    }
    if !docked {
        return row().h(28.0).gap(TILE_GAP).items_center().child(tile)
            .child(name.single_line().truncate().help_when_truncated(title.to_string()).grow(1.0))
            .children(chips);
    }
    let title_row = row().min_h(28.0).gap(TILE_GAP).items_center().child(tile).child(name.wrap().grow(1.0).shrink().min_w(0.0));
    let chip_row = row().h(26.0).gap(TILE_GAP).ml(TILE + TILE_GAP).items_center().children(chips)
        .child(el().grow(1.0)).child(row().gap(2.0).items_center().children(tools));
    col().gap(2.0).child(title_row).child(chip_row)
}

/// The card around `body`. Docked, it is as tall as its content up to the canvas height, and
/// scrolls inside when longer.
fn shell(p: &Pal, body: El, docked: bool, family: Color) -> El {
    if docked {
        col().max_h(viewport()[1] - TOP - M).fill(p.panel).radius(RADIUS).border(1.0, p.panel_border).clip_rounded(RADIUS)
            .shadow(4.0, 16.0, p.shadow.alpha(if p.dark { 0.4 } else { 0.08 }))
            .child(col().key("details-scroll").min_h(0.0).shrink().scroll(ScrollSpec { stick_bottom: false }).child(body.padding(16.0)))
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
    fn the_card_moves_off_a_neighbours_socket_into_empty_canvas() {
        // Busy on both sides, so the card goes below; a neighbour's socket (the end of a wire the
        // card describes) sits at the item's bottom-left, where "below" would land, with empty
        // canvas further along. The card must leave the socket showing and stay near.
        let item = [400.0, 300.0, 140.0, 90.0];
        let socket = wire_end([420.0, 427.0], false);
        let others = [
            ([560.0, 280.0, 300.0, 110.0], 3.0),
            ([60.0, 280.0, 320.0, 110.0], 3.0),
            (socket, HOT_WEIGHT),
        ];
        // Without the socket the card sits right below the item, on it.
        let plain = place(item, &others[..2]);
        assert!(overlap(plain, socket) > 0.0, "the setup puts {plain:?} on the socket");
        let r = place(item, &others);
        assert_eq!(overlap(r, socket), 0.0, "card {r:?} covers the socket");
        assert_eq!(overlap(r, item), 0.0);
        assert!(inside(r, FREE));
        assert!(rect_gap(r, item) < 100.0, "card {r:?} strays from the item");
    }

    #[test]
    fn the_card_avoids_a_neighbours_header_band() {
        // A canvas band so short that only the left and right candidates stay clear of the item.
        // Right, the card's corner catches the top of a node below: little area, but its name.
        // Left, it covers a sliver of a tall node's body. The name must win.
        let free = [16.0, 380.0, 1000.0, 200.0];
        let item = [400.0, 400.0, 140.0, 90.0];
        let below_right = [600.0, 540.0, 200.0, 100.0];
        let tall_left = [40.0, 300.0, 30.0, 300.0];
        let band = |r: [f32; 4]| [r[0], r[1], r[2], 30.0];
        let bodies = [(below_right, 1.0), (tall_left, 1.0)];
        let with_titles = [(below_right, 1.0), (tall_left, 1.0), (band(below_right), TITLE_WEIGHT), (band(tall_left), TITLE_WEIGHT)];
        let at = |others: &[([f32; 4], f32)]| {
            let [x, y] = place_card(item, 30.0, CARD, free, others);
            [x, y, CARD[0], CARD[1]]
        };
        // Bodies alone: the smallest covered area is the neighbour's title.
        assert!(overlap(at(&bodies), band(below_right)) > 0.0);
        let r = at(&with_titles);
        assert_eq!(overlap(r, band(below_right)), 0.0, "card {r:?} covers a title");
        assert_eq!(overlap(r, item), 0.0);
    }

    #[test]
    fn an_item_hidden_under_the_panels_gets_no_card() {
        let free = [16.0, 72.0, 1000.0, 800.0];
        // Wholly under the docked panel, then a sliver less than a header's worth.
        assert_eq!(visible_part([1100.0, 300.0, 140.0, 90.0], 30.0, free), None);
        assert_eq!(visible_part([990.0, 300.0, 140.0, 90.0], 30.0, free), None);
        // Mostly visible: placed beside its visible part.
        let (vis, head) = visible_part([900.0, 300.0, 140.0, 90.0], 30.0, free).unwrap();
        assert_eq!(vis, [900.0, 300.0, 116.0, 90.0]);
        assert_eq!(head, 30.0);
        // Top cut by the top bar: the header still showing shrinks with it.
        let (vis, head) = visible_part([400.0, 52.0, 140.0, 90.0], 30.0, free).unwrap();
        assert_eq!((vis[1], head), (72.0, 10.0));
    }

    #[test]
    fn hovering_a_node_under_the_docked_details_shows_no_card() {
        let size = [1024.0, 640.0];
        let before = set_viewport(size);
        let mut s = State::load("fixture");
        s.resized(size);
        let report = s.find("report").unwrap();
        let other = (0..s.view.nodes.len()).find(|&i| i != report).unwrap();
        s.selected = Some(Item::Node(other));
        // Pan `report` into the middle of the docked panel.
        let [x, _, w, _] = s.screen_rect(report);
        s.cam.pan[0] += size[0] - M - detail_w() / 2.0 - (x + w / 2.0);
        assert_eq!(hover_anchor(&s, Item::Node(report), HOVER_W, 170.0), None);
        // With the panel closed the same spot is open canvas: the card shows.
        s.selected = None;
        assert!(hover_anchor(&s, Item::Node(report), HOVER_W, 170.0).is_some());
        set_viewport(before);
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

#[cfg(test)]
mod chrome_tests {
    use super::*;
    use argo_protocol::NodeKind;

    fn kind(s: &State, id: NodeId) -> NodeKind {
        s.live.as_ref().unwrap().snap.graph.nodes.iter().find(|n| n.id == id).unwrap().kind
    }

    #[test]
    fn bar_controls_are_concentric_with_the_card() {
        assert_eq!(CONTROL_H + 2.0 * INSET, BAR_H);
        assert_eq!(CONTROL_R, graph::concentric(RADIUS, INSET, CONTROL_H));
    }

    #[test]
    fn protocol_crumbs_name_real_containers_root_first() {
        let s = State::protocol_fixture();
        let path = crumb_path(&s).expect("protocol crumbs are a hierarchy");
        assert_eq!(s.view.crumbs, ["fixture", "compute.c", "compute"]);
        assert_eq!(path.len(), 3);
        assert_eq!(kind(&s, path[0]), NodeKind::Program);
        assert_eq!(kind(&s, path[1]), NodeKind::Module);
        assert_eq!(Some(path[2]), s.live.as_ref().unwrap().container);
    }

    #[test]
    fn mock_scene_crumbs_are_not_navigable() {
        let s = State::load("fixture");
        assert!(!s.view.crumbs.is_empty());
        assert_eq!(crumb_path(&s), None);
    }

    #[test]
    fn clicking_an_ancestor_crumb_shows_that_level() {
        let mut s = State::protocol_fixture();
        let path = crumb_path(&s).unwrap();
        s.selected = Some(Item::Node(0));
        go_to_crumb(&mut s, path[1]);
        assert_eq!(s.live.as_ref().unwrap().container, Some(path[1]));
        assert_eq!(s.view.crumbs, ["fixture", "compute.c"]);
        assert_eq!(crumb_path(&s).unwrap(), path[..2]);
        assert!(s.view.nodes.iter().any(|n| n.title == "compute"), "the file's functions are drawn");
        assert_eq!(s.selected, None, "the old selection indexed another level");
        assert_eq!(s.placed.pos.len(), s.view.nodes.len(), "relaid out");
        // And on up to the program.
        go_to_crumb(&mut s, path[0]);
        assert_eq!(s.view.crumbs, ["fixture"]);
        assert!(s.view.nodes.iter().any(|n| n.title == "compute.c"));
        // Back down: the function opens with its body in place, as the explorer opens it.
        go_to_crumb(&mut s, path[2]);
        assert_eq!(s.view.crumbs, ["fixture", "compute.c", "compute"]);
        assert!(s.view.nodes.iter().any(|n| n.title.starts_with("for (")));
    }

    #[test]
    fn the_source_says_where_the_graph_comes_from() {
        assert_eq!(source(&State::load("devices")), Source::Mock("devices"));
        let mut s = State::protocol_fixture();
        assert!(matches!(source(&s), Source::Fixture { left } if left > 0));
        s.replay(usize::MAX);
        assert_eq!(source(&s), Source::Fixture { left: 0 });
        for src in [source(&State::load("devices")), source(&s)] {
            let tip = src.tip();
            assert!(!tip.contains("SpacetimeDB") && !tip.contains("live"), "{tip}");
        }
    }

    #[test]
    fn clicking_a_worker_selects_its_item_and_centres_it() {
        let mut s = State::load("fixture");
        s.selected = None;
        let ws = workers(&s);
        assert!(ws.len() >= 2, "the fixture has two workers");
        for (_, _, title, item) in ws {
            s.cam.pan = [-900.0, 700.0];
            reveal(&mut s, item);
            assert_eq!(s.selected, Some(item), "{title}");
            let [x, y, w, h] = item_rect(&s, item);
            let [fx, fy, fw, fh] = free_area(true, false);
            assert!((x + w / 2.0 - (fx + fw / 2.0)).abs() < 0.5, "{title} centred across");
            assert!((y + h / 2.0 - (fy + fh / 2.0)).abs() < 0.5, "{title} centred down");
        }
    }

    #[test]
    fn the_moon_is_one_simple_crescent() {
        let (c0, r0, c1, r1) = ([7.0, 7.0], 6.0, [10.0, 4.0], 4.8);
        let pts = crescent(c0, r0, c1, r1);
        let dist = |p: [f32; 2], c: [f32; 2]| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt();
        for p in &pts {
            assert!(dist(*p, c0) <= r0 + 1e-3, "inside the disc: {p:?}");
            assert!(dist(*p, c1) >= r1 - 1e-3, "outside the bite: {p:?}");
        }
        // Successive points stay close, so the bite's arc starts where the outer arc ends and
        // closes back to its start: one simple outline.
        let n = pts.len();
        for k in 0..n {
            let (a, b) = (pts[k], pts[(k + 1) % n]);
            assert!(dist(a, b) < 2.0, "{a:?} → {b:?}");
        }
    }
}
