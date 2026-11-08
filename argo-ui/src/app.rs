//! Application state: the semantic graph, the derived view graph and layout, the camera, hover
//! and selection. Live updates change claims and evidence in place, so layout, camera and
//! selection survive them.

use std::collections::HashSet;

use argo_protocol::{NodeId, Snapshot};

use crate::bridge;
use crate::db;
use crate::layout::{self, Placed};
use crate::project;
use crate::model::{Evidence, Graph, Kind, Origin};
use crate::ui;

/// World padding around the laid-out graph.
pub const MARGIN: f32 = 40.0;
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 2.0;
/// The zoom the default view prefers when the graph does not fit the readable band: node titles
/// and pin labels at their natural size (about 11 px).
const READABLE_ZOOM: f32 = 1.0;
/// The readable band's floor: an overflowing graph's view searches down to this zoom for more of
/// its goals.
const FIT_FLOOR: f32 = 0.85;
/// The readable band's ceiling in windows up to [`WIDE`] points wide: framing never magnifies
/// past this, so a small graph keeps node text near the chrome's type size.
const FIT_MAX: f32 = 1.25;
/// Window width past which the framing ceiling grows from [`FIT_MAX`], reaching [`WIDE_MAX`] at
/// 2560 points, so a small graph in a large window does not sit small amid wide margins.
const WIDE: f32 = 1800.0;
const WIDE_MAX: f32 = 1.6;
/// Share of the free area's limiting axis a small graph fills under the grown ceiling.
const WIDE_FILL: f32 = 0.7;
/// The default view frames the whole graph down to this zoom. Below the readable band, the view
/// of an overflowing graph also goes down to it, but only to show the selection with its claimed
/// and failing items, or with its whole upstream context (small windows beside the details panel).
const CONTEXT_FLOOR: f32 = 0.72;
/// Gap between the free canvas area's edge and content aligned to it.
const EDGE_PAD: f32 = 24.0;
/// Zoom step the default view tries between [`READABLE_ZOOM`] and [`CONTEXT_FLOOR`].
const ZOOM_STEP: f32 = 0.02;
/// How far badges (worker avatar, evidence flag) hang outside an item's rectangle, in points.
const BADGE_OUT: f32 = 10.0;
/// The start of a frame's title row (name, language chip) that must show, in world units.
const TITLE_ROW: [f32; 2] = [140.0, 32.0];
/// Gap kept between a revealed item and the overview card, in points.
const MAP_GAP: f32 = 8.0;
/// Pan distance (points) worth moving to keep one more node out from under the overview card.
const COVER_COST: f32 = 80.0;
/// Most candidate pans per axis the view solver tries.
const MAX_CANDS: usize = 160;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    /// A node in the view graph.
    Node(usize),
    Group(usize),
}

/// An item named independently of the current view (view indices change on collapse/expand).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ident {
    /// A node of the mock semantic graph.
    Node(usize),
    /// A mock node group, drawn as a frame or as one node.
    Group(usize),
    /// A protocol node: a node, or the container a frame shows.
    Proto(NodeId),
}

#[derive(Clone, Copy, Default)]
pub struct Cam {
    pub pan: [f32; 2],
    pub zoom: f32,
    pub drag_from: [f32; 2],
    /// A pan drag is held: hover changes are ignored until it ends.
    pub dragging: bool,
    /// The last pan drag travelled past a click, so the press its release produces is no click.
    pub panned: bool,
}

/// Pointer travel (points, per axis) a press may have and still count as a click, not a pan.
const CLICK_SLOP: f32 = 4.0;

impl Cam {
    /// One phase of a pan drag; `d` is the pointer's travel since the press.
    pub fn drag(&mut self, phase: rune_ui::Phase, d: [f32; 2]) {
        match phase {
            rune_ui::Phase::Down => {
                self.drag_from = self.pan;
                self.dragging = true;
                self.panned = false;
            }
            rune_ui::Phase::Move | rune_ui::Phase::Up => {
                self.pan = [self.drag_from[0] + d[0], self.drag_from[1] + d[1]];
                self.panned |= d[0].abs() > CLICK_SLOP || d[1].abs() > CLICK_SLOP;
                self.dragging = phase == rune_ui::Phase::Move;
            }
        }
    }

    /// Whether the press being handled is the release of a pan rather than a click (once).
    pub fn take_panned(&mut self) -> bool {
        std::mem::take(&mut self.panned)
    }

    pub fn zoom_about(&mut self, x: f32, y: f32, factor: f32) {
        let z = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let k = z / self.zoom;
        self.pan = [x - (x - self.pan[0]) * k, y - (y - self.pan[1]) * k];
        self.zoom = z;
    }
}

pub struct State {
    /// The semantic graph as received (claims and evidence update live).
    pub graph: Graph,
    pub collapsed: Vec<bool>,
    /// The graph as drawn: collapsed groups folded into single nodes.
    pub view: Graph,
    pub placed: Placed,
    pub cam: Cam,
    pub hovered: Option<Item>,
    pub selected: Option<Item>,
    /// Set once the database's rows describe a program: the snapshot they rebuild.
    pub live: Option<Live>,
    /// The ingested project, with its file/function index.
    pub project: Option<project::Project>,
    pub forms: project::Forms,
    /// The subscription every graph and status shown comes from.
    pub db: Option<db::Link>,
    /// Whether the subscription is connected.
    pub online: bool,
    /// Whether the camera has been fitted to the real window size yet.
    sized: bool,
}

/// A protocol snapshot and the level being shown.
pub struct Live {
    pub snap: Snapshot,
    pub container: Option<NodeId>,
    pub open: HashSet<NodeId>,
    /// The whole-program view is showing (`open` then holds the files drawn as frames).
    pub whole: bool,
    /// Calls between functions, worked out once per structure for the whole-program view.
    pub(crate) calls: Option<std::sync::Arc<Vec<(NodeId, NodeId)>>>,
    /// View node index → protocol node (see [`bridge::Projection::origin`]).
    pub(crate) origin: Vec<NodeId>,
    pub(crate) frames: Vec<NodeId>,
}

fn open_all_requested() -> bool {
    std::env::var("ARGO_OPEN").is_ok_and(|v| v == "all")
}

/// Every node inside `container`, at any depth.
fn descendants(snap: &Snapshot, container: NodeId) -> HashSet<NodeId> {
    let mut open = HashSet::new();
    let mut stack = vec![container];
    while let Some(c) = stack.pop() {
        for n in snap.graph.nodes.iter().filter(|n| n.parent == Some(c)) {
            open.insert(n.id);
            stack.push(n.id);
        }
    }
    open
}

/// How well a camera meets the view goals; fields compare in priority order (better is greater).
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
struct Score {
    /// The selection shows whole in the padded free area, clear of the overview card.
    selection: bool,
    /// No claimed or failing item is cut by the free area's edge or the overview card.
    claims: bool,
    /// How many claimed or failing items show whole: where agents work and checks fail outranks
    /// the selection's upstream context, which falls off the far edge first.
    must: usize,
    /// Every frame on screen shows the start of its title row.
    frames: bool,
    /// No exit node (a group output or a node no wire leaves) is cut by the free area's edge.
    exits: bool,
    /// The selection's whole upstream context (every node feeding it, back to the graph's
    /// entry) shows whole.
    entry: bool,
    /// How many nodes of the selection's upstream context show whole.
    context: usize,
    /// How many of the graph's left and top edges are in view.
    start: u8,
    /// How many nodes show whole or not at all (none cut by the free area's edge).
    whole: usize,
    /// Minus the pan distance moved and [`COVER_COST`] per node under the overview card.
    closeness: f32,
}

impl Score {
    const WORST: Score = Score {
        selection: false,
        claims: false,
        must: 0,
        frames: false,
        exits: false,
        entry: false,
        context: 0,
        start: 0,
        whole: 0,
        closeness: f32::NEG_INFINITY,
    };

    /// The goals a different zoom in the readable band must improve on to be chosen (not the
    /// pan tie-breakers).
    fn goals(&self) -> (bool, bool, usize, bool, bool, bool, usize, u8) {
        (self.selection, self.claims, self.must, self.frames, self.exits, self.entry, self.context, self.start)
    }

    /// What a zoom below the readable band must improve on: only showing the selection with
    /// more of its claimed and failing items, or with its whole upstream context, justifies
    /// smaller text.
    fn essentials(&self) -> (bool, bool, usize, bool, bool) {
        (self.selection, self.claims, self.must, self.frames, self.entry)
    }
}

/// The most framing magnifies a graph whose whole-graph zoom is `whole`: [`FIT_MAX`] in windows
/// up to [`WIDE`] points wide; wider, enough for the graph to fill [`WIDE_FILL`] of the free
/// area's limiting axis, within a cap that grows with the window to [`WIDE_MAX`] at 2560.
fn ceiling(whole: f32) -> f32 {
    let t = ((ui::viewport()[0] - WIDE) / (2560.0 - WIDE)).clamp(0.0, 1.0);
    (WIDE_FILL * whole).clamp(FIT_MAX, FIT_MAX + t * (WIDE_MAX - FIT_MAX))
}

/// Screen rectangles `a` and `b` overlap by more than half a point.
fn overlaps(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] - 0.5 && b[0] < a[0] + a[2] - 0.5 && a[1] < b[1] + b[3] - 0.5 && b[1] < a[1] + a[3] - 0.5
}

/// `r` lies inside `a`, with `slack` points to spare.
fn contains(a: [f32; 4], r: [f32; 4], slack: f32) -> bool {
    r[0] >= a[0] - slack && r[1] >= a[1] - slack && r[0] + r[2] <= a[0] + a[2] + slack && r[1] + r[3] <= a[1] + a[3] + slack
}

impl State {
    /// Nothing on the canvas but its breadcrumb: where the database view starts.
    pub fn empty(crumbs: Vec<String>) -> State {
        State::with_graph(Graph { crumbs, nodes: vec![], wires: vec![], groups: vec![], selected_group: None, expanded: None })
    }

    fn with_graph(graph: Graph) -> State {
        let collapsed = vec![false; graph.groups.len()];
        let selected = graph.selected_group.map(Item::Group);
        let mut s = State {
            view: graph.clone(), graph, collapsed, placed: Placed { pos: vec![], size: vec![], frames: vec![], bounds: [0.0; 4], row_gaps: vec![], generation: 0 },
            cam: Cam { zoom: 1.0, ..Cam::default() }, hovered: None, selected, online: true, live: None,
            project: None, forms: project::Forms::default(), db: None,
            // A state built after the window's first layout already knows its size.
            sized: ui::viewport() != [ui::W, ui::H],
        };
        s.relayout();
        s
    }

    /// A protocol snapshot: show `container`'s level with `open` drawn in place.
    pub fn from_snapshot(snap: Snapshot, container: Option<NodeId>, open: HashSet<NodeId>) -> State {
        let mut s = State::empty(vec![]);
        s.live = Some(Live { snap, container, open, whole: false, calls: None, origin: vec![], frames: vec![] });
        s.reproject();
        s.relayout();
        s.fit();
        s
    }

    /// Captures: `ARGO_OPEN=all` opens every construct inside the shown function.
    pub fn open_all_if_requested(&mut self) {
        let Some(live) = &mut self.live else { return };
        let Some(container) = live.container.filter(|_| open_all_requested()) else { return };
        live.open = descendants(&live.snap, container);
        self.reproject();
        self.relayout();
        self.fit();
    }

    /// Re-derive the semantic graph from the protocol snapshot (once rows describe a program).
    pub(crate) fn reproject(&mut self) {
        let Some(live) = &mut self.live else { return };
        let p = match (&live.calls, live.whole) {
            (Some(calls), true) => bridge::program(&live.snap, calls, &live.open),
            _ => bridge::project(&live.snap, live.container, &live.open),
        };
        live.origin = p.origin;
        live.frames = p.frames;
        self.graph = layout::with_getters(p.graph);
        self.collapsed = vec![false; self.graph.groups.len()];
    }

    /// Rebuild the view graph and its layout after a structural view change (collapse/expand).
    pub(crate) fn relayout(&mut self) {
        self.view = self.graph.collapse(&self.collapsed).0;
        self.placed = if self.live.as_ref().is_some_and(|l| l.whole) {
            layout::packed(&self.view)
        } else {
            layout::layout(&self.view)
        };
    }

    /// Rebuild the view graph only: claims and evidence changed, positions did not.
    pub(crate) fn refresh(&mut self) {
        self.view = self.graph.collapse(&self.collapsed).0;
    }

    /// The window was laid out at `size`. The first layout frames the graph for the real window;
    /// later resizes keep whatever was at the centre of the canvas there.
    pub fn resized(&mut self, size: [f32; 2]) {
        let old = ui::set_viewport(size);
        if !self.sized {
            self.sized = true;
            self.fit();
        } else if old != size {
            self.cam.pan[0] += (size[0] - old[0]) / 2.0;
            self.cam.pan[1] += (size[1] - old[1]) / 2.0;
        }
    }

    /// Frame the whole graph in the canvas area left free by the floating panels.
    pub fn fit_all(&mut self) {
        let [_, _, bw, bh] = self.placed.bounds;
        let (ww, wh) = (bw + 2.0 * MARGIN, bh + 2.0 * MARGIN);
        let [_, _, vw, vh] = self.free();
        let whole = (vw / ww).min(vh / wh);
        self.place(whole.clamp(MIN_ZOOM, ceiling(whole)));
    }

    /// The canvas area left free by the floating panels currently shown.
    fn free(&self) -> [f32; 4] {
        ui::free_area(self.selected.is_some(), self.project.is_some())
    }

    /// Show the graph at `zoom`: centred on each axis where it fits the free area, and read
    /// from its top or left edge (just inside the free area) where it does not.
    fn place(&mut self, zoom: f32) {
        let [_, _, bw, bh] = self.placed.bounds;
        let [vx, vy, vw, vh] = self.free();
        let axis = |len: f32, lo: f32, room: f32| {
            let start = if len * zoom <= room { lo + (room - len * zoom) / 2.0 } else { lo + EDGE_PAD };
            start - MARGIN * zoom
        };
        self.cam.zoom = zoom;
        self.cam.pan = [axis(bw, vx, vw), axis(bh, vy, vh)];
    }

    /// Default view: fit the whole graph (beside the details panel when something is selected)
    /// when that zoom is at least [`CONTEXT_FLOOR`]; otherwise keep a readable zoom (from
    /// [`READABLE_ZOOM`] down to [`FIT_FLOOR`]) and pan, leaving the rest to panning, so that, in
    /// priority order: the selection shows whole in the free area; no claimed or failing item is
    /// cut (by the details panel or any other edge); as many of them as possible show whole;
    /// every frame on screen shows its title; no exit node is cut; the selection's upstream
    /// context, back to the graph's entry, shows; the graph's top-left (where it starts reading)
    /// stays in view; and as few nodes as possible are cut. A lower zoom is used only when it
    /// meets more of these, and one below the band (down to [`CONTEXT_FLOOR`]) only to show the
    /// selection with more claimed or failing items or with its whole upstream context.
    pub fn fit(&mut self) {
        self.fit_all();
        // Captures can pan like a person would: `ARGO_TILE=col,row` at `ARGO_ZOOM`.
        if let Ok(t) = std::env::var("ARGO_TILE") {
            let zoom = std::env::var("ARGO_ZOOM").ok().and_then(|z| z.parse().ok()).unwrap_or(1.0);
            let (c, r) = t.split_once(',').and_then(|(c, r)| Some((c.parse().ok()?, r.parse().ok()?))).unwrap_or((0, 0));
            self.tile(zoom, c, r);
            return;
        }
        // The whole-program view is a map: as much of it as fits, from its top-left.
        if self.live.as_ref().is_some_and(|l| l.whole) {
            return;
        }
        // The whole flow, entry to exits, beats a readable slice of it. Captures can ask for the
        // whole graph regardless of readability (`ARGO_FIT=all`).
        if self.cam.zoom >= CONTEXT_FLOOR || std::env::var("ARGO_FIT").is_ok_and(|v| v == "all") {
            return;
        }
        let mut best: Option<(Score, f32, [f32; 2])> = None;
        let mut below: Option<(Score, f32, [f32; 2])> = None;
        for zoom in Self::zooms() {
            // Start where the program starts (reading order); the far edges fade to show more.
            self.place(zoom);
            let (score, pan) = self.solve(zoom, self.cam.pan, self.selected, true);
            // Ties keep the larger, more readable zoom.
            if zoom >= FIT_FLOOR {
                if best.as_ref().is_none_or(|(b, _, _)| score.goals() > b.goals()) {
                    best = Some((score, zoom, pan));
                }
            } else if self.selected.is_some() && below.as_ref().is_none_or(|(b, _, _)| score.essentials() > b.essentials()) {
                below = Some((score, zoom, pan));
            }
        }
        if let (Some((b, _, _)), Some((l, _, _))) = (&best, &below)
            && l.essentials() > b.essentials()
        {
            best = below;
        }
        if let Some((_, zoom, pan)) = best {
            self.cam.zoom = zoom;
            self.cam.pan = pan;
        }
    }

    /// The zooms the default view tries for a graph that overflows the readable band, largest
    /// first: [`READABLE_ZOOM`] down to [`FIT_FLOOR`] (inclusive), then on to [`CONTEXT_FLOOR`].
    fn zooms() -> impl Iterator<Item = f32> {
        let band = ((READABLE_ZOOM - FIT_FLOOR) / ZOOM_STEP).ceil() as usize;
        let under = ((FIT_FLOOR - CONTEXT_FLOOR) / ZOOM_STEP).ceil() as usize;
        let band = (0..=band).map(|k| (READABLE_ZOOM - k as f32 * ZOOM_STEP).max(FIT_FLOOR));
        let under = (1..=under).map(|k| (FIT_FLOOR - k as f32 * ZOOM_STEP).max(CONTEXT_FLOOR));
        band.chain(under)
    }

    /// The view nodes feeding `item` along wires, transitively (for a frame, feeding its
    /// members), excluding the item itself: what reads before it, back to the graph's entry.
    fn upstream(&self, item: Item) -> Vec<usize> {
        let own = |i: usize| match item {
            Item::Node(n) => i == n,
            Item::Group(g) => self.view.nodes[i].group == Some(g),
        };
        let mut seen: Vec<bool> = (0..self.view.nodes.len()).map(own).collect();
        let mut stack: Vec<usize> = (0..seen.len()).filter(|&i| seen[i]).collect();
        let mut out = vec![];
        while let Some(i) = stack.pop() {
            for w in self.view.wires.iter().filter(|w| w.to.0 == i) {
                let j = w.from.0;
                if !seen[j] {
                    seen[j] = true;
                    out.push(j);
                    stack.push(j);
                }
            }
        }
        out
    }

    /// Every item an agent has claimed (active or queued) or whose checks failed, as drawn: what
    /// the default view must show.
    fn claims(&self) -> Vec<Item> {
        let must = |claim: Option<crate::model::Claim>, evidence: Evidence| claim.is_some() || evidence == Evidence::Rejected;
        let nodes = self.view.nodes.iter().enumerate().filter(|(_, n)| must(n.claim, n.evidence)).map(|(i, _)| Item::Node(i));
        let groups = (0..self.view.groups.len()).filter(|&g| must(self.view.groups[g].claim, self.view.groups[g].evidence) && self.expanded(g)).map(Item::Group);
        nodes.chain(groups).collect()
    }

    /// The view nodes the flow leaves through: group outputs and nodes no wire leaves.
    fn exits(&self) -> Vec<usize> {
        let n = self.view.nodes.len().min(self.placed.pos.len());
        (0..n).filter(|&i| self.view.nodes[i].kind == Kind::GroupOut || !self.view.wires.iter().any(|w| w.from.0 == i)).collect()
    }

    /// Whether group `g` is drawn as a frame.
    fn expanded(&self, g: usize) -> bool {
        !self.collapsed.get(g).copied().unwrap_or(false) && self.placed.frames.get(g).is_some_and(|f| f[2] > 0.0)
    }

    /// The pan, at `zoom`, closest to `from` that best meets the view goals (see [`State::fit`]):
    /// `selection` whole in the free area, claimed and failing items whole (else hidden, never
    /// cut), frames on screen with their titles, and, when `reading`, exit nodes whole or hidden,
    /// the selection's upstream and the graph's top-left in view. Items revealed keep clear
    /// of the overview card. Candidate pans line item edges up with the free area's edges, its
    /// padding and the card's edges.
    fn solve(&self, zoom: f32, from: [f32; 2], selection: Option<Item>, reading: bool) -> (Score, [f32; 2]) {
        let free = self.free();
        let world = |item| self.world(item);
        let sel = selection.map(world);
        let claims: Vec<[f32; 4]> = self.claims().into_iter().map(world).collect();
        let frames: Vec<[f32; 4]> = (0..self.view.groups.len()).filter(|&g| self.expanded(g)).map(|g| world(Item::Group(g))).collect();
        let nodes: Vec<[f32; 4]> = (0..self.placed.pos.len()).map(|i| world(Item::Node(i))).collect();
        // What leads into the selection, read before it (default view only).
        let upstream: Vec<[f32; 4]> = selection.filter(|_| reading).map_or(vec![], |it| self.upstream(it).into_iter().map(|i| nodes[i]).collect());
        // Where the flow leaves, whole or not at all (default view only).
        let exits: Vec<[f32; 4]> = if reading { self.exits().into_iter().map(|i| nodes[i]).collect() } else { vec![] };
        let [_, _, bw, bh] = self.placed.bounds;
        let content = [MARGIN, MARGIN, bw, bh];
        let map = crate::overview::rect(self);

        let mut cands = [vec![from[0]], vec![from[1]]];
        for axis in 0..2 {
            let (lo, room) = (free[axis], free[axis + 2]);
            let mut targets = vec![lo, lo + EDGE_PAD, lo + room - EDGE_PAD, lo + room];
            if let Some(m) = map {
                targets.push(if axis == 0 { m[0] + m[2] + MAP_GAP } else { m[1] - MAP_GAP });
            }
            let title = TITLE_ROW[axis] * zoom;
            let mut edges = vec![];
            for (r, out) in sel.iter().chain(&claims).map(|r| (r, BADGE_OUT)).chain(frames.iter().map(|r| (r, 0.0))) {
                let (start, len) = (r[axis] * zoom - out, r[axis + 2] * zoom + 2.0 * out);
                edges.extend([start, start + len]);
                if out == 0.0 {
                    edges.push(start + title.min(len));
                }
            }
            for e in edges {
                cands[axis].extend(targets.iter().map(|t| t - e));
            }
            // The graph's entry and the selection's upstream nodes, lined up with the edges: a
            // budget of their own, so a long upstream never crowds out the selection's pans.
            let mut context: Vec<f32> = vec![];
            // Every node's edges lined up with the free area's edges, so nodes there show whole
            // or not at all: a budget of their own too.
            let mut whole: Vec<f32> = vec![];
            if reading {
                let edges = upstream.iter().chain(&exits).flat_map(|r| [r[axis] * zoom, (r[axis] + r[axis + 2]) * zoom]);
                for e in std::iter::once(content[axis] * zoom).chain(edges) {
                    context.extend(targets.iter().map(|t| t - e));
                }
                for r in &nodes {
                    whole.extend([lo - r[axis] * zoom, lo + room - (r[axis] + r[axis + 2]) * zoom]);
                }
            }
            // Nodes lined up with the overview card's edge, to slide them out from under it.
            if let Some(m) = map {
                let edge = if axis == 0 { m[0] + m[2] + MAP_GAP } else { m[1] - MAP_GAP };
                let near = nodes.iter().map(|r| if axis == 0 { r[0] * zoom } else { (r[1] + r[3]) * zoom });
                cands[axis].extend(near.map(|e| edge - e));
            }
            // Large graphs with many frames: the pans nearest `from` bound the search.
            let near = from[axis];
            for (v, n) in [(&mut cands[axis], MAX_CANDS), (&mut context, MAX_CANDS / 2), (&mut whole, MAX_CANDS / 4)] {
                v.sort_by(|a, b| (a - near).abs().total_cmp(&(b - near).abs()));
                v.truncate(n);
            }
            cands[axis].extend(context);
            cands[axis].extend(whole);
            cands[axis].sort_by(f32::total_cmp);
            cands[axis].dedup_by(|a, b| (*a - *b).abs() < 0.25);
        }

        let score = |px: f32, py: f32| {
            let rect = |r: &[f32; 4], out: f32| [px + r[0] * zoom - out, py + r[1] * zoom - out, r[2] * zoom + 2.0 * out, r[3] * zoom + 2.0 * out];
            let c = rect(&content, 0.0);
            // The overview card shows (at the free area's bottom-left) when the graph overflows.
            let card = map.filter(|_| !contains(free, c, 1.0)).map(|m| [m[0], m[1] - MAP_GAP, m[2] + MAP_GAP, m[3] + MAP_GAP]);
            let clear = |r: [f32; 4]| card.is_none_or(|k| !overlaps(r, k));
            // Whole inside the free area, `pad` from its edges; an item too big for it shows
            // from its top or left edge.
            let shown = |r: [f32; 4], pad: f32| {
                let mut fits = true;
                let axes = (0..2).all(|a| {
                    let (lo, hi) = (free[a] + pad, free[a] + free[a + 2] - pad);
                    if r[a + 2] <= hi - lo {
                        r[a] >= lo - 0.5 && r[a] + r[a + 2] <= hi + 0.5
                    } else {
                        fits = false;
                        r[a] >= free[a] - 0.5 && r[a] <= lo + 0.5
                    }
                });
                axes && (!fits || clear(r))
            };
            let selection = sel.is_none_or(|r| shown(rect(&r, BADGE_OUT), EDGE_PAD));
            let mut must = 0;
            let mut claims_ok = true;
            for r in &claims {
                let r = rect(r, BADGE_OUT);
                if shown(r, 0.0) {
                    must += 1;
                } else if overlaps(r, free) {
                    claims_ok = false;
                }
            }
            // Cut by the free area's edge: partly in it, partly out.
            let cut = |r: &[f32; 4]| {
                let r = rect(r, 0.0);
                overlaps(r, free) && !contains(free, r, 0.5)
            };
            let exits_ok = !exits.iter().any(cut);
            let whole = if reading { nodes.len() - nodes.iter().filter(|r| cut(r)).count() } else { 0 };
            let frames_ok = frames.iter().all(|r| {
                let r = rect(r, 0.0);
                let title = [r[0], r[1], r[2].min(TITLE_ROW[0] * zoom), r[3].min(TITLE_ROW[1] * zoom)];
                !overlaps(r, free) || (contains(free, title, 0.5) && clear(title))
            });
            let context = upstream.iter().filter(|r| shown(rect(r, 0.0), 0.0)).count();
            // Per axis, the graph reads from its start: whole in view where it fits, else
            // its edge just inside the free area (not pushed in, leaving an empty band).
            let reads = |a: usize| {
                let (lo, room) = (free[a], free[a + 2]);
                let ok_end = if c[a + 2] <= room + 1.0 { c[a] + c[a + 2] <= lo + room + 0.5 } else { c[a] <= lo + EDGE_PAD + 0.5 };
                c[a] >= lo - 0.5 && ok_end
            };
            let start = if reading { u8::from(reads(0)) + u8::from(reads(1)) } else { 0 };
            let shift = (px - from[0]).abs() + (py - from[1]).abs();
            let covered = card.map_or(0, |k| nodes.iter().filter(|r| overlaps(rect(r, 0.0), k)).count());
            Score {
                selection,
                claims: claims_ok,
                must,
                frames: frames_ok,
                exits: exits_ok,
                entry: context == upstream.len(),
                context,
                start,
                whole,
                closeness: -shift - COVER_COST * covered as f32,
            }
        };

        let mut best = (Score::WORST, from);
        for &px in &cands[0] {
            for &py in &cands[1] {
                let s = score(px, py);
                if s > best.0 {
                    best = (s, [px, py]);
                }
            }
        }
        // The default view centres the leftover space on an axis where the graph fits, rather
        // than leaving an empty band on one side, whenever that costs none of the goals.
        if reading {
            for axis in 0..2 {
                let (lo, room, len) = (free[axis], free[axis + 2], content[axis + 2] * zoom);
                if len > room {
                    continue;
                }
                let mut pan = best.1;
                pan[axis] = lo + (room - len) / 2.0 - content[axis] * zoom;
                let s = score(pan[0], pan[1]);
                if s.goals() >= best.0.goals() {
                    best = (s, pan);
                }
            }
        }
        best
    }

    /// Pan the least distance that puts `item` inside the free canvas area (padded) and clear of
    /// the overview card, without cutting a claimed item or a frame's title at the area's edge.
    /// An item larger than that area on an axis shows from its top or left edge, where a frame's
    /// title and collapse button are.
    fn show(&mut self, item: Item) {
        self.cam.pan = self.solve(self.cam.zoom, self.cam.pan, Some(item), false).1;
    }

    /// An item's rectangle in world units, relative to the camera pan at zoom 1 (screen rectangle
    /// = pan + this × zoom).
    fn world(&self, item: Item) -> [f32; 4] {
        let [bx, by, _, _] = self.placed.bounds;
        let ([x, y], [w, h]) = match item {
            Item::Node(i) => (self.placed.pos[i], self.placed.size[i]),
            Item::Group(g) => {
                let [x, y, w, h] = self.placed.frames[g];
                ([x, y], [w, h])
            }
        };
        [x - bx + MARGIN, y - by + MARGIN, w, h]
    }

    /// Point the camera at one tile of a grid covering the whole graph at `zoom`. Neighbouring
    /// tiles overlap by about one typical node, so their edges can be matched.
    /// Prints the grid size (`tiles COLSxROWS`) to stderr for capture scripts.
    pub fn tile(&mut self, zoom: f32, col: usize, row: usize) {
        let [_, _, bw, bh] = self.placed.bounds;
        let (ww, wh) = ((bw + 2.0 * MARGIN) * zoom, (bh + 2.0 * MARGIN) * zoom);
        let [vx, vy, vw, vh] = ui::free_area(self.selected.is_some(), self.project.is_some());
        let median = |i: usize| {
            let mut v: Vec<f32> = self.placed.size.iter().map(|s| s[i]).collect();
            v.sort_by(f32::total_cmp);
            v.get(v.len() / 2).copied().unwrap_or(100.0)
        };
        let (ox, oy) = (median(0) * zoom, median(1) * zoom);
        // Tiles per axis, the step between them, and where the first starts: a graph that fits
        // one view is centred; a larger one is read from its top-left corner, like a page.
        let axis = |world: f32, view: f32, overlap: f32| {
            if world <= view {
                return (1, 0.0, (view - world) / 2.0);
            }
            let step = view - overlap;
            (((world - view) / step).ceil() as usize + 1, step, 0.0)
        };
        let ((cols, sx, x0), (rows, sy, y0)) = (axis(ww, vw, ox), axis(wh, vh, oy));
        eprintln!("tiles {cols}x{rows}");
        let (col, row) = (col.min(cols - 1), row.min(rows - 1));
        let x = vx + x0 - col as f32 * sx;
        let y = vy + y0 - row as f32 * sy;
        self.cam.zoom = zoom;
        self.cam.pan = [x, y];
        let shows_a_node = (0..self.placed.pos.len()).any(|i| {
            let [nx, ny, nw, nh] = self.screen_rect(i);
            nx < vx + vw && nx + nw > vx && ny < vy + vh && ny + nh > vy
        });
        if !shows_a_node {
            eprintln!("tile empty");
        }
    }

    /// Screen rectangle of a view node under the current camera.
    pub fn screen_rect(&self, i: usize) -> [f32; 4] {
        self.item_rect(Item::Node(i))
    }

    /// Select an item; when the docked details panel would cover part of it (a node or a
    /// frame), pan it into the canvas left free beside the panel.
    pub fn select(&mut self, item: Item) {
        self.selected = Some(item);
        let [vx, _, vw, _] = self.free();
        let [x, _, w, _] = self.item_rect(item);
        if x + w > vx + vw - EDGE_PAD {
            self.show(item);
        }
    }

    /// Hover changes are ignored while a pan drag is held: the world moves with the pointer, so
    /// what it is over cannot change, but the pointer runs up to a frame ahead of where the world
    /// was last drawn and hit tests against that frame would flicker hover (card, outline, wire
    /// focus) on and off.
    pub fn hover(&mut self, item: Item, on: bool) {
        if self.cam.dragging {
            return;
        }
        if on {
            self.hovered = Some(item);
        } else if self.hovered == Some(item) {
            self.hovered = None;
        }
    }

    /// Screen rectangle of a frame or node under the current camera.
    pub fn item_rect(&self, item: Item) -> [f32; 4] {
        let [x, y, w, h] = self.world(item);
        let z = self.cam.zoom;
        [self.cam.pan[0] + x * z, self.cam.pan[1] + y * z, w * z, h * z]
    }

    /// Whether a view node is a collapsed node group that can be viewed as a frame.
    pub fn can_expand(&self, i: usize) -> bool {
        let Some(n) = self.view.nodes.get(i) else { return false };
        if n.kind != Kind::Group {
            return false;
        }
        match &self.live {
            Some(live) => live.origin.get(i).is_some_and(|&id| bridge::openable(&live.snap, id)),
            None => true,
        }
    }

    /// Frame button / double-click on a frame: view the group as a single node. On a collapsed
    /// mock group (scripted scenes), expands it instead.
    pub fn toggle_group(&mut self, g: usize) {
        if self.live.is_none() && self.collapsed.get(g).copied().unwrap_or(false) {
            if let Some(Item::Node(i)) = self.locate(Ident::Group(g)) {
                self.expand_node(i);
            }
            return;
        }
        self.collapse_group(g);
    }

    /// View an expanded group (frame `g`) as one node whose pins are the wires crossing its frame.
    pub fn collapse_group(&mut self, g: usize) {
        if g >= self.view.groups.len() {
            return;
        }
        self.reshape(Item::Group(g), |s| match &mut s.live {
            // Closing a frame closes that protocol container; it becomes one node with its pins.
            Some(live) => live.frames.get(g).copied().is_some_and(|f| live.open.remove(&f)),
            None => s.collapsed.get_mut(g).is_some_and(|c| !std::mem::replace(c, true)),
        });
    }

    /// Expand button / double-click on a collapsed group node: view it as a frame in place.
    pub fn expand_node(&mut self, i: usize) {
        if !self.can_expand(i) {
            return;
        }
        let ident = self.ident(Item::Node(i));
        // In the whole-program view a function opens on its own level, as from the explorer.
        if let (Some(live), Some(Ident::Proto(id))) = (&self.live, ident)
            && live.whole
            && live.snap.graph.nodes.iter().any(|n| n.id == id && n.kind == argo_protocol::NodeKind::Function)
        {
            crate::project::open_function(self, id);
            return;
        }
        self.reshape(Item::Node(i), |s| match (&mut s.live, ident) {
            (Some(live), Some(Ident::Proto(id))) => live.open.insert(id),
            (None, Some(Ident::Group(g))) => s.collapsed.get_mut(g).is_some_and(|c| std::mem::replace(c, false)),
            _ => false,
        });
    }

    /// Double-click on a node: a collapsed group expands into a frame; in live mode a closed
    /// construct opens in place, and an opened construct's own node closes it again.
    pub fn open_node(&mut self, i: usize) {
        if self.can_expand(i) {
            self.expand_node(i);
            return;
        }
        let Some(live) = &self.live else { return };
        let Some(&id) = live.origin.get(i) else { return };
        let parent = live.snap.graph.nodes.iter().find(|n| n.id == id).and_then(|n| n.parent);
        if bridge::openable(&live.snap, id) {
            self.reshape(Item::Node(i), |s| s.live.as_mut().is_some_and(|l| l.open.insert(id)));
        } else if let Some(p) = parent.filter(|p| live.open.contains(p)) {
            // A member of a frame closes the frame, like a double-click on the frame body.
            match live.frames.iter().position(|&f| f == p) {
                Some(g) => self.collapse_group(g),
                None => self.reshape(Item::Node(i), |s| s.live.as_mut().is_some_and(|l| l.open.remove(&p))),
            }
        }
    }

    /// Apply a structural view change (`change` returns whether anything changed), keeping
    /// `anchor` where it was on screen: its top-left corner stays put, then the camera nudges
    /// just enough to show more of what replaced it. Hover clears; selection follows the item
    /// it named (a member of a collapsed group selects the group's node).
    fn reshape(&mut self, anchor: Item, change: impl FnOnce(&mut State) -> bool) {
        let before = self.item_rect(anchor);
        let target = self.ident(anchor);
        let selected = self.selected.and_then(|it| self.ident(it));
        if !change(self) {
            return;
        }
        self.hovered = None;
        self.reproject();
        self.relayout();
        self.selected = selected.and_then(|id| self.locate(id));
        if let Some(item) = target.and_then(|id| self.locate(id)) {
            let after = self.item_rect(item);
            self.cam.pan[0] += before[0] - after[0];
            self.cam.pan[1] += before[1] - after[1];
            self.reveal(self.item_rect(item));
        }
    }

    /// Pan so more of `r` (a screen rectangle) shows in the free canvas area, without moving its
    /// top-left corner out of view.
    fn reveal(&mut self, r: [f32; 4]) {
        const PAD: f32 = 24.0;
        let [vx, vy, vw, vh] = ui::free_area(self.selected.is_some(), self.project.is_some());
        for (axis, start, len, lo, hi) in [(0, r[0], r[2], vx, vx + vw), (1, r[1], r[3], vy, vy + vh)] {
            let over = start + len + PAD - hi;
            let room = start - (lo + PAD);
            if over > 0.0 && room > 0.0 {
                self.cam.pan[axis] -= over.min(room);
            }
        }
    }

    /// A view-independent name for an item, valid across collapse/expand and reprojection.
    fn ident(&self, item: Item) -> Option<Ident> {
        match (&self.live, item) {
            (Some(live), Item::Node(i)) => live.origin.get(i).copied().map(Ident::Proto),
            (Some(live), Item::Group(g)) => live.frames.get(g).copied().map(Ident::Proto),
            (None, Item::Group(g)) => Some(Ident::Group(g)),
            (None, Item::Node(i)) => Some(match *self.graph.collapse(&self.collapsed).1.get(i)? {
                Origin::Node(n) => Ident::Node(n),
                Origin::Group(g) => Ident::Group(g),
            }),
        }
    }

    /// Where a named item is drawn now: itself, or the collapsed node or frame standing for it;
    /// for an opened construct, its first member.
    fn locate(&self, id: Ident) -> Option<Item> {
        match (&self.live, id) {
            (Some(live), Ident::Proto(id)) => {
                let node = |c: NodeId| live.origin.iter().position(|&o| o == c).map(Item::Node);
                let frame = |c: NodeId| live.frames.iter().position(|&f| f == c).map(Item::Group);
                let ix = argo_protocol::GraphIndex::new(&live.snap.graph);
                std::iter::once(id)
                    .chain(ix.ancestors(id))
                    .find_map(|c| node(c).or_else(|| frame(c)))
                    .or_else(|| live.origin.iter().position(|&o| ix.ancestors(o).contains(&id)).map(Item::Node))
            }
            (None, Ident::Node(n)) => {
                let g = self.graph.nodes.get(n)?.group;
                if let Some(g) = g.filter(|&g| self.collapsed.get(g).copied().unwrap_or(false)) {
                    return self.locate(Ident::Group(g));
                }
                let origin = self.graph.collapse(&self.collapsed).1;
                origin.iter().position(|&o| o == Origin::Node(n)).map(Item::Node)
            }
            (None, Ident::Group(g)) => {
                if !self.collapsed.get(g).copied().unwrap_or(false) {
                    return (g < self.view.groups.len()).then_some(Item::Group(g));
                }
                let origin = self.graph.collapse(&self.collapsed).1;
                origin.iter().position(|&o| o == Origin::Group(g)).map(Item::Node)
            }
            _ => None,
        }
    }

    /// View index of the first node with this title.
    pub fn find(&self, title: &str) -> Option<usize> {
        self.view.nodes.iter().position(|n| n.title == title)
    }
}

/// Test data only: the seeded mock scenes and the `argo-protocol` fixture, which the app never
/// loads (its only data source is the database).
#[cfg(test)]
impl State {
    /// A mock scene, fitted to the window.
    pub fn load(scene: &'static str) -> State {
        let mut s = State::with_graph(layout::with_getters(crate::scenes::scene(scene)));
        s.fit();
        s
    }

    /// The §5.6 fixture from `argo-protocol`: `compute` with its loop opened in place.
    pub fn protocol_fixture() -> State {
        let (_, ids) = argo_protocol::fixture::graph();
        let open = [ids.loop_region, ids.loop_body].into_iter().collect();
        State::from_snapshot(argo_protocol::fixture::snapshot(), Some(ids.compute), open)
    }

    /// The §5.6 fixture's module level: its functions as collapsed node groups, with `compute`
    /// opened in place as a frame when `open_compute`.
    pub fn protocol_module(open_compute: bool) -> State {
        let (_, ids) = argo_protocol::fixture::graph();
        let open = if open_compute { [ids.compute].into_iter().collect() } else { HashSet::new() };
        State::from_snapshot(argo_protocol::fixture::snapshot(), Some(ids.module), open)
    }

    /// A name for test messages: the breadcrumb.
    pub fn name(&self) -> String {
        self.view.crumbs.join(" › ")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use argo_protocol::fixture;

    use super::*;
    use crate::model::Wire;

    /// A selection and a hover a person could see beside the details panel: `normalize`
    /// (claimed, feeding round_pack) stays in view at every review size.
    const DATAFLOW_SELECT: [&str; 2] = ["round_pack", "normalize"];

    fn near(a: [f32; 4], b: [f32; 4]) -> bool {
        (a[0] - b[0]).abs() < 0.5 && (a[1] - b[1]).abs() < 0.5
    }

    /// `after` keeps `before`'s top-left corner on screen, or slid up/left only to show more of
    /// itself, never past the free canvas area's padded edge.
    fn anchored(s: &State, after: [f32; 4], before: [f32; 4]) -> bool {
        let [vx, vy, _, _] = ui::free_area(s.selected.is_some(), s.project.is_some());
        [(after[0], before[0], vx), (after[1], before[1], vy)]
            .iter()
            .all(|&(a, b, lo)| (a - b).abs() < 0.5 || (a < b && a >= lo + 24.0 - 0.5))
    }

    fn group_index(s: &State, title: &str) -> usize {
        s.view.groups.iter().position(|g| g.title == title).unwrap_or_else(|| panic!("no group {title}"))
    }

    /// The node standing for a collapsed group, by title.
    fn group_node(s: &State, title: &str) -> usize {
        s.view.nodes.iter().position(|n| n.kind == Kind::Group && n.title == title).unwrap_or_else(|| panic!("no group node {title}"))
    }

    /// Wires of a view graph with exactly one end inside group `gi`'s frame, as
    /// (outside node title, outside pin label, entering the frame); entering wires once per
    /// outside source value, as the collapsed node draws them.
    fn crossing(g: &Graph, gi: usize) -> Vec<(String, String, bool)> {
        let inside = |i: usize| g.nodes[i].group == Some(gi);
        let mut seen = HashSet::new();
        let mut v: Vec<_> = g.wires.iter().filter(|w| inside(w.from.0) != inside(w.to.0) && (!inside(w.to.0) || seen.insert(w.from))).map(|w| outside_end(g, w, inside(w.to.0))).collect();
        v.sort();
        v
    }

    /// Wires of a view graph touching node `i`, named as [`crossing`] names them.
    fn touching(g: &Graph, i: usize) -> Vec<(String, String, bool)> {
        let mut v: Vec<_> = g.wires.iter().filter(|w| (w.from.0 == i) != (w.to.0 == i)).map(|w| outside_end(g, w, w.to.0 == i)).collect();
        v.sort();
        v
    }

    fn outside_end(g: &Graph, w: &Wire, entering: bool) -> (String, String, bool) {
        let (node, pin) = if entering {
            (&g.nodes[w.from.0], &g.nodes[w.from.0].outs[w.from.1])
        } else {
            (&g.nodes[w.to.0], &g.nodes[w.to.0].ins[w.to.1])
        };
        (node.title.clone(), pin.label.clone(), entering)
    }

    /// Collapse a mock group and expand it again: the node's pins are exactly the wires that
    /// crossed the frame, the group stays put on screen, and selection follows it both ways.
    fn mock_round_trip(scene: &'static str, title: &str) {
        let mut s = State::load(scene);
        let g = group_index(&s, title);
        let frame = s.item_rect(Item::Group(g));
        let crossed = crossing(&s.view, g);
        assert!(!crossed.is_empty(), "{title}: a group with no wires in or out proves nothing");
        // One input per outside source value, one output per inside source value.
        let inside = |i: usize| s.view.nodes[i].group == Some(g);
        let sources = |entering: bool| {
            s.view.wires.iter().filter(|w| inside(w.from.0) != inside(w.to.0) && inside(w.to.0) == entering).map(|w| w.from).collect::<HashSet<_>>().len()
        };
        let (ins, outs) = (sources(true), sources(false));
        let members = s.view.nodes.iter().filter(|n| n.group == Some(g)).count();
        let total = s.view.nodes.len();
        s.selected = Some(Item::Group(g));
        s.hovered = Some(Item::Group(g));

        s.collapse_group(g);
        let i = group_node(&s, title);
        assert!(s.can_expand(i));
        assert_eq!(s.view.nodes.len(), total - members + 1);
        assert_eq!(s.view.nodes[i].members, members);
        assert_eq!(touching(&s.view, i), crossed, "{title}: the collapsed node's wires are the ones that crossed the frame");
        assert_eq!((s.view.nodes[i].ins.len(), s.view.nodes[i].outs.len()), (ins, outs), "{title}: one pin per crossing value");
        assert!(near(s.item_rect(Item::Node(i)), frame), "{title}: the node takes the frame's place");
        assert_eq!(s.selected, Some(Item::Node(i)));
        assert_eq!(s.hovered, None);

        s.expand_node(i);
        assert_eq!(group_index(&s, title), g);
        assert!(!s.collapsed[g]);
        assert_eq!(s.view.nodes.len(), total);
        assert_eq!(crossing(&s.view, g), crossed);
        // Opening may slide the frame up or left to show more of it, never off its anchor.
        assert!(anchored(&s, s.item_rect(Item::Group(g)), frame), "{title}: the frame opens where the node was: {:?} vs {frame:?}", s.item_rect(Item::Group(g)));
        assert_eq!(s.selected, Some(Item::Group(g)));
    }

    #[test]
    fn mock_groups_round_trip_between_frame_and_node() {
        mock_round_trip("fixture", "step ∘ normalize");
        mock_round_trip("interp", "alu");
        mock_round_trip("dataflow", "mul_core");
    }

    #[test]
    fn selection_follows_members_into_the_collapsed_node() {
        let mut s = State::load("fixture");
        let g = group_index(&s, "step ∘ normalize");
        let member = s.view.nodes.iter().position(|n| n.group == Some(g)).unwrap();
        s.selected = Some(Item::Node(member));
        s.collapse_group(g);
        assert_eq!(s.selected, Some(Item::Node(group_node(&s, "step ∘ normalize"))));
        // An outside node stays selected across a change of view indices.
        s.selected = s.find("report").map(Item::Node);
        s.expand_node(group_node(&s, "step ∘ normalize"));
        let Some(Item::Node(i)) = s.selected else { panic!("selection lost") };
        assert_eq!(s.view.nodes[i].title, "report");
    }

    #[test]
    fn double_click_expands_a_collapsed_group_and_the_frame_toggle_collapses() {
        let mut s = State::load("interp");
        s.toggle_group(0);
        assert!(s.collapsed[0]);
        s.open_node(group_node(&s, "alu"));
        assert!(!s.collapsed[0], "double-click on the group node expands it");
        s.toggle_group(0);
        assert!(s.collapsed[0], "double-click on the frame collapses it");
        s.toggle_group(0);
        assert!(!s.collapsed[0], "on a collapsed mock group, toggling expands");
        // Ordinary nodes have nothing to open.
        let other = s.view.nodes.iter().position(|n| n.kind != Kind::Group).unwrap();
        assert!(!s.can_expand(other));
        let before = s.view.nodes.len();
        s.open_node(other);
        assert_eq!(s.view.nodes.len(), before);
    }

    #[test]
    fn live_function_opens_in_place_as_a_frame_and_closes_to_a_node() {
        let mut s = State::protocol_module(false);
        assert!(s.view.groups.is_empty());
        let i = group_node(&s, "compute");
        assert!(s.can_expand(i), "a closed function is a collapsed node group");
        let pins = |n: &crate::model::Node| {
            (n.ins.iter().map(|p| p.label.clone()).collect::<Vec<_>>(), n.outs.iter().map(|p| p.label.clone()).collect::<Vec<_>>())
        };
        let closed_pins = pins(&s.view.nodes[i]);
        let closed_wires = touching(&s.view, i);
        let before = s.item_rect(Item::Node(i));
        s.selected = Some(Item::Node(i));

        s.expand_node(i);
        assert_eq!(s.view.groups.iter().map(|g| g.title.as_str()).collect::<Vec<_>>(), ["compute"]);
        assert!(s.view.nodes.iter().any(|n| n.group == Some(0) && n.title == "uint32_t acc = 0"));
        assert!(s.view.nodes.iter().all(|n| n.title != "compute"), "the frame replaces the node");
        assert!(anchored(&s, s.item_rect(Item::Group(0)), before), "the frame opens where the node was");
        assert_eq!(s.selected, Some(Item::Group(0)));
        assert_eq!(crossing(&s.view, 0), closed_wires);
        let frame = s.item_rect(Item::Group(0));

        s.collapse_group(0);
        assert!(s.view.groups.is_empty());
        let i = group_node(&s, "compute");
        assert_eq!(pins(&s.view.nodes[i]), closed_pins);
        assert_eq!(touching(&s.view, i), closed_wires);
        assert!(anchored(&s, s.item_rect(Item::Node(i)), frame), "the node takes the frame's place");
        assert_eq!(s.selected, Some(Item::Node(i)));
    }

    /// A merged group (§5.10, `status.collapsed`) inside a function: closed, it is one node whose
    /// pins carry exactly the wires that cross its frame when opened.
    #[test]
    fn live_merged_group_pins_are_the_wires_crossing_its_frame() {
        let (_, ids) = fixture::graph();
        let mut snap = fixture::snapshot();
        match snap.status.iter_mut().find(|st| st.node == ids.loop_region) {
            Some(st) => st.collapsed = true,
            None => {
                let mut st = argo_protocol::NodeStatus::new(ids.loop_region, argo_protocol::Language::C);
                st.collapsed = true;
                snap.status.push(st);
            }
        }
        let mut s = State::from_snapshot(snap, Some(ids.compute), HashSet::new());
        let i = s.view.nodes.iter().position(|n| n.kind == Kind::Group).expect("the merged group is one node");
        assert!(s.can_expand(i));
        let closed = touching(&s.view, i);
        assert!(!closed.is_empty());

        s.expand_node(i);
        assert_eq!(s.view.groups.len(), 1, "a merged group opens as a frame");
        assert_eq!(crossing(&s.view, 0), closed, "its pins were the wires crossing the frame");

        s.collapse_group(0);
        assert!(s.view.groups.is_empty());
        let i = s.view.nodes.iter().position(|n| n.kind == Kind::Group).unwrap();
        assert_eq!(touching(&s.view, i), closed);
    }

    #[test]
    fn live_double_click_on_a_frame_member_closes_the_frame() {
        let mut s = State::protocol_module(true);
        let member = s.view.nodes.iter().position(|n| n.title == "uint32_t acc = 0").unwrap();
        s.open_node(member);
        assert!(s.view.groups.is_empty());
        assert!(s.can_expand(group_node(&s, "compute")));
    }

    /// Window sizes from the review set where the default view must not hide content.
    const SIZES: [[f32; 2]; 3] = [[1024.0, 640.0], [1440.0, 900.0], [2560.0, 1440.0]];

    /// `r` lies inside `a` (half a point of slack).
    fn inside(r: [f32; 4], a: [f32; 4]) -> bool {
        r[0] >= a[0] - 0.5 && r[1] >= a[1] - 0.5 && r[0] + r[2] <= a[0] + a[2] + 0.5 && r[1] + r[3] <= a[1] + a[3] + 0.5
    }

    /// Every drawn node and frame of the current view, on screen.
    fn drawn(s: &State) -> Vec<[f32; 4]> {
        let frames = (0..s.placed.frames.len()).filter(|&g| s.placed.frames[g][2] > 0.0 && !s.collapsed.get(g).copied().unwrap_or(false));
        (0..s.placed.pos.len()).map(|i| s.screen_rect(i)).chain(frames.map(|g| s.item_rect(Item::Group(g)))).collect()
    }

    /// The protocol fixture shows whole, from its entry to its exits, at every review size: in a
    /// small window the whole flow at a zoom down to [`CONTEXT_FLOOR`] beats a readable slice
    /// whose far end runs off the right edge.
    #[test]
    fn fit_shows_the_protocol_fixture_from_its_entry() {
        for size in SIZES {
            ui::set_viewport(size);
            let s = State::protocol_fixture();
            let free = s.free();
            assert!(s.cam.zoom >= CONTEXT_FLOOR - 1e-4 && s.cam.zoom <= WIDE_MAX + 1e-4, "{size:?}: zoom {}", s.cam.zoom);
            if size[0] <= WIDE {
                assert!(s.cam.zoom <= FIT_MAX + 1e-4, "{size:?}: zoom {} above the band", s.cam.zoom);
            }
            for (i, r) in drawn(&s).into_iter().enumerate() {
                assert!(inside(r, free), "{size:?}: item {i} at {r:?} outside {free:?}");
            }
            let input = s.screen_rect(s.find("Input").unwrap());
            assert!(inside(input, free), "{size:?}: the entry {input:?} outside {free:?}");
        }
    }

    /// At 1024x640 every node of the protocol fixture, its exits included, lies wholly inside
    /// the free canvas area: nothing is sliced at the window's right edge.
    #[test]
    fn fit_shows_every_protocol_node_at_1024() {
        ui::set_viewport([1024.0, 640.0]);
        let s = State::protocol_fixture();
        let free = s.free();
        assert!(!s.exits().is_empty());
        for i in 0..s.view.nodes.len() {
            let r = s.screen_rect(i);
            assert!(inside(r, free), "{} at {r:?} outside {free:?}", s.view.nodes[i].title);
        }
    }

    /// A small graph in a wide window grows past [`FIT_MAX`] to fill about [`WIDE_FILL`] of the
    /// free area's limiting axis, capped at [`WIDE_MAX`]; up to [`WIDE`] points the ceiling stays.
    #[test]
    fn wide_windows_magnify_a_small_graph_gently() {
        ui::set_viewport([2560.0, 1440.0]);
        let s = State::protocol_fixture();
        let [_, _, bw, bh] = s.placed.bounds;
        let [_, _, vw, vh] = s.free();
        let whole = (vw / (bw + 2.0 * MARGIN)).min(vh / (bh + 2.0 * MARGIN));
        let z = s.cam.zoom;
        assert!(z > FIT_MAX && z <= WIDE_MAX + 1e-4, "zoom {z}");
        assert!(z <= whole + 1e-4, "zoom {z} overflows the free area (whole {whole})");
        assert!((z - (WIDE_FILL * whole).clamp(FIT_MAX, WIDE_MAX)).abs() < 1e-3, "zoom {z}, whole {whole}");
        // The ceiling is continuous in the window width and unchanged up to WIDE.
        for w in [1024.0, 1440.0, WIDE] {
            ui::set_viewport([w, 900.0]);
            assert_eq!(ceiling(10.0), FIT_MAX, "{w}");
        }
        ui::set_viewport([WIDE + 1.0, 900.0]);
        assert!(ceiling(10.0) - FIT_MAX < 0.01);
    }

    /// The fixture's selected group shows whole beside the details panel, with what feeds it (the
    /// for loop, `acc = 0` and the graph's entry, Input) whole in view where the window has room
    /// for it after the claimed report; in a small window that upstream context is the goal that
    /// gives way, falling off the left edge whole, never cut.
    #[test]
    fn fit_keeps_the_fixtures_selected_group_and_its_upstream_beside_the_details_panel() {
        for size in SIZES {
            ui::set_viewport(size);
            let s = State::load("fixture");
            let Some(Item::Group(g)) = s.selected else { panic!("the fixture selects its group") };
            let free = ui::free_area(true, false);
            let r = s.item_rect(Item::Group(g));
            assert!(inside(r, free), "{size:?}: selected group {r:?} outside {free:?}");
            let up = s.upstream(Item::Group(g));
            let mut titles: Vec<&str> = up.iter().map(|&i| s.view.nodes[i].title.as_str()).collect();
            titles.sort();
            assert_eq!(titles, ["Input", "acc = 0", "for (i = 0; i < n; i++)"]);
            for i in up {
                let r = s.screen_rect(i);
                let name = &s.view.nodes[i].title;
                if size[0] > 1024.0 {
                    assert!(inside(r, free), "{size:?}: upstream {name} {r:?} outside {free:?}");
                } else {
                    assert!(inside(r, free) || r[0] + r[2] <= free[0] + 0.5, "{size:?}: upstream {name} {r:?} cut by {free:?}");
                }
            }
        }
    }

    /// At 1024x640 beside the details panel, the fixture's far upstream may fall off the left
    /// edge (the claimed report outranks it), but no wire into a node on screen comes from a
    /// node cut at that edge: each source shows whole or not at all.
    #[test]
    fn fit_shows_the_fixtures_input_in_a_small_window() {
        ui::set_viewport([1024.0, 640.0]);
        let s = State::load("fixture");
        assert!(matches!(s.selected, Some(Item::Group(_))), "the fixture selects its group");
        let free = s.free();
        let input = s.screen_rect(s.find("Input").unwrap());
        assert!(inside(input, free) || !overlaps(input, free), "Input {input:?} cut by {free:?}");
        for w in &s.view.wires {
            let from = s.screen_rect(w.from.0);
            let to = s.screen_rect(w.to.0);
            if overlaps(to, free) {
                let (name, into) = (&s.view.nodes[w.from.0].title, &s.view.nodes[w.to.0].title);
                assert!(inside(from, free) || !overlaps(from, free), "a wire into {into} leaves from {name} cut at {from:?}");
            }
        }
    }

    /// The default zoom stays in the readable band ([`FIT_FLOOR`] to the framing ceiling) for
    /// every review scene and window size, so node text never shrinks far below or swells far
    /// above the chrome's type. It goes below the band, down to [`CONTEXT_FLOOR`], only where
    /// that shows the whole graph, or a selection together with all of its claimed and failing
    /// items or its whole upstream context.
    #[test]
    fn default_zoom_stays_in_the_readable_band() {
        for size in SIZES {
            ui::set_viewport(size);
            let mut states = focused_scenes();
            states.push(State::load("devices"));
            let mut fixture = State::load("fixture");
            fixture.selected = None;
            fixture.fit();
            states.push(fixture);
            for s in states {
                let z = s.cam.zoom;
                let top = if size[0] <= WIDE { FIT_MAX } else { WIDE_MAX };
                assert!(z <= top + 1e-4, "{size:?} {}: zoom {z} above the band", s.name());
                if z >= FIT_FLOOR - 1e-4 {
                    continue;
                }
                assert!(z >= CONTEXT_FLOOR - 1e-4, "{size:?} {}: zoom {z} below the context floor", s.name());
                let free = s.free();
                if drawn(&s).into_iter().all(|r| inside(r, free)) {
                    continue;
                }
                let sel = s.selected.unwrap_or_else(|| panic!("{size:?} {}: zoom {z} below the band with no selection", s.name()));
                assert!(inside(s.item_rect(sel), free), "{size:?} {}: zoom {z} below the band yet the selection is hidden", s.name());
                let claims = s.claims().into_iter().all(|it| inside(s.item_rect(it), free));
                let upstream = s.upstream(sel).into_iter().all(|i| inside(s.screen_rect(i), free));
                assert!(claims || upstream, "{size:?} {}: zoom {z} below the band shows neither all claims nor all upstream", s.name());
            }
        }
        // Without a selection the view never goes below the band except to show the whole graph:
        // the fixture and the protocol fixture show whole at every review size.
        for size in SIZES {
            ui::set_viewport(size);
            let mut fixture = State::load("fixture");
            fixture.selected = None;
            fixture.fit();
            for s in [fixture, State::protocol_fixture()] {
                let free = s.free();
                assert!(s.cam.zoom >= CONTEXT_FLOOR - 1e-4, "{size:?} {}: zoom {}", s.name(), s.cam.zoom);
                assert!(drawn(&s).into_iter().all(|r| inside(r, free)), "{size:?} {}: not whole", s.name());
            }
        }
    }

    /// The default view of each review scene with something selected or claimed, as captured.
    fn focused_scenes() -> Vec<State> {
        let mut states: Vec<State> = ["fixture", "branches", "dataflow", "interp"].into_iter().map(State::load).collect();
        // round_pack selected after loading, then the first layout's fit.
        let mut df = State::load("dataflow");
        df.select(Item::Node(df.find("round_pack").unwrap()));
        states.push(df);
        let mut df = State::load("dataflow");
        df.selected = Some(Item::Node(df.find("round_pack").unwrap()));
        df.fit();
        states.push(df);
        let mut collapsed = State::load("interp");
        collapsed.toggle_group(0);
        collapsed.fit();
        states.push(collapsed);
        states.push(State::protocol_fixture());
        states.push(State::protocol_module(true));
        states
    }

    #[test]
    fn fit_never_cuts_a_claimed_item_at_the_details_panel() {
        ui::set_viewport([1024.0, 640.0]);
        let s = State::load("fixture");
        let free = s.free();
        let right = free[0] + free[2];
        let sel = s.item_rect(s.selected.expect("the fixture selects its group"));
        assert!(inside(sel, free), "selection {sel:?} outside {free:?}");
        let claims = s.claims();
        assert!(claims.len() >= 2, "the fixture claims its group and report");
        for item in claims {
            let r = s.item_rect(item);
            assert!(!(r[0] < right - 0.5 && r[0] + r[2] > right + 0.5), "{item:?} {r:?} straddles the free area's right edge {right}");
            assert!(inside(r, free) || !overlaps(r, free), "{item:?} {r:?} is cut by the free area {free:?}");
        }
        // The claimed report with its failing checks outranks the selection's far upstream: the
        // entry falls off the left before the report falls under the panel.
        let report = s.item_rect(Item::Node(s.find("report").unwrap()));
        assert!(inside(report, free), "report {report:?} hidden beside {free:?}");
        for s in focused_scenes() {
            let free = s.free();
            let right = free[0] + free[2];
            if let Some(sel) = s.selected {
                let r = s.item_rect(sel);
                assert!(inside(r, free), "{}: selection {r:?} outside {free:?}", s.name());
            }
            // Only claims in view count: one below or above the free area may line up with
            // its right edge without being cut by it.
            for item in s.claims() {
                let r = s.item_rect(item);
                let straddles = r[0] < right - 0.5 && r[0] + r[2] > right + 0.5;
                assert!(!(straddles && overlaps(r, free)), "{}: {item:?} {r:?} straddles {right}", s.name());
            }
        }
    }

    /// With the fixture's group selected, the downstream `acc =` (Set acc) and the claimed
    /// `report` with failing checks show whole beside the details panel, not under it.
    #[test]
    fn fit_shows_the_fixtures_claimed_report_beside_the_details_panel() {
        for size in SIZES {
            ui::set_viewport(size);
            let s = State::load("fixture");
            assert!(matches!(s.selected, Some(Item::Group(_))), "the fixture selects its group");
            let free = ui::free_area(true, false);
            for title in ["report", "acc ="] {
                let r = s.screen_rect(s.find(title).unwrap());
                assert!(inside(r, free), "{size:?}: {title} {r:?} outside {free:?}");
            }
        }
    }

    /// A dataflow selection's hover is a node a person could see: on screen beside the
    /// details panel at every review size, and feeding the selection.
    #[test]
    fn dataflow_select_hovers_a_node_in_view() {
        let [sel, hover] = DATAFLOW_SELECT;
        for size in SIZES {
            ui::set_viewport(size);
            // Selected after loading, then (in the window) the first layout's fit.
            let mut a = State::load("dataflow");
            a.select(Item::Node(a.find(sel).unwrap()));
            let mut b = State::load("dataflow");
            b.selected = Some(Item::Node(b.find(sel).unwrap()));
            b.fit();
            for s in [a, b] {
                let h = s.find(hover).unwrap();
                assert!(s.upstream(s.selected.unwrap()).contains(&h), "{hover} feeds {sel}");
                let (r, free) = (s.screen_rect(h), s.free());
                assert!(inside(r, free), "{size:?}: hovered {hover} {r:?} outside {free:?}");
            }
        }
    }

    #[test]
    fn frames_on_screen_show_their_titles() {
        for size in SIZES {
            ui::set_viewport(size);
            for s in focused_scenes() {
                let free = s.free();
                for g in (0..s.view.groups.len()).filter(|&g| s.expanded(g)) {
                    let r = s.item_rect(Item::Group(g));
                    if overlaps(r, free) {
                        let (x, y) = (r[0], r[1]);
                        let start = x >= free[0] - 0.5 && x <= free[0] + free[2] && y >= free[1] - 0.5 && y <= free[1] + free[3];
                        assert!(start, "{size:?} {}: frame {g} at {r:?} shows without its title in {free:?}", s.name());
                    }
                }
            }
        }
    }

    #[test]
    fn revealed_items_keep_clear_of_the_overview_card() {
        for size in SIZES {
            ui::set_viewport(size);
            for s in focused_scenes() {
                if !crate::overview::overflows(&s) {
                    continue;
                }
                let card = crate::overview::rect(&s).unwrap();
                let free = s.free();
                for item in s.selected.into_iter().chain(s.claims()) {
                    let r = s.item_rect(item);
                    if inside(r, free) {
                        assert!(!overlaps(r, card), "{size:?} {}: {item:?} {r:?} under the overview {card:?}", s.name());
                    }
                }
            }
        }
    }

    #[test]
    fn content_never_starts_under_the_top_bar() {
        for size in SIZES {
            ui::set_viewport(size);
            let mut states: Vec<State> = ["fixture", "branches", "dataflow", "devices", "interp"].into_iter().map(State::load).collect();
            states.push(State::protocol_fixture());
            let mut collapsed = State::load("interp");
            collapsed.toggle_group(0);
            collapsed.fit();
            states.push(collapsed);
            for s in states {
                let free = s.free();
                // A selected or claimed item wins over reading from the top-left: showing it may
                // pan other content off an edge, but it starts inside the free area itself.
                let focus: Vec<Item> = s.selected.into_iter().chain(s.claims()).collect();
                if !focus.is_empty() {
                    // Claimed items may be hidden whole past an edge; those shown start inside.
                    for r in focus.into_iter().map(|it| s.item_rect(it)).filter(|&r| overlaps(r, free)) {
                        assert!(r[0] >= free[0] - 0.5 && r[1] >= free[1] - 0.5, "{size:?} {}: focus {r:?} starts outside the free area", s.name());
                    }
                    continue;
                }
                for r in drawn(&s) {
                    assert!(r[1] >= free[1] - 0.5 && r[0] >= free[0] - 0.5, "{size:?} {}: {r:?} starts outside the free area", s.name());
                }
            }
        }
    }

    /// When the graph overflows at readable zoom, it is read from its top-left: no empty band
    /// above or left of content that runs off the far edge.
    #[test]
    fn overflowing_fit_reads_from_the_top_left() {
        ui::set_viewport([1440.0, 900.0]);
        let s = State::load("branches");
        let [vx, vy, vw, vh] = s.free();
        let r = drawn(&s);
        let top = r.iter().map(|r| r[1]).fold(f32::MAX, f32::min);
        let bottom = r.iter().map(|r| r[1] + r[3]).fold(f32::MIN, f32::max);
        if bottom > vy + vh {
            assert!(top >= vy - 0.5 && top <= vy + EDGE_PAD + 0.5, "top {top}, free top {vy}");
        } else {
            assert!(top - vy - (vy + vh - bottom) < 1.0, "content is centred vertically: top {top}, bottom {bottom}");
        }
        let left = r.iter().map(|r| r[0]).fold(f32::MAX, f32::min);
        let right = r.iter().map(|r| r[0] + r[2]).fold(f32::MIN, f32::max);
        if right > vx + vw {
            assert!(left <= vx + EDGE_PAD + 0.5, "left {left}");
        }
    }

    #[test]
    fn selecting_a_frame_under_the_details_panel_pans_it_beside_the_panel() {
        ui::set_viewport([1024.0, 640.0]);
        let mut s = State::load("fixture");
        let g = group_index(&s, "step ∘ normalize");
        s.selected = None;
        s.fit();
        // Put the frame's right part under where the panel opens.
        let [fx, _, fw, _] = ui::free_area(true, false);
        let r = s.item_rect(Item::Group(g));
        s.cam.pan[0] += fx + fw - r[0] - r[2] / 2.0;
        s.select(Item::Group(g));
        let r = s.item_rect(Item::Group(g));
        let free = ui::free_area(true, false);
        if r[2] <= free[2] - 2.0 * EDGE_PAD {
            assert!(inside(r, free), "{r:?} outside {free:?}");
        } else {
            assert!((r[0] - (free[0] + EDGE_PAD)).abs() < 0.5, "a wide frame shows its left edge: {r:?}");
        }
        // A node fully in view does not move when selected.
        let padded = [free[0] + EDGE_PAD, free[1] + EDGE_PAD, free[2] - 2.0 * EDGE_PAD, free[3] - 2.0 * EDGE_PAD];
        let i = (0..s.view.nodes.len()).find(|&i| inside(s.screen_rect(i), padded)).expect("a node in view");
        let pan = s.cam.pan;
        s.select(Item::Node(i));
        assert_eq!(s.cam.pan, pan);
    }

    #[test]
    fn toggling_a_group_keeps_it_where_it_was_on_screen() {
        for size in SIZES {
            ui::set_viewport(size);
            let mut s = State::load("fixture");
            let g = group_index(&s, "step ∘ normalize");
            let frame = s.item_rect(Item::Group(g));
            s.toggle_group(g);
            let i = group_node(&s, "step ∘ normalize");
            let node = s.item_rect(Item::Node(i));
            assert!((node[0] - frame[0]).abs() < 3.0 && (node[1] - frame[1]).abs() < 3.0, "{size:?}: collapsed {node:?} vs frame {frame:?}");
            s.toggle_group(g);
            let again = s.item_rect(Item::Group(g));
            assert!((again[0] - node[0]).abs() < 3.0 && (again[1] - node[1]).abs() < 3.0, "{size:?}: reopened {again:?} vs node {node:?}");
        }
    }

    use rune_ui::Phase;

    #[test]
    fn pan_drag_follows_travel_and_marks_a_pan_past_click_slop() {
        let mut cam = Cam { zoom: 1.0, pan: [10.0, 20.0], ..Cam::default() };
        cam.drag(Phase::Down, [0.0, 0.0]);
        assert!(cam.dragging && !cam.panned);
        cam.drag(Phase::Move, [2.0, -1.0]);
        assert_eq!(cam.pan, [12.0, 19.0]);
        assert!(!cam.panned, "a jitter within click slop is still a click");
        cam.drag(Phase::Move, [-40.0, 30.0]);
        cam.drag(Phase::Up, [-40.0, 30.0]);
        assert_eq!(cam.pan, [-30.0, 50.0]);
        assert!(!cam.dragging);
        assert!(cam.take_panned(), "the release's press is a pan, not a click");
        assert!(!cam.take_panned(), "only once");
    }

    #[test]
    fn a_fresh_press_clears_an_unclaimed_pan() {
        let mut cam = Cam { zoom: 1.0, ..Cam::default() };
        cam.drag(Phase::Down, [0.0, 0.0]);
        cam.drag(Phase::Up, [80.0, 0.0]);
        // That release landed off the canvas, so no press claimed the pan; the next click counts.
        cam.drag(Phase::Down, [0.0, 0.0]);
        cam.drag(Phase::Up, [0.0, 0.0]);
        assert!(!cam.take_panned());
    }

    #[test]
    fn hover_holds_still_while_panning() {
        let mut s = State::load("fixture");
        s.hover(Item::Node(0), true);
        s.cam.drag(Phase::Down, [0.0, 0.0]);
        // Hit tests against the last drawn frame lag the pointer: these leaves and enters are noise.
        s.hover(Item::Node(0), false);
        s.hover(Item::Node(1), true);
        assert_eq!(s.hovered, Some(Item::Node(0)));
        s.cam.drag(Phase::Up, [30.0, 0.0]);
        s.hover(Item::Node(1), true);
        assert_eq!(s.hovered, Some(Item::Node(1)));
    }
}
