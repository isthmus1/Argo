//! A small passive overview of the whole graph, shown at the bottom-left of the free canvas
//! area when the graph runs past it: nodes as tiny muted blocks, the visible part as an
//! outline. Pressing or dragging on it centres the camera there. Navigation only, no text.

use crate::app::{MARGIN, State};
use crate::palette::Pal;
use crate::ui;
use rune_ui::{Drag, El, Phase, Shape, Tx, canvas};

/// Largest size of the overview card, in points.
const W: f32 = 160.0;
const H: f32 = 100.0;
/// Gap between the card's edge and the graph drawn in it.
const INNER: f32 = 6.0;

/// Where the world (the laid-out graph with its margin) sits inside the card: offset and scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Map {
    pub size: [f32; 2],
    pub at: [f32; 2],
    pub k: f32,
}

/// The overview's card size and world mapping, or `None` when nothing is laid out.
pub fn map(s: &State) -> Option<Map> {
    let [_, _, bw, bh] = s.placed.bounds;
    let (ww, wh) = (bw + 2.0 * MARGIN, bh + 2.0 * MARGIN);
    if s.placed.pos.is_empty() || ww <= 0.0 || wh <= 0.0 {
        return None;
    }
    let k = ((W - 2.0 * INNER) / ww).min((H - 2.0 * INNER) / wh);
    let size = [ww * k + 2.0 * INNER, wh * k + 2.0 * INNER];
    Some(Map { size, at: [INNER, INNER], k })
}

/// The card's screen rectangle, at the free canvas area's bottom-left, whether or not it shows.
pub fn rect(s: &State) -> Option<[f32; 4]> {
    let m = map(s)?;
    let [vx, vy, _, vh] = ui::free_area(s.selected.is_some(), s.project.is_some());
    Some([vx, vy + vh - m.size[1], m.size[0], m.size[1]])
}

/// Whether some of the graph lies outside the free canvas area.
pub fn overflows(s: &State) -> bool {
    let [_, _, bw, bh] = s.placed.bounds;
    let [vx, vy, vw, vh] = ui::free_area(s.selected.is_some(), s.project.is_some());
    let z = s.cam.zoom;
    let (x, y) = (s.cam.pan[0] + MARGIN * z, s.cam.pan[1] + MARGIN * z);
    const SLACK: f32 = 1.0;
    x < vx - SLACK || y < vy - SLACK || x + bw * z > vx + vw + SLACK || y + bh * z > vy + vh + SLACK
}

/// Centre the camera on the world point under `p`, a point in the card.
pub fn centre_on(s: &mut State, m: Map, p: [f32; 2]) {
    let [vx, vy, vw, vh] = ui::free_area(s.selected.is_some(), s.project.is_some());
    let z = s.cam.zoom;
    let w = [(p[0] - m.at[0]) / m.k, (p[1] - m.at[1]) / m.k];
    s.cam.pan = [vx + vw / 2.0 - w[0] * z, vy + vh / 2.0 - w[1] * z];
}

/// The overview card, positioned absolutely, when the graph overflows the free area.
pub fn view(p: &Pal, s: &State) -> Option<El> {
    if s.modal.is_some() || !overflows(s) {
        return None;
    }
    let m = map(s)?;
    let [x, y, _, _] = rect(s)?;
    // Opaque, with the chrome bars' shadow: wires under it must not show through.
    let card = ui::card(p)
        .key("overview")
        .size(m.size[0], m.size[1])
        .child(canvas(shape(p, s, m)).size(m.size[0], m.size[1]).absolute(0.0, 0.0))
        .on_drag(move |s: &mut State, d: &mut Drag, tx: &mut Tx| {
            if d.phase != Phase::Up || !d.cancelled() {
                centre_on(s, m, [d.x, d.y]);
            }
            tx.instant();
        });
    Some(card.absolute(x, y))
}

/// Frames as faint outlines, nodes as muted blocks, the visible part as an accent outline.
fn shape(p: &Pal, s: &State, m: Map) -> Shape {
    let [bx, by, _, _] = s.placed.bounds;
    // World (laid-out) coordinates to card points.
    let pt = |x: f32, y: f32| [m.at[0] + (x - bx + MARGIN) * m.k, m.at[1] + (y - by + MARGIN) * m.k];
    let rect = |r: [f32; 4]| {
        let ([x0, y0], [x1, y1]) = (pt(r[0], r[1]), pt(r[0] + r[2], r[1] + r[3]));
        // Keep the tiniest nodes visible as at least a dot.
        let (x1, y1) = (x1.max(x0 + 1.5), y1.max(y0 + 1.5));
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    };
    let mut sh = Shape::new();
    for (g, f) in s.placed.frames.iter().enumerate() {
        if s.collapsed.get(g).copied().unwrap_or(false) || f[2] <= 0.0 {
            continue;
        }
        let mut r = rect(*f);
        r.push(r[0]);
        sh = sh.stroke(r, 1.0, p.faint);
    }
    let block = p.muted.alpha(0.55);
    for (pos, size) in s.placed.pos.iter().zip(&s.placed.size) {
        sh = sh.fill(rect([pos[0], pos[1], size[0], size[1]]), block);
    }
    // The free canvas area, in card points, clipped to the card.
    let [vx, vy, vw, vh] = ui::free_area(s.selected.is_some(), s.project.is_some());
    let z = s.cam.zoom;
    let card = |sx: f32, sy: f32| {
        let w = [(sx - s.cam.pan[0]) / z, (sy - s.cam.pan[1]) / z];
        [(m.at[0] + w[0] * m.k).clamp(1.0, m.size[0] - 1.0), (m.at[1] + w[1] * m.k).clamp(1.0, m.size[1] - 1.0)]
    };
    let ([x0, y0], [x1, y1]) = (card(vx, vy), card(vx + vw, vy + vh));
    sh.stroke(vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]], 1.5, p.accent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::State;

    #[test]
    fn overview_shows_only_when_the_graph_overflows_and_centres_where_pressed() {
        ui::set_viewport([1024.0, 640.0]);
        let mut s = State::load("interp");
        s.selected = None;
        s.fit_all();
        assert!(!overflows(&s), "a fitted graph needs no overview");
        s.cam.zoom = 1.5;
        assert!(overflows(&s));
        let m = map(&s).unwrap();
        assert!(m.size[0] <= W + 0.01 && m.size[1] <= H + 0.01);
        // Pressing the card's middle centres the middle of the world in the free area.
        centre_on(&mut s, m, [m.size[0] / 2.0, m.size[1] / 2.0]);
        let [_, _, bw, bh] = s.placed.bounds;
        let [vx, vy, vw, vh] = ui::free_area(false, false);
        let mid = [s.cam.pan[0] + (bw / 2.0 + MARGIN) * s.cam.zoom, s.cam.pan[1] + (bh / 2.0 + MARGIN) * s.cam.zoom];
        assert!((mid[0] - (vx + vw / 2.0)).abs() < 0.5 && (mid[1] - (vy + vh / 2.0)).abs() < 0.5, "{mid:?}");
        ui::set_viewport([ui::W, ui::H]);
    }
}
