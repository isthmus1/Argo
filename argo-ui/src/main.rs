//! Argo UI: a read-only visualization of agent work recorded in SpacetimeDB.
//!
//! `cargo run -p argo-ui --release` opens the window: drag to pan, scroll or pinch to zoom, hover
//! for details, click to select, double-click a frame or collapsed group to fold it, `F` to
//! frame, `1`–`5` to switch mock scene, `6` for the protocol fixture. Headless:
//! `-- --snap out.png --scene interp --theme dark`.
//!
//! Protocol data: scene `protocol` replays `argo_protocol::fixture` live. `ARGO_SNAPSHOT=x.json`
//! (from `cargo run -- --protocol-json x.c`) opens a snapshot file instead; `ARGO_FUNCTION` picks
//! the function shown.
//!
//! C projects: `argo-ui <git-url>[#rev] [-DNAME=v -Ipath]` (or a local directory) clones into a
//! temporary folder, preprocesses and opens it; Ctrl+O opens the same from inside the app.

mod app;
mod bridge;
mod describe;
mod dialogs;
mod graph;
mod layout;
mod model;
mod palette;
mod project;
mod scenes;
mod ui;

use app::{Item, State};
use palette::Pal;
use rune_ui::{AppSpec, Cx, El, Key, KeyEvent, Tx};
use std::time::Duration;

const SCENES: [&str; 5] = ["fixture", "branches", "dataflow", "devices", "interp"];

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let p = Pal::new(ui::dark(cx.system_dark()));
    // Text fields and anything else left uncoloured draw in the theme's text colour, not white.
    cx.foreground(p.text);
    ui::view(&p, s, cx.titlebar_inset())
}

fn poll(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    tx.wake_after(Duration::from_millis(500));
    let loaded = project::poll(s);
    s.poll(now) || loaded
}

fn on_key(s: &mut State, k: &KeyEvent, _: &mut Tx) -> bool {
    if k.is_command('o') {
        if s.modal != Some(project::Modal::Loading) {
            s.modal = Some(project::Modal::Open);
        }
        return true;
    }
    if s.modal.is_some() {
        return false;
    }
    match &k.key {
        Key::Text(t) if t == "f" => s.fit_all(),
        Key::Text(t) if t == "6" => *s = State::protocol_fixture(),
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

/// Start-up state: a project named on the command line (cloned and opened in the background,
/// or right away for headless captures), else `ARGO_SNAPSHOT`, else the mock fixture.
fn init() -> State {
    let mut spec = None;
    let (mut flags, mut rev) = (vec![], String::new());
    let mut args = rune_ui::extra_args().iter();
    while let Some(a) = args.next() {
        if a == "--rev" {
            rev = args.next().cloned().unwrap_or_default();
        } else if ["-D", "-U", "-I"].iter().any(|p| a.starts_with(p)) {
            flags.push(a.clone());
        } else if !a.starts_with('-') {
            spec = Some(a.clone());
        }
    }
    let Some(spec) = spec else {
        return snapshot_from_env().unwrap_or_else(|| State::load("fixture"));
    };
    let mut s = State::load("fixture");
    project::prefill(&mut s.forms, &spec, &flags);
    if !rev.is_empty() {
        s.forms.rev = rune_ui::TextEdit::new(rev);
    }
    if std::env::args().any(|a| a == "--snap") {
        let rev = s.forms.rev.text.clone();
        if let Err(e) = project::open_now(&mut s, &spec, &rev, &flags.join(" ")) {
            eprintln!("argo-ui: {e}");
            std::process::exit(2);
        }
        if let Ok(f) = std::env::var("ARGO_FUNCTION") {
            let id = s.project.as_ref().and_then(|p| p.modules.iter().flat_map(|m| &m.functions).find(|(_, n)| *n == f).map(|(id, _)| *id));
            if let Some(id) = id {
                project::open_function(&mut s, id);
            }
        }
    } else {
        project::start(&mut s);
    }
    s
}

/// The snapshot named by `ARGO_SNAPSHOT`, if set.
fn snapshot_from_env() -> Option<State> {
    let path = std::env::var("ARGO_SNAPSHOT").ok()?;
    let function = std::env::var("ARGO_FUNCTION").ok();
    match State::snapshot_file(&path, function.as_deref()) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("argo-ui: {e}");
            std::process::exit(2);
        }
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
            ("protocol", |s| *s = State::protocol_fixture()),
            ("protocol-hover", |s| {
                *s = State::protocol_fixture();
                s.replay(15);
                hover(s, "step(acc, i)");
            }),
            ("protocol-live", |s| {
                *s = State::protocol_fixture();
                s.replay(usize::MAX);
                hover(s, "uint32_t acc = 0");
            }),
            ("project", |_| {}),
            ("project-open", |s| s.modal = Some(project::Modal::Open)),
            ("project-rewrite", |s| s.modal = Some(project::Modal::Rewrite)),
            ("project-rewrite-file", |s| {
                s.forms.scope = project::Scope::File;
                s.modal = Some(project::Modal::Rewrite);
            }),
            ("project-not-implemented", |s| s.modal = Some(project::Modal::NotImplemented)),
            ("project-filter", |s| {
                s.forms.filter = rune_ui::TextEdit::new(std::env::var("ARGO_FILTER").unwrap_or_else(|_| "read".into()));
            }),
            ("snapshot", |s| {
                *s = snapshot_from_env().unwrap_or_else(State::protocol_fixture);
                // Captures can show a hover card: `ARGO_HOVER` names the node by its title.
                if let Ok(name) = std::env::var("ARGO_HOVER") {
                    hover(s, &name);
                }
            }),
            ("hover", |s| {
                // Hover-card placement checks: `ARGO_SCENE` (a mock scene or `protocol`) with
                // `ARGO_HOVER` naming the node; `ARGO_TILE`/`ARGO_ZOOM` move the camera.
                let scene = std::env::var("ARGO_SCENE").unwrap_or_default();
                *s = match SCENES.iter().find(|n| **n == scene) {
                    Some(n) => State::load(n),
                    None => {
                        let mut s = State::protocol_fixture();
                        s.replay(usize::MAX);
                        s
                    }
                };
                hover(s, &std::env::var("ARGO_HOVER").unwrap_or_default());
            }),
            ("interp-collapsed", |s| {
                *s = State::load("interp");
                s.toggle_group(0);
                s.fit();
                hover(s, "alu");
            }),
        ],
        min_size: Some((ui::MIN_W, ui::MIN_H)),
        poll: Some(poll),
        on_key: Some(on_key),
        setup: Some(|_, waker| {
            let _ = project::WAKER.set(waker);
        }),
        ..AppSpec::new("Argo", (ui::W, ui::H), init, view)
    });
}
