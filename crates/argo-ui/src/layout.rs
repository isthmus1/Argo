//! Automatic layered layout. Flow runs left to right; each expanded group is laid out on its own
//! and then placed as one block. Positions are view state, never semantic data.

use crate::model::{self, Evidence, Graph, Kind, Lang, Node, Pin, Ty, Wire};

pub struct Placed {
    pub pos: Vec<[f32; 2]>,
    pub size: Vec<[f32; 2]>,
    /// Frame rectangles per group: x, y, w, h.
    pub frames: Vec<[f32; 4]>,
    /// x, y, w, h of everything.
    pub bounds: [f32; 4],
}

const COL_GAP: f32 = 80.0;
/// Exec wires dominate placement so the execution path stays straight.
const EXEC_WEIGHT: f32 = 30.0;
/// A function input feeding at least this many pins gets a getter per consumer instead.
const GETTER_FANOUT: usize = 4;
const ROW_GAP: f32 = 28.0;
const FRAME_PAD: f32 = 18.0;
const FRAME_TITLE: f32 = 34.0;

/// A layout unit: one free node or one whole group.
struct Unit {
    size: [f32; 2],
    /// Member nodes and their offsets inside the unit.
    members: Vec<(usize, [f32; 2])>,
}

struct Edge {
    from: usize,
    to: usize,
    /// Socket offsets from each unit's top.
    from_y: f32,
    to_y: f32,
    weight: f32,
}

pub fn layout(g: &Graph) -> Placed {
    let n = g.nodes.len();
    let size: Vec<[f32; 2]> = g.nodes.iter().map(model::size).collect();
    let mut pos = vec![[0.0, 0.0]; n];
    let mut frames = Vec::new();

    // Lay out each group's members on their own.
    let mut units: Vec<Unit> = Vec::new();
    let mut unit_of = vec![usize::MAX; n];
    for gi in 0..g.groups.len() {
        let members: Vec<usize> = (0..n).filter(|&i| g.nodes[i].group == Some(gi)).collect();
        let (offs, w, h) = arrange(g, &size, &members);
        let ui = units.len();
        for &m in &members {
            unit_of[m] = ui;
        }
        let members = members.iter().zip(offs).map(|(&m, [x, y])| (m, [x + FRAME_PAD, y + FRAME_TITLE])).collect();
        units.push(Unit { size: [w + FRAME_PAD * 2.0, h + FRAME_TITLE + FRAME_PAD], members });
    }
    for i in 0..n {
        if g.nodes[i].group.is_none() {
            unit_of[i] = units.len();
            units.push(Unit { size: size[i], members: vec![(i, [0.0, 0.0])] });
        }
    }

    // Outer layout over units.
    let mut offset_in_unit = vec![[0.0, 0.0]; n];
    for u in &units {
        for &(m, o) in &u.members {
            offset_in_unit[m] = o;
        }
    }
    let edges: Vec<Edge> = g.wires.iter().filter(|w| unit_of[w.from.0] != unit_of[w.to.0]).map(|w| {
        let a = w.from.0;
        let b = w.to.0;
        let ay = offset_in_unit[a][1] + model::socket_local(&g.nodes[a], size[a][0], true, w.from.1)[1];
        let by = offset_in_unit[b][1] + model::socket_local(&g.nodes[b], size[b][0], false, w.to.1)[1];
        let weight = if g.wire_ty(w) == Ty::Exec { EXEC_WEIGHT } else { 1.0 };
        Edge { from: unit_of[a], to: unit_of[b], from_y: ay, to_y: by, weight }
    }).collect();
    let pure: Vec<bool> = units.iter().map(|u| u.members.iter().all(|&(m, _)| {
        let nd = &g.nodes[m];
        !nd.ins.iter().chain(&nd.outs).any(|p| p.ty == Ty::Exec)
    })).collect();
    let sizes: Vec<[f32; 2]> = units.iter().map(|u| u.size).collect();
    let (upos, _, _) = layered(&sizes, &edges, &pure);

    for (ui, u) in units.iter().enumerate() {
        let [ux, uy] = upos[ui];
        for &(m, [ox, oy]) in &u.members {
            pos[m] = [ux + ox, uy + oy];
        }
    }
    for gi in 0..g.groups.len() {
        let ui = unit_of[(0..n).find(|&i| g.nodes[i].group == Some(gi)).expect("group has members")];
        frames.push([upos[ui][0], upos[ui][1], units[ui].size[0], units[ui].size[1]]);
    }

    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for (ui, u) in units.iter().enumerate() {
        b[0] = b[0].min(upos[ui][0]);
        b[1] = b[1].min(upos[ui][1]);
        b[2] = b[2].max(upos[ui][0] + u.size[0]);
        b[3] = b[3].max(upos[ui][1] + u.size[1]);
    }
    Placed { pos, size, frames, bounds: [b[0], b[1], b[2] - b[0], b[3] - b[1]] }
}

/// Lays out a subset of nodes (one group's members) by themselves; returns their offsets
/// relative to the subset's top-left, and its width and height.
fn arrange(g: &Graph, size: &[[f32; 2]], members: &[usize]) -> (Vec<[f32; 2]>, f32, f32) {
    let local = |i: usize| members.iter().position(|&m| m == i);
    let edges: Vec<Edge> = g.wires.iter().filter_map(|w| {
        let (a, b) = (local(w.from.0)?, local(w.to.0)?);
        let ay = model::socket_local(&g.nodes[w.from.0], size[w.from.0][0], true, w.from.1)[1];
        let by = model::socket_local(&g.nodes[w.to.0], size[w.to.0][0], false, w.to.1)[1];
        Some(Edge { from: a, to: b, from_y: ay, to_y: by, weight: if g.wire_ty(w) == Ty::Exec { EXEC_WEIGHT } else { 1.0 } })
    }).collect();
    let sizes: Vec<[f32; 2]> = members.iter().map(|&m| size[m]).collect();
    let pure: Vec<bool> = members.iter().map(|&m| {
        let nd = &g.nodes[m];
        !nd.ins.iter().chain(&nd.outs).any(|p| p.ty == Ty::Exec)
    }).collect();
    layered(&sizes, &edges, &pure)
}

/// Layered placement of boxes connected by weighted port-to-port edges.
fn layered(sizes: &[[f32; 2]], edges: &[Edge], pure: &[bool]) -> (Vec<[f32; 2]>, f32, f32) {
    let n = sizes.len();
    if n == 0 {
        return (vec![], 0.0, 0.0);
    }
    let preds: Vec<Vec<usize>> = (0..n).map(|v| edges.iter().enumerate().filter(|(_, e)| e.to == v).map(|(i, _)| i).collect()).collect();
    let succs: Vec<Vec<usize>> = (0..n).map(|v| edges.iter().enumerate().filter(|(_, e)| e.from == v).map(|(i, _)| i).collect()).collect();

    // Topological order (Kahn); any cycle remainder is appended in index order.
    let mut indeg: Vec<usize> = (0..n).map(|v| preds[v].len()).collect();
    let mut order: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut head = 0;
    while head < order.len() {
        let v = order[head];
        head += 1;
        for &e in &succs[v] {
            let t = edges[e].to;
            indeg[t] -= 1;
            if indeg[t] == 0 {
                order.push(t);
            }
        }
    }
    for v in 0..n {
        if !order.contains(&v) {
            order.push(v);
        }
    }

    // Longest-path ranks, then pull pure producers next to their earliest consumer.
    let mut rank = vec![0usize; n];
    for &v in &order {
        for &e in &preds[v] {
            rank[v] = rank[v].max(rank[edges[e].from] + 1);
        }
    }
    for &v in order.iter().rev() {
        if pure[v] && !succs[v].is_empty() {
            let min_succ = succs[v].iter().map(|&e| rank[edges[e].to]).min().unwrap_or(rank[v] + 1);
            rank[v] = rank[v].max(min_succ.saturating_sub(1));
        }
    }
    let layers = rank.iter().copied().max().unwrap_or(0) + 1;

    // Column x positions.
    let mut col_w = vec![0.0f32; layers];
    for v in 0..n {
        col_w[rank[v]] = col_w[rank[v]].max(sizes[v][0]);
    }
    let mut col_x = vec![0.0f32; layers];
    for r in 1..layers {
        col_x[r] = col_x[r - 1] + col_w[r - 1] + COL_GAP;
    }

    // Initial stacking in topological order.
    let mut by_layer: Vec<Vec<usize>> = vec![vec![]; layers];
    for &v in &order {
        by_layer[rank[v]].push(v);
    }
    let mut y = vec![0.0f32; n];
    for layer in &by_layer {
        let mut acc = 0.0;
        for &v in layer {
            y[v] = acc;
            acc += sizes[v][1] + ROW_GAP;
        }
    }

    // Alternate sweeps: each box moves toward the weighted mean of its neighbours' ports.
    // Development knob for baseline renders.
    let sweeps = std::env::var("ARGO_LAYOUT_SWEEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(12);
    for it in 0..sweeps {
        let forward = it % 2 == 0;
        let ranks: Vec<usize> = if forward { (0..layers).collect() } else { (0..layers).rev().collect() };
        for r in ranks {
            let mut desired: Vec<(usize, f32)> = by_layer[r].iter().map(|&v| {
                let (mut sum, mut wsum) = (0.0, 0.0);
                let near = if forward { &preds[v] } else { &succs[v] };
                for &e in near {
                    let ed = &edges[e];
                    let (other_port, own_port) = if forward { (y[ed.from] + ed.from_y, ed.to_y) } else { (y[ed.to] + ed.to_y, ed.from_y) };
                    sum += ed.weight * (other_port - own_port);
                    wsum += ed.weight;
                }
                (v, if wsum > 0.0 { sum / wsum } else { y[v] })
            }).collect();
            desired.sort_by(|a, b| a.1.total_cmp(&b.1));
            by_layer[r] = desired.iter().map(|d| d.0).collect();
            let tops = pack(&desired.iter().map(|&(v, d)| (d, sizes[v][1])).collect::<Vec<_>>());
            for (i, &(v, _)) in desired.iter().enumerate() {
                y[v] = tops[i];
            }
        }
    }

    let min_y = (0..n).map(|v| y[v]).fold(f32::MAX, f32::min);
    let pos: Vec<[f32; 2]> = (0..n).map(|v| [col_x[rank[v]], y[v] - min_y]).collect();
    let w = (0..n).map(|v| pos[v][0] + sizes[v][0]).fold(0.0, f32::max);
    let h = (0..n).map(|v| pos[v][1] + sizes[v][1]).fold(0.0, f32::max);
    (pos, w, h)
}

/// Places boxes (desired top, height) in order without overlap, minimising squared displacement
/// (pool-adjacent-violators on offset-adjusted targets).
fn pack(items: &[(f32, f32)]) -> Vec<f32> {
    let mut offset = Vec::with_capacity(items.len());
    let mut acc = 0.0;
    for &(_, h) in items {
        offset.push(acc);
        acc += h + ROW_GAP;
    }
    // Blocks of (sum, count) over z = desired - offset, kept non-decreasing.
    let mut blocks: Vec<(f32, usize)> = Vec::new();
    for (i, &(d, _)) in items.iter().enumerate() {
        blocks.push((d - offset[i], 1));
        while blocks.len() > 1 {
            let (s1, c1) = blocks[blocks.len() - 1];
            let (s0, c0) = blocks[blocks.len() - 2];
            if s0 / c0 as f32 > s1 / c1 as f32 {
                blocks.pop();
                let last = blocks.len() - 1;
                blocks[last] = (s0 + s1, c0 + c1);
            } else {
                break;
            }
        }
    }
    let mut out = Vec::with_capacity(items.len());
    let mut i = 0;
    for (s, c) in blocks {
        let z = s / c as f32;
        for _ in 0..c {
            out.push(z + offset[i]);
            i += 1;
        }
    }
    out
}

/// View transform: a function input pin feeding many consumers is drawn as a small getter next
/// to each consumer (UE's "Get" nodes) rather than one long fan of wires.
pub fn with_getters(mut g: Graph) -> Graph {
    let mut fanout = std::collections::HashMap::<(usize, usize), usize>::new();
    for w in &g.wires {
        *fanout.entry(w.from).or_default() += 1;
    }
    let wires = std::mem::take(&mut g.wires);
    for w in wires {
        let src = &g.nodes[w.from.0];
        let pin = &src.outs[w.from.1];
        if src.kind == Kind::GroupIn && pin.ty != Ty::Exec && fanout[&w.from] >= GETTER_FANOUT {
            let getter = Node {
                kind: Kind::Get, title: pin.label.clone(), lang: Lang::None, evidence: Evidence::None, claim: None,
                ins: vec![], outs: vec![Pin { label: String::new(), ty: pin.ty }], group: g.nodes[w.to.0].group,
            };
            g.nodes.push(getter);
            g.wires.push(Wire { from: (g.nodes.len() - 1, 0), to: w.to });
        } else {
            g.wires.push(w);
        }
    }
    g
}
