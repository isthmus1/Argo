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

/// Export one translation unit's syntax graph as a draft protocol revision plus its source.
pub fn to_protocol(graph: &Graph, ast: &Ast, path: &str) -> (argo_protocol::Graph, SourceFile) {
    let file_name = path.rsplit('/').next().unwrap_or(path).to_string();
    let text = String::from_utf8_lossy(&graph.source).into_owned();
    let source = SourceFile::new(FileId(1), path, text);
    let at = Placement::whole(source.id, graph.source.len() as u32);
    let roots: Vec<(usize, String)> = graph
        .roots
        .iter()
        .map(|&r| (r, file_name.clone()))
        .collect();
    let mut program = ProgramBuilder::new("program");
    program.add(graph, ast, &source, &at, &roots);
    (program.finish(), source)
}

/// Where the bytes of one parsed text land in the source file recorded for it: either the whole
/// text, or only some of its top-level items laid end to end (the rest, such as system headers,
/// left out of the recorded file). Every exported range must fall inside one of the kept spans.
pub struct Placement {
    pub file: FileId,
    /// `(lo, hi, at)`: parsed bytes `lo..hi` are recorded from offset `at`; sorted by `lo`.
    spans: Vec<(u32, u32, u32)>,
}

impl Placement {
    /// The whole parsed text, unchanged.
    pub fn whole(file: FileId, len: u32) -> Self {
        Placement {
            file,
            spans: vec![(0, len, 0)],
        }
    }

    /// Parsed bytes `lo..hi` of each span, in order, recorded at the offsets given.
    pub fn spans(file: FileId, spans: Vec<(u32, u32, u32)>) -> Self {
        debug_assert!(
            spans.windows(2).all(|w| w[0].1 <= w[1].0),
            "spans are sorted and disjoint"
        );
        Placement { file, spans }
    }

    /// The recorded offset of parsed offset `pos`, where a range starts (`end` false) or ends.
    /// When two kept spans touch, their shared offset is the first's end and the second's start.
    /// A position outside every span (which an exported node never has, since each lies inside
    /// its top-level item) is clamped to the nearest edge of the span before it.
    fn at(&self, pos: u32, end: bool) -> u32 {
        let i = self
            .spans
            .partition_point(|&(lo, _, _)| if end { lo < pos } else { lo <= pos })
            .saturating_sub(1);
        self.spans
            .get(i)
            .map_or(0, |&(lo, hi, at)| at + (pos.clamp(lo, hi) - lo))
    }

    fn range(&self, lo: u32, hi: u32) -> SourceRange {
        let lo = self.at(lo, false);
        SourceRange {
            file: self.file,
            lo,
            hi: self.at(hi, true).max(lo),
        }
    }
}

/// One program assembled from any number of parsed texts: each adds its top-level items to the
/// module named for them, and a module several texts add to is one node.
pub struct ProgramBuilder {
    b: GraphBuilder,
    program: NodeId,
    modules: HashMap<String, NodeId>,
}

impl ProgramBuilder {
    pub fn new(program: &str) -> Self {
        let mut b = GraphBuilder::new(GraphRevision {
            id: RevisionId(1),
            project: ProjectId(1),
            parent: None,
            analysis: Analysis::SyntaxOnly,
            state: RevisionState::Draft,
        });
        let program = b.node(None, NodeKind::Program, program);
        ProgramBuilder {
            b,
            program,
            modules: HashMap::new(),
        }
    }

    /// Add `roots` of `graph` (top-level items, in source order, each with its module's name),
    /// recorded in `source` as `at` places them. A module first seen here spans its items here
    /// (the hull, so they nest in it); one seen before keeps its first span, in another file.
    pub fn add(
        &mut self,
        graph: &Graph,
        ast: &Ast,
        source: &SourceFile,
        at: &Placement,
        roots: &[(usize, String)],
    ) {
        self.b.file(source.file_ref());
        let mut order: Vec<&str> = vec![];
        let mut items: HashMap<&str, (Vec<usize>, u32, u32)> = HashMap::new();
        for (root, name) in roots {
            let span = graph.nodes[*root].span;
            let entry = items.entry(name).or_insert_with(|| {
                order.push(name);
                (vec![], u32::MAX, 0)
            });
            entry.0.push(*root);
            entry.1 = entry.1.min(span.lo);
            entry.2 = entry.2.max(span.hi);
        }
        let functions = ast
            .walk()
            .filter(|n| n.node_type == CNodeType::Function)
            .map(|n| ((n.span.lo, n.span.hi), n))
            .collect();
        let mut ex = Exporter {
            g: graph,
            b: &mut self.b,
            functions,
            at,
        };
        for name in order {
            let (roots, lo, hi) = &items[name];
            let module = match self.modules.get(name) {
                Some(&m) => m,
                None => {
                    let m = ex.b.node(Some(self.program), NodeKind::Module, name);
                    ex.b.provenance(m, Provenance::at(at.range(*lo, *hi)));
                    self.modules.insert(name.to_string(), m);
                    m
                }
            };
            for &root in roots {
                ex.node(root, module);
            }
        }
    }

    /// The graph, with each statement-level piece of a function body (its sequences, loops,
    /// branches and switches: what opening the function shows) a work unit of its own, so it
    /// gets its own row and is translated by itself before the function as a whole.
    pub fn finish(self) -> argo_protocol::Graph {
        let mut graph = self.b.finish();
        let kind: HashMap<NodeId, (NodeKind, Option<NodeId>)> = graph
            .nodes
            .iter()
            .map(|n| (n.id, (n.kind, n.parent)))
            .collect();
        for n in &mut graph.nodes {
            let body = n
                .parent
                .and_then(|p| kind.get(&p))
                .filter(|(k, _)| *k == NodeKind::Scope);
            let in_function = body
                .and_then(|(_, f)| f.and_then(|f| kind.get(&f)))
                .is_some_and(|(k, _)| *k == NodeKind::Function);
            let declarations = n.template.as_str() == format!("c.{}", crate::graph::DECLARATIONS);
            if in_function && !n.kind.is_synthesized() && !declarations {
                n.work_unit = true;
            }
        }
        graph
    }
}

struct Exporter<'a> {
    g: &'a Graph,
    b: &'a mut GraphBuilder,
    /// Function syntax by span, for signature pins.
    functions: HashMap<(u32, u32), &'a CNode>,
    at: &'a Placement,
}

impl Exporter<'_> {
    fn text(&self, lo: u32, hi: u32) -> String {
        String::from_utf8_lossy(&self.g.source[lo as usize..hi as usize]).into_owned()
    }

    fn range(&self, lo: u32, hi: u32) -> SourceRange {
        self.at.range(lo, hi)
    }

    fn parts(&self, n: &GraphNode) -> Vec<SyntaxPart> {
        n.parts
            .iter()
            .map(|p| SyntaxPart {
                role: names::role(p.role),
                range: self.range(p.span.lo, p.span.hi),
            })
            .collect()
    }

    fn node(&mut self, id: usize, parent: NodeId) -> NodeId {
        let g = self.g;
        let n = &g.nodes[id];
        let kind = names::kind(n);
        let node = self.b.node(Some(parent), kind, self.name(n));
        let mut prov = Provenance::at(self.range(n.span.lo, n.span.hi));
        prov.parts = self.parts(n);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_maps_kept_spans_and_their_shared_edges() {
        // Parsed bytes 0..5 and 5..9 kept (touching), 9..20 dropped, 20..24 kept; each kept span
        // is followed by a separator byte in the recorded text.
        let at = Placement::spans(FileId(3), vec![(0, 5, 0), (5, 9, 6), (20, 24, 11)]);
        let r = |lo, hi| {
            let r = at.range(lo, hi);
            assert_eq!(r.file, FileId(3));
            (r.lo, r.hi)
        };
        assert_eq!(
            r(0, 5),
            (0, 5),
            "the end of the first is not the start of the second"
        );
        assert_eq!(r(5, 9), (6, 10));
        assert_eq!(r(6, 8), (7, 9));
        assert_eq!(r(20, 24), (11, 15));
        assert_eq!(
            r(12, 14),
            (10, 10),
            "a dropped gap clamps to the end before it"
        );
        assert_eq!(
            r(5, 5),
            (6, 6),
            "an empty range at a shared edge is never inverted"
        );
        assert_eq!(
            Placement::whole(FileId(1), 9).range(2, 9),
            SourceRange {
                file: FileId(1),
                lo: 2,
                hi: 9
            }
        );
    }
}
