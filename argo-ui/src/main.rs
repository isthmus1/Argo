//! Argo UI: a read-only visualization of agent work on a C program's graph.
//!
//! `cargo run -p argo-ui --release` opens the window on the protocol fixture: drag to pan,
//! scroll or pinch to zoom, hover for details, click to select, a frame's collapse button (or a
//! double-click) folds a node group into one node and the node's expand button (or a
//! double-click) opens it again, click a breadcrumb to go up a level, `F` to frame, `1`–`5` to
//! switch mock scene, `6` for the protocol fixture. Headless:
//! `-- --snap out.png --scene interp --theme dark`.
//!
//! Protocol data: scene `protocol` replays `argo_protocol::fixture`'s recorded updates (the
//! default with no arguments). `ARGO_SNAPSHOT=x.json`
//! (from `cargo run -- --protocol-json x.c`) opens a snapshot file instead; `ARGO_FUNCTION` picks
//! the function shown.
//!
//! C projects: `argo-ui <git-url>[#rev] [-DNAME=v -Ipath]` (or a local directory) clones into a
//! temporary folder, preprocesses and opens it; Ctrl+O opens the same from inside the app.
//!
//! SpacetimeDB: `argo-ui --db ws://127.0.0.1:3000 [--module argo]` (or `ARGO_DB`, `ARGO_MODULE`)
//! watches the database `argo-server` ingested and workers translate, live. `ARGO_VIEW=program`
//! starts a capture on the whole-program view. Everything shown comes
//! from its rows; nothing is cloned or parsed. A lost connection keeps the last rows on screen,
//! marked offline, and reconnects. The window is read-only there: Ctrl+O and the scene keys
//! `1`–`6` do nothing. `--snap` waits for the rows, then captures.

mod app;
mod bridge;
mod calls;
mod db;
mod describe;
mod dialogs;
mod graph;
mod layout;
mod model;
mod overview;
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
    let root = ui::view(&p, s, cx.titlebar_inset());
    // The overview layers over the canvas and chrome, at the free area's bottom-left.
    match overview::view(&p, s) {
        Some(map) => root.child(map),
        None => root,
    }
}

fn poll(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    tx.wake_after(Duration::from_millis(500));
    let loaded = project::poll(s) | db::poll(s);
    s.poll(now) || loaded
}

fn on_key(s: &mut State, k: &KeyEvent, _: &mut Tx) -> bool {
    // Watching a database, the canvas only ever shows its rows: no opening, no mock scenes.
    let watching = s.db.is_some();
    if k.is_command('o') && !watching {
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
        Key::Text(t) if t == "6" && !watching => *s = State::protocol_fixture(),
        Key::Text(t) if t.len() == 1 && ("1"..="5").contains(&t.as_str()) && !watching => {
            *s = State::load(SCENES[t.parse::<usize>().expect("digit") - 1]);
        }
        Key::Escape => s.selected = None,
        _ => return false,
    }
    true
}

/// The `dataflow-select` capture: the node it selects, and the one it hovers. A scripted pointer
/// hovers only what a person could see: `normalize` (claimed, feeding round_pack) stays in view
/// beside the details panel at every review size.
const DATAFLOW_SELECT: [&str; 2] = ["round_pack", "normalize"];

/// Hover a node by title, as a scripted pointer would.
fn hover(s: &mut State, title: &str) {
    if let Some(i) = s.find(title) {
        s.hovered = Some(Item::Node(i));
    }
}

/// Start-up state: a SpacetimeDB to watch (`--db`/`ARGO_DB`), else a project named on the command
/// line (cloned and opened in the background, or right away for headless captures), else
/// `ARGO_SNAPSHOT`, else the mock fixture.
fn init() -> State {
    let mut spec = None;
    let (mut flags, mut rev) = (vec![], String::new());
    let (mut uri, mut module) = (std::env::var("ARGO_DB").ok(), std::env::var("ARGO_MODULE").ok());
    let mut args = rune_ui::extra_args().iter();
    while let Some(a) = args.next() {
        if a == "--rev" {
            rev = args.next().cloned().unwrap_or_default();
        } else if a == "--db" {
            uri = args.next().cloned();
        } else if a == "--module" {
            module = args.next().cloned();
        } else if ["-D", "-U", "-I"].iter().any(|p| a.starts_with(p)) {
            flags.push(a.clone());
        } else if !a.starts_with('-') {
            spec = Some(a.clone());
        }
    }
    if let Some(uri) = uri {
        return watch(&uri, module.as_deref().unwrap_or("argo"));
    }
    let Some(spec) = spec else {
        // Real protocol data by default, so the breadcrumb is a hierarchy to navigate; the
        // mock scenes stay on keys 1–5 and `--scene`.
        return snapshot_from_env().unwrap_or_else(State::protocol_fixture);
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
        open_env_function(&mut s);
    } else {
        project::start(&mut s);
    }
    s
}

/// Watch a database. A headless capture waits for its rows first (`ARGO_FUNCTION` picks the
/// function shown, `ARGO_OPEN=all` opens everything in it); a window opens right away and fills
/// in as they arrive.
fn watch(uri: &str, module: &str) -> State {
    let link = db::connect(uri, module);
    if !std::env::args().any(|a| a == "--snap") {
        return State::db_waiting(link, "connecting");
    }
    let mut s = db::first(link, Duration::from_secs(30)).unwrap_or_else(|e| {
        eprintln!("argo-ui: {e}");
        std::process::exit(2);
    });
    open_env_function(&mut s);
    s.open_all_if_requested();
    // Captures: `ARGO_VIEW=program` shows the whole-program view.
    if std::env::var("ARGO_VIEW").is_ok_and(|v| v == "program") {
        project::open_program(&mut s);
    }
    s
}

/// Show the function `ARGO_FUNCTION` names, if set and present (headless captures).
fn open_env_function(s: &mut State) {
    let Ok(f) = std::env::var("ARGO_FUNCTION") else { return };
    let id = s.project.as_ref().and_then(|p| p.modules.iter().flat_map(|m| &m.functions).find(|(_, n)| *n == f).map(|(id, _)| *id));
    if let Some(id) = id {
        project::open_function(s, id);
    }
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
                if let Some(i) = s.find(DATAFLOW_SELECT[0]) {
                    s.select(Item::Node(i));
                }
                hover(s, DATAFLOW_SELECT[1]);
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
            ("project-open-error", |s| {
                s.forms.error = Some(project::SAMPLE_ERROR.into());
                s.modal = Some(project::Modal::Open);
            }),
            ("project-open-error-all", |s| {
                s.forms.error = Some(project::SAMPLE_ERROR.into());
                s.forms.error_open = true;
                s.modal = Some(project::Modal::Open);
            }),
            // A project that parsed but defines no functions: the explorer's empty state.
            ("project-empty", |s| {
                if let Err(e) = project::open_sample_empty(s) {
                    eprintln!("argo-ui: {e}");
                }
            }),
            ("project-loading", |s|project::show_loading(s, "Fetching https://github.com/fernandotcl/TinyEMU.git @ HEAD")),
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
            // Node groups both ways: as a frame, and collapsed to one node.
            ("fixture-collapsed", |s| {
                *s = State::load("fixture");
                if let Some(g) = s.graph.groups.iter().position(|g| g.title == "step ∘ normalize") {
                    s.collapse_group(g);
                }
                s.fit();
            }),
            ("protocol-module", |s| *s = State::protocol_module(false)),
            ("protocol-module-open", |s| *s = State::protocol_module(true)),
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
