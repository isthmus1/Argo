//! Argo UI: a read-only visualization of agent work recorded in SpacetimeDB.
//! `cargo run -p argo-ui` opens the window; the mock renders headless with
//! `cargo run -p argo-ui --release -- --snap out.png --theme dark|light --scale 2`.

mod graph;
mod mock;
mod palette;
mod ui;

use palette::Pal;
use rune_ui::{AppSpec, Cx, El};

#[derive(Default)]
struct State;

fn view(_: &State, cx: &mut Cx<State>) -> El {
    ui::view(&Pal::new(cx.system_dark()))
}

fn main() {
    rune_ui::run(AppSpec::<State> {
        min_size: Some((ui::W, ui::H)),
        ..AppSpec::new("Argo", (ui::W, ui::H), State::default, view)
    });
}
