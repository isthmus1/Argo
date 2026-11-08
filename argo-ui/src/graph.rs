//! Read-only node-graph canvas: grid, expanded-group frames, Bézier wires and nodes, under a
//! pan/zoom camera. State is shown with colour, chips, badges and worker avatars.

use crate::app::{Item, MARGIN, State};
use crate::model::{self, Claim, Evidence, Graph, Kind, Lang, Node, Pin, ROW, Ty};
use crate::palette::Pal;
use rune_ui::{Color, Cursor, Drag, El, LineCap, Path, Shape, TextStyle, Tx, canvas, col, el, row, text};

const SOCKET: TextStyle = TextStyle::new(11.0, 450.0);
/// Matches the floating panels' corner radius.
pub const RADIUS: f32 = 12.0;
/// Frames round a little more than the nodes inside them.
const FRAME_RADIUS: f32 = RADIUS + 4.0;
/// Header chips (language, member count) are this tall.
const CHIP_H: f32 = 16.0;
/// A node header's last chip sits this far from the node's top and right edges.
const NODE_CHIP_INSET: f32 = (model::HEADER - CHIP_H) / 2.0;
/// A frame's title row; its chip sits as far from the frame's top as from its right.
const FRAME_TITLE_H: f32 = 32.0;
const FRAME_CHIP_INSET: f32 = (FRAME_TITLE_H - CHIP_H) / 2.0;

/// Radius of an element `height` tall inset by `inset` on both sides of a corner of radius
/// `outer`, so the two curves share a centre (never negative, at most a pill).
pub fn concentric(outer: f32, inset: f32, height: f32) -> f32 {
    (outer - inset).clamp(0.0, height / 2.0)
}
/// Below this zoom, pin labels are hidden: they would be unreadable noise.
const LABEL_ZOOM: f32 = 0.55;
/// Node titles' world size at rest.
const TITLE_SIZE: f32 = 11.5;
/// Zoomed out, node titles keep at least this screen size (truncating sooner inside their
/// header), up to `TITLE_MAX` world points so they still fit the header's height.
const TITLE_MIN_SCREEN: f32 = 9.5;
const TITLE_MAX: f32 = 16.0;

/// The world size of node titles at `zoom`: `TITLE_SIZE`, counter-scaled when zoomed out far
/// enough that it would read smaller than `TITLE_MIN_SCREEN` on screen.
fn title_size(zoom: f32) -> f32 {
    (TITLE_MIN_SCREEN / zoom.max(1e-3)).clamp(TITLE_SIZE, TITLE_MAX)
}

/// How an item is emphasised on the canvas.
#[derive(Clone, Copy)]
struct Look {
    selected: bool,
    hovered: bool,
    /// Screen-size compensation for badges and avatars at low zoom.
    badge_scale: f32,
    labels: bool,
    /// Node title size in world points (see [`title_size`]).
    title: f32,
}

/// The canvas filling `w`×`h`, with the world under the camera.
pub fn view(p: &Pal, s: &State, w: f32, h: f32) -> El {
    let g = &s.view;
    let pl = &s.placed;
    let [bx, by, bw, bh] = pl.bounds;
    let (ww, wh) = (bw + 2.0 * MARGIN, bh + 2.0 * MARGIN);
    let o = [MARGIN - bx, MARGIN - by];
    let focus = s.hovered.or(s.selected);
    let look = |item: Item| Look {
        selected: s.selected == Some(item),
        hovered: s.hovered == Some(item) && s.selected != Some(item),
        badge_scale: (1.0 / s.cam.zoom).clamp(1.0, 2.2),
        labels: s.cam.zoom >= LABEL_ZOOM,
        title: title_size(s.cam.zoom),
    };

    let mut world = vec![];
    for (i, f) in pl.frames.iter().enumerate() {
        if s.collapsed.get(i).copied().unwrap_or(false) || f[2] <= 0.0 {
            continue;
        }
        world.push(frame(p, &g.groups[i], i, f[2], f[3], look(Item::Group(i))).absolute(o[0] + f[0], o[1] + f[1]));
    }
    world.push(canvas(wires(p, g, pl, o, focus)).size(ww, wh).absolute(0.0, 0.0));
    for (i, n) in g.nodes.iter().enumerate() {
        world.push(node(p, n, i, pl.size[i], s.can_expand(i), look(Item::Node(i))).absolute(o[0] + pl.pos[i][0], o[1] + pl.pos[i][1]));
    }
    let world = el().size(ww, wh).scale(s.cam.zoom).origin(0.0, 0.0).children(world).absolute(s.cam.pan[0], s.cam.pan[1]);

    let (sx, sy) = (s.cam.pan[0], s.cam.pan[1]);
    let (ex, ey) = (sx + ww * s.cam.zoom, sy + wh * s.cam.zoom);
    let fade = |more: bool| if more { 56.0 } else { 0.0 };
    el().key("canvas").size(w, h).fill(p.canvas).clip_fade(fade(sx < 0.0), fade(sy < 0.0), fade(ex > w), fade(ey > h)).cursor(Cursor::Grab)
        .on_drag(pan)
        // Every camera change lands at once: the world (nodes and wires, under one scale) and
        // the grid canvas beside it must show the same camera on every frame, and a layout
        // spring on the world would trail the grid.
        .on_wheel(|s: &mut State, e, tx| {
            s.cam.zoom_about(e.x, e.y, (-e.dy * 0.0025).exp());
            tx.instant();
            true
        })
        .on_magnify(|s: &mut State, e, tx| {
            s.cam.zoom_about(e.x, e.y, 1.0 + e.delta);
            tx.instant();
            true
        })
        .on_press(|s: &mut State| {
            // The release of a pan is no click: keep the selection.
            if !s.cam.take_panned() {
                s.selected = None;
            }
        })
        .child(canvas(grid(p, s, w, h)).size(w, h).absolute(0.0, 0.0))
        .child(world)
}

/// A pan drag, from the canvas or anything on it (the graph is read-only, so dragging a node
/// pans too). Every phase lands at once, its release included, so nothing springs after it.
fn pan(s: &mut State, d: &mut Drag, tx: &mut Tx) {
    s.cam.drag(d.phase, [d.dx, d.dy]);
    tx.instant();
}

/// Grid lines that move with the camera; minor lines fade out when they get dense.
fn grid(p: &Pal, s: &State, w: f32, h: f32) -> Shape {
    let mut sh = Shape::new();
    let step = 24.0 * s.cam.zoom;
    let minor = if step < 8.0 { None } else if step < 12.0 { Some(p.grid_minor.mix(p.canvas, 0.5)) } else { Some(p.grid_minor) };
    let (ox, oy) = (s.cam.pan[0].rem_euclid(step * 5.0), s.cam.pan[1].rem_euclid(step * 5.0));
    for (along, extent, origin, horizontal) in [(w, h, ox, false), (h, w, oy, true)] {
        let mut i: i32 = -5;
        while origin + i as f32 * step <= along {
            let v = origin + i as f32 * step;
            let major = i.rem_euclid(5) == 0;
            if let Some(c) = if major { Some(p.grid_major) } else { minor } {
                let (a, b) = if horizontal { ([0.0, v], [extent, v]) } else { ([v, 0.0], [v, extent]) };
                sh = sh.line(a, b, 1.0, LineCap::Butt, c);
            }
            i += 1;
        }
    }
    sh
}

/// An expanded node group: a frame around its members. The title's button (or a double-click
/// anywhere on the frame) collapses it into a single node in place.
fn frame(p: &Pal, f: &model::Group, gi: usize, w: f32, h: f32, look: Look) -> El {
    let collapse = group_button(p.frame_text.alpha(0.9), p.frame_text.alpha(0.16), false, look.badge_scale)
        .key(format!("collapse-group-{gi}"))
        .on_press(move |s: &mut State| s.collapse_group(gi));
    // Trailing controls sit in the frame's corner: equal top and right insets, concentric radii;
    // an evidence badge on that corner pushes the button clear of it.
    let pr = FRAME_CHIP_INSET + if f.evidence == Evidence::None { 0.0 } else { BADGE_CLEAR };
    let title = row().h(FRAME_TITLE_H).w(w).pl(14.0).pr(pr).gap(8.0).items_center()
        .child(text("▣").font(TextStyle::new(12.0, 650.0)).color(p.frame_text.alpha(0.7)))
        .child(text(f.title.clone()).font(TextStyle::new(13.0, 650.0)).color(p.frame_text).single_line().truncate().grow(1.0).min_w(0.0))
        .child(lang_chip(p, f.lang).radius(concentric(FRAME_RADIUS, FRAME_CHIP_INSET, CHIP_H)))
        .child(collapse);
    let border = if look.selected { (2.0, p.accent) } else if look.hovered { (1.5, p.muted) } else { (1.5, p.frame_text.alpha(0.22)) };
    let fr = el().size(w, h).fill(p.frame_fill).radius(FRAME_RADIUS)
        .border(border.0, border.1)
        .child(title.absolute(0.0, 0.0))
        .on_press(move |s: &mut State| if !s.cam.take_panned() { s.select(Item::Group(gi)) })
        .on_double_press(move |s: &mut State| s.toggle_group(gi))
        .on_drag(pan)
        .on_hover_change(move |s: &mut State, on, _| s.hover(Item::Group(gi), on));
    decorate(p, fr, [w, h], FRAME_RADIUS, f.evidence, f.claim, look)
}

/// The collapse/expand button's hit target is at least this many screen points across.
const GROUP_HIT: f32 = 22.0;
/// Extra right inset that keeps a corner button clear of the evidence badge on that corner.
const BADGE_CLEAR: f32 = 6.0;

/// The collapse (frame -> node) or expand (node -> frame) button: diagonal arrows drawn as
/// strokes, pointing in to collapse and out to expand, as for leaving and entering full screen.
/// It takes the `model::GROUP_BUTTON` slot in layout but draws, highlights and hit-tests as a
/// `GROUP_HIT` square (times `badge_scale`, so it stays that big on screen when zoomed out); the
/// glyph inside keeps the slot's size.
fn group_button(fg: Color, hover: Color, expand: bool, badge_scale: f32) -> El {
    let d = model::GROUP_BUTTON;
    // Drawn in the slot, then scaled up by `grow`: draw the glyph `1 / grow` as large.
    let grow = GROUP_HIT / d;
    let u = 1.0 / grow;
    let c = d / 2.0;
    let (reach, stroke, head, gap) = (d * 0.28 * u, 1.5 * u, 4.0 * u, 1.5 * u);
    let mut sh = Shape::new();
    // One arrow per diagonal quadrant: a shaft from the corner to near the centre, and an
    // L-shaped head at its corner end (expand) or centre end (collapse).
    for (sx, sy) in [(1.0, -1.0), (-1.0, 1.0)] {
        let outer = [c + sx * reach, c + sy * reach];
        let inner = [c + sx * gap, c + sy * gap];
        let tip = if expand { outer } else { inner };
        let (hx, hy) = if expand { (-sx * head, -sy * head) } else { (sx * head, sy * head) };
        sh = sh.line(outer, inner, stroke, LineCap::Round, fg).stroke_path(
            &Path::new().move_to([tip[0] + hx, tip[1]]).line_to(tip).line_to([tip[0], tip[1] + hy]),
            stroke,
            LineCap::Round,
            fg,
        );
    }
    // Named as the docked panel's button is.
    el().size(d, d).radius(5.0).hover(hover).cursor(Cursor::Pointer).scale(grow * badge_scale).child(canvas(sh).size(d, d))
        .help(if expand { "Expand group" } else { "Collapse to node" })
}

fn socket_world(g: &Graph, pl: &crate::layout::Placed, i: usize, output: bool, idx: usize) -> [f32; 2] {
    let [x, y] = model::socket_local(&g.nodes[i], pl.size[i][0], output, idx);
    [pl.pos[i][0] + x, pl.pos[i][1] + y]
}

/// A wire's shape, decided before anything is drawn so wires can make room for each other.
#[derive(Clone, Copy)]
enum Curve {
    /// A forward S-curve: `[from, handle, handle, to]`.
    Cubic([[f32; 2]; 4]),
    /// Out of `a`, down to `below`, back along it and up into `b` (a loop backedge or a wire
    /// into a wrapped row), its legs swinging `reach` beside the sockets.
    Routed { a: [f32; 2], b: [f32; 2], below: f32, reach: f32 },
    /// Straight out of `a`, one S-bend whose vertical travel runs down the free gutter at `x`
    /// (swinging `bend` either side of it), then straight into `b`: a forward wire that would
    /// otherwise cut through the nodes between its ends.
    Gutter { a: [f32; 2], b: [f32; 2], x: f32, bend: f32 },
}

impl Curve {
    fn path(self) -> Path {
        match self {
            Curve::Cubic([a, c1, c2, b]) => Path::new().move_to(a).cubic_to(c1, c2, b),
            Curve::Gutter { a, b, x, bend } => Path::new()
                .move_to(a)
                .line_to([x - bend, a[1]])
                .cubic_to([x, a[1]], [x, b[1]], [x + bend, b[1]])
                .line_to(b),
            Curve::Routed { a, b, below, reach } => Path::new()
                .move_to(a)
                .cubic_to([a[0] + reach, a[1]], [a[0] + reach, below], [a[0], below])
                .line_to([b[0], below])
                .cubic_to([b[0] - reach, below], [b[0] - reach, b[1]], b),
        }
    }

    /// The same curve as a polyline, so it can be dashed and measured.
    fn points(self) -> Vec<[f32; 2]> {
        match self {
            Curve::Cubic([a, c1, c2, b]) => {
                let mut pts = vec![];
                cubic_points(a, c1, c2, b, &mut pts);
                pts
            }
            Curve::Routed { a, b, below, reach } => back_points(a, b, below, reach),
            Curve::Gutter { a, b, x, bend } => {
                let mut pts = vec![a];
                cubic_points([x - bend, a[1]], [x, a[1]], [x, b[1]], [x + bend, b[1]], &mut pts);
                pts.push(b);
                pts
            }
        }
    }
}

/// Clearance a gutter-routed wire keeps from node bodies.
const NODE_CLEAR: f32 = 6.0;
/// Most a gutter S-bend swings either side of its vertical run.
const GUTTER_BEND: f32 = 28.0;
/// Spacing between wires sharing a gutter, so their vertical runs don't merge.
const GUTTER_LANE: f32 = 8.0;
/// Where a wire passes behind a node, this much of it beside the node's edge is masked in the
/// background colour, so it reads as passing behind rather than plugging in there.
const BEHIND_GAP: f32 = 8.0;
/// Wires shorter than this get no direction arrow: on a short hop the pins already show which
/// way it runs, and an arrow there reads as a dashed double line. A hop straight across one
/// layout column gap (80) is short; one that also climbs or drops a row or so is not.
const ARROW_MIN_ROUTE: f32 = 96.0;

/// Wires sharing one gutter: per gutter (keyed by its left edge), the source socket and run x of
/// each wire placed in it so far.
type GutterLanes = std::collections::HashMap<i32, Vec<((usize, usize), f32)>>;

/// The part of segment `p`–`q` inside `r` (`[x0, y0, x1, y1]`), as a `t` range along it;
/// `None` if it misses or only touches an edge. Liang–Barsky clipping.
fn clip(p: [f32; 2], q: [f32; 2], r: [f32; 4]) -> Option<(f32, f32)> {
    let d = [q[0] - p[0], q[1] - p[1]];
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (pk, qk) in [(-d[0], p[0] - r[0]), (d[0], r[2] - p[0]), (-d[1], p[1] - r[1]), (d[1], r[3] - p[1])] {
        if pk == 0.0 {
            if qk <= 0.0 {
                return None;
            }
        } else if pk < 0.0 {
            t0 = t0.max(qk / pk);
        } else {
            t1 = t1.min(qk / pk);
        }
    }
    (t0 < t1).then_some((t0, t1))
}

/// Does segment `p`–`q` pass through the inside of `r`?
fn seg_hits(p: [f32; 2], q: [f32; 2], r: [f32; 4]) -> bool {
    clip(p, q, r).is_some()
}

/// Does a polyline pass through any of `rects`?
fn crosses(pts: &[[f32; 2]], rects: &[[f32; 4]]) -> bool {
    pts.windows(2).any(|s| rects.iter().any(|&r| seg_hits(s[0], s[1], r)))
}

fn pad(r: [f32; 4], by: f32) -> [f32; 4] {
    [r[0] - by, r[1] - by, r[2] + by, r[3] + by]
}

/// The x spans strictly between `a` and `b` where a vertical run from `a`'s height to `b`'s
/// meets none of `rects` (kept `NODE_CLEAR` away), left to right.
fn gutters(a: [f32; 2], b: [f32; 2], rects: &[[f32; 4]]) -> Vec<(f32, f32)> {
    let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
    let mut blocked: Vec<(f32, f32)> = rects.iter().map(|&r| pad(r, NODE_CLEAR))
        .filter(|r| r[1] < y1 && r[3] > y0 && r[2] > a[0] && r[0] < b[0])
        .map(|r| (r[0], r[2]))
        .collect();
    blocked.sort_by(|p, q| p.0.total_cmp(&q.0));
    let mut free = vec![];
    let mut x = a[0];
    for (l, r) in blocked {
        if l > x {
            free.push((x, l.min(b[0])));
        }
        x = x.max(r);
    }
    if x < b[0] {
        free.push((x, b[0]));
    }
    free.retain(|&(l, r)| r - l >= 2.0 * NODE_CLEAR);
    free
}

/// A forward wire from `a` to `b` whose S-curve would cut through some of `others` (the nodes
/// other than its own two): its vertical travel moved into a free gutter between columns, with
/// both straight legs clear of every node too. Gutters nearest the middle are tried first; each
/// wire in a gutter takes its own lane, except wires from one socket, which run together.
/// `None` when no gutter clears everything (the caller then masks where it passes behind).
fn gutter_route(a: [f32; 2], b: [f32; 2], from: (usize, usize), others: &[[f32; 4]], lanes: &mut GutterLanes) -> Option<Curve> {
    if b[0] <= a[0] {
        return None;
    }
    let mid = (a[0] + b[0]) / 2.0;
    let mut free = gutters(a, b, others);
    free.sort_by(|p, q| ((p.0 + p.1) / 2.0 - mid).abs().total_cmp(&((q.0 + q.1) / 2.0 - mid).abs()));
    let legs_clear = |x0: f32, x1: f32, y: f32| others.iter().all(|&r| {
        let r = pad(r, NODE_CLEAR / 2.0);
        !(y > r[1] && y < r[3] && x1 > r[0] && x0 < r[2])
    });
    for (l, r) in free {
        let key = l.round() as i32;
        let used = lanes.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let x = match used.iter().find(|u| u.0 == from) {
            Some(u) => u.1,
            None => {
                // 0, +1, -1, +2, -2 … lanes out from the gutter's middle.
                let k = used.len();
                let step = if k % 2 == 1 { (k / 2 + 1) as f32 } else { -((k / 2) as f32) };
                ((l + r) / 2.0 + step * GUTTER_LANE).clamp(l + 4.0, r - 4.0)
            }
        };
        let bend = GUTTER_BEND.min(x - l).min(r - x);
        if !legs_clear(a[0], x - bend, a[1]) || !legs_clear(x + bend, b[0], b[1]) {
            continue;
        }
        let entry = lanes.entry(key).or_default();
        if !entry.iter().any(|u| u.0 == from) {
            entry.push((from, x));
        }
        return Some(Curve::Gutter { a, b, x, bend });
    }
    None
}

/// Total length of a polyline.
fn polyline_len(pts: &[[f32; 2]]) -> f32 {
    pts.windows(2).map(|s| (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1])).sum()
}

/// Whether a wire along `route` gets a mid-wire direction arrow (see [`ARROW_MIN_ROUTE`]).
fn wants_arrow(route: &[[f32; 2]]) -> bool {
    polyline_len(route) >= ARROW_MIN_ROUTE
}

/// Where a polyline enters and leaves the inside of `r`: (distance along it, entering?).
fn rect_crossings(pts: &[[f32; 2]], r: [f32; 4]) -> Vec<(f32, bool)> {
    let mut out = vec![];
    let mut along = 0.0;
    for s in pts.windows(2) {
        let (p, q) = (s[0], s[1]);
        let len = (q[0] - p[0]).hypot(q[1] - p[1]);
        if let Some((t0, t1)) = clip(p, q, r) {
            // A clip starting or ending mid-segment is an edge; at a segment's end the polyline
            // carries on inside.
            if t0 > 0.0 {
                out.push((along + len * t0, true));
            }
            if t1 < 1.0 {
                out.push((along + len * t1, false));
            }
        }
        along += len;
    }
    out
}

/// The part of a polyline between distances `s0` and `s1` along it.
fn sub_polyline(pts: &[[f32; 2]], s0: f32, s1: f32) -> Vec<[f32; 2]> {
    let mut out = vec![];
    let mut along = 0.0;
    for s in pts.windows(2) {
        let (p, q) = (s[0], s[1]);
        let len = (q[0] - p[0]).hypot(q[1] - p[1]);
        let at = |d: f32| {
            let t = if len > 0.0 { ((d - along) / len).clamp(0.0, 1.0) } else { 0.0 };
            [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]
        };
        if along + len >= s0 && along <= s1 {
            if out.is_empty() {
                out.push(at(s0));
            }
            out.push(at(s1.min(along + len)));
        }
        along += len;
    }
    out
}

fn wires(p: &Pal, g: &Graph, pl: &crate::layout::Placed, o: [f32; 2], focus: Option<Item>) -> Shape {
    let mut s = Shape::new();
    let touches = |w: &model::Wire| match focus {
        Some(Item::Node(i)) => w.from.0 == i || w.to.0 == i,
        Some(Item::Group(gi)) => g.nodes[w.from.0].group == Some(gi) || g.nodes[w.to.0].group == Some(gi),
        None => true,
    };
    let ends: Vec<([f32; 2], [f32; 2])> = g.wires.iter().map(|w| {
        let a = socket_world(g, pl, w.from.0, true, w.from.1);
        let b = socket_world(g, pl, w.to.0, false, w.to.1);
        ([a[0] + o[0], a[1] + o[1]], [b[0] + o[0], b[1] + o[1]])
    }).collect();

    // Route every wire first, in a fixed order, so lanes don't jump as the focus changes.
    let backs = back_lanes(&g.wires.iter().zip(&ends).filter(|(w, _)| w.back).map(|(w, &(a, b))| BackRun {
        socket_y: a[1],
        lo: a[0].min(b[0]) - BACK_REACH,
        hi: a[0].max(b[0]) + BACK_REACH,
        below: back_below(pl, o, w, a, b),
    }).collect::<Vec<_>>());
    let mut backs = backs.into_iter();
    let mut gap_lanes = std::collections::HashMap::<usize, usize>::new();
    // Node bodies as drawn (a collapsed group's stacked plates included), in canvas space.
    let rects: Vec<[f32; 4]> = (0..g.nodes.len()).map(|i| {
        let (x, y) = (pl.pos[i][0] + o[0], pl.pos[i][1] + o[1]);
        let stack = if g.nodes[i].kind == Kind::Group { STACK_STEP * STACK_PLATES as f32 } else { 0.0 };
        [x, y, x + pl.size[i][0] + stack, y + pl.size[i][1] + stack]
    }).collect();
    // The nodes a wire doesn't plug into: what it must not seem to plug into either.
    let others = |w: &model::Wire| -> Vec<[f32; 4]> {
        rects.iter().enumerate().filter(|&(j, _)| j != w.from.0 && j != w.to.0).map(|(_, &r)| r).collect()
    };
    let mut gutter_lanes = GutterLanes::new();
    let curves: Vec<Curve> = g.wires.iter().zip(&ends).map(|(w, &(a, b))| {
        if w.back {
            let (below, reach) = backs.next().expect("one lane per backedge");
            return Curve::Routed { a, b, below, reach };
        }
        // Wrapped rows: a wire into a later row runs back through the gap between the rows.
        let wrap_gap = (b[0] < a[0])
            .then(|| pl.row_gaps.iter().position(|&g| g + o[1] > a[1] && g + o[1] < b[1]))
            .flatten();
        if let Some(gi) = wrap_gap {
            // Each wire in a gap gets its own lane, so parallel runs don't merge into one line.
            let lane = gap_lanes.entry(gi).or_insert(0usize);
            let below = pl.row_gaps[gi] + o[1] + (*lane as f32 - 1.0) * 12.0;
            *lane += 1;
            return Curve::Routed { a, b, below, reach: BACK_REACH };
        }
        let dx = ((b[0] - a[0]).abs() * 0.5).max(28.0);
        let cubic = Curve::Cubic([a, [a[0] + dx, a[1]], [b[0] - dx, b[1]], b]);
        // An S-curve that would cut through other nodes drops down a gutter instead.
        let obstacles = others(w);
        if crosses(&cubic.points(), &obstacles)
            && let Some(c) = gutter_route(a, b, w.from, &obstacles, &mut gutter_lanes)
        {
            return c;
        }
        cubic
    }).collect();
    let routes: Vec<Vec<[f32; 2]>> = curves.iter().map(|c| c.points()).collect();
    let boxes: Vec<[f32; 4]> = routes.iter().map(|r| bbox(r)).collect();
    // What shows behind a wire: the canvas, or an expanded frame's fill over it.
    let frames: Vec<[f32; 4]> = pl.frames.iter().filter(|f| f[2] > 0.0).map(|f| [f[0] + o[0], f[1] + o[1], f[0] + f[2] + o[0], f[1] + f[3] + o[1]]).collect();
    let framed = p.canvas.mix(p.frame_fill.alpha(1.0), p.frame_fill.a);
    let backdrop = |pt: [f32; 2]| {
        if frames.iter().any(|f| pt[0] > f[0] && pt[0] < f[2] && pt[1] > f[1] && pt[1] < f[3]) { framed } else { p.canvas }
    };

    // Faded wires first, then data, exec on top.
    let mut order: Vec<usize> = (0..g.wires.len()).collect();
    order.sort_by_key(|&i| (touches(&g.wires[i]), g.wire_ty(&g.wires[i]) == Ty::Exec));
    for i in order {
        let w = &g.wires[i];
        let ty = g.wire_ty(w);
        let exec = ty == Ty::Exec;
        let lit = touches(w);
        let emphasised = focus.is_some() && lit;
        let width = if exec { 2.8 } else { 1.8 } + if emphasised { 0.8 } else { 0.0 };
        let core = if lit { p.ty(ty) } else { p.dim_wire(p.ty(ty)) };
        // The outline under the core, if any: exec wires' casing; lit data wires' dark halo.
        // Mixed toward the canvas (opaque), never by alpha: translucent polylines double-blend
        // at their joints and read as dotted.
        let under = if exec {
            Some((1.0, if lit { p.exec_casing } else { p.exec_casing.mix(p.canvas, 0.6) }))
        } else if p.dark && lit && !w.back {
            Some((0.8, p.canvas.mix(Color::BLACK, 0.4)))
        } else {
            None
        };
        if w.back {
            // A loop's way back is dashed, so it never reads as forward flow.
            for dash in dashes(&routes[i], 9.0, 6.0) {
                if let Some((out, c)) = under {
                    s = s.stroke(dash.clone(), width + 2.0 * out, c);
                }
                s = s.stroke(dash, width, core);
            }
        } else {
            let path = curves[i].path();
            if let Some((out, c)) = under {
                s = s.stroke_path(&path, width + 2.0 * out, LineCap::Round, c);
            }
            s = s.stroke_path(&path, width, LineCap::Round, core);
        }
        // Where it still passes behind a node, mask a short stretch beside the node's edge.
        let mask = width + 2.0 * under.map_or(0.0, |u| u.0) + 2.0;
        for r in others(w) {
            for (at, entering) in rect_crossings(&routes[i], r) {
                let (s0, s1) = if entering { (at - BEHIND_GAP, at + 1.0) } else { (at - 1.0, at + BEHIND_GAP) };
                let piece = sub_polyline(&routes[i], s0, s1);
                if piece.len() > 1 {
                    let c = backdrop(piece[if entering { 0 } else { piece.len() - 1 }]);
                    s = s.stroke_with(piece, false, mask, LineCap::Butt, c);
                }
            }
        }
        if !wants_arrow(&routes[i]) {
            continue;
        }
        // One direction arrow along the route, clear of other wires: its outline, then its core.
        // Wires sharing a socket run together there, so they don't push each other's arrows.
        let bundled = |j: usize| g.wires[j].from == w.from || g.wires[j].to == w.to;
        let tri = arrow(arrow_spot(i, &routes, &boxes, bundled), width, exec);
        if let Some((out, c)) = under {
            s = s.fill(grow_triangle(tri, out).to_vec(), c);
        }
        s = s.fill(tri.to_vec(), core);
    }
    s
}

/// How far a routed (backedge or wrapped-row) wire swings out of and into its sockets.
const BACK_REACH: f32 = 26.0 * 1.4;
/// Spacing between the runs of nested loop backedges.
const BACK_LANE: f32 = 12.0;
/// How much wider each outer backedge's legs swing: a leg's widest point is ¾ of its reach out,
/// so this keeps nested legs `BACK_LANE` apart there too.
const BACK_LEG: f32 = BACK_LANE / 0.75;
/// A direction arrow (its tip, centre and base) keeps this far from every other wire, so it
/// never sits on a crossing.
const ARROW_CLEAR: f32 = 8.0;
/// How far an arrow's tip and base reach either side of its centre, about (see [`arrow`]).
const ARROW_TIP: f32 = 6.0;
const ARROW_BASE: f32 = 3.0;

/// Where a loop backedge's run would go on its own: under both its nodes and every node it
/// would cross, but above the next wrapped row.
fn back_below(pl: &crate::layout::Placed, o: [f32; 2], w: &model::Wire, a: [f32; 2], b: [f32; 2]) -> f32 {
    let bottom = |i: usize| pl.pos[i][1] + pl.size[i][1] + o[1];
    let (lo, hi) = (a[0].min(b[0]) - 40.0, a[0].max(b[0]) + 40.0);
    let limit = pl.row_gaps.iter().map(|g| g + o[1]).find(|&g| g > a[1]).unwrap_or(f32::MAX);
    let mut below = bottom(w.from.0).max(bottom(w.to.0)) + 22.0;
    loop {
        let deeper = (0..pl.pos.len())
            .filter(|&i| {
                let (x, y) = (pl.pos[i][0] + o[0], pl.pos[i][1] + o[1]);
                x < hi && x + pl.size[i][0] > lo && y < below && bottom(i) + 22.0 > below
            })
            .map(|i| bottom(i) + 22.0)
            .fold(below, f32::max);
        if deeper <= below || deeper >= limit {
            break below;
        }
        below = deeper;
    }
}

/// A loop backedge before lane assignment.
struct BackRun {
    /// Its source socket's height: the lower socket takes the inner lane, so nested runs
    /// never cross where they leave their sockets.
    socket_y: f32,
    /// Horizontal extent of the route, legs included.
    lo: f32,
    hi: f32,
    /// Where its run would go on its own.
    below: f32,
}

/// Gives loop backedges whose runs overlap separate lanes: each run sits at least `BACK_LANE`
/// from every overlapping run, and each outer one's legs swing `BACK_LEG` wider than the inner
/// ones', so a loop's exec and data ways back stay apart end to end. Returns `(below, reach)`
/// per run, in input order.
fn back_lanes(runs: &[BackRun]) -> Vec<(f32, f32)> {
    let mut order: Vec<usize> = (0..runs.len()).collect();
    order.sort_by(|&i, &j| runs[j].socket_y.total_cmp(&runs[i].socket_y).then(i.cmp(&j)));
    let mut out = vec![(0.0, BACK_REACH); runs.len()];
    let mut placed: Vec<usize> = vec![];
    for i in order {
        let r = &runs[i];
        let near: Vec<usize> = placed.iter().copied().filter(|&j| runs[j].lo < r.hi && r.lo < runs[j].hi).collect();
        let mut below = r.below;
        // Deeper until clear of every overlapping run; pushes only go down, so this ends.
        while let Some(&j) = near.iter().find(|&&j| (out[j].0 - below).abs() < BACK_LANE) {
            below = out[j].0 + BACK_LANE;
        }
        let reach = near.iter().filter(|&&j| out[j].0 < below).map(|&j| out[j].1 + BACK_LEG).fold(BACK_REACH, f32::max);
        out[i] = (below, reach);
        placed.push(i);
    }
    out
}

/// Point and unit tangent for wire `own`'s direction arrow: halfway along its route, unless
/// another (non-`bundled`) wire passes within `ARROW_CLEAR` of the arrow there, as at a
/// crossing; then the nearest clear spot among 40, 60, 30 and 70 % of the way.
fn arrow_spot(own: usize, routes: &[Vec<[f32; 2]>], boxes: &[[f32; 4]], bundled: impl Fn(usize) -> bool) -> ([f32; 2], [f32; 2]) {
    let clear = |(c, d): ([f32; 2], [f32; 2])| {
        let m = ARROW_CLEAR;
        let along = |k: f32| [c[0] + d[0] * k, c[1] + d[1] * k];
        let probes = [along(ARROW_TIP), c, along(-ARROW_BASE)];
        let reach = m + ARROW_TIP;
        (0..routes.len())
            .filter(|&j| j != own && !bundled(j))
            // Most wires are nowhere near: skip them on their bounding box.
            .filter(|&j| {
                let b = boxes[j];
                c[0] > b[0] - reach && c[0] < b[2] + reach && c[1] > b[1] - reach && c[1] < b[3] + reach
            })
            .all(|j| routes[j].windows(2).all(|s| probes.iter().all(|&p| seg_dist(p, s[0], s[1]) >= m)))
    };
    let spot = |f: f32| polyline_at(&routes[own], f);
    [0.5, 0.4, 0.6, 0.3, 0.7].into_iter().map(spot).find(|&s| clear(s)).unwrap_or_else(|| spot(0.5))
}

/// `[min x, min y, max x, max y]` of a polyline.
fn bbox(pts: &[[f32; 2]]) -> [f32; 4] {
    pts.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])])
}

/// Distance from `p` to the segment `a`–`b`.
fn seg_dist(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (d, v) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
    let l2 = d[0] * d[0] + d[1] * d[1];
    let t = if l2 > 0.0 { ((v[0] * d[0] + v[1] * d[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (v[0] - d[0] * t).hypot(v[1] - d[1] * t)
}

/// Point and unit tangent `frac` of the way along a polyline, by arc length.
fn polyline_at(pts: &[[f32; 2]], frac: f32) -> ([f32; 2], [f32; 2]) {
    let len = |s: &[[f32; 2]]| ((s[1][0] - s[0][0]).powi(2) + (s[1][1] - s[0][1]).powi(2)).sqrt();
    let total: f32 = pts.windows(2).map(len).sum();
    let mut left = total * frac;
    for seg in pts.windows(2) {
        let l = len(seg);
        if l > 0.0 && left <= l {
            let t = left / l;
            let d = [seg[1][0] - seg[0][0], seg[1][1] - seg[0][1]];
            return ([seg[0][0] + d[0] * t, seg[0][1] + d[1] * t], unit(d, [1.0, 0.0]));
        }
        left -= l;
    }
    (pts.last().copied().unwrap_or_default(), [1.0, 0.0])
}

/// `v` normalised, or `fallback` normalised when `v` is degenerate.
fn unit(v: [f32; 2], fallback: [f32; 2]) -> [f32; 2] {
    let n = v[0].hypot(v[1]);
    if n > 1e-4 {
        [v[0] / n, v[1] / n]
    } else if fallback[0].hypot(fallback[1]) > 1e-4 {
        unit(fallback, [1.0, 0.0])
    } else {
        [1.0, 0.0]
    }
}

/// A direction arrow centred on `mid`, pointing along its unit tangent, growing with the stroke:
/// `[tip, left base, right base]`. An exec arrow's core is barely larger than a data one's; its
/// casing makes the drawn exec arrow about 1.5 times a data arrow's area, so neither swamps the
/// wires around it.
fn arrow((c, d): ([f32; 2], [f32; 2]), width: f32, exec: bool) -> [[f32; 2]; 3] {
    let len = width * 1.5 + if exec { 3.9 } else { 6.3 };
    let half = len * 0.62;
    let n = [-d[1], d[0]];
    // Centre the triangle's centroid on the route, so it sits balanced on the curve.
    let tip = [c[0] + d[0] * len * 2.0 / 3.0, c[1] + d[1] * len * 2.0 / 3.0];
    let base = [c[0] - d[0] * len / 3.0, c[1] - d[1] * len / 3.0];
    [tip, [base[0] + n[0] * half, base[1] + n[1] * half], [base[0] - n[0] * half, base[1] - n[1] * half]]
}

/// `t` with every edge pushed `by` points outward (scaled about its incentre), so its outline
/// matches a stroke's casing of the same thickness.
fn grow_triangle(t: [[f32; 2]; 3], by: f32) -> [[f32; 2]; 3] {
    let d = |p: [f32; 2], q: [f32; 2]| (p[0] - q[0]).hypot(p[1] - q[1]);
    // Side lengths opposite each vertex weight the incentre.
    let (la, lb, lc) = (d(t[1], t[2]), d(t[0], t[2]), d(t[0], t[1]));
    let per = la + lb + lc;
    let area = ((t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[2][0] - t[0][0]) * (t[1][1] - t[0][1])).abs() / 2.0;
    if per <= 0.0 || area <= 0.0 {
        return t;
    }
    let ic = [(la * t[0][0] + lb * t[1][0] + lc * t[2][0]) / per, (la * t[0][1] + lb * t[1][1] + lc * t[2][1]) / per];
    let k = (2.0 * area / per + by) / (2.0 * area / per);
    t.map(|p| [ic[0] + (p[0] - ic[0]) * k, ic[1] + (p[1] - ic[1]) * k])
}

/// A cubic Bézier sampled at 17 evenly spaced `t`, appended to `out`.
fn cubic_points(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], out: &mut Vec<[f32; 2]>) {
    for k in 0..=16 {
        let t = k as f32 / 16.0;
        let u = 1.0 - t;
        let (c0, c1, c2, c3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        out.push([c0 * p0[0] + c1 * p1[0] + c2 * p2[0] + c3 * p3[0], c0 * p0[1] + c1 * p1[1] + c2 * p2[1] + c3 * p3[1]]);
    }
}

/// The routed (backedge or wrapped-row) path as a polyline: out of `a`, down to `below`,
/// across, and up into `b`, its legs swinging `reach` beside the sockets (the same curve as
/// [`Curve::path`]). The straight run is the segment between the two sampled curves.
fn back_points(a: [f32; 2], b: [f32; 2], below: f32, reach: f32) -> Vec<[f32; 2]> {
    let mut pts = vec![];
    cubic_points(a, [a[0] + reach, a[1]], [a[0] + reach, below], [a[0], below], &mut pts);
    cubic_points([b[0], below], [b[0] - reach, below], [b[0] - reach, b[1]], b, &mut pts);
    pts
}

/// Split a polyline into dashes of length `on` separated by `off`.
fn dashes(pts: &[[f32; 2]], on: f32, off: f32) -> Vec<Vec<[f32; 2]>> {
    let mut out = vec![];
    let mut cur = vec![];
    let (mut drawing, mut left) = (true, on);
    for seg in pts.windows(2) {
        let ([x0, y0], [x1, y1]) = (seg[0], seg[1]);
        let len = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
        let mut t = 0.0;
        if drawing && cur.is_empty() {
            cur.push([x0, y0]);
        }
        while len - t > left {
            t += left;
            let p = [x0 + (x1 - x0) * t / len, y0 + (y1 - y0) * t / len];
            if drawing {
                cur.push(p);
                out.push(std::mem::take(&mut cur));
            } else {
                cur.push(p);
            }
            drawing = !drawing;
            left = if drawing { on } else { off };
        }
        left -= len - t;
        if drawing {
            cur.push([x1, y1]);
        } else {
            cur.clear();
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// Headers that hold source code are set in monospace, so `&` and `&&`, `=` and `==` stay
/// distinct; the function's own Input/Output keep the UI face. `size`: see [`title_size`].
fn header_font(kind: Kind, size: f32) -> TextStyle {
    match kind {
        Kind::GroupIn | Kind::GroupOut => TextStyle::new(size, 650.0),
        _ => TextStyle::new(size, 600.0).family(rune_ui::Family::Named("JetBrains Mono, DejaVu Sans Mono")),
    }
}

pub fn kind_glyph(kind: Kind) -> &'static str {
    match kind {
        Kind::GroupIn => "⇥",
        Kind::GroupOut => "⇤",
        Kind::Local => "≔",
        Kind::Loop => "↻",
        Kind::Branch => "◇",
        Kind::Switch => "☰",
        Kind::Op => "ƒx",
        Kind::Call => "ƒ",
        Kind::Set => "←",
        Kind::Get => "↦",
        Kind::Group => "▣",
        Kind::Region => "≡",
        Kind::Case => "⤷",
        Kind::Type => "▤",
    }
}

/// Implementation language as a compact chip on a neutral (C) or rust (Rust) fill, legible on
/// any header colour; structural nodes have none.
pub fn lang_chip(p: &Pal, lang: Lang) -> El {
    let t = match lang {
        Lang::None => return el(),
        Lang::C => "C",
        Lang::Rust => "RS",
    };
    let (fill, fg) = p.chip(lang);
    row().h(CHIP_H).px(5.0).radius(4.0).fill(fill).border(1.0, Color::BLACK.alpha(0.2)).items_center()
        .child(text(t).font(TextStyle::new(9.5, 750.0).tracking(0.3)).color(fg))
}

fn compact(n: &Node) -> bool {
    n.ins.is_empty() && n.outs.len() == 1 && n.outs[0].label.is_empty()
}

/// Selection is the accent ring; hover is a neutral ring. Worker presence is drawn separately.
fn emphasis(p: &Pal, e: El, look: Look) -> El {
    if look.selected {
        e.outline(2.0, 3.0, p.accent)
    } else if look.hovered {
        e.outline(2.0, 3.0, p.muted.alpha(0.8))
    } else {
        e
    }
}

/// A view node. `expandable`: a collapsed node group, drawn as a stack of cards with an expand
/// button that (like a double-click) opens it into a frame in place.
fn node(p: &Pal, n: &Node, i: usize, [w, h]: [f32; 2], expandable: bool, look: Look) -> El {
    let head = p.header(n.kind.category());
    let interact = move |e: El| {
        e.on_press(move |s: &mut State| if !s.cam.take_panned() { s.select(Item::Node(i)) })
            .on_double_press(move |s: &mut State| s.open_node(i))
            .on_drag(pan)
            .on_hover_change(move |s: &mut State, on, _| s.hover(Item::Node(i), on))
            .cursor(Cursor::Pointer)
    };
    if compact(n) {
        // Constants and getters: a pill tinted with their family colour.
        let getter = n.kind == Kind::Get;
        let (fill, fg) = if getter {
            (p.node_body, p.node_text)
        } else if p.dark {
            (head.mix(p.node_body, 0.45), Color::WHITE)
        } else {
            (head.mix(Color::WHITE, 0.78), head.mix(Color::BLACK, 0.35))
        };
        let edge = if getter { (1.5, p.ty(n.outs[0].ty)) } else { (1.0, head.alpha(0.5)) };
        let card = row().size(w, h).px(10.0).gap(7.0).items_center().fill(fill).radius(h / 2.0)
            .border(edge.0, edge.1)
            .child(text(kind_glyph(n.kind)).font(TextStyle::new(11.0, 600.0)).color(fg.alpha(0.7)))
            .child(text(n.title.clone()).font(TextStyle::new(look.title, 600.0)).color(fg).single_line().truncate().grow(1.0).min_w(0.0))
            .child(socket(p, n.outs[0].ty, model::socket_local(n, w, true, 0)));
        return interact(decorate(p, emphasis(p, card, look), [w, h], h / 2.0, n.evidence, n.claim, look));
    }
    // Glyph in a fixed 16 px slot, then only the chips that exist: `model::size` budgets exactly
    // these widths and gaps, so titles that fit are never truncated. The last chip sits in the
    // node's corner, as far from the right edge as from the top, with a concentric radius.
    let chips = n.kind == Kind::Group || n.lang != Lang::None;
    let chip_r = concentric(RADIUS, NODE_CHIP_INSET, CHIP_H);
    // A node without pins (a field-less type) is its header alone, rounded all round.
    let pinless = n.ins.is_empty() && n.outs.is_empty();
    let pr = match (chips, expandable && n.evidence != Evidence::None) {
        (_, true) => NODE_CHIP_INSET + BADGE_CLEAR,
        (true, false) => NODE_CHIP_INSET,
        (false, false) => 10.0,
    };
    let bottom = if pinless { RADIUS } else { 0.0 };
    let mut header = row().h(model::HEADER).pl(10.0).pr(pr).gap(7.0).items_center().fill(head).corners([RADIUS, RADIUS, bottom, bottom])
        .child(el().w(16.0).center()
            .child(text(kind_glyph(n.kind)).font(TextStyle::new(11.5, 650.0)).color(Color::WHITE.alpha(0.75)).single_line()))
        .child(text(n.title.clone()).font(header_font(n.kind, look.title)).color(Color::WHITE).single_line().truncate().grow(1.0).min_w(0.0));
    if n.kind == Kind::Group {
        header = header.child(row().h(CHIP_H).px(5.0).radius(chip_r).fill(Color::BLACK.alpha(0.25)).items_center()
            .child(text(format!("▣ {}", n.members)).font(TextStyle::new(9.5, 700.0)).color(Color::WHITE)));
    }
    if n.lang != Lang::None {
        header = header.child(lang_chip(p, n.lang).radius(chip_r));
    }
    if expandable {
        header = header.child(group_button(Color::WHITE.alpha(0.95), Color::BLACK.alpha(0.25), true, look.badge_scale)
            .key(format!("expand-group-{i}"))
            .on_press(move |s: &mut State| s.expand_node(i)));
    }

    let mut body = col().py(model::BODY_PAD);
    for (o, ii) in model::rows(n) {
        body = body.child(match (o, ii) {
            (Some(o), Some(ii)) => row().h(ROW).items_center().justify_between()
                .child(socket_row(p, &n.ins[ii], false, look.labels)).child(socket_row(p, &n.outs[o], true, look.labels)),
            (Some(o), None) => socket_row(p, &n.outs[o], true, look.labels),
            (None, Some(ii)) => socket_row(p, &n.ins[ii], false, look.labels),
            (None, None) => unreachable!("layout rows hold a pin"),
        });
    }

    let mut card = col().w(w).fill(p.node_body).radius(RADIUS).border(1.0, p.node_border).child(header);
    if !pinless {
        card = card.child(body);
    }
    let stacked = n.kind == Kind::Group;
    // A stack casts one shadow, from its back plate.
    if !stacked {
        card = card.shadow(if p.dark { 4.0 } else { 1.0 }, if p.dark { 14.0 } else { 4.0 }, p.shadow);
    }
    for (pi, pin) in n.outs.iter().enumerate() {
        card = card.child(socket(p, pin.ty, model::socket_local(n, w, true, pi)));
    }
    for (pi, pin) in n.ins.iter().enumerate() {
        card = card.child(socket(p, pin.ty, model::socket_local(n, w, false, pi)));
    }
    let card = decorate(p, emphasis(p, card, look), [w, h], RADIUS, n.evidence, n.claim, look);
    if !stacked {
        return interact(card);
    }
    // A collapsed group is a stack of cards, as in UE's collapsed graphs: plates step down and
    // right behind its body, each a shade deeper, edged and topped in its header colour. The
    // first step clears the hover/selection ring around the body.
    let mut stack = el().size(w, h);
    for k in (1..=STACK_PLATES).rev() {
        let d = STACK_STEP * k as f32;
        let t = k as f32 / (STACK_PLATES as f32 + 1.0);
        let mut plate = el().size(w, h).absolute(d, d).radius(RADIUS)
            .fill(p.node_body.mix(p.canvas, t * 0.6)).border(1.5, head.mix(p.canvas, 0.15 + 0.3 * t))
            .child(el().size(w, model::HEADER).corners([RADIUS, RADIUS, 0.0, 0.0]).fill(head.mix(p.canvas, 0.25 + 0.25 * t)));
        if k == STACK_PLATES {
            plate = plate.shadow(if p.dark { 4.0 } else { 1.0 }, if p.dark { 14.0 } else { 4.0 }, p.shadow);
        }
        stack = stack.child(plate);
    }
    interact(stack.child(card.absolute(0.0, 0.0)))
}

/// A collapsed group's back plates: how many, and how far each steps down and right.
const STACK_PLATES: usize = 2;
const STACK_STEP: f32 = 6.0;

/// Evidence badge (top-right), worker avatar (top-left) and, for active work, a dashed ring in
/// the worker's colour. Badges keep a readable screen size when zoomed out.
fn decorate(p: &Pal, mut e: El, [w, h]: [f32; 2], radius: f32, evidence: Evidence, claim: Option<Claim>, look: Look) -> El {
    if let Some(c) = claim {
        if c.active && !look.selected && !look.hovered {
            let pad = 6.0;
            e = e.child(el().size(w + 2.0 * pad, h + 2.0 * pad).absolute(-pad, -pad).radius(radius + pad)
                .border(2.0, p.worker(c.worker)).border_dash(6.0, 4.0));
        }
        e = e.child(avatar(p, c.worker, c.active, 20.0).scale(look.badge_scale).absolute(-10.0, -10.0));
    }
    let badge = match evidence {
        Evidence::None => return e,
        Evidence::Accepted => ("✓", p.ok),
        Evidence::Rejected => ("!", p.bad),
    };
    e.child(el().size(18.0, 18.0).radius(9.0).fill(badge.1).border(2.0, p.canvas).center()
        .child(text(badge.0).font(TextStyle::new(10.5, 750.0)).color(if p.dark { p.canvas } else { Color::WHITE }))
        .scale(look.badge_scale)
        .absolute(w - 9.0, -9.0))
}

pub fn avatar(p: &Pal, worker: u8, active: bool, d: f32) -> El {
    let a = el().size(d, d).radius(d / 2.0).fill(p.worker(worker)).border(2.0, p.canvas).center()
        .child(text(worker.to_string()).font(TextStyle::new(d * 0.52, 700.0)).color(p.on_worker(worker)));
    if active { a } else { a.opacity(0.65) }
}

fn socket_row(p: &Pal, pin: &Pin, output: bool, labels: bool) -> El {
    let r = row().h(ROW).px(12.0).items_center();
    if !labels {
        return r;
    }
    let t = text(pin.label.clone()).font(SOCKET).color(p.node_text);
    if output { r.justify_end().child(t) } else { r.child(t) }
}

fn socket(p: &Pal, ty: Ty, [x, y]: [f32; 2]) -> El {
    match ty {
        Ty::Exec => {
            let s = 9.0;
            el().size(s, s).absolute(x - s / 2.0, y - s / 2.0).fill(p.exec).border(1.5, p.exec_casing).radius(1.5)
                .rotate(std::f32::consts::FRAC_PI_4)
        }
        _ => {
            let d = 11.0;
            let outline = if p.dark { Color::BLACK.alpha(0.7) } else { p.node_body };
            el().size(d, d).absolute(x - d / 2.0, y - d / 2.0).fill(p.ty(ty)).radius(d / 2.0).border(1.0, outline)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_chips_are_concentric_with_equal_insets() {
        // Node header: the chip is centred, so its top inset is half the header's slack; the
        // header's right padding matches it.
        assert_eq!(NODE_CHIP_INSET, 7.0);
        assert_eq!(concentric(RADIUS, NODE_CHIP_INSET, CHIP_H), RADIUS - NODE_CHIP_INSET);
        // Frame title: the radius left over is half the chip, a pill.
        assert_eq!(FRAME_CHIP_INSET, 8.0);
        assert_eq!(concentric(FRAME_RADIUS, FRAME_CHIP_INSET, CHIP_H), CHIP_H / 2.0);
    }

    #[test]
    fn concentric_radius_is_never_negative_or_past_a_pill() {
        assert_eq!(concentric(12.0, 4.0, 32.0), 8.0);
        assert_eq!(concentric(12.0, 16.0, 32.0), 0.0);
        assert_eq!(concentric(30.0, 2.0, 16.0), 8.0);
    }

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    #[test]
    fn a_wire_arrow_sits_mid_curve_along_its_tangent() {
        // An S-curve from (0,0) to (100,40) with horizontal handles is symmetric about its
        // middle, so halfway by length is the midpoint, heading steeper than the chord.
        let pts = Curve::Cubic([[0.0, 0.0], [50.0, 0.0], [50.0, 40.0], [100.0, 40.0]]).points();
        let (p, d) = polyline_at(&pts, 0.5);
        assert!((p[0] - 50.0).abs() < 1e-2 && (p[1] - 20.0).abs() < 1e-2, "{p:?}");
        assert!(d[1] / d[0] > 0.4, "{d:?}");
        // A straight wire points along itself.
        let (_, d) = polyline_at(&Curve::Cubic([[0.0, 0.0], [28.0, 0.0], [72.0, 0.0], [100.0, 0.0]]).points(), 0.5);
        assert!(close(d, [1.0, 0.0]));
    }

    #[test]
    fn polyline_at_measures_by_length() {
        // 30 down, then 100 back left: halfway (65) is 35 along the run, heading left.
        let pts = [[0.0, 0.0], [0.0, 30.0], [-100.0, 30.0]];
        let (p, d) = polyline_at(&pts, 0.5);
        assert!(close(p, [-35.0, 30.0]));
        assert!(close(d, [-1.0, 0.0]));
        let (p, d) = polyline_at(&pts, 0.1);
        assert!(close(p, [0.0, 13.0]));
        assert!(close(d, [0.0, 1.0]));
    }

    #[test]
    fn a_backedge_arrow_sits_on_its_straight_run_heading_back() {
        // Out of a source on the right, under both nodes, back into a target on the left.
        let (a, b, below) = ([400.0, 100.0], [100.0, 120.0], 300.0);
        let (p, d) = polyline_at(&back_points(a, b, below, BACK_REACH), 0.5);
        assert!((p[1] - below).abs() < 1e-3 && p[0] > b[0] && p[0] < a[0], "{p:?}");
        assert!(close(d, [-1.0, 0.0]));
    }

    #[test]
    fn overlapping_backedges_get_separate_lanes() {
        // A loop's exec and data ways back: same span, same natural depth; the data socket sits
        // lower at the source.
        let run = |socket_y: f32, lo: f32, hi: f32| BackRun { socket_y, lo, hi, below: 685.0 };
        let lanes = back_lanes(&[run(340.0, 500.0, 1100.0), run(365.0, 500.0, 1100.0)]);
        let ((exec_below, exec_reach), (data_below, data_reach)) = (lanes[0], lanes[1]);
        // The lower socket keeps the inner lane; the other runs deeper and swings wider.
        assert_eq!((data_below, data_reach), (685.0, BACK_REACH));
        assert!(exec_below - data_below >= 8.0, "runs {exec_below} / {data_below}");
        // Their legs, at their widest, and their arrows, halfway along each, are as far apart.
        let (a, b) = ([1100.0, 340.0], [530.0, 575.0]);
        let exec = back_points(a, b, exec_below, exec_reach);
        let data = back_points([a[0], 365.0], [b[0], 645.0], data_below, data_reach);
        let widest = |pts: &[[f32; 2]]| pts.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
        assert!(widest(&exec) - widest(&data) >= 8.0, "legs {} / {}", widest(&exec), widest(&data));
        let (ea, da) = (polyline_at(&exec, 0.5).0, polyline_at(&data, 0.5).0);
        assert!((ea[1] - da[1]).abs() >= 8.0, "{ea:?} {da:?}");

        // A third run nested under both stacks again; a run elsewhere keeps its own depth.
        let lanes = back_lanes(&[run(340.0, 500.0, 1100.0), run(365.0, 500.0, 1100.0), run(300.0, 600.0, 900.0), run(340.0, 1200.0, 1500.0)]);
        assert!(lanes[2].0 - lanes[0].0 >= BACK_LANE && lanes[2].1 > lanes[0].1, "{lanes:?}");
        assert_eq!(lanes[3], (685.0, BACK_REACH));
    }

    #[test]
    fn an_arrow_slides_off_a_crossing() {
        // A horizontal wire crossed just past its middle by a vertical one: its arrow (tip
        // included) moves back to 40 %.
        let routes = vec![vec![[0.0, 0.0], [100.0, 0.0]], vec![[55.0, -50.0], [55.0, 50.0]]];
        let boxes: Vec<_> = routes.iter().map(|r| bbox(r)).collect();
        let (p, d) = arrow_spot(0, &routes, &boxes, |_| false);
        assert!(close(p, [40.0, 0.0]) && close(d, [1.0, 0.0]), "{p:?}");
        // Unless the two are bundled (sharing a socket), or nothing is near.
        assert!(close(arrow_spot(0, &routes, &boxes, |_| true).0, [50.0, 0.0]));
        let far = vec![routes[0].clone(), vec![[55.0, 20.0], [55.0, 50.0]]];
        let boxes: Vec<_> = far.iter().map(|r| bbox(r)).collect();
        assert!(close(arrow_spot(0, &far, &boxes, |_| false).0, [50.0, 0.0]));
    }

    #[test]
    fn arrow_points_along_the_tangent_centred_on_the_route() {
        let d = unit([3.0, 4.0], [1.0, 0.0]);
        let [tip, l, r] = arrow(([10.0, 10.0], d), 2.8, true);
        let centroid = [(tip[0] + l[0] + r[0]) / 3.0, (tip[1] + l[1] + r[1]) / 3.0];
        assert!(close(centroid, [10.0, 10.0]));
        let base = [(l[0] + r[0]) / 2.0, (l[1] + r[1]) / 2.0];
        assert!(close(unit([tip[0] - base[0], tip[1] - base[1]], [0.0, 0.0]), d));
        // Both grow with the stroke.
        let size = |w: f32, exec: bool| {
            let [t, l, _] = arrow(([0.0, 0.0], [1.0, 0.0]), w, exec);
            t[0] - l[0]
        };
        assert!(size(2.6, false) > size(1.8, false));
        // Drawn, an exec arrow (core plus its 1 pt casing) is about 1.5 times a data arrow's
        // area, at rest and emphasised: larger, not a blot over the wires around it.
        let area = |t: [[f32; 2]; 3]| ((t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[2][0] - t[0][0]) * (t[1][1] - t[0][1])).abs() / 2.0;
        for extra in [0.0, 0.8] {
            let exec = area(grow_triangle(arrow(([0.0, 0.0], [1.0, 0.0]), 2.8 + extra, true), 1.0));
            let data = area(arrow(([0.0, 0.0], [1.0, 0.0]), 1.8 + extra, false));
            assert!((1.3..=1.7).contains(&(exec / data)), "exec:data {}", exec / data);
        }
    }

    #[test]
    fn a_wire_past_a_middle_column_node_drops_down_the_gutter() {
        // Three columns, 80 apart: a source at the top of the first, a target at the bottom of
        // the third, and a node in the middle column between their rows.
        let a = [100.0, 40.0];
        let b = [360.0, 300.0];
        let middle = [180.0, 100.0, 280.0, 220.0];
        let (dx, rects) = ((b[0] - a[0]) * 0.5, [middle]);
        // The plain S-curve cuts through it.
        assert!(crosses(&Curve::Cubic([a, [a[0] + dx, a[1]], [b[0] - dx, b[1]], b]).points(), &rects));
        let route = gutter_route(a, b, (0, 0), &rects, &mut GutterLanes::new()).expect("a free gutter");
        let pts = route.points();
        assert!(!crosses(&pts, &[pad(middle, NODE_CLEAR - 0.5)]), "{pts:?}");
        assert!(close(pts[0], a) && close(*pts.last().unwrap(), b));
        // Its vertical travel sits in a gutter: left or right of the middle column.
        let Curve::Gutter { x, .. } = route else { panic!("gutter route") };
        assert!(x < middle[0] - NODE_CLEAR || x > middle[2] + NODE_CLEAR, "{x}");

        // A second wire in the same gutter takes its own lane; one from the same socket shares.
        let mut lanes = GutterLanes::new();
        let run = |c: Curve| match c {
            Curve::Gutter { x, .. } => x,
            _ => panic!("gutter route"),
        };
        let x0 = run(gutter_route(a, b, (0, 0), &rects, &mut lanes).unwrap());
        let x1 = run(gutter_route([a[0], 50.0], [b[0], 310.0], (1, 0), &rects, &mut lanes).unwrap());
        let x2 = run(gutter_route(a, [b[0], 320.0], (0, 0), &rects, &mut lanes).unwrap());
        assert!((x0 - x1).abs() >= GUTTER_LANE - 1e-3 && x2 == x0, "{x0} {x1} {x2}");

        // With the gutters walled in too, there is no clear route.
        let walls = [middle, [120.0, 0.0, 180.0, 400.0], [280.0, 0.0, 340.0, 400.0]];
        assert!(gutter_route(a, b, (0, 0), &walls, &mut GutterLanes::new()).is_none());
    }

    #[test]
    fn a_wire_behind_a_node_is_masked_where_it_enters_and_leaves() {
        let line = [[0.0, 50.0], [200.0, 50.0]];
        let hits = rect_crossings(&line, [80.0, 0.0, 120.0, 100.0]);
        assert_eq!(hits.len(), 2);
        assert!((hits[0].0 - 80.0).abs() < 0.01 && hits[0].1, "{hits:?}");
        assert!((hits[1].0 - 120.0).abs() < 0.01 && !hits[1].1, "{hits:?}");
        let piece = sub_polyline(&line, hits[0].0 - BEHIND_GAP, hits[0].0 + 1.0);
        assert!(close(piece[0], [80.0 - BEHIND_GAP, 50.0]) && close(*piece.last().unwrap(), [81.0, 50.0]), "{piece:?}");
        // A wire that only touches an edge neither crosses nor gets masked.
        assert!(rect_crossings(&[[0.0, 0.0], [200.0, 0.0]], [80.0, 0.0, 120.0, 100.0]).is_empty());
        assert!(!crosses(&[[0.0, 0.0], [200.0, 0.0]], &[[80.0, 0.0, 120.0, 100.0]]));
    }

    #[test]
    fn short_hops_get_no_direction_arrow() {
        // Adjacent nodes: `start → run` straight across one 80-point column gap, or with a
        // small step.
        for drop in [0.0, 12.0] {
            let hop = Curve::Cubic([[0.0, 0.0], [40.0, 0.0], [40.0, drop], [80.0, drop]]).points();
            assert!(!wants_arrow(&hop), "{drop}");
        }
        let long = Curve::Cubic([[0.0, 0.0], [100.0, 0.0], [100.0, 120.0], [200.0, 120.0]]).points();
        assert!(wants_arrow(&long));
    }

    #[test]
    fn zoomed_out_titles_keep_a_readable_screen_size() {
        assert_eq!(title_size(1.0), TITLE_SIZE);
        assert_eq!(title_size(0.9), TITLE_SIZE);
        // At the fit floor the title is counter-scaled to the minimum on screen.
        for zoom in [0.72, 0.8] {
            assert!((title_size(zoom) * zoom - TITLE_MIN_SCREEN).abs() < 1e-3, "{zoom}");
        }
        // Far out it stops growing, so it still fits its header.
        assert_eq!(title_size(0.2), TITLE_MAX);
        const { assert!(TITLE_MAX < model::HEADER - 8.0) };
    }

    #[test]
    fn grown_triangle_moves_every_edge_out_by_the_casing() {
        let t = arrow(([0.0, 0.0], [1.0, 0.0]), 2.8, true);
        let g = grow_triangle(t, 1.0);
        // Each grown edge runs parallel to its original, one casing width away.
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let n = unit([b[1] - a[1], a[0] - b[0]], [1.0, 0.0]);
            let off = (g[k][0] - a[0]) * n[0] + (g[k][1] - a[1]) * n[1];
            assert!((off.abs() - 1.0).abs() < 1e-3, "edge {k}: {off}");
        }
    }
}
