//! The window's only data source (`--db ws://host:port`): every graph and status it shows comes
//! from the `c_chunk` and `rust_chunk` rows, rebuilt by `argo-db-client`. Nothing here clones,
//! reads or parses the repository; the rows carry the structure, the source text and the origin.
//!
//! A status-only change (a `rust_chunk` row) keeps layout, camera, the shown function and the
//! selection. A new ingest (different `c_chunk` rows) lays out again, keeping the shown function,
//! opened constructs, selection and camera where they still exist, even across a spell of
//! incomplete rows. A lost connection keeps the last program on screen, marked offline, while the
//! subscription reconnects.

mod show;
mod subscribe;

use std::sync::OnceLock;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use argo_db_client::Origin;
use argo_protocol::Snapshot;
use rune_ui::Waker;

use crate::app::State;
use crate::bridge;

/// Wakes the window when the subscription delivers something.
pub static WAKER: OnceLock<Waker> = OnceLock::new();

/// What the subscription tells the event loop.
pub enum Msg {
    Online(bool),
    /// The rows don't describe a whole program (nothing ingested yet, an ingest still arriving,
    /// never connected), and why.
    Waiting(String),
    /// The program as the rows describe it. `structure` advances only when `c_chunk` rows change.
    Rows {
        structure: u64,
        snap: Box<Snapshot>,
        origin: Origin,
    },
}

/// The subscription feeding the window, which structure it shows, and where the user was.
pub struct Link {
    pub module: String,
    rx: Receiver<Msg>,
    structure: Option<u64>,
    /// Navigation kept while incomplete rows empty the canvas, restored by the next program.
    nav: Option<show::Nav>,
    /// Claims shown with an expired lease, as of the last projection.
    lapsed: usize,
}

impl Link {
    pub fn new(module: String, rx: Receiver<Msg>) -> Link {
        Link {
            module,
            rx,
            structure: None,
            nav: None,
            lapsed: 0,
        }
    }
}

/// Subscribe to `module` at `uri` on a background thread.
pub fn connect(uri: &str, module: &str) -> Link {
    Link::new(module.into(), subscribe::start(uri.into(), module.into()))
}

impl State {
    /// An empty canvas naming the database and what it waits for, until rows describe a program.
    pub fn db_waiting(link: Link, why: &str) -> State {
        let mut s = State::empty(vec![link.module.clone(), why.into()]);
        s.online = false;
        s.db = Some(link);
        s
    }
}

/// Apply what the subscription delivered, and let claims whose lease ran out go idle; true when
/// anything shown changed. Each `Rows` message is the whole program, so only the newest is shown.
pub fn poll(s: &mut State) -> bool {
    let Some(link) = &s.db else { return false };
    let msgs: Vec<Msg> = link.rx.try_iter().collect();
    let newest = msgs.iter().rposition(|m| matches!(m, Msg::Rows { .. }));
    let changed = !msgs.is_empty();
    for (i, msg) in msgs.into_iter().enumerate() {
        apply(s, msg, Some(i) == newest);
    }
    lapse(s) || changed
}

fn apply(s: &mut State, msg: Msg, newest: bool) {
    match msg {
        Msg::Online(on) => s.online = on,
        Msg::Waiting(why) => wait(s, &why),
        Msg::Rows {
            structure,
            snap,
            origin,
        } if newest => show::show(s, structure, *snap, &origin),
        Msg::Rows { .. } => {}
    }
}

/// A lease runs out with no row changing (its worker died), so the clock decides: reproject when
/// another claim's lease expired since the last projection.
fn lapse(s: &mut State) -> bool {
    let n = s.live.as_ref().map_or(0, |l| {
        l.snap
            .status
            .iter()
            .filter_map(|st| st.claim.as_ref())
            .filter(|c| bridge::lease_expired(&l.snap, c.attempt))
            .count()
    });
    let Some(link) = s.db.as_mut().filter(|l| l.lapsed != n) else {
        return false;
    };
    link.lapsed = n;
    s.reproject();
    s.refresh();
    true
}

/// Back to the empty canvas: rows that no longer describe a program (a reset) must not leave a
/// stale graph on screen. Where the user was is kept for the next program.
fn wait(s: &mut State, why: &str) {
    if s.live.is_none() {
        if let Some(c) = s.graph.crumbs.get_mut(1) {
            *c = why.into();
        }
        s.view.crumbs = s.graph.crumbs.clone();
        return;
    }
    let nav = show::checkpoint(s);
    let Some(mut link) = s.db.take() else { return };
    (link.structure, link.nav) = (None, nav);
    let (online, forms) = (s.online, std::mem::take(&mut s.forms));
    *s = State::db_waiting(link, why);
    s.online = online;
    s.forms = forms;
}

/// Block until the rows describe a program (headless captures), or fail after `timeout` with the
/// last reason given.
pub fn first(link: Link, timeout: Duration) -> Result<State, String> {
    let mut s = State::db_waiting(link, "connecting");
    let deadline = Instant::now() + timeout;
    while s.live.is_none() {
        let left = deadline.saturating_duration_since(Instant::now());
        let msg = s.db.as_ref().and_then(|l| l.rx.recv_timeout(left).ok());
        let Some(msg) = msg else {
            return Err(format!(
                "no program in the database: {}",
                s.graph.crumbs.get(1).map_or("", String::as_str)
            ));
        };
        apply(&mut s, msg, true);
    }
    Ok(s)
}

#[cfg(test)]
mod tests;
