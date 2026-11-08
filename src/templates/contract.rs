use crate::CNode;
use crate::graph::{Expansion, Unsupported};

/// A syntax recognizer. These matches are not visual nodes or graph interfaces.
/// Put more-specific recognizers before general ones in the input list.
pub trait Template: Send + Sync {
    fn id(&self) -> &'static str;
    fn matches(&self, node: &CNode) -> bool;

    /// Unsupported expansion falls through to the next matching template.
    fn expand<'a>(&self, _node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        Err(Unsupported("this template only recognizes syntax"))
    }
}

/// Allow mixed concrete template types in `Vec<Box<dyn Template>>`.
impl<T: Template + ?Sized> Template for Box<T> {
    fn id(&self) -> &'static str {
        (**self).id()
    }

    fn matches(&self, node: &CNode) -> bool {
        (**self).matches(node)
    }

    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        (**self).expand(node)
    }
}

/// Borrows syntax from the AST, never from the consumed template list.
#[derive(Debug, Clone, Copy)]
pub struct TemplateMatch<'a> {
    pub template: &'static str,
    pub node: &'a CNode,
}
