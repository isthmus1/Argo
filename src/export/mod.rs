//! Syntax graph → `argo-protocol` records.
//!
//! What syntax alone can establish, and nothing more (`Analysis::SyntaxOnly`):
//!
//! - containment, template IDs, provenance and syntax parts;
//! - function parameter and return pins, spelled as written (no type resolution);
//! - every region's exec pins: entry, fallthrough when it can fall through, and one exit pin per
//!   lexically present `return`/`break`/`continue` that escapes it;
//! - exec wiring inside every container, through group input/output and dispatch nodes.
//!
//! Region data pins (live-in/live-out), effects, and call/type/state relations need resolved names
//! and are left out rather than guessed from identifier text (§5.4).

mod dispatch;
mod flow;
mod interior;
mod names;
mod signature;

use std::collections::HashMap;

use argo_protocol::{
    Analysis, FileId, GraphBuilder, GraphRevision, NodeId, NodeKind, PinSpec, ProjectId,
    Provenance, RevisionId, RevisionState, SourceFile, SourceRange, SyntaxPart,
};

use crate::graph::{Graph, GraphNode};
use crate::{Ast, CNode, CNodeType};

const FILE: FileId = FileId(1);

/// Export one translation unit's syntax graph as a draft protocol revision plus its source.
pub fn to_protocol(graph: &Graph, ast: &Ast, path: &str) -> (argo_protocol::Graph, SourceFile) {
    let text = String::from_utf8_lossy(&graph.source).into_owned();
    let source = SourceFile::new(FILE, path, text);
    let mut b = GraphBuilder::new(GraphRevision {
        id: RevisionId(1),
        project: ProjectId(1),
        parent: None,
        analysis: Analysis::SyntaxOnly,
        state: RevisionState::Draft,
    });
    b.file(source.file_ref());
    let program = b.node(None, NodeKind::Program, "program");
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let module = b.node(Some(program), NodeKind::Module, file_name);
    b.provenance(module, Provenance::at(range(0, graph.source.len() as u32)));

    let functions = ast
        .walk()
        .filter(|n| n.node_type == CNodeType::Function)
        .map(|n| ((n.span.lo, n.span.hi), n))
        .collect();
    let mut ex = Exporter {
        g: graph,
        b,
        functions,
    };
    for &root in &graph.roots {
        ex.node(root, module);
    }
    (ex.b.finish(), source)
}

struct Exporter<'a> {
    g: &'a Graph,
    b: GraphBuilder,
    /// Function syntax by span, for signature pins.
    functions: HashMap<(u32, u32), &'a CNode>,
}

fn range(lo: u32, hi: u32) -> SourceRange {
    SourceRange { file: FILE, lo, hi }
}

impl Exporter<'_> {
    fn text(&self, lo: u32, hi: u32) -> String {
        String::from_utf8_lossy(&self.g.source[lo as usize..hi as usize]).into_owned()
    }

    fn parts(n: &GraphNode) -> Vec<SyntaxPart> {
        n.parts
            .iter()
            .map(|p| SyntaxPart {
                role: names::role(p.role),
                range: range(p.span.lo, p.span.hi),
            })
            .collect()
    }

    fn node(&mut self, id: usize, parent: NodeId) -> NodeId {
        let g = self.g;
        let n = &g.nodes[id];
        let kind = names::kind(n);
        let node = self.b.node(Some(parent), kind, self.name(n));
        let mut prov = Provenance::at(range(n.span.lo, n.span.hi));
        prov.parts = Self::parts(n);
        self.b
            .template(node, format!("c.{}", n.template))
            .provenance(node, prov);

        if kind.needs_exec() {
            self.b.pin(node, PinSpec::enter());
            if kind == NodeKind::Function || self.falls_through(id) {
                self.b.pin(node, PinSpec::then());
            }
            for exit in self.escaping(id) {
                self.b.pin(node, PinSpec::exit(exit.flow()));
            }
        }
        if kind == NodeKind::Function {
            self.signature(n, node);
        }
        if kind.is_container() && !n.children.is_empty() {
            self.interior(id, node, kind);
        } else {
            for &c in &n.children {
                // Children of a data-only node (nested type declarations) sit beside it.
                self.node(c, parent);
            }
        }
        node
    }
}
