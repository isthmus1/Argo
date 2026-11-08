use rucc_ast::{Designator, Init, InitId};
use rucc_diag::Span;

use super::Lowerer;
use crate::{CNode, CNodeType as Kind};

impl Lowerer<'_> {
    pub(super) fn initializer(&self, id: InitId, span: Span) -> CNode {
        let mut node = CNode::new(Kind::Initializer, span);
        match self.ast[id] {
            Init::Expr(expr) => {
                node.span = self.ast.expr_span(expr);
                node.children.push(self.expression(expr));
            }
            Init::List(items) => {
                for item in &self.ast[items] {
                    let mut child = self.initializer(item.init, item.span);
                    child.span = item.span;
                    let mut designators = Vec::new();
                    for designator in &self.ast[item.designators] {
                        match *designator {
                            Designator::Field(name) | Designator::ObsoleteField(name) => {
                                designators.push(self.interner.resolve(name).to_string());
                            }
                            Designator::Index(index) => {
                                child.children.insert(0, self.expression(index));
                            }
                            Designator::Range { lo, hi } => {
                                child.children.insert(0, self.expression(hi));
                                child.children.insert(0, self.expression(lo));
                            }
                        }
                    }
                    if !designators.is_empty() {
                        child.name = Some(designators.join("."));
                    }
                    node.children.push(child);
                }
            }
        }
        node
    }
}
