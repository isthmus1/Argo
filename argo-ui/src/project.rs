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
}

/// The explorer's fields, kept across reloads.
#[derive(Default)]
pub struct Forms {
    pub filter: TextEdit,
    /// The module expanded in the explorer.
    pub expanded: Option<NodeId>,
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
    if let Some(p) = &s.project {
        s.forms.expanded = p.module_of(f).map(|m| m.id);
    }
    s.selected = None;
    s.hovered = None;
    s.reproject();
    s.relayout();
    s.fit();
}

/// The function on the canvas, if a project is open.
pub fn current(s: &State) -> Option<(NodeId, String)> {
    let live = s.live.as_ref()?;
    let id = live.container?;
    let n = live.snap.graph.nodes.iter().find(|n| n.id == id)?;
    Some((id, n.name.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(id: u64, name: &str, functions: &[(u64, &str)]) -> Module {
        Module {
            id: NodeId(id),
            name: name.into(),
            functions: functions.iter().map(|&(i, n)| (NodeId(i), n.into())).collect(),
        }
    }

    #[test]
    fn without_main_the_first_c_function_shows_else_the_first_function() {
        let h = module(1, "a.h", &[(10, "inline_helper")]);
        let c = module(2, "b.c", &[(20, "run"), (21, "stop")]);
        assert_eq!(entry(&[h, c]), Some(NodeId(20)), "a .c file wins over a header");
        let h = module(1, "a.h", &[(10, "inline_helper")]);
        assert_eq!(entry(&[h]), Some(NodeId(10)), "only headers: their first function");
        assert_eq!(entry(&[]), None);
    }
}
