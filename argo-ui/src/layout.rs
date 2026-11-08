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
    /// World y of the empty band between wrapped rows, top to bottom.
    pub row_gaps: Vec<f32>,
    /// Distinct for every layout run, so drawings derived from a layout can be cached by it.
    pub generation: u64,
}

/// A fresh [`Placed::generation`].
pub fn next_generation() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Width-to-height ratio of the canvas area a graph is framed in.
const VIEW_ASPECT: f32 = 1.9;
/// Vertical space between wrapped rows; wires going back to the next row's start run through it.
const WRAP_GAP: f32 = 70.0;

const COL_GAP: f32 = 80.0;
/// Exec wires dominate placement so the execution path stays straight.
const EXEC_WEIGHT: f32 = 30.0;
/// A function input feeding at least this many pins gets a getter per consumer instead.
const GETTER_FANOUT: usize = 4;
const ROW_GAP: f32 = 28.0;
const FRAME_PAD: f32 = 18.0;
const FRAME_TITLE: f32 = 34.0;
/// Dummy nodes stand in for long edges crossing a layer; they only reserve a thin lane.
const DUMMY_H: f32 = 4.0;
const DUMMY_GAP: f32 = 10.0;
/// Crossing-minimisation sweeps (alternating direction) and transposition rounds per sweep.
const SWEEPS: usize = 48;
const TRANSPOSE_ROUNDS: usize = 8;
/// Crossing weight of an exec segment relative to a data segment.
const EXEC_CROSS: f32 = 6.0;
/// Priority-method coordinate passes.
const PLACE_PASSES: usize = 10;

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

/// A node's size as drawn: `model::size`, except that a node without pins (a field-less type)
/// is its header alone (see `graph::node`).
fn drawn_size(n: &Node) -> [f32; 2] {
    let [w, h] = model::size(n);
    if n.ins.is_empty() && n.outs.is_empty() { [w, model::HEADER] } else { [w, h] }
}

pub fn layout(g: &Graph) -> Placed {
    layout_lanes(g).0
}

/// The outer layout's input: every free node and every expanded group as one unit, and the
/// wires between units as weighted edges.
struct Outer {
    size: Vec<[f32; 2]>,
    units: Vec<Unit>,
    unit_of: Vec<usize>,
    edges: Vec<Edge>,
    pure: Vec<bool>,
}

fn outer(g: &Graph) -> Outer {
    let n = g.nodes.len();
    let size: Vec<[f32; 2]> = g.nodes.iter().map(drawn_size).collect();

    // Lay out each group's members on their own.
    let mut units: Vec<Unit> = Vec::new();
    let mut unit_of = vec![usize::MAX; n];
    for gi in 0..g.groups.len() {
        let members: Vec<usize> = (0..n).filter(|&i| g.nodes[i].group == Some(gi)).collect();
        if members.is_empty() {
            continue; // collapsed: drawn as a single node
        }
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

    let mut offset_in_unit = vec![[0.0, 0.0]; n];
    for u in &units {
        for &(m, o) in &u.members {
            offset_in_unit[m] = o;
        }
    }
    let edges: Vec<Edge> = g.wires.iter().filter(|w| !w.back && unit_of[w.from.0] != unit_of[w.to.0]).map(|w| {
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
    Outer { size, units, unit_of, edges, pure }
}

/// Everything's extent, given each unit's position and size.
fn bounds(upos: &[[f32; 2]], sizes: &[[f32; 2]]) -> [f32; 4] {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for (p, s) in upos.iter().zip(sizes) {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0] + s[0]), b[3].max(p[1] + s[1])];
    }
    [b[0], b[1], b[2] - b[0], b[3] - b[1]]
}

/// The whole-program view's layout: every file's functions arranged inside its frame, callers
/// generally left of callees, wrapped into columns no taller than the frame is wide; then the
/// frames (and closed files) packed into rows about as wide as the view is shaped. A layered
/// layout of hundreds of mostly unconnected functions would be one endless column.
pub fn packed(g: &Graph) -> Placed {
    const GAP: f32 = 24.0;
    const COLUMN_GAP: f32 = 70.0;
    const UNIT_GAP: f32 = 90.0;
    let n = g.nodes.len();
    let size: Vec<[f32; 2]> = g.nodes.iter().map(drawn_size).collect();
    let mut pos = vec![[0.0, 0.0]; n];
    // Units: each framed group (members placed relative to it), then each free node.
    let mut units: Vec<([f32; 2], Vec<(usize, [f32; 2])>, Option<usize>)> = Vec::new();
    for gi in 0..g.groups.len() {
        let members: Vec<usize> = (0..n).filter(|&i| g.nodes[i].group == Some(gi)).collect();
        if members.is_empty() {
            continue;
        }
        // The layered order inside the file: by column, then down it.
        let (offs, _, _) = arrange(g, &size, &members);
        let mut order: Vec<usize> = (0..members.len()).collect();
        order.sort_by(|&a, &b| offs[a][0].total_cmp(&offs[b][0]).then(offs[a][1].total_cmp(&offs[b][1])));
        let area: f32 = members.iter().map(|&m| (size[m][0] + COLUMN_GAP) * (size[m][1] + GAP)).sum();
        let tallest = members.iter().map(|&m| size[m][1]).fold(0.0, f32::max);
        let limit = (area / 1.6).sqrt().max(tallest);
        let (mut x, mut y, mut col_w, mut h) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut placed = Vec::new();
        for k in order {
            let m = members[k];
            if y > 0.0 && y + size[m][1] > limit {
                x += col_w + COLUMN_GAP;
                (y, col_w) = (0.0, 0.0);
            }
            placed.push((m, [x + FRAME_PAD, y + FRAME_TITLE]));
            col_w = col_w.max(size[m][0]);
            h = h.max(y + size[m][1]);
            y += size[m][1] + GAP;
        }
        units.push(([x + col_w + 2.0 * FRAME_PAD, h + FRAME_TITLE + FRAME_PAD], placed, Some(gi)));
    }
    for i in 0..n {
        if g.nodes[i].group.is_none() {
            units.push((size[i], vec![(i, [0.0, 0.0])], None));
        }
    }
    // Rows of units, about as wide as the view is shaped.
    let area: f32 = units.iter().map(|(s, _, _)| (s[0] + UNIT_GAP) * (s[1] + UNIT_GAP)).sum();
    let widest = units.iter().map(|(s, _, _)| s[0]).fold(0.0, f32::max);
    let row_w = (area * VIEW_ASPECT).sqrt().max(widest);
    let (mut x, mut y, mut row_h) = (0.0f32, 0.0f32, 0.0f32);
    let mut frames = vec![[0.0; 4]; g.groups.len()];
    let (mut upos, mut usize_) = (Vec::new(), Vec::new());
    for (s, members, group) in &units {
        if x > 0.0 && x + s[0] > row_w {
            (x, y, row_h) = (0.0, y + row_h + UNIT_GAP, 0.0);
        }
        for &(m, [ox, oy]) in members {
            pos[m] = [x + ox, y + oy];
        }
        if let Some(gi) = group {
            frames[*gi] = [x, y, s[0], s[1]];
        }
        upos.push([x, y]);
        usize_.push(*s);
        x += s[0] + UNIT_GAP;
        row_h = row_h.max(s[1]);
    }
    Placed { pos, size, frames, bounds: bounds(&upos, &usize_), row_gaps: vec![], generation: next_generation() }
}

/// [`layout`], with the lanes its long edges reserve and the units' rects, both in the outer
/// layered layout's space before rows wrap (see [`layered_lanes`]). A view with collapsed groups
/// is laid out [`in_place`] (and reports no lanes).
fn layout_lanes(g: &Graph) -> (Placed, Vec<[f32; 4]>, Vec<[f32; 4]>) {
    if let Some(p) = g.expanded.as_deref().and_then(|e| in_place(g, e)) {
        return (p, vec![], vec![]);
    }
    let n = g.nodes.len();
    let o = outer(g);
    let sizes: Vec<[f32; 2]> = o.units.iter().map(|u| u.size).collect();
    let (upos, _, _, lanes) = layered_lanes(&sizes, &o.edges, &o.pure);
    let unit_rects: Vec<[f32; 4]> = upos.iter().zip(&sizes).map(|(p, s)| [p[0], p[1], s[0], s[1]]).collect();
    let (upos, row_gaps) = wrap_rows(upos, &sizes, &o.edges, &o.pure, None);

    let mut pos = vec![[0.0, 0.0]; n];
    for (ui, u) in o.units.iter().enumerate() {
        let [ux, uy] = upos[ui];
        for &(m, [ox, oy]) in &u.members {
            pos[m] = [ux + ox, uy + oy];
        }
    }
    let frames = (0..g.groups.len()).map(|gi| match (0..n).find(|&i| g.nodes[i].group == Some(gi)) {
        Some(first) => {
            let ui = o.unit_of[first];
            [upos[ui][0], upos[ui][1], sizes[ui][0], sizes[ui][1]]
        }
        None => [0.0; 4],
    }).collect();
    (Placed { pos, size: o.size, frames, bounds: bounds(&upos, &sizes), row_gaps, generation: next_generation() }, lanes, unit_rects)
}

/// How far a collapsed group's stacked plates reach past its node (see `graph::node`).
const STACK: f32 = 12.0;

/// The layout of `view`, which collapses some groups of `expanded`, as a view change rather
/// than a new layout: `expanded` is laid out, each collapsed group's node takes its frame's
/// place, and columns only narrow to fit, so nothing else changes row or column and the graph
/// wraps no more than `expanded` does. `None` if `view` is not a collapse of `expanded`.
fn in_place(view: &Graph, expanded: &Graph) -> Option<Placed> {
    let e = expanded;
    let folded: Vec<bool> = (0..e.groups.len())
        .map(|gi| e.nodes.iter().any(|n| n.group == Some(gi)) && !view.nodes.iter().any(|n| n.group == Some(gi)))
        .collect();
    // Each expanded node's view node, numbered as `Graph::collapse` numbers them: in order, a
    // collapsed group's node where its first member was.
    let mut vi_of = vec![0; e.nodes.len()];
    let mut group_vi: Vec<Option<usize>> = vec![None; e.groups.len()];
    let mut next = 0;
    for (i, nd) in e.nodes.iter().enumerate() {
        let fresh = next;
        vi_of[i] = match nd.group {
            Some(gi) if folded[gi] => *group_vi[gi].get_or_insert(fresh),
            _ => fresh,
        };
        if vi_of[i] == fresh {
            next += 1;
        }
    }
    if next != view.nodes.len() || group_vi.iter().flatten().any(|&v| view.nodes[v].kind != Kind::Group) {
        return None;
    }
    let o = outer(e);
    let open: Vec<[f32; 2]> = o.units.iter().map(|u| u.size).collect();
    let (upos, _, _, _) = layered_lanes(&open, &o.edges, &o.pure);
    let rows = wrap_rows(upos.clone(), &open, &o.edges, &o.pure, None).1.len() + 1;

    // Each collapsed group's frame shrinks to its node, stacked plates included.
    let mut sizes = open.clone();
    for (gi, v) in group_vi.iter().enumerate() {
        let (Some(v), Some(first)) = (*v, e.nodes.iter().position(|n| n.group == Some(gi))) else { continue };
        let [w, h] = drawn_size(&view.nodes[v]);
        sizes[o.unit_of[first]] = [w + STACK, h + STACK];
    }
    // Columns narrow to their widest unit now; every later column shifts left by as much.
    let mut cols: Vec<f32> = upos.iter().map(|p| p[0]).collect();
    cols.sort_by(f32::total_cmp);
    cols.dedup();
    let widest = |ss: &[[f32; 2]], x: f32| (0..ss.len()).filter(|&u| upos[u][0] == x).map(|u| ss[u][0]).fold(0.0, f32::max);
    let mut shift = vec![0.0f32; cols.len()];
    for k in 1..cols.len() {
        shift[k] = shift[k - 1] + widest(&open, cols[k - 1]) - widest(&sizes, cols[k - 1]);
    }
    let col = |x: f32| cols.iter().position(|&c| c == x).expect("a column");
    let upos: Vec<[f32; 2]> = upos.iter().map(|p| [p[0] - shift[col(p[0])], p[1]]).collect();
    let (upos, row_gaps) = wrap_rows(upos, &sizes, &o.edges, &o.pure, Some(rows));

    let mut pos = vec![[0.0, 0.0]; view.nodes.len()];
    for (ui, u) in o.units.iter().enumerate() {
        for &(m, [ox, oy]) in &u.members {
            let member = e.nodes[m].group.is_some_and(|gi| folded[gi]);
            pos[vi_of[m]] = if member { upos[ui] } else { [upos[ui][0] + ox, upos[ui][1] + oy] };
        }
    }
    let frames = (0..view.groups.len()).map(|gi| match e.nodes.iter().position(|n| n.group == Some(gi)) {
        Some(first) if !folded[gi] => {
            let ui = o.unit_of[first];
            [upos[ui][0], upos[ui][1], sizes[ui][0], sizes[ui][1]]
        }
        _ => [0.0; 4],
    }).collect();
    let size = view.nodes.iter().map(drawn_size).collect();
    Some(Placed { pos, size, frames, bounds: bounds(&upos, &sizes), row_gaps, generation: next_generation() })
}

/// A long left-to-right chain shrinks to unreadable when framed. Cut it into rows at column
/// boundaries so the whole graph keeps roughly the canvas's shape, reading order intact: each row
/// continues where the previous one ended, like lines of text. At most `max_rows`, if given.
fn wrap_rows(mut pos: Vec<[f32; 2]>, sizes: &[[f32; 2]], edges: &[Edge], pure: &[bool], max_rows: Option<usize>) -> (Vec<[f32; 2]>, Vec<f32>) {
    if pos.is_empty() {
        return (pos, vec![]);
    }
    let x0 = pos.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
    let x1 = pos.iter().zip(sizes).map(|(p, s)| p[0] + s[0]).fold(f32::MIN, f32::max);
    let y0 = pos.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
    let y1 = pos.iter().zip(sizes).map(|(p, s)| p[1] + s[1]).fold(f32::MIN, f32::max);
    let (w, h) = (x1 - x0, (y1 - y0).max(1.0));
    let rows = ((w / (h * VIEW_ASPECT)).sqrt().round() as usize).clamp(1, max_rows.unwrap_or(usize::MAX).max(1));
    if rows == 1 {
        return (pos, vec![]);
    }
    // Break at column starts, so a column (one rank) never splits across rows.
    let mut starts: Vec<f32> = pos.iter().map(|p| p[0]).collect();
    starts.sort_by(f32::total_cmp);
    starts.dedup();
    let target = w / rows as f32;
    let mut breaks = vec![x0];
    for &s in &starts {
        if s - breaks.last().copied().unwrap_or(x0) >= target && breaks.len() < rows {
            breaks.push(s);
        }
    }
    // A column of pure producers that only feed the next row (the inputs of the loop starting
    // it, say) moves down with it, so their wires stay short instead of running back under the
    // whole row; the long wrap is left to the execution path.
    for k in 1..breaks.len() {
        let b = breaks[k];
        let Some(&prev) = starts.iter().rev().find(|&&s| s < b) else { continue };
        if prev <= breaks[k - 1] {
            continue;
        }
        let column: Vec<usize> = (0..pos.len()).filter(|&u| pos[u][0] == prev).collect();
        let feeds_next_row = column.iter().all(|&u| {
            let mut succ = edges.iter().filter(|e| e.from == u).map(|e| pos[e.to][0]).peekable();
            pure[u] && succ.peek().is_some() && succ.all(|x| x >= b)
        });
        if feeds_next_row {
            breaks[k] = prev;
        }
    }
    let row_at = |x: f32| breaks.iter().rposition(|&b| x >= b).unwrap_or(0);
    let mut row: Vec<usize> = pos.iter().map(|p| row_at(p[0])).collect();
    // A dead end fed by one node — an exit path such as `return -1` → Output — stays beside that
    // node rather than starting the next row behind a long wire.
    let preds = |u: usize| -> Vec<usize> {
        let mut p: Vec<usize> = edges.iter().filter(|e| e.to == u).map(|e| e.from).collect();
        p.dedup();
        p
    };
    let succs = |u: usize| -> Vec<usize> {
        let mut s: Vec<usize> = edges.iter().filter(|e| e.from == u).map(|e| e.to).collect();
        s.dedup();
        s
    };
    for u in 0..pos.len() {
        let [p] = preds(u)[..] else { continue };
        if row[p] >= row[u] {
            continue;
        }
        // Follow the single path from `u`; it must end without rejoining anything.
        let mut chain = vec![u];
        let ok = loop {
            let last = *chain.last().expect("non-empty");
            match succs(last)[..] {
                [] => break true,
                [n] if preds(n) == [last] && !chain.contains(&n) && chain.len() < 8 => chain.push(n),
                _ => break false,
            }
        };
        if !ok {
            continue;
        }
        let mut x = pos[p][0] + sizes[p][0] + COL_GAP / 2.0;
        for &c in &chain {
            row[c] = row[p];
            pos[c] = [x, pos[p][1]];
            x += sizes[c][0] + COL_GAP / 2.0;
        }
    }
    // A pure producer with no inputs (a getter, a constant) feeding one consumer in a later row
    // moves beside it, socket to socket, when that spot is free: its wire stays short.
    for u in 0..pos.len() {
        let out: Vec<&Edge> = edges.iter().filter(|e| e.from == u).collect();
        let Some(e) = out.first() else { continue };
        let c = e.to;
        if !pure[u] || edges.iter().any(|e| e.to == u) || out.iter().any(|e| e.to != c) || row[c] <= row[u] {
            continue;
        }
        let at = [pos[c][0] - COL_GAP / 2.0 - sizes[u][0], pos[c][1] + e.to_y - e.from_y];
        let m = ROW_GAP / 2.0;
        let clear = (0..pos.len()).filter(|&v| v != u && row[v] == row[c]).all(|v| {
            at[0] - m >= pos[v][0] + sizes[v][0] || pos[v][0] >= at[0] + sizes[u][0] + m
                || at[1] - m >= pos[v][1] + sizes[v][1] || pos[v][1] >= at[1] + sizes[u][1] + m
        });
        if clear {
            row[u] = row[c];
            pos[u] = at;
        }
    }
    let n_rows = breaks.len();
    let mut top = vec![f32::MAX; n_rows];
    let mut bottom = vec![f32::MIN; n_rows];
    for ((p, s), &r) in pos.iter().zip(sizes).zip(&row) {
        top[r] = top[r].min(p[1]);
        bottom[r] = bottom[r].max(p[1] + s[1]);
    }
    let mut offset = vec![0.0; n_rows];
    let mut gaps = Vec::new();
    let mut y = y0;
    for r in 0..n_rows {
        if top[r] > bottom[r] {
            continue; // empty row
        }
        offset[r] = y - top[r];
        y += bottom[r] - top[r] + WRAP_GAP;
        if r + 1 < n_rows {
            gaps.push(y - WRAP_GAP / 2.0);
        }
    }
    for (p, &r) in pos.iter_mut().zip(&row) {
        p[0] -= breaks[r] - x0;
        p[1] += offset[r];
    }
    (pos, gaps)
}

/// Lays out a subset of nodes (one group's members) by themselves; returns their offsets
/// relative to the subset's top-left, and its width and height.
fn arrange(g: &Graph, size: &[[f32; 2]], members: &[usize]) -> (Vec<[f32; 2]>, f32, f32) {
    let local = |i: usize| members.iter().position(|&m| m == i);
    let edges: Vec<Edge> = g.wires.iter().filter(|w| !w.back).filter_map(|w| {
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
    let (mut offs, w, _) = layered(&sizes, &edges, &pure);
    // Inside a frame each column hangs from the top, so members line up under the title and
    // the frame shrink-wraps them (a straight wire matters less than a tidy block here).
    let mut cols: Vec<f32> = offs.iter().map(|o| o[0]).collect();
    cols.sort_by(f32::total_cmp);
    cols.dedup();
    for x in cols {
        let top = offs.iter().filter(|o| o[0] == x).map(|o| o[1]).fold(f32::MAX, f32::min);
        offs.iter_mut().filter(|o| o[0] == x).for_each(|o| o[1] -= top);
    }
    let h = offs.iter().zip(&sizes).map(|(o, s)| o[1] + s[1]).fold(0.0, f32::max);
    (offs, w, h)
}

/// Sugiyama layered placement of boxes connected by weighted port-to-port edges: ranks, dummy
/// nodes on long edges, port-aware crossing minimisation, then priority-method coordinates.
fn layered(sizes: &[[f32; 2]], edges: &[Edge], pure: &[bool]) -> (Vec<[f32; 2]>, f32, f32) {
    let (pos, w, h, _) = layered_lanes(sizes, edges, pure);
    (pos, w, h)
}

/// [`layered`], also returning the lane each long edge's dummy reserves in every column it
/// crosses: as wide as the column, [`DUMMY_H`] tall. Between columns a long wire runs in the
/// channel; within one it keeps to its lane, clear of the column's nodes.
fn layered_lanes(sizes: &[[f32; 2]], edges: &[Edge], pure: &[bool]) -> (Vec<[f32; 2]>, f32, f32, Vec<[f32; 4]>) {
    let n = sizes.len();
    if n == 0 {
        return (vec![], 0.0, 0.0, vec![]);
    }
    let (rank, topo) = ranks(n, edges, pure);
    let lg = split(sizes, edges, &rank);
    let layers = order(&lg, &topo);
    let y = place(&lg, &layers);

    // Column x positions from real nodes only; dummies just reserve vertical room.
    let cols = layers.len();
    let mut col_w = vec![0.0f32; cols];
    for v in 0..n {
        col_w[rank[v]] = col_w[rank[v]].max(sizes[v][0]);
    }
    let mut col_x = vec![0.0f32; cols];
    for r in 1..cols {
        col_x[r] = col_x[r - 1] + col_w[r - 1] + COL_GAP;
    }
    let min_y = (0..n).map(|v| y[v]).fold(f32::MAX, f32::min);
    let pos: Vec<[f32; 2]> = (0..n).map(|v| [col_x[rank[v]], y[v] - min_y]).collect();
    let w = (0..n).map(|v| pos[v][0] + sizes[v][0]).fold(0.0, f32::max);
    let h = (0..n).map(|v| pos[v][1] + sizes[v][1]).fold(0.0, f32::max);
    let lanes = (n..lg.h.len()).map(|d| [col_x[lg.rank[d]], y[d] - min_y, col_w[lg.rank[d]], DUMMY_H]).collect();
    (pos, w, h, lanes)
}

/// Longest-path ranks with pure producers, and nodes feeding a single consumer, pulled one
/// column left of their earliest consumer.
/// Also returns a topological order (any cycle remainder appended in index order).
fn ranks(n: usize, edges: &[Edge], pure: &[bool]) -> (Vec<usize>, Vec<usize>) {
    let mut indeg = vec![0usize; n];
    for e in edges {
        indeg[e.to] += 1;
    }
    let mut topo: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut head = 0;
    while head < topo.len() {
        let v = topo[head];
        head += 1;
        for e in edges.iter().filter(|e| e.from == v) {
            indeg[e.to] -= 1;
            if indeg[e.to] == 0 {
                topo.push(e.to);
            }
        }
    }
    let mut seen = vec![false; n];
    topo.iter().for_each(|&v| seen[v] = true);
    topo.extend((0..n).filter(|&v| !seen[v]));

    let mut rank = vec![0usize; n];
    for &v in &topo {
        for e in edges.iter().filter(|e| e.to == v) {
            rank[v] = rank[v].max(rank[e.from] + 1);
        }
    }
    for &v in topo.iter().rev() {
        let out: Vec<&Edge> = edges.iter().filter(|e| e.from == v).collect();
        let Some(m) = out.iter().map(|e| rank[e.to]).min() else { continue };
        // A pure producer, or a node fed by others whose data outputs all go to one consumer,
        // sits in the column just before it rather than wherever its own inputs left it. (Exec
        // successors already follow the execution path; a source like the function's input
        // stays first.)
        let fed = edges.iter().any(|e| e.to == v);
        let one_consumer = out.iter().all(|e| e.to == out[0].to && e.weight < EXEC_WEIGHT);
        if pure[v] || (fed && one_consumer) {
            rank[v] = rank[v].max(m.saturating_sub(1));
        }
    }
    (rank, topo)
}

/// One edge segment between adjacent layers: `a` in layer r, `b` in layer r + 1.
struct Seg {
    a: usize,
    b: usize,
    ay: f32,
    by: f32,
    w: f32,
}

/// The proper layered graph: real nodes first, then dummies.
struct Lg {
    h: Vec<f32>,
    rank: Vec<usize>,
    dummy: Vec<bool>,
    segs: Vec<Seg>,
    /// Segment indices entering (from the left) and leaving (to the right) each node.
    left: Vec<Vec<usize>>,
    right: Vec<Vec<usize>>,
}

/// Splits every edge spanning several layers into a chain of dummy nodes.
fn split(sizes: &[[f32; 2]], edges: &[Edge], rank: &[usize]) -> Lg {
    let mut lg = Lg {
        h: sizes.iter().map(|s| s[1]).collect(),
        rank: rank.to_vec(),
        dummy: vec![false; sizes.len()],
        segs: vec![],
        left: vec![],
        right: vec![],
    };
    for e in edges.iter().filter(|e| rank[e.to] > rank[e.from]) {
        let (mut a, mut ay) = (e.from, e.from_y);
        for r in rank[e.from] + 1..rank[e.to] {
            let d = lg.h.len();
            lg.h.push(DUMMY_H);
            lg.rank.push(r);
            lg.dummy.push(true);
            lg.segs.push(Seg { a, b: d, ay, by: DUMMY_H / 2.0, w: e.weight });
            (a, ay) = (d, DUMMY_H / 2.0);
        }
        lg.segs.push(Seg { a, b: e.to, ay, by: e.to_y, w: e.weight });
    }
    let m = lg.h.len();
    lg.left = vec![vec![]; m];
    lg.right = vec![vec![]; m];
    for (i, s) in lg.segs.iter().enumerate() {
        lg.right[s.a].push(i);
        lg.left[s.b].push(i);
    }
    lg
}

/// Port key used for ordering: the node's index in its layer plus the port's relative height.
fn key(lg: &Lg, idx: &[usize], v: usize, port: f32) -> f32 {
    idx[v] as f32 + (port / lg.h[v].max(1.0)).clamp(0.0, 0.99)
}

/// Orders every layer, starting once from the topological order and once from a depth-first
/// walk (which keeps each branch's nodes together); keeps the fewest weighted crossings.
fn order(lg: &Lg, topo: &[usize]) -> Vec<Vec<usize>> {
    let cols = lg.rank.iter().copied().max().unwrap_or(0) + 1;
    let m = lg.h.len();
    let mut seen = vec![false; m];
    let mut walk = vec![];
    for &v in topo {
        dfs(lg, v, &mut seen, &mut walk, false);
    }
    // And from the sinks back: each consumer's producers stay together.
    let mut seen = vec![false; m];
    let mut back = vec![];
    for &v in topo.iter().rev() {
        dfs(lg, v, &mut seen, &mut back, true);
    }
    let starts = [topo.iter().copied().chain(topo.len()..m).collect::<Vec<_>>(), walk, back];
    let mut best: Option<(f32, Vec<Vec<usize>>)> = None;
    for start in starts {
        let mut layers: Vec<Vec<usize>> = vec![vec![]; cols];
        for v in start {
            layers[lg.rank[v]].push(v);
        }
        let run = sweep(lg, layers);
        if best.as_ref().is_none_or(|b| run.0 < b.0) {
            best = Some(run);
        }
    }
    best.expect("three starts").1
}

/// Depth-first discovery order: outputs top to bottom, then inputs top to bottom (inputs first
/// when walking `back` from the sinks).
fn dfs(lg: &Lg, v: usize, seen: &mut [bool], out: &mut Vec<usize>, back: bool) {
    if seen[v] {
        return;
    }
    seen[v] = true;
    out.push(v);
    let mut right: Vec<&Seg> = lg.right[v].iter().map(|&s| &lg.segs[s]).collect();
    right.sort_by(|a, b| a.ay.total_cmp(&b.ay));
    let mut left: Vec<&Seg> = lg.left[v].iter().map(|&s| &lg.segs[s]).collect();
    left.sort_by(|a, b| a.by.total_cmp(&b.by));
    let outs = right.iter().map(|s| s.b);
    let ins = left.iter().map(|s| s.a);
    let next: Vec<usize> = if back { ins.chain(outs).collect() } else { outs.chain(ins).collect() };
    for u in next {
        dfs(lg, u, seen, out, back);
    }
}

/// Sweeps with adjacent transposition, alternating direction; each down-and-up pair sorts by
/// weighted barycentre (which settles the overall order) or by weighted median (which
/// straightens single wires), in turn. Returns the best ordering seen.
fn sweep(lg: &Lg, mut layers: Vec<Vec<usize>>) -> (f32, Vec<Vec<usize>>) {
    let cols = layers.len();
    let mut idx = vec![0usize; lg.h.len()];
    for l in &layers {
        for (i, &v) in l.iter().enumerate() {
            idx[v] = i;
        }
    }
    let mut best = (crossings(lg, &idx), layers.clone());
    for it in 0..SWEEPS {
        let down = it % 2 == 0;
        let mean = (it / 2) % 2 == 0;
        let rs: Vec<usize> = if down { (1..cols).collect() } else { (0..cols.saturating_sub(1)).rev().collect() };
        for r in rs {
            median_sort(lg, &mut layers[r], &idx, down, mean);
            for (i, &v) in layers[r].iter().enumerate() {
                idx[v] = i;
            }
        }
        transpose(lg, &mut layers, &mut idx);
        let c = crossings(lg, &idx);
        if c < best.0 {
            best = (c, layers.clone());
        }
        if best.0 == 0.0 {
            break;
        }
    }
    best
}

/// Sorts one layer by the weighted median (or, with `mean`, the weighted barycentre) port key
/// of each node's neighbours on one side. Nodes without neighbours on that side keep their slot.
fn median_sort(lg: &Lg, layer: &mut [usize], idx: &[usize], from_left: bool, mean: bool) {
    let mut keyed: Vec<(f32, usize)> = vec![];
    let mut slots = vec![];
    for (slot, &v) in layer.iter().enumerate() {
        let segs = if from_left { &lg.left[v] } else { &lg.right[v] };
        let ks: Vec<(f32, f32)> = segs.iter().map(|&s| {
            let sg = &lg.segs[s];
            if from_left { (key(lg, idx, sg.a, sg.ay), sg.w) } else { (key(lg, idx, sg.b, sg.by), sg.w) }
        }).collect();
        if let Some(m) = if mean { barycentre(&ks) } else { weighted_median(ks) } {
            keyed.push((m, v));
            slots.push(slot);
        }
    }
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (slot, (_, v)) in slots.into_iter().zip(keyed) {
        layer[slot] = v;
    }
}

/// Weighted mean of port keys, `None` without any.
fn barycentre(ks: &[(f32, f32)]) -> Option<f32> {
    let total: f32 = ks.iter().map(|k| k.1).sum();
    (total > 0.0).then(|| ks.iter().map(|k| k.0 * k.1).sum::<f32>() / total)
}

/// Upper weighted median: on an even split it picks the upper value, so a node lines up with
/// one neighbour (straight wire) instead of sitting between two.
fn weighted_median(mut ks: Vec<(f32, f32)>) -> Option<f32> {
    ks.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f32 = ks.iter().map(|k| k.1).sum();
    let mut acc = 0.0;
    for &(k, w) in &ks {
        acc += w;
        if acc * 2.0 >= total - 1e-3 {
            return Some(k);
        }
    }
    None
}

/// Swaps adjacent nodes while that lowers the weighted crossings around them.
fn transpose(lg: &Lg, layers: &mut [Vec<usize>], idx: &mut [usize]) {
    for _ in 0..TRANSPOSE_ROUNDS {
        let mut improved = false;
        for l in layers.iter_mut() {
            for i in 0..l.len().saturating_sub(1) {
                let (u, v) = (l[i], l[i + 1]);
                if pair_crossings(lg, idx, v, u) < pair_crossings(lg, idx, u, v) {
                    l.swap(i, i + 1);
                    (idx[u], idx[v]) = (i + 1, i);
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }
}

/// Weighted crossings between the segments of `u` and `v` if `u` sits above `v`.
fn pair_crossings(lg: &Lg, idx: &[usize], u: usize, v: usize) -> f32 {
    let side = |segs: &[usize], left: bool| -> Vec<(f32, f32)> {
        segs.iter().map(|&s| {
            let sg = &lg.segs[s];
            if left { (key(lg, idx, sg.a, sg.ay), cross_w(sg)) } else { (key(lg, idx, sg.b, sg.by), cross_w(sg)) }
        }).collect()
    };
    let mut c = 0.0;
    for left in [true, false] {
        let (su, sv) = if left { (&lg.left[u], &lg.left[v]) } else { (&lg.right[u], &lg.right[v]) };
        for &(ku, wu) in &side(su, left) {
            for &(kv, wv) in &side(sv, left) {
                if ku > kv {
                    c += wu * wv;
                }
            }
        }
    }
    c
}

/// Crossing weight of a segment: exec wires matter more, but not so much that data lanes get
/// pushed away from the exec path to avoid crossing it.
fn cross_w(s: &Seg) -> f32 {
    if s.w >= EXEC_WEIGHT { EXEC_CROSS } else { 1.0 }
}

/// Total weighted crossings over all adjacent layer pairs.
fn crossings(lg: &Lg, idx: &[usize]) -> f32 {
    let cols = lg.rank.iter().copied().max().unwrap_or(0) + 1;
    let mut gaps: Vec<Vec<(f32, f32, f32)>> = vec![vec![]; cols];
    for s in &lg.segs {
        gaps[lg.rank[s.a]].push((key(lg, idx, s.a, s.ay), key(lg, idx, s.b, s.by), cross_w(s)));
    }
    let mut c = 0.0;
    for g in &gaps {
        for (i, x) in g.iter().enumerate() {
            for y in &g[i + 1..] {
                if (x.0 - y.0) * (x.1 - y.1) < 0.0 {
                    c += x.2 * y.2;
                }
            }
        }
    }
    c
}

/// Priority-method vertical coordinates: layer by layer, each node moves toward its neighbours'
/// ports, highest priority first; lower-priority nodes are pushed aside, higher ones block.
fn place(lg: &Lg, layers: &[Vec<usize>]) -> Vec<f32> {
    let m = lg.h.len();
    let exec = |v: usize| lg.left[v].iter().chain(&lg.right[v]).any(|&s| lg.segs[s].w >= EXEC_WEIGHT);
    let prio: Vec<f32> = (0..m).map(|v| {
        let deg = (lg.left[v].len() + lg.right[v].len()) as f32 / 100.0;
        deg + match (lg.dummy[v], exec(v)) {
            (true, true) => 4.0,
            (false, true) => 3.0,
            (true, false) => 2.0,
            (false, false) => 1.0,
        }
    }).collect();
    let mut y = vec![0.0f32; m];
    for l in layers {
        let mut acc = 0.0;
        for (i, &v) in l.iter().enumerate() {
            y[v] = acc;
            acc += lg.h[v] + gap(lg, l, i);
        }
    }
    let cols = layers.len();
    for pass in 0..PLACE_PASSES {
        // Alternate left-to-right and right-to-left, ending right-to-left. In the last pass the
        // exec path (already straightened by the preceding left-to-right pass) stays put and
        // only pure producers move toward their consumers.
        let down = pass % 2 == PLACE_PASSES % 2;
        let last = pass + 1 == PLACE_PASSES;
        let rs: Vec<usize> = if down { (0..cols).collect() } else { (0..cols).rev().collect() };
        for r in rs {
            let desired: Vec<Option<f32>> = layers[r].iter().map(|&v| {
                if last && exec(v) {
                    return None;
                }
                let segs = if down { &lg.left[v] } else { &lg.right[v] };
                weighted_median(segs.iter().map(|&s| {
                    let sg = &lg.segs[s];
                    if down { (y[sg.a] + sg.ay - sg.by, sg.w) } else { (y[sg.b] + sg.by - sg.ay, sg.w) }
                }).collect())
            }).collect();
            place_layer(lg, &layers[r], &mut y, &prio, &desired);
        }
    }
    y
}

/// Vertical gap below the `i`th node of a layer.
fn gap(lg: &Lg, layer: &[usize], i: usize) -> f32 {
    let next_dummy = layer.get(i + 1).is_some_and(|&v| lg.dummy[v]);
    if lg.dummy[layer[i]] || next_dummy { DUMMY_GAP } else { ROW_GAP }
}

fn place_layer(lg: &Lg, layer: &[usize], y: &mut [f32], prio: &[f32], desired: &[Option<f32>]) {
    let k = layer.len();
    let h = |i: usize| lg.h[layer[i]];
    let sp = |i: usize| h(i) + gap(lg, layer, i);
    let mut by_prio: Vec<usize> = (0..k).collect();
    by_prio.sort_by(|&a, &b| prio[layer[b]].total_cmp(&prio[layer[a]]));
    let mut fixed = vec![false; k];
    for i in by_prio {
        let cur = y[layer[i]];
        match desired[i] {
            Some(d) if d < cur => {
                let (mut need, mut bound) = (0.0, f32::MIN);
                for j in (0..i).rev() {
                    need += sp(j);
                    if fixed[j] {
                        bound = y[layer[j]] + need;
                        break;
                    }
                }
                y[layer[i]] = d.max(bound);
                for j in (0..i).rev() {
                    let lim = y[layer[j + 1]] - sp(j);
                    if y[layer[j]] <= lim {
                        break;
                    }
                    y[layer[j]] = lim;
                }
            }
            Some(d) if d > cur => {
                let (mut need, mut bound) = (0.0, f32::MAX);
                for j in i + 1..k {
                    need += sp(j - 1);
                    if fixed[j] {
                        bound = y[layer[j]] - need;
                        break;
                    }
                }
                y[layer[i]] = d.min(bound);
                for j in i + 1..k {
                    let lim = y[layer[j - 1]] + sp(j - 1);
                    if y[layer[j]] >= lim {
                        break;
                    }
                    y[layer[j]] = lim;
                }
            }
            _ => {}
        }
        fixed[i] = true;
    }
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
                ins: vec![], outs: vec![Pin { label: String::new(), ty: pin.ty, spelled: pin.spelled.clone() }], group: g.nodes[w.to.0].group, members: 0, effects: None, rejection: None,
            };
            g.nodes.push(getter);
            g.wires.push(Wire { from: (g.nodes.len() - 1, 0), to: w.to, back: false });
        } else {
            g.wires.push(w);
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::State;

    #[test]
    fn frames_inset_their_members_evenly() {
        let scenes = [State::protocol_module(true), State::protocol_fixture(), State::load("fixture"), State::load("interp")];
        let mut checked = 0;
        for s in &scenes {
            let (g, pl) = (&s.view, &s.placed);
            for (gi, f) in pl.frames.iter().enumerate() {
                let m: Vec<usize> = (0..g.nodes.len()).filter(|&i| g.nodes[i].group == Some(gi)).collect();
                if m.is_empty() {
                    continue;
                }
                let min = |k: usize| m.iter().map(|&i| pl.pos[i][k]).fold(f32::MAX, f32::min);
                let max = |k: usize| m.iter().map(|&i| pl.pos[i][k] + pl.size[i][k]).fold(f32::MIN, f32::max);
                let insets = [min(0) - f[0], f[0] + f[2] - max(0), f[1] + f[3] - max(1), min(1) - f[1]];
                let want = [FRAME_PAD, FRAME_PAD, FRAME_PAD, FRAME_TITLE];
                assert!(insets.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-3), "{}: group {gi} insets {insets:?}", s.name());
                // Each column of members hangs from the title: its top member right under it.
                for &i in &m {
                    let top = m.iter().filter(|&&j| pl.pos[j][0] == pl.pos[i][0]).map(|&j| pl.pos[j][1]).fold(f32::MAX, f32::min);
                    assert!((top - (f[1] + FRAME_TITLE)).abs() < 1e-3, "{}: group {gi}: {} column starts {} below the title", s.name(), g.nodes[i].title, top - f[1] - FRAME_TITLE);
                }
                checked += 1;
            }
        }
        assert!(checked >= 2, "only {checked} frames");
    }

    #[test]
    fn the_fixture_frames_members_are_top_aligned() {
        let s = State::load("fixture");
        let at = |t: &str| s.placed.pos[s.view.nodes.iter().position(|n| n.title == t).unwrap()];
        assert_eq!(at("acc + i")[1], at("normalize")[1]);
    }

    #[test]
    fn long_edge_lanes_stay_clear_of_nodes() {
        for scene in ["interp", "branches", "dataflow", "devices", "fixture"] {
            let s = State::load(scene);
            let (_, lanes, units) = layout_lanes(&s.view);
            if scene == "interp" {
                assert!(lanes.len() >= 10, "interp has long edges: {} lanes", lanes.len());
            }
            let overlaps = |a: [f32; 4], b: [f32; 4]| a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3];
            for l in &lanes {
                for u in &units {
                    assert!(!overlaps(*l, *u), "{scene}: lane {l:?} crosses node {u:?}");
                }
            }
        }
    }

    #[test]
    fn a_loops_inputs_sit_just_before_it_in_its_row() {
        // `entry_count` and `total = 0` feed only the loop: they sit in the column right before
        // it, in its row, so their wires are short and not a second return lane under the row.
        let s = State::load("branches");
        let (g, pl) = (&s.view, &s.placed);
        let find = |t: &str| g.nodes.iter().position(|n| n.title == t).unwrap_or_else(|| panic!("{t}"));
        let lp = find("for (i = 0; i < n; i++)");
        let row = |i: usize| pl.row_gaps.iter().filter(|&&y| y < pl.pos[i][1]).count();
        for t in ["entry_count", "total = 0"] {
            let i = find(t);
            let gap = pl.pos[lp][0] - (pl.pos[i][0] + pl.size[i][0]);
            assert!(gap >= COL_GAP - 1e-3, "{t} sits {gap} before the loop");
            let between = (0..g.nodes.len()).filter(|&j| row(j) == row(lp) && pl.pos[j][0] > pl.pos[i][0] && pl.pos[j][0] < pl.pos[lp][0]).count();
            assert_eq!(between, 0, "{t}: the loop is the next column");
            assert_eq!(row(i), row(lp), "{t} is in the loop's row");
        }
    }

    #[test]
    fn collapsing_a_group_never_wraps_the_graph_into_a_new_row() {
        let g = with_getters(crate::scenes::scene("fixture"));
        let gi = g.groups.iter().position(|grp| grp.title == "step ∘ normalize").expect("the fixture's group");
        let mut collapsed = vec![false; g.groups.len()];
        let open = layout(&g.collapse(&collapsed).0);
        collapsed[gi] = true;
        let view = g.collapse(&collapsed).0;
        let shut = layout(&view);
        assert_eq!(shut.row_gaps.len(), open.row_gaps.len());
        // `report` stays level with the loop rather than starting a row under it.
        let at = |t: &str| shut.pos[view.nodes.iter().position(|n| n.title == t).unwrap_or_else(|| panic!("{t}"))];
        let lp = view.nodes.iter().position(|n| n.kind == Kind::Loop).expect("the loop");
        assert!(at("report")[1] < shut.pos[lp][1] + shut.size[lp][1], "report wrapped under the loop");
    }

    #[test]
    fn a_collapsed_group_takes_its_frames_place_and_nothing_else_moves_but_left() {
        for (scene, title) in [("fixture", "step ∘ normalize"), ("interp", "alu")] {
            let g = with_getters(crate::scenes::scene(scene));
            let gi = g.groups.iter().position(|grp| grp.title == title).expect("the group");
            let mut collapsed = vec![false; g.groups.len()];
            let open = layout(&g.collapse(&collapsed).0);
            collapsed[gi] = true;
            let (view, origin) = g.collapse(&collapsed);
            let shut = layout(&view);
            for (v, o) in origin.iter().enumerate() {
                let (was, now) = match *o {
                    model::Origin::Node(i) => (open.pos[i], shut.pos[v]),
                    model::Origin::Group(_) => ([open.frames[gi][0], open.frames[gi][1]], shut.pos[v]),
                };
                // Same height; columns only close up.
                assert!((was[1] - now[1]).abs() < 1e-3 && now[0] <= was[0] + 1e-3, "{scene}: {} moved {was:?} -> {now:?}", view.nodes[v].title);
            }
        }
    }

    #[test]
    fn a_node_without_pins_is_only_its_header() {
        let s = State::protocol_module(false);
        let ty = s.view.nodes.iter().position(|n| n.ins.is_empty() && n.outs.is_empty()).expect("a field-less type");
        assert_eq!(s.placed.size[ty][1], model::HEADER);
    }
}
