use super::{ChildRegion, Expansion, Graph, GraphDiagnostic, GraphEdge, GraphEdgeKind};
use super::{GraphError, GraphNode, NodeId, validation};
use crate::{Ast, Template};

pub fn build(ast: &Ast, source: &[u8], templates: &[impl Template]) -> Result<Graph, GraphError> {
    for node in ast.walk() {
        validation::span(source, node.span.into())?;
    }
    let mut builder = Builder {
        templates,
        graph: Graph {
            schema_version: 1,
            analysis: "syntax_only",
            source: source.to_vec(),
            roots: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            diagnostics: Vec::new(),
        },
    };
    for node in &ast.roots {
        builder.region(ChildRegion::Syntax(node), None)?;
    }
    Ok(builder.graph)
}

pub(super) struct Builder<'a, T> {
    pub templates: &'a [T],
    pub graph: Graph,
}

impl<T: Template> Builder<'_, T> {
    pub(super) fn insert(
        &mut self,
        expansion: Expansion<'_>,
        parent: Option<NodeId>,
        template: &str,
    ) -> Result<(), GraphError> {
        let parent = if let Some(draft) = expansion.node {
            validation::draft(
                &draft,
                &self.graph.source,
                parent.map(|id| self.graph.nodes[id].span),
            )?;
            let id = self.graph.nodes.len();
            self.graph.nodes.push(GraphNode {
                id,
                parent,
                kind: draft.kind,
                template: template.into(),
                name: draft.name,
                role: draft.role,
                span: draft.span,
                parts: draft.parts,
                exits: draft.exits,
                children: Vec::new(),
            });
            if let Some(message) = draft.diagnostic {
                self.graph
                    .diagnostics
                    .push(GraphDiagnostic { node: id, message });
            }
            if let Some(parent) = parent {
                self.graph.nodes[parent].children.push(id);
                self.graph.edges.push(GraphEdge {
                    source: parent,
                    target: id,
                    kind: GraphEdgeKind::Contains,
                });
            } else {
                self.graph.roots.push(id);
            }
            Some(id)
        } else {
            parent
        };
        for child in expansion.children {
            self.region(child, parent)?;
        }
        Ok(())
    }
}
