//! Argo UI: a read-only visualization of agent work recorded in SpacetimeDB.
//!
//! `cargo run -p argo-ui --release` opens the window: drag to pan, scroll or pinch to zoom, hover
//! for details, click to select, double-click a frame or collapsed group to fold it, `F` to
//! frame, `1`–`5` to switch scene. Headless: `-- --snap out.png --scene interp --theme dark`.

mod app;
mod describe;
mod graph;
mod layout;
mod model;
mod palette;
mod scenes;
mod ui;

use app::{Item, State};
use palette::Pal;
use rune_ui::{AppSpec, Cx, El, Key, KeyEvent, Tx};
use std::time::Duration;

const SCENES: [&str; 5] = ["fixture", "branches", "dataflow", "devices", "interp"];

fn view(s: &State, cx: &mut Cx<State>) -> El {
    ui::view(&Pal::new(cx.system_dark()), s)
}

fn poll(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    tx.wake_after(Duration::from_millis(500));
    s.poll(now)
}

fn on_key(s: &mut State, k: &KeyEvent, _: &mut Tx) -> bool {
    match &k.key {
        Key::Text(t) if t == "f" => s.fit_all(),
        Key::Text(t) if t.len() == 1 && ("1"..="5").contains(&t.as_str()) => {
            *s = State::load(SCENES[t.parse::<usize>().expect("digit") - 1]);
        }
        Key::Escape => s.selected = None,
        _ => return false,
    }
    true
}

/// Hover a node by title, as a scripted pointer would.
fn hover(s: &mut State, title: &str) {
    if let Some(i) = s.find(title) {
        s.hovered = Some(Item::Node(i));
    }
}

fn main() {
    rune_ui::run(AppSpec::<State> {
        scenes: &[
            ("fixture", |s| *s = State::load("fixture")),
            ("branches", |s| *s = State::load("branches")),
            ("dataflow", |s| *s = State::load("dataflow")),
            ("devices", |s| *s = State::load("devices")),
            ("interp", |s| *s = State::load("interp")),
            ("fixture-hover", |s| {
                *s = State::load("fixture");
                hover(s, "report");
            }),
            ("branches-hover", |s| {
                *s = State::load("branches");
                hover(s, "parse_entry");
            }),
            ("dataflow-select", |s| {
                *s = State::load("dataflow");
                if let Some(i) = s.find("round_pack") {
                    s.select(Item::Node(i));
                }
                hover(s, "is_nan(a|b)");
            }),
            ("interp-collapsed", |s| {
                *s = State::load("interp");
                s.toggle_group(0);
                s.fit();
                hover(s, "alu");
            }),
        ],
        min_size: Some((ui::W, ui::H)),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new("Argo", (ui::W, ui::H), || State::load("fixture"), view)
    });
}
