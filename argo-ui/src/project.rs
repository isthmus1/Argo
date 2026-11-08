//! The ingested project as the explorer shows it: its file/function index, the function on the
//! canvas, and the explorer's own fields. Everything here comes from the database's rows.

use std::collections::HashSet;

use argo_db_client::Unit;
use argo_protocol::{NodeId, NodeKind, Snapshot};
use rune_ui::TextEdit;

use crate::app::State;

pub struct Project {
    pub name: String,
    /// The repository as given to `argo-server`: a git URL or a path.
    pub source: String,
    /// What was read: the recorded commit, or why there is none.
    pub checkout: String,
    pub units: Vec<Unit>,
    pub flags: Vec<String>,
    /// Modules (original files) that define functions, sorted by path.
    pub modules: Vec<Module>,
    /// File-level items that aren't functions (globals, types), counted from the graph.
    pub declarations: usize,
}

pub struct Module {
    pub id: NodeId,
    pub name: String,
    pub functions: Vec<(NodeId, String)>,
}

impl Project {
    pub fn function_count(&self) -> usize {
        self.modules.iter().map(|m| m.functions.len()).sum()
    }

    pub fn included(&self) -> usize {
        self.units.iter().filter(|u| u.skipped.is_none()).count()
    }

    pub fn module_of(&self, f: NodeId) -> Option<&Module> {
        self.modules
            .iter()
            .find(|m| m.functions.iter().any(|(id, _)| *id == f))
    }

    /// What the explorer says for a project with no function bodies: what it does hold.
    pub fn empty_note(&self) -> String {
        match self.declarations {
            0 => "Nothing declared".into(),
            1 => "1 declaration, no function bodies".into(),
            n => format!("{n} declarations, no function bodies"),
        }
    }
}

/// The explorer's fields, kept across reloads.
#[derive(Default)]
pub struct Forms {
    pub filter: TextEdit,
    /// The module expanded in the explorer.
    pub expanded: Option<NodeId>,
}

/// File-level items that aren't functions: every non-function child of a module.
pub(crate) fn declarations(snap: &Snapshot) -> usize {
    let nodes = &snap.graph.nodes;
    let modules: HashSet<NodeId> = nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Module)
        .map(|n| n.id)
        .collect();
    nodes
        .iter()
        .filter(|n| n.kind != NodeKind::Function && n.parent.is_some_and(|p| modules.contains(&p)))
        .count()
}

/// The container whose level lists a project's files' contents: its one module, else the
/// program (each module a group), else none.
pub(crate) fn file_level(snap: &Snapshot) -> Option<NodeId> {
    let nodes = &snap.graph.nodes;
    let mut modules = nodes.iter().filter(|n| n.kind == NodeKind::Module);
    match (modules.next(), modules.next()) {
        (Some(m), None) => Some(m.id),
        _ => nodes.iter().find(|n| n.kind == NodeKind::Program).map(|n| n.id),
    }
}

/// Show container `c`'s level with nothing inside it open.
pub(crate) fn show_level(s: &mut State, c: NodeId) {
    let Some(live) = &mut s.live else { return };
    live.container = Some(c);
    live.open.clear();
    s.selected = None;
    s.hovered = None;
    s.reproject();
    s.relayout();
    s.fit();
}

/// The function to show first: `main` in a .c file if there is one, else the first function of
/// the first .c file, else the first function.
pub(crate) fn entry(modules: &[Module]) -> Option<NodeId> {
    let c_files = || {
        modules
            .iter()
            .filter(|m| m.name.ends_with(".c"))
            .flat_map(|m| &m.functions)
    };
    c_files()
        .find(|(_, n)| n == "main")
        .or_else(|| c_files().next())
        .or_else(|| modules.iter().flat_map(|m| &m.functions).next())
        .map(|(id, _)| *id)
}

/// Modules (files) that define functions, with their functions, sorted by path.
pub(crate) fn index(snap: &Snapshot) -> Vec<Module> {
    let nodes = &snap.graph.nodes;
    let mut modules: Vec<Module> = nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Module)
        .map(|m| Module {
            id: m.id,
            name: m.name.clone(),
            functions: vec![],
        })
        .collect();
    for n in nodes.iter().filter(|n| n.kind == NodeKind::Function) {
        if let Some(m) = modules.iter_mut().find(|m| Some(m.id) == n.parent) {
            m.functions.push((n.id, n.name.clone()));
        }
    }
    modules.retain(|m| !m.functions.is_empty());
    // By path, so a file's header sits beside it and directories group together.
    modules.sort_by(|a, b| a.name.cmp(&b.name));
    modules
}

/// Show one function: its pins on the canvas, its body block open.
pub fn open_function(s: &mut State, f: NodeId) {
    let Some(live) = &mut s.live else { return };
    let body: HashSet<NodeId> = live
        .snap
        .graph
        .nodes
        .iter()
        .filter(|n| n.parent == Some(f) && n.kind == NodeKind::Scope)
        .map(|n| n.id)
        .collect();
    live.container = Some(f);
    live.open = body;
    live.whole = false;
    if let Some(p) = &s.project {
        s.forms.expanded = p.module_of(f).map(|m| m.id);
    }
    s.selected = None;
    s.hovered = None;
    s.reproject();
    s.relayout();
    s.fit();
}

/// Show the whole program: every file as a dashed frame of its functions, wired by their calls.
/// The calls are worked out the first time (parsing the stored C), then kept.
pub fn open_program(s: &mut State) {
    let Some(live) = &mut s.live else { return };
    let Some(program) = live.snap.graph.nodes.iter().find(|n| n.kind == NodeKind::Program).map(|n| n.id) else { return };
    if live.calls.is_none() {
        live.calls = Some(std::sync::Arc::new(crate::calls::calls(&live.snap)));
    }
    live.open = live.snap.graph.nodes.iter().filter(|n| n.kind == NodeKind::Module).map(|n| n.id).collect();
    live.container = Some(program);
    live.whole = true;
    s.forms.expanded = None;
    s.selected = None;
    s.hovered = None;
    s.reproject();
    s.relayout();
    s.fit();
}

/// Whether the whole-program view is showing.
pub fn showing_program(s: &State) -> bool {
    s.live.as_ref().is_some_and(|l| l.whole)
}

/// The function on the canvas, if a project is open.
pub fn current(s: &State) -> Option<(NodeId, String)> {
    let live = s.live.as_ref()?;
    let id = live.container?;
    let n = live.snap.graph.nodes.iter().find(|n| n.id == id)?;
    Some((id, n.name.clone()))
}
