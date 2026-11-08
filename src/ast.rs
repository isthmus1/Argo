use crate::c_node::CNode;
use crate::templates::{Template, TemplateMatch};

/// An owned C syntax forest, one root for each top-level syntactic item.
///
/// Definitions sharing one C declaration (a struct and its objects, for
/// example) can have overlapping spans. They are distinct syntax items, not
/// separate safe editing or migration boundaries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ast {
    pub roots: Vec<CNode>,
}

impl Ast {
    pub fn new(roots: Vec<CNode>) -> Self {
        Self { roots }
    }

    pub fn walk(&self) -> impl Iterator<Item = &CNode> {
        self.roots.iter().flat_map(CNode::walk)
    }

    /// Match nodes in preorder; the first matching template wins at each node.
    ///
    /// Use `templates::all()` for the built-ins, or pass your own ordered list.
    /// `Box<dyn Template>` implements `Template`, so heterogeneous lists work
    /// without sacrificing the `Vec<impl Template>` calling convention.
    /// Unmatched nodes stay in the AST but produce no match. Matching never
    /// mutates syntax or implies executable graph/ABI boundaries.
    pub fn match_templates(&self, templates: Vec<impl Template>) -> Vec<TemplateMatch<'_>> {
        self.walk()
            .filter_map(|node| {
                templates
                    .iter()
                    .find(|template| template.matches(node))
                    .map(|template| TemplateMatch {
                        template: template.id(),
                        node,
                    })
            })
            .collect()
    }
}
