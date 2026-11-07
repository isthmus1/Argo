//! Read-only node-graph canvas: grid, expanded-group frames, Bézier wires and nodes.
//! State is shown with colour, badges and worker avatars, not labels.

use crate::mock::{self, Claim, Evidence, FRAMES, Kind, NODES, Node, Pin, ROW, SELECTED_FRAME, Ty, WIRES};
use crate::palette::Pal;
use rune_ui::{Color, El, LineCap, Path, Shape, TextStyle, canvas, col, el, row, text};

const SOCKET: TextStyle = TextStyle::new(11.0, 450.0);
const RADIUS: f32 = 8.0;

/// The canvas, `w`×`h` points, with the graph offset by `origin`.
pub fn view(p: &Pal, w: f32, h: f32, origin: [f32; 2]) -> El {
    let mut layers = vec![canvas(grid(p, w, h)).size(w, h).absolute(0.0, 0.0)];
    for (i, f) in FRAMES.iter().enumerate() {
        layers.push(frame(p, f, i == SELECTED_FRAME).absolute(origin[0] + f.x, origin[1] + f.y));
    }
    layers.push(canvas(wires(p, origin)).size(w, h).absolute(0.0, 0.0));
    for n in NODES {
        layers.push(node(p, n).absolute(origin[0] + n.x, origin[1] + n.y));
    }
    el().size(w, h).fill(p.canvas).clip_rounded(0.0).children(layers)
}

fn grid(p: &Pal, w: f32, h: f32) -> Shape {
    let mut s = Shape::new();
    let step = 24.0;
    for i in 0..=(w / step) as i32 + 1 {
        let x = i as f32 * step;
        s = s.line([x, 0.0], [x, h], 1.0, LineCap::Butt, if i % 5 == 0 { p.grid_major } else { p.grid_minor });
    }
    for j in 0..=(h / step) as i32 + 1 {
        let y = j as f32 * step;
        s = s.line([0.0, y], [w, y], 1.0, LineCap::Butt, if j % 5 == 0 { p.grid_major } else { p.grid_minor });
    }
    s
}

fn frame(p: &Pal, f: &mock::Frame, selected: bool) -> El {
    let c = p.lang(f.lang);
    let title = row().h(30.0).px(12.0).gap(8.0).items_center()
        .child(el().size(8.0, 8.0).radius(4.0).fill(c))
        .child(text(f.title).font(TextStyle::new(12.0, 600.0)).color(p.frame_text));
    let mut fr = el().size(f.w, f.h).fill(p.frame_fill).radius(12.0)
        .border(if selected { 2.0 } else { 1.0 }, if selected { p.accent } else { c.alpha(0.35) })
        .child(title.absolute(0.0, 0.0));
    fr = decorate(p, fr, f.w, f.evidence, f.claim);
    fr
}

fn wires(p: &Pal, o: [f32; 2]) -> Shape {
    let mut s = Shape::new();
    for w in WIRES {
        let a = mock::socket_world(w.from.0, true, w.from.1);
        let b = mock::socket_world(w.to.0, false, w.to.1);
        let (a, b) = ([a[0] + o[0], a[1] + o[1]], [b[0] + o[0], b[1] + o[1]]);
        let dx = ((b[0] - a[0]).abs() * 0.5).max(28.0);
        let path = Path::new().move_to(a).cubic_to([a[0] + dx, a[1]], [b[0] - dx, b[1]], b);
        let (width, color) = match mock::wire_ty(w) {
            Ty::Exec => (2.6, p.exec.alpha(0.9)),
            Ty::U32 => (1.7, p.u32_),
        };
        if p.dark {
            s = s.stroke_path(&path, width + 1.6, LineCap::Round, Color::BLACK.alpha(0.35));
        }
        s = s.stroke_path(&path, width, LineCap::Round, color);
    }
    s
}

fn kind_glyph(kind: Kind) -> &'static str {
    match kind {
        Kind::GroupIn => "⇥",
        Kind::GroupOut => "⇤",
        Kind::Local => "≔",
        Kind::Loop => "↻",
        Kind::Op => "+",
        Kind::Call => "ƒ",
        Kind::Set => "←",
    }
}

fn node(p: &Pal, n: &Node) -> El {
    let stripe = p.lang(n.lang);
    let header = col()
        .child(el().h(3.0).w_full().fill(stripe).corners([RADIUS, RADIUS, 0.0, 0.0]))
        .child(row().h(mock::HEADER - 3.0).px(10.0).gap(7.0).items_center()
            .child(text(kind_glyph(n.kind)).font(TextStyle::new(12.0, 600.0)).color(p.node_muted))
            .child(text(n.title).font(TextStyle::new(11.5, 600.0)).color(p.node_text).single_line()));

    let mut body = col().py(mock::BODY_PAD);
    for (o, i) in mock::layout(n) {
        body = body.child(match (o, i) {
            (Some(o), Some(i)) => row().h(ROW).items_center().justify_between()
                .child(socket_row(p, &n.ins[i], false)).child(socket_row(p, &n.outs[o], true)),
            (Some(o), None) => socket_row(p, &n.outs[o], true),
            (None, Some(i)) => socket_row(p, &n.ins[i], false),
            (None, None) => unreachable!("layout rows hold a pin"),
        });
    }

    let mut card = col().w(n.w).fill(p.node_body).radius(RADIUS).border(1.0, p.node_border)
        .child(header).child(body)
        .shadow(if p.dark { 4.0 } else { 2.0 }, if p.dark { 14.0 } else { 8.0 }, p.shadow);
    card = card.shadow(10.0, 28.0, p.shadow.alpha(if p.dark { 0.35 } else { 0.10 }));
    for (i, pin) in n.outs.iter().enumerate() {
        card = card.child(socket(p, pin.ty, mock::socket_local(n, true, i)));
    }
    for (i, pin) in n.ins.iter().enumerate() {
        card = card.child(socket(p, pin.ty, mock::socket_local(n, false, i)));
    }
    decorate(p, card, n.w, n.evidence, n.claim)
}

/// Evidence badge (top-right) and worker claim (avatar top-left; active work rings the item).
fn decorate(p: &Pal, mut e: El, w: f32, evidence: Evidence, claim: Option<Claim>) -> El {
    if let Some(c) = claim {
        let wc = p.worker(c.worker);
        if c.active {
            e = e.outline(2.0, 3.0, wc);
        }
        let avatar = el().size(20.0, 20.0).radius(10.0).fill(wc).border(2.0, p.canvas).center()
            .child(text(c.worker.to_string()).font(TextStyle::new(10.5, 700.0)).color(Color::WHITE))
            .absolute(-10.0, -10.0);
        e = e.child(if c.active { avatar } else { avatar.opacity(0.6) });
    }
    let badge = match evidence {
        Evidence::None => return e,
        Evidence::Accepted => ("✓", p.ok),
        Evidence::Rejected => ("!", p.bad),
    };
    e.child(el().size(18.0, 18.0).radius(9.0).fill(badge.1).border(2.0, p.canvas).center()
        .child(text(badge.0).font(TextStyle::new(10.5, 700.0)).color(Color::WHITE))
        .absolute(w - 9.0, -9.0))
}

fn socket_row(p: &Pal, pin: &Pin, output: bool) -> El {
    let r = row().h(ROW).px(12.0).items_center();
    let t = text(pin.label).font(SOCKET).color(p.node_text);
    if output { r.justify_end().child(t) } else { r.child(t) }
}

fn socket(p: &Pal, ty: Ty, [x, y]: [f32; 2]) -> El {
    let outline = if p.dark { Color::BLACK.alpha(0.7) } else { p.node_body };
    match ty {
        Ty::Exec => {
            let s = 9.0;
            el().size(s, s).absolute(x - s / 2.0, y - s / 2.0).fill(p.exec).border(1.0, outline).radius(1.5)
                .rotate(std::f32::consts::FRAC_PI_4)
        }
        Ty::U32 => {
            let d = 11.0;
            el().size(d, d).absolute(x - d / 2.0, y - d / 2.0).fill(p.ty(ty)).radius(d / 2.0).border(1.0, outline)
        }
    }
}
