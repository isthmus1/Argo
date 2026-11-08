//! Application state: the semantic graph, the derived view graph and layout, the camera, hover
//! and selection. Live updates change claims and evidence in place, so layout, camera and
//! selection survive them.

use std::collections::{HashSet, VecDeque};

use argo_protocol::{NodeId, NodeKind, Snapshot, Update, fixture};

use crate::bridge;
use crate::layout::{self, Placed};
use crate::project;
use crate::model::{Evidence, Graph, Kind, Lang, Origin};
use crate::scenes;
use crate::ui;

/// World padding around the laid-out graph.
pub const MARGIN: f32 = 40.0;
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 2.0;
/// The default view never zooms out past this; larger graphs pan instead. Below about this zoom
/// headers and pin labels are too small to read.
const READABLE_ZOOM: f32 = 0.9;
/// The default view still frames the whole graph down to this zoom (labels stay legible), so
/// small graphs show whole rather than panned at [`READABLE_ZOOM`].
const FIT_FLOOR: f32 = 0.72;
/// Framing never magnifies past this, so a small graph in a large window stays node-sized.
const FIT_MAX: f32 = 1.6;
/// Gap between the free canvas area's edge and content aligned to it.
const EDGE_PAD: f32 = 24.0;

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
    pub scene: &'static str,
    /// The semantic graph as received (claims and evidence update live).
    pub graph: Graph,
    pub collapsed: Vec<bool>,
    /// The graph as drawn: collapsed groups folded into single nodes.
    pub view: Graph,
    pub placed: Placed,
    pub cam: Cam,
    pub hovered: Option<Item>,
    pub selected: Option<Item>,
    /// Set when the graph comes from an `argo-protocol` snapshot rather than a seeded mock.
    pub live: Option<Live>,
    /// An opened C project, with its file/function index.
    pub project: Option<project::Project>,
    pub forms: project::Forms,
    pub modal: Option<project::Modal>,
    pub loader: Option<project::Loader>,
    next_event: f64,
    step: usize,
    /// Whether the camera has been fitted to the real window size yet.
    sized: bool,
}

/// A protocol snapshot, the level being shown, and the deltas still to replay.
pub struct Live {
    pub snap: Snapshot,
    pub container: Option<NodeId>,
    pub open: HashSet<NodeId>,
    pending: VecDeque<Update>,
    origin: Vec<NodeId>,
    frames: Vec<NodeId>,
}

impl Live {
    /// Recorded updates still to replay.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

impl State {
    pub fn load(scene: &'static str) -> State {
        let graph = layout::with_getters(scenes::scene(scene));
        let collapsed = vec![false; graph.groups.len()];
        let selected = graph.selected_group.map(Item::Group);
        let mut s = State {
            scene, view: graph.clone(), graph, collapsed, placed: Placed { pos: vec![], size: vec![], frames: vec![], bounds: [0.0; 4], row_gaps: vec![] },
            cam: Cam { zoom: 1.0, ..Cam::default() }, hovered: None, selected, live: None, next_event: 4.0, step: 0,
            project: None, forms: project::Forms::default(), modal: None, loader: None,
            // A state built after the window's first layout already knows its size.
            sized: ui::viewport() != [ui::W, ui::H],
        };
        s.relayout();
        s.fit();
        s
    }

    /// A protocol-backed scene: show `container`'s level with `open` drawn in place, then replay
    /// `transcript` as live deltas.
    pub fn from_snapshot(scene: &'static str, snap: Snapshot, container: Option<NodeId>, open: HashSet<NodeId>, transcript: Vec<Update>) -> State {
        let mut s = State::load("fixture");
        s.scene = scene;
        s.selected = None;
        s.live = Some(Live { snap, container, open, pending: transcript.into(), origin: vec![], frames: vec![] });
        s.reproject();
        s.relayout();
        s.fit();
        s
    }

    /// The §5.6 fixture from `argo-protocol`: `compute` with its loop opened in place.
    pub fn protocol_fixture() -> State {
        let (_, ids) = fixture::graph();
        let open = [ids.loop_region, ids.loop_body].into_iter().collect();
        State::from_snapshot("protocol", fixture::snapshot(), Some(ids.compute), open, fixture::transcript())
    }

    /// The §5.6 fixture's module level: its functions as collapsed node groups, with `compute`
    /// opened in place as a frame when `open_compute`.
    pub fn protocol_module(open_compute: bool) -> State {
        let (_, ids) = fixture::graph();
        let open = if open_compute { [ids.compute].into_iter().collect() } else { HashSet::new() };
        State::from_snapshot("protocol-module", fixture::snapshot(), Some(ids.module), open, vec![])
    }

    /// A snapshot file (e.g. `main --protocol-json x.c`), showing one function's body.
    /// `function` picks it by name; otherwise the first function.
    pub fn snapshot_file(path: &str, function: Option<&str>) -> Result<State, String> {
        let json = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        let snap: Snapshot = serde_json::from_str(&json).map_err(|e| format!("{path}: {e}"))?;
        let f = snap
            .graph
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Function && function.is_none_or(|name| n.name == name))
            .ok_or_else(|| format!("{path}: no function {}", function.unwrap_or("")))?;
        // Show the function itself, so its parameters and result are on the canvas, with its body
        // block open (the bridge draws no frame for it).
        let container = f.id;
        let body: Vec<NodeId> = snap
            .graph
            .nodes
            .iter()
            .filter(|n| n.parent == Some(f.id) && n.kind == NodeKind::Scope)
            .map(|n| n.id)
            .collect();
        // `ARGO_OPEN=all` opens every construct inside the shown level.
        let open = if std::env::var("ARGO_OPEN").is_ok_and(|v| v == "all") {
            let mut open = HashSet::new();
            let mut stack = vec![container];
            while let Some(c) = stack.pop() {
                for n in snap.graph.nodes.iter().filter(|n| n.parent == Some(c)) {
                    open.insert(n.id);
                    stack.push(n.id);
                }
            }
            open
        } else {
            body.into_iter().collect()
        };
        Ok(State::from_snapshot("snapshot", snap, Some(container), open, vec![]))
    }

    /// Apply up to `n` pending deltas at once (scripted captures).
    pub fn replay(&mut self, n: usize) {
        let Some(live) = &mut self.live else { return };
        for update in live.pending.drain(..n.min(live.pending.len())) {
            live.snap.apply(update);
        }
        self.reproject();
        self.refresh();
    }

    /// Re-derive the semantic graph from the protocol snapshot (live scenes only).
    pub(crate) fn reproject(&mut self) {
        let Some(live) = &mut self.live else { return };
        let p = bridge::project(&live.snap, live.container, &live.open);
        live.origin = p.origin;
        live.frames = p.frames;
        self.graph = layout::with_getters(p.graph);
        self.collapsed = vec![false; self.graph.groups.len()];
    }

    /// Rebuild the view graph and its layout after a structural view change (collapse/expand).
    pub(crate) fn relayout(&mut self) {
        self.view = self.graph.collapse(&self.collapsed).0;
        self.placed = layout::layout(&self.view);
    }

    /// Rebuild the view graph only: claims and evidence changed, positions did not.
    fn refresh(&mut self) {
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
        self.place((vw / ww).min(vh / wh).clamp(MIN_ZOOM, FIT_MAX));
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

    /// Default view: fit everything when that stays readable; otherwise keep a readable zoom,
    /// read the graph from its top-left, and bring the selection (or where an agent is
    /// working) into the free area, leaving the rest to panning.
    pub fn fit(&mut self) {
        self.fit_all();
        // Captures can pan like a person would: `ARGO_TILE=col,row` at `ARGO_ZOOM`.
        if let Ok(t) = std::env::var("ARGO_TILE") {
            let zoom = std::env::var("ARGO_ZOOM").ok().and_then(|z| z.parse().ok()).unwrap_or(1.0);
            let (c, r) = t.split_once(',').and_then(|(c, r)| Some((c.parse().ok()?, r.parse().ok()?))).unwrap_or((0, 0));
            self.tile(zoom, c, r);
            return;
        }
        // Captures can ask for the whole graph regardless of readability (`ARGO_FIT=all`).
        if self.cam.zoom >= FIT_FLOOR || std::env::var("ARGO_FIT").is_ok_and(|v| v == "all") {
            return;
        }
        // Start where the program starts (reading order); the far edges fade to show more.
        self.place(READABLE_ZOOM);
        // Where an agent is working, then the selection (which wins), stay in view.
        for item in [self.claimed(), self.selected].into_iter().flatten() {
            self.show(item);
        }
    }

    /// The item an agent is working on: an active claim first, else a queued one.
    fn claimed(&self) -> Option<Item> {
        let nodes = self.view.nodes.iter().enumerate().filter_map(|(i, n)| Some((n.claim?, Item::Node(i))));
        let groups = self.view.groups.iter().enumerate()
            .filter(|&(g, _)| !self.collapsed.get(g).copied().unwrap_or(false) && self.placed.frames.get(g).is_some_and(|f| f[2] > 0.0))
            .filter_map(|(g, grp)| Some((grp.claim?, Item::Group(g))));
        let claims: Vec<_> = nodes.chain(groups).collect();
        claims.iter().find(|(c, _)| c.active).or(claims.first()).map(|&(_, item)| item)
    }

    /// Pan the least distance that puts `item` inside the free canvas area (padded). An item
    /// larger than that area on an axis shows from its top or left edge, where a frame's title
    /// and collapse button are.
    fn show(&mut self, item: Item) {
        let r = self.item_rect(item);
        let [vx, vy, vw, vh] = self.free();
        for (axis, start, len, lo, room) in [(0, r[0], r[2], vx, vw), (1, r[1], r[3], vy, vh)] {
            let (lo, hi) = (lo + EDGE_PAD, lo + room - EDGE_PAD);
            let shift = if len > hi - lo || start < lo {
                lo - start
            } else if start + len > hi {
                hi - (start + len)
            } else {
                0.0
            };
            self.cam.pan[axis] += shift;
        }
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
        let [bx, by, _, _] = self.placed.bounds;
        let z = self.cam.zoom;
        let [x, y] = self.placed.pos[i];
        let [w, h] = self.placed.size[i];
        [self.cam.pan[0] + (x - bx + MARGIN) * z, self.cam.pan[1] + (y - by + MARGIN) * z, w * z, h * z]
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
        match item {
            Item::Node(i) => self.screen_rect(i),
            Item::Group(g) => {
                let [bx, by, _, _] = self.placed.bounds;
                let z = self.cam.zoom;
                let [x, y, w, h] = self.placed.frames[g];
                [self.cam.pan[0] + (x - bx + MARGIN) * z, self.cam.pan[1] + (y - by + MARGIN) * z, w * z, h * z]
            }
        }
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

    /// Simulated subscription deltas until the protocol crate lands: the active worker finishes
    /// its node (accepted, now Rust) and claims the next C node. Returns true when state changed.
    pub fn poll(&mut self, now: f64) -> bool {
        if now < self.next_event {
            return false;
        }
        self.next_event = now + 4.0;
        self.step += 1;
        if let Some(live) = &mut self.live {
            // Replay the next recorded delta; structure is unchanged, so layout stays.
            let Some(update) = live.pending.pop_front() else { return false };
            live.snap.apply(update);
            self.reproject();
            self.refresh();
            return true;
        }
        let nodes = &mut self.graph.nodes;
        if let Some(active) = nodes.iter().position(|n| n.claim.is_some_and(|c| c.active)) {
            let worker = nodes[active].claim.map_or(1, |c| c.worker);
            nodes[active].claim = None;
            nodes[active].evidence = if self.step.is_multiple_of(3) { Evidence::Rejected } else { Evidence::Accepted };
            if nodes[active].evidence == Evidence::Accepted {
                nodes[active].lang = Lang::Rust;
            }
            let next = (1..=nodes.len()).map(|k| (active + k) % nodes.len())
                .find(|&j| nodes[j].lang == Lang::C && nodes[j].claim.is_none() && nodes[j].evidence != Evidence::Accepted);
            if let Some(j) = next {
                nodes[j].claim = Some(crate::model::Claim { worker, active: true });
            }
            self.refresh();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::model::Wire;

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
    /// (outside node title, outside pin label, entering the frame).
    fn crossing(g: &Graph, gi: usize) -> Vec<(String, String, bool)> {
        let inside = |i: usize| g.nodes[i].group == Some(gi);
        let mut v: Vec<_> = g.wires.iter().filter(|w| inside(w.from.0) != inside(w.to.0)).map(|w| outside_end(g, w, inside(w.to.0))).collect();
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
        assert!(near(s.item_rect(Item::Group(g)), frame), "{title}: the frame opens where the node was");
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
        let mut s = State::from_snapshot("merged", snap, Some(ids.compute), HashSet::new(), vec![]);
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

    #[test]
    fn fit_shows_the_whole_protocol_fixture() {
        for size in SIZES {
            ui::set_viewport(size);
            let s = State::protocol_fixture();
            let free = s.free();
            for (i, r) in drawn(&s).into_iter().enumerate() {
                assert!(inside(r, free), "{size:?}: item {i} at {r:?} outside {free:?}");
            }
            assert!(s.cam.zoom >= FIT_FLOOR, "{size:?}: zoom {}", s.cam.zoom);
        }
        // A large window magnifies a small graph past the old 1.25 cap.
        ui::set_viewport([2560.0, 1440.0]);
        assert!(State::protocol_fixture().cam.zoom > 1.25);
    }

    #[test]
    fn fit_keeps_the_fixtures_selected_group_and_claimed_node_beside_the_details_panel() {
        for size in SIZES {
            ui::set_viewport(size);
            let s = State::load("fixture");
            let Some(Item::Group(g)) = s.selected else { panic!("the fixture selects its group") };
            let free = ui::free_area(true, false);
            let r = s.item_rect(Item::Group(g));
            assert!(inside(r, free), "{size:?}: selected group {r:?} outside {free:?}");
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
                if let Some(focus) = s.selected.or(s.claimed()) {
                    let r = s.item_rect(focus);
                    assert!(r[0] >= free[0] - 0.5 && r[1] >= free[1] - 0.5, "{size:?} {}: focus {r:?} starts outside the free area", s.scene);
                    continue;
                }
                for r in drawn(&s) {
                    assert!(r[1] >= free[1] - 0.5 && r[0] >= free[0] - 0.5, "{size:?} {}: {r:?} starts outside the free area", s.scene);
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
            assert!((top - (vy + EDGE_PAD)).abs() < 0.5, "top {top}, free top {vy}");
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
