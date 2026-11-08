//! Application state: the semantic graph, the derived view graph and layout, the camera, hover
//! and selection. Live updates change claims and evidence in place, so layout, camera and
//! selection survive them.

use std::collections::HashSet;

use argo_protocol::{NodeId, Snapshot};

use crate::bridge;
use crate::db;
use crate::layout::{self, Placed};
use crate::project;
use crate::model::{Graph, Kind};
use crate::ui;

/// World padding around the laid-out graph.
pub const MARGIN: f32 = 40.0;
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 2.0;
/// The default view never zooms out past this; larger graphs pan instead. Below about this zoom
/// headers and pin labels are too small to read.
const READABLE_ZOOM: f32 = 0.9;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    /// A node in the view graph.
    Node(usize),
    Group(usize),
}

#[derive(Clone, Copy, Default)]
pub struct Cam {
    pub pan: [f32; 2],
    pub zoom: f32,
    pub drag_from: [f32; 2],
}

impl Cam {
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
    pub online: bool,
    /// Set once the database's rows describe a program: the snapshot they rebuild.
    pub live: Option<Live>,
    /// The ingested project, with its file/function index.
    pub project: Option<project::Project>,
    pub forms: project::Forms,
    /// The subscription every graph and status shown comes from.
    pub db: Option<db::Link>,
}

/// A protocol snapshot and the level being shown.
pub struct Live {
    pub snap: Snapshot,
    pub container: Option<NodeId>,
    pub open: HashSet<NodeId>,
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

impl State {
    /// Nothing on the canvas but its breadcrumb: where the database view starts.
    pub fn empty(crumbs: Vec<String>) -> State {
        State::with_graph(Graph { crumbs, nodes: vec![], wires: vec![], groups: vec![], selected_group: None })
    }

    fn with_graph(graph: Graph) -> State {
        let collapsed = vec![false; graph.groups.len()];
        let selected = graph.selected_group.map(Item::Group);
        let mut s = State {
            view: graph.clone(), graph, collapsed, placed: Placed { pos: vec![], size: vec![], frames: vec![], bounds: [0.0; 4], row_gaps: vec![] },
            cam: Cam { zoom: 1.0, ..Cam::default() }, hovered: None, selected, online: true, live: None,
            project: None, forms: project::Forms::default(), db: None,
        };
        s.relayout();
        s
    }

    /// A protocol snapshot: show `container`'s level with `open` drawn in place.
    pub fn from_snapshot(snap: Snapshot, container: Option<NodeId>, open: HashSet<NodeId>) -> State {
        let mut s = State::empty(vec![]);
        s.live = Some(Live { snap, container, open, origin: vec![], frames: vec![] });
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

    /// Re-derive the semantic graph from the protocol snapshot.
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
    pub(crate) fn refresh(&mut self) {
        self.view = self.graph.collapse(&self.collapsed).0;
    }

    /// Frame the whole graph in the canvas area left free by the floating panels.
    pub fn fit_all(&mut self) {
        let [_, _, bw, bh] = self.placed.bounds;
        let (ww, wh) = (bw + 2.0 * MARGIN, bh + 2.0 * MARGIN);
        let [vx, vy, vw, vh] = ui::free_area(self.selected.is_some(), self.project.is_some());
        let zoom = (vw / ww).min(vh / wh).clamp(MIN_ZOOM, 1.25);
        self.cam.zoom = zoom;
        self.cam.pan = [vx + (vw - ww * zoom) / 2.0, vy + (vh - wh * zoom) / 2.0];
    }

    /// Default view: fit everything when that stays readable; otherwise keep a readable zoom
    /// and centre on where an agent is working (or the entry), leaving the rest to panning.
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
        if self.cam.zoom >= READABLE_ZOOM || std::env::var("ARGO_FIT").is_ok_and(|v| v == "all") {
            return;
        }
        // Start where the program starts (reading order); the far edge fades to show more.
        let [vx, vy, _, vh] = ui::free_area(self.selected.is_some(), self.project.is_some());
        let [_, _, _, bh] = self.placed.bounds;
        let wh = (bh + 2.0 * MARGIN) * READABLE_ZOOM;
        self.cam.zoom = READABLE_ZOOM;
        self.cam.pan = [vx, if wh < vh { vy + (vh - wh) / 2.0 } else { vy }];
        if let Some(i) = self.view.nodes.iter().position(|n| n.kind == Kind::GroupIn) {
            let [_, y, _, h] = self.screen_rect(i);
            if wh > vh {
                self.cam.pan[1] += vy + vh / 2.0 - (y + h / 2.0);
            }
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

    /// Select an item; pan so a selected node is not hidden under the docked details panel.
    pub fn select(&mut self, item: Item) {
        self.selected = Some(item);
        if let Item::Node(i) = item {
            let [vx, _, vw, _] = ui::free_area(true, self.project.is_some());
            let [x, _, w, _] = self.screen_rect(i);
            let limit = vx + vw - 48.0;
            if x + w > limit {
                self.cam.pan[0] -= x + w - limit;
            }
        }
    }

    pub fn hover(&mut self, item: Item, on: bool) {
        if on {
            self.hovered = Some(item);
        } else if self.hovered == Some(item) {
            self.hovered = None;
        }
    }

    pub fn toggle_group(&mut self, g: usize) {
        let Some(live) = &mut self.live else { return };
        // Closing a frame closes that protocol container; it becomes one node with its pins.
        if let Some(f) = live.frames.get(g) {
            live.open.remove(f);
            self.hovered = None;
            self.selected = None;
            self.reproject();
            self.relayout();
        }
    }

    /// Double-click on a node: a closed construct opens in place; on an opened construct's own
    /// node (it has no frame to click), it closes again.
    pub fn open_node(&mut self, i: usize) {
        let Some(live) = &mut self.live else { return };
        let Some(&id) = live.origin.get(i) else { return };
        let parent = live.snap.graph.nodes.iter().find(|n| n.id == id).and_then(|n| n.parent);
        if bridge::openable(&live.snap, id) {
            live.open.insert(id);
        } else if let Some(p) = parent.filter(|p| live.open.contains(p)) {
            live.open.remove(&p);
        } else {
            return;
        }
        self.hovered = None;
        self.selected = None;
        self.reproject();
        self.relayout();
    }

    /// View index of the first node with this title.
    pub fn find(&self, title: &str) -> Option<usize> {
        self.view.nodes.iter().position(|n| n.title == title)
    }
}
