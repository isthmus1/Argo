//! Argo UI: a read-only visualization of agent work recorded in SpacetimeDB.
//! `cargo run -p argo-ui` opens the window; scenes render headless with
//! `cargo run -p argo-ui --release -- --snap out.png --scene interp --theme dark --scale 2`.

mod graph;
mod layout;
mod model;
mod palette;
mod scenes;
mod ui;

use palette::Pal;
use rune_ui::{AppSpec, Cx, El};

struct State {
    graph: model::Graph,
    placed: layout::Placed,
}

impl State {
    fn load(name: &str) -> State {
        let graph = layout::with_getters(scenes::scene(name));
        let placed = layout::layout(&graph);
        State { graph, placed }
    }
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    ui::view(&Pal::new(cx.system_dark()), &s.graph, &s.placed)
}

fn main() {
    rune_ui::run(AppSpec::<State> {
        scenes: &[
            ("fixture", |s| *s = State::load("fixture")),
            ("branches", |s| *s = State::load("branches")),
            ("dataflow", |s| *s = State::load("dataflow")),
            ("devices", |s| *s = State::load("devices")),
            ("interp", |s| *s = State::load("interp")),
        ],
        min_size: Some((ui::W, ui::H)),
        ..AppSpec::new("Argo", (ui::W, ui::H), || State::load("fixture"), view)
    });
}
