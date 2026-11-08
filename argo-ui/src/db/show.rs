//! Rows onto the canvas: the rebuilt snapshot, the explorer's index, and where the user was.

use std::collections::HashSet;

use argo_db_client::Origin;
use argo_protocol::{NodeId, NodeKind, Snapshot};

use crate::app::{Cam, Item, State};
use crate::project::{self, Project};

/// A selection by protocol node, which survives a relayout where view indices don't.
enum Picked {
    Node(NodeId),
    Group(NodeId),
}

/// Where the user was, by protocol node: the shown function, what they opened in it, the
/// selection and the camera. It survives a relayout, and a spell of incomplete rows.
pub struct Nav {
    container: NodeId,
    open: HashSet<NodeId>,
    picked: Option<Picked>,
    cam: Cam,
}

pub fn checkpoint(s: &State) -> Option<Nav> {
    let live = s.live.as_ref()?;
    let picked = s.selected.and_then(|i| picked(s, i));
    Some(Nav {
        container: live.container?,
        open: live.open.clone(),
        picked,
        cam: s.cam,
    })
}

pub fn show(s: &mut State, structure: u64, snap: Snapshot, origin: &Origin) {
    let Some(link) = s.db.as_mut() else { return };
    if link.structure == Some(structure)
        && let Some(live) = &mut s.live
    {
        // Only status changed: view indices, layout, camera and selection all stay.
        live.snap = snap;
        s.reproject();
        s.refresh();
        return;
    }
    link.structure = Some(structure);
    let nav = checkpoint(s).or_else(|| s.db.as_mut().and_then(|l| l.nav.take()));
    let ids: HashSet<NodeId> = snap.graph.nodes.iter().map(|n| n.id).collect();
    let modules = project::index(&snap);
    let declarations = project::declarations(&snap);
    let entry = project::entry(&modules);
    // With no function to open, the canvas shows what there is: the one file's declarations, or
    // each file as a group of them.
    let level = project::file_level(&snap);
    let name = snap
        .graph
        .nodes
        .iter()
        .find(|n| n.kind == NodeKind::Program)
        .map_or_else(|| origin.locator.clone(), |n| n.name.clone());
    let mut next = State::from_snapshot(snap, None, HashSet::new());
    next.project = Some(Project {
        name,
        source: origin.locator.clone(),
        checkout: origin.commit.as_ref().map_or_else(
            || {
                format!(
                    "{} (not a git checkout: no commit recorded)",
                    origin.locator
                )
            },
            |c| format!("commit {c}"),
        ),
        units: origin.units.clone(),
        flags: origin.flags.clone(),
        modules,
        declarations,
    });
    next.db = s.db.take();
    next.online = s.online;
    next.forms = std::mem::take(&mut s.forms);
    *s = next;
    match nav.filter(|n| ids.contains(&n.container)) {
        Some(nav) => {
            if let Some(live) = &mut s.live {
                live.container = Some(nav.container);
                live.open = nav.open.into_iter().filter(|id| ids.contains(id)).collect();
            }
            s.reproject();
            s.relayout();
            s.cam = nav.cam;
            s.selected = nav.picked.and_then(|p| find(s, &p));
        }
        None => {
            if let Some(f) = entry {
                project::open_function(s, f);
            } else if let Some(c) = level {
                project::show_level(s, c);
            }
        }
    }
}

fn picked(s: &State, item: Item) -> Option<Picked> {
    let live = s.live.as_ref()?;
    match item {
        Item::Node(i) => live.origin.get(i).copied().map(Picked::Node),
        Item::Group(g) => live.frames.get(g).copied().map(Picked::Group),
    }
}

fn find(s: &State, p: &Picked) -> Option<Item> {
    let live = s.live.as_ref()?;
    match p {
        Picked::Node(id) => live.origin.iter().position(|o| o == id).map(Item::Node),
        Picked::Group(id) => live.frames.iter().position(|f| f == id).map(Item::Group),
    }
}
