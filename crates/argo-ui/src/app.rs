//! Application state: the semantic graph, the derived view graph and layout, the camera, hover
//! and selection. Live updates change claims and evidence in place, so layout, camera and
//! selection survive them.

use std::collections::{HashSet, VecDeque};

use argo_protocol::{NodeId, NodeKind, Snapshot, Update, fixture};

use crate::bridge;
use crate::layout::{self, Placed};
use crate::model::{Evidence, Graph, Kind, Lang};
use crate::scenes;
use crate::ui;

/// World padding around the laid-out graph.
pub const MARGIN: f32 = 40.0;
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 2.0;
/// The default view never zooms out past this; larger graphs pan instead.
const READABLE_ZOOM: f32 = 0.5;

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
    pub online: bool,
    /// Set when the graph comes from an `argo-protocol` snapshot rather than a seeded mock.
    pub live: Option<Live>,
    next_event: f64,
    step: usize,
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

impl State {
    pub fn load(scene: &'static str) -> State {
        let graph = layout::with_getters(scenes::scene(scene));
        let collapsed = vec![false; graph.groups.len()];
        let selected = graph.selected_group.map(Item::Group);
        let mut s = State {
            scene, view: graph.clone(), graph, collapsed, placed: Placed { pos: vec![], size: vec![], frames: vec![], bounds: [0.0; 4], row_gaps: vec![] },
            cam: Cam { zoom: 1.0, ..Cam::default() }, hovered: None, selected, online: true, live: None, next_event: 4.0, step: 0,
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
    fn reproject(&mut self) {
        let Some(live) = &mut self.live else { return };
        let p = bridge::project(&live.snap, live.container, &live.open);
        live.origin = p.origin;
        live.frames = p.frames;
        self.graph = layout::with_getters(p.graph);
        self.collapsed = vec![false; self.graph.groups.len()];
    }

    /// Rebuild the view graph and its layout after a structural view change (collapse/expand).
    fn relayout(&mut self) {
        self.view = self.graph.collapse(&self.collapsed).0;
        self.placed = layout::layout(&self.view);
    }

    /// Rebuild the view graph only: claims and evidence changed, positions did not.
    fn refresh(&mut self) {
        self.view = self.graph.collapse(&self.collapsed).0;
    }

    /// Frame the whole graph in the canvas area left free by the floating panels.
    pub fn fit_all(&mut self) {
        let [_, _, bw, bh] = self.placed.bounds;
        let (ww, wh) = (bw + 2.0 * MARGIN, bh + 2.0 * MARGIN);
        let [vx, vy, vw, vh] = ui::free_area(self.selected.is_some());
        let zoom = (vw / ww).min(vh / wh).clamp(MIN_ZOOM, 1.25);
        self.cam.zoom = zoom;
        self.cam.pan = [vx + (vw - ww * zoom) / 2.0, vy + (vh - wh * zoom) / 2.0];
    }

    /// Default view: fit everything when that stays readable; otherwise keep a readable zoom
    /// and centre on where an agent is working (or the entry), leaving the rest to panning.
    pub fn fit(&mut self) {
        self.fit_all();
        // Captures can ask for the whole graph regardless of readability (`ARGO_FIT=all`).
        if self.cam.zoom >= READABLE_ZOOM || std::env::var("ARGO_FIT").is_ok_and(|v| v == "all") {
            return;
        }
        // Start where the program starts (reading order); the far edge fades to show more.
        let [vx, vy, _, vh] = ui::free_area(self.selected.is_some());
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
            let [vx, _, vw, _] = ui::free_area(true);
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
        if let Some(live) = &mut self.live {
            // Closing a frame closes that protocol container; it becomes one node with its pins.
            if let Some(f) = live.frames.get(g) {
                live.open.remove(f);
                self.hovered = None;
                self.selected = None;
                self.reproject();
                self.relayout();
            }
            return;
        }
        if let Some(c) = self.collapsed.get_mut(g) {
            *c = !*c;
            self.hovered = None;
            self.selected = None;
            self.relayout();
        }
    }

    /// Double-click on a node: a collapsed group expands; other nodes have nothing inside yet.
    pub fn open_node(&mut self, i: usize) {
        if let Some(live) = &mut self.live {
            if let Some(&id) = live.origin.get(i).filter(|&&id| bridge::openable(&live.snap, id)) {
                live.open.insert(id);
                self.hovered = None;
                self.selected = None;
                self.reproject();
                self.relayout();
            }
            return;
        }
        if self.view.nodes[i].kind == Kind::Group
            && let Some(g) = self.graph.groups.iter().position(|g| g.title == self.view.nodes[i].title)
        {
            self.toggle_group(g);
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
