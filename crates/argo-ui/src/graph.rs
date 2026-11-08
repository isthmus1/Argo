//! Read-only node-graph canvas: grid, expanded-group frames, Bézier wires and nodes, under a
//! pan/zoom camera. State is shown with colour, chips, badges and worker avatars.

use crate::app::{Item, MARGIN, State};
use crate::model::{self, Claim, Evidence, Graph, Kind, Lang, Node, Pin, ROW, Ty};
use crate::palette::Pal;
use rune_ui::{Color, Cursor, El, LineCap, Path, Phase, Shape, TextStyle, canvas, col, el, row, text};

const SOCKET: TextStyle = TextStyle::new(11.0, 450.0);
/// Matches the floating panels' corner radius.
pub const RADIUS: f32 = 12.0;
/// Below this zoom, pin labels are hidden: they would be unreadable noise.
const LABEL_ZOOM: f32 = 0.55;

/// How an item is emphasised on the canvas.
#[derive(Clone, Copy)]
struct Look {
    selected: bool,
    hovered: bool,
    /// Screen-size compensation for badges and avatars at low zoom.
    badge_scale: f32,
    labels: bool,
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
        world.push(node(p, n, i, pl.size[i], look(Item::Node(i))).absolute(o[0] + pl.pos[i][0], o[1] + pl.pos[i][1]));
    }
    let world = el().size(ww, wh).scale(s.cam.zoom).origin(0.0, 0.0).children(world).absolute(s.cam.pan[0], s.cam.pan[1]);

    let (sx, sy) = (s.cam.pan[0], s.cam.pan[1]);
    let (ex, ey) = (sx + ww * s.cam.zoom, sy + wh * s.cam.zoom);
    let fade = |more: bool| if more { 56.0 } else { 0.0 };
    el().key("canvas").size(w, h).fill(p.canvas).clip_fade(fade(sx < 0.0), fade(sy < 0.0), fade(ex > w), fade(ey > h)).cursor(Cursor::Grab)
        .on_drag(|s: &mut State, d, _| {
            match d.phase {
                Phase::Down => s.cam.drag_from = s.cam.pan,
                Phase::Move | Phase::Up => s.cam.pan = [s.cam.drag_from[0] + d.dx, s.cam.drag_from[1] + d.dy],
            }
        })
        .on_wheel(|s: &mut State, e, _| {
            s.cam.zoom_about(e.x, e.y, (-e.dy * 0.0025).exp());
            true
        })
        .on_magnify(|s: &mut State, e, _| {
            s.cam.zoom_about(e.x, e.y, 1.0 + e.delta);
            true
        })
        .on_press(|s: &mut State| s.selected = None)
        .child(canvas(grid(p, s, w, h)).size(w, h).absolute(0.0, 0.0))
        .child(world)
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

fn frame(p: &Pal, f: &model::Group, gi: usize, w: f32, h: f32, look: Look) -> El {
    let title = row().h(32.0).w(w).px(14.0).gap(8.0).items_center()
        .child(text("▣").font(TextStyle::new(12.0, 650.0)).color(p.frame_text.alpha(0.7)))
        .child(text(f.title.clone()).font(TextStyle::new(13.0, 650.0)).color(p.frame_text).single_line().truncate().grow(1.0).min_w(0.0))
        .child(lang_chip(p, f.lang));
    let border = if look.selected { (2.0, p.accent) } else if look.hovered { (1.5, p.muted) } else { (1.5, p.frame_text.alpha(0.22)) };
    let fr = el().size(w, h).fill(p.frame_fill).radius(RADIUS + 4.0)
        .border(border.0, border.1)
        .child(title.absolute(0.0, 0.0))
        .on_press(move |s: &mut State| s.select(Item::Group(gi)))
        .on_double_press(move |s: &mut State| s.toggle_group(gi))
        .on_hover_change(move |s: &mut State, on, _| s.hover(Item::Group(gi), on))
        .help("Double-click to collapse");
    decorate(p, fr, [w, h], RADIUS + 4.0, f.evidence, f.claim, look)
}

fn socket_world(g: &Graph, pl: &crate::layout::Placed, i: usize, output: bool, idx: usize) -> [f32; 2] {
    let [x, y] = model::socket_local(&g.nodes[i], pl.size[i][0], output, idx);
    [pl.pos[i][0] + x, pl.pos[i][1] + y]
}

fn wires(p: &Pal, g: &Graph, pl: &crate::layout::Placed, o: [f32; 2], focus: Option<Item>) -> Shape {
    let mut s = Shape::new();
    let touches = |w: &model::Wire| match focus {
        Some(Item::Node(i)) => w.from.0 == i || w.to.0 == i,
        Some(Item::Group(gi)) => g.nodes[w.from.0].group == Some(gi) || g.nodes[w.to.0].group == Some(gi),
        None => true,
    };
    // Faded wires first, then data, exec on top.
    let mut gap_lanes = std::collections::HashMap::<usize, usize>::new();
    let mut order: Vec<_> = g.wires.iter().collect();
    order.sort_by_key(|w| (touches(w), g.wire_ty(w) == Ty::Exec));
    for w in order {
        let a = socket_world(g, pl, w.from.0, true, w.from.1);
        let b = socket_world(g, pl, w.to.0, false, w.to.1);
        let (a, b) = ([a[0] + o[0], a[1] + o[1]], [b[0] + o[0], b[1] + o[1]]);
        // Wrapped rows: a wire into a later row runs back through the gap between the rows.
        let wrap_gap = (b[0] < a[0] && !w.back)
            .then(|| pl.row_gaps.iter().position(|&g| g + o[1] > a[1] && g + o[1] < b[1]))
            .flatten()
            .map(|gi| {
                // Each wire in a gap gets its own lane, so parallel runs don't merge into one line.
                let lane = gap_lanes.entry(gi).or_insert(0usize);
                let y = pl.row_gaps[gi] + o[1] + (*lane as f32 - 1.0) * 12.0;
                *lane += 1;
                y
            });
        let mut path_below = None;
        let path = if w.back || wrap_gap.is_some() {
            // A loop backedge runs out of its source, under both nodes, and back into the target.
            let bottom = |i: usize| pl.pos[i][1] + pl.size[i][1] + o[1];
            let below = wrap_gap.unwrap_or_else(|| {
                // Drop under every node the run would cross, but stay above the next row.
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
            });
            path_below = Some(below);
            let r = 26.0;
            Path::new()
                .move_to(a)
                .cubic_to([a[0] + r * 1.4, a[1]], [a[0] + r * 1.4, below], [a[0], below])
                .line_to([b[0], below])
                .cubic_to([b[0] - r * 1.4, below], [b[0] - r * 1.4, b[1]], b)
        } else {
            let dx = ((b[0] - a[0]).abs() * 0.5).max(28.0);
            Path::new().move_to(a).cubic_to([a[0] + dx, a[1]], [b[0] - dx, b[1]], b)
        };
        let ty = g.wire_ty(w);
        let exec = ty == Ty::Exec;
        let lit = touches(w);
        let emphasised = focus.is_some() && lit;
        let width = if exec { 2.8 } else { 1.8 } + if emphasised { 0.8 } else { 0.0 };
        // Fade by mixing toward the canvas (opaque), never by alpha: translucent polylines
        // double-blend at their joints and read as dotted.
        let fade = |c: Color| if lit { c } else { c.mix(p.canvas, 0.6) };
        let core = fade(p.ty(ty));
        if w.back {
            // A loop's way back is dashed, so it never reads as forward flow.
            for dash in dashes(&back_points(a, b, path_below.unwrap_or_default()), 9.0, 6.0) {
                if exec {
                    s = s.stroke(dash.clone(), width + 2.0, fade(p.exec_casing));
                }
                s = s.stroke(dash, width, core);
            }
        } else {
            if exec {
                s = s.stroke_path(&path, width + 2.0, LineCap::Round, fade(p.exec_casing));
            } else if p.dark && lit {
                s = s.stroke_path(&path, width + 1.6, LineCap::Round, p.canvas.mix(Color::BLACK, 0.4));
            }
            s = s.stroke_path(&path, width, LineCap::Round, core);
        }
        // Arrowhead just before the target socket: direction never depends on layout.
        let (tip, back, half) = (b[0] - 7.0, b[0] - 15.0, if exec { 5.0 } else { 4.0 });
        s = s.fill(vec![[back, b[1] - half], [tip, b[1]], [back, b[1] + half]], core);
    }
    s
}

/// The backedge route as a polyline: out of `a`, down to `below`, across, and up into `b`
/// (the same curve as the solid route, sampled so it can be dashed).
fn back_points(a: [f32; 2], b: [f32; 2], below: f32) -> Vec<[f32; 2]> {
    let r = 26.0 * 1.4;
    let cubic = |p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], out: &mut Vec<[f32; 2]>| {
        for k in 0..=16 {
            let t = k as f32 / 16.0;
            let u = 1.0 - t;
            let (c0, c1, c2, c3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            out.push([
                c0 * p0[0] + c1 * p1[0] + c2 * p2[0] + c3 * p3[0],
                c0 * p0[1] + c1 * p1[1] + c2 * p2[1] + c3 * p3[1],
            ]);
        }
    };
    let mut pts = vec![];
    cubic(a, [a[0] + r, a[1]], [a[0] + r, below], [a[0], below], &mut pts);
    cubic([b[0], below], [b[0] - r, below], [b[0] - r, b[1]], b, &mut pts);
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
/// distinct; the function's own Input/Output keep the UI face.
fn header_font(kind: Kind) -> TextStyle {
    match kind {
        Kind::GroupIn | Kind::GroupOut => TextStyle::new(11.5, 650.0),
        _ => TextStyle::new(11.5, 600.0).family(rune_ui::Family::Named("JetBrains Mono, DejaVu Sans Mono")),
    }
}

pub fn kind_glyph(kind: Kind) -> &'static str {
    match kind {
        Kind::GroupIn => "⇥",
        Kind::GroupOut => "⇤",
        Kind::Local => "≔",
        Kind::Loop => "↻",
        Kind::Branch => "⑂",
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
    row().h(16.0).px(5.0).radius(4.0).fill(fill).border(1.0, Color::BLACK.alpha(0.2)).items_center()
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

fn node(p: &Pal, n: &Node, i: usize, [w, h]: [f32; 2], look: Look) -> El {
    let head = p.header(n.kind.category());
    let interact = move |e: El| {
        e.on_press(move |s: &mut State| s.select(Item::Node(i)))
            .on_double_press(move |s: &mut State| s.open_node(i))
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
            .child(text(n.title.clone()).font(TextStyle::new(11.5, 600.0)).color(fg).single_line())
            .child(socket(p, n.outs[0].ty, model::socket_local(n, w, true, 0)));
        return interact(decorate(p, emphasis(p, card, look), [w, h], h / 2.0, n.evidence, n.claim, look));
    }
    let header = row().h(model::HEADER).px(10.0).gap(7.0).items_center().fill(head).corners([RADIUS, RADIUS, 0.0, 0.0])
        .child(text(kind_glyph(n.kind)).font(TextStyle::new(11.5, 650.0)).color(Color::WHITE.alpha(0.75)))
        .child(text(n.title.clone()).font(header_font(n.kind)).color(Color::WHITE).single_line().truncate().grow(1.0).min_w(0.0))
        .child(if n.kind == Kind::Group {
            row().h(16.0).px(5.0).radius(4.0).fill(Color::BLACK.alpha(0.25)).items_center()
                .child(text(format!("▣ {}", n.members)).font(TextStyle::new(9.5, 700.0)).color(Color::WHITE))
        } else {
            el()
        })
        .child(lang_chip(p, n.lang));

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

    let mut card = col().w(w).fill(p.node_body).radius(RADIUS).border(1.0, p.node_border)
        .child(header).child(body)
        .shadow(if p.dark { 4.0 } else { 1.0 }, if p.dark { 14.0 } else { 4.0 }, p.shadow);
    if n.kind == Kind::Group {
        // A collapsed group reads as a stack of cards.
        card = card.box_shadow(rune_ui::Shadow { x: 5.0, y: 5.0, blur: 0.0, spread: 0.0, color: p.node_border, inset: false });
    }
    for (pi, pin) in n.outs.iter().enumerate() {
        card = card.child(socket(p, pin.ty, model::socket_local(n, w, true, pi)));
    }
    for (pi, pin) in n.ins.iter().enumerate() {
        card = card.child(socket(p, pin.ty, model::socket_local(n, w, false, pi)));
    }
    interact(decorate(p, emphasis(p, card, look), [w, h], RADIUS, n.evidence, n.claim, look))
}

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
