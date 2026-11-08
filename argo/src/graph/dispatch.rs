use super::builder::Builder;
use super::{ChildRegion, GraphError, GraphNodeKind, NodeId, cases, sequence};
use crate::{CNode, Template};

impl<T: Template> Builder<'_, T> {
    pub(super) fn region(
        &mut self,
        region: ChildRegion<'_>,
        parent: Option<NodeId>,
    ) -> Result<(), GraphError> {
        match region {
            ChildRegion::Syntax(node) => self.syntax(node, parent),
            ChildRegion::Sequence(nodes) => {
                let template = sequence::template(&nodes);
                self.insert(sequence::expand(&nodes)?, parent, template)
            }
            ChildRegion::CaseGroup { labels, statements } => {
                self.insert(cases::expand(&labels, &statements)?, parent, "case_group")
            }
        }
    }

    fn syntax(&mut self, node: &CNode, parent: Option<NodeId>) -> Result<(), GraphError> {
        let mut rejected = Vec::new();
        let mut selected = None;
        for template in self.templates {
            if !template.matches(node) {
                continue;
            }
            match template.expand(node) {
                Ok(mut expansion) => {
                    if let Some(draft) = expansion.node.as_mut()
                        && draft.kind == GraphNodeKind::Opaque
                        && !rejected.is_empty()
                    {
                        draft.diagnostic = Some(rejected.join("; "));
                    }
                    selected = Some((template.id(), expansion));
                    break;
                }
                Err(reason) => rejected.push(format!("{}: {}", template.id(), reason.0)),
            }
        }
        let (template, expansion) = selected.ok_or(GraphError::NoTemplate(node.span.into()))?;
        self.insert(expansion, parent, template)
    }
}
