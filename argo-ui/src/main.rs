//! Argo UI: a read-only visualization of agent work recorded in SpacetimeDB.
//!
//! `argo-ui --db ws://127.0.0.1:3100 [--module argo]` (or `ARGO_DB`, `ARGO_MODULE`) watches the
//! database `argo-server` ingested and workers translate, live. It is the only data source:
//! everything shown comes from its rows; nothing is cloned or parsed, and without a database the
//! binary prints its usage and exits. A lost connection keeps the last rows on screen, marked
//! offline, and reconnects.
//!
//! Drag to pan, scroll or pinch to zoom, hover for details, click to select, a frame's collapse
//! button (or a double-click) folds a node group into one node and the node's expand button (or
//! a double-click) opens it again, click a breadcrumb to go up a level, `F` to frame. Headless:
//! add `--snap out.png`, which waits for the rows, then captures; `ARGO_FUNCTION`,
//! `ARGO_OPEN=all`, `ARGO_HOVER`, `ARGO_TILE`/`ARGO_ZOOM`, `ARGO_FIT=all` and `ARGO_VIEW=program` (the whole-program view)
//! script it.

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
#[cfg(all(test, feature = "frame-profile"))]
mod perf;
mod project;
#[cfg(test)]
mod scenes;
mod ui;

use app::{Item, State};
use palette::Pal;
use rune_ui::{AppSpec, Cx, El, Key, KeyEvent, Tx};
use std::time::Duration;

const USAGE: &str = "usage: argo-ui --db ws://HOST:PORT [--module NAME] [--snap out.png]
  (or ARGO_DB / ARGO_MODULE); the database argo-server ingested is the only data source";

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

fn poll(s: &mut State, _: f64, tx: &mut Tx) -> bool {
    tx.wake_after(Duration::from_millis(500));
    db::poll(s)
}

fn on_key(s: &mut State, k: &KeyEvent, _: &mut Tx) -> bool {
    match &k.key {
        Key::Text(t) if t == "f" => s.fit_all(),
        Key::Escape => s.selected = None,
        _ => return false,
    }
    true
}

/// Hover a node by title, as a scripted pointer would; a title nothing has fails the capture.
fn hover(s: &mut State, title: &str) {
    match s.find(title) {
        Some(i) => s.hovered = Some(Item::Node(i)),
        None => fail(&format!("ARGO_HOVER: no node titled {title:?} in the shown function")),
    }
}

/// End a headless capture that would otherwise show the wrong thing.
fn fail(why: &str) -> ! {
    eprintln!("argo-ui: {why}");
    std::process::exit(2);
}

/// The database to watch and its module: `--db`/`ARGO_DB`, `--module`/`ARGO_MODULE` (`argo`).
fn target() -> Option<(String, String)> {
    let (mut uri, mut module) = (std::env::var("ARGO_DB").ok(), std::env::var("ARGO_MODULE").ok());
    let mut args = rune_ui::extra_args().iter();
    while let Some(a) = args.next() {
        if a == "--db" {
            uri = args.next().cloned();
        } else if a == "--module" {
            module = args.next().cloned();
        }
    }
    // Empty, or the next flag (`--db --snap x.png`), is no database.
    let uri = uri.filter(|u| !u.is_empty() && !u.starts_with('-'))?;
    Some((uri, module.unwrap_or_else(|| "argo".into())))
}

/// Start-up state: the database named on the command line (checked in `main`).
fn init() -> State {
    let (uri, module) = target().expect("checked in main");
    watch(&uri, &module)
}

/// Watch a database. A headless capture waits for its rows first (`ARGO_FUNCTION` picks the
/// function shown, `ARGO_OPEN=all` opens everything in it, `ARGO_HOVER` hovers a node by title);
/// a window opens right away and fills in as they arrive.
fn watch(uri: &str, module: &str) -> State {
    let link = db::connect(uri, module);
    if !std::env::args().any(|a| a == "--snap") {
        return State::db_waiting(link, "connecting");
    }
    let mut s = db::first(link, Duration::from_secs(30)).unwrap_or_else(|e| fail(&e));
    open_env_function(&mut s);
    s.open_all_if_requested();
    // Captures: `ARGO_VIEW=program` shows the whole-program view.
    if std::env::var("ARGO_VIEW").is_ok_and(|v| v == "program") {
        project::open_program(&mut s);
    }
    if let Ok(name) = std::env::var("ARGO_HOVER") {
        hover(&mut s, &name);
    }
    s
}

/// Show the function `ARGO_FUNCTION` names, if set (headless captures); one the database lacks
/// fails the capture.
fn open_env_function(s: &mut State) {
    let Ok(f) = std::env::var("ARGO_FUNCTION") else { return };
    let id = s.project.as_ref().and_then(|p| p.modules.iter().flat_map(|m| &m.functions).find(|(_, n)| *n == f).map(|(id, _)| *id));
    match id {
        Some(id) => project::open_function(s, id),
        None => fail(&format!("ARGO_FUNCTION: no function named {f:?} in the database")),
    }
}

fn main() {
    if target().is_none() {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    rune_ui::run(AppSpec::<State> {
        min_size: Some((ui::MIN_W, ui::MIN_H)),
        poll: Some(poll),
        on_key: Some(on_key),
        setup: Some(|_, waker| {
            let _ = db::WAKER.set(waker);
        }),
        ..AppSpec::new("Argo", (ui::W, ui::H), init, view)
    });
}
