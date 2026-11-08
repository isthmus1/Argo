use rucc_ast::{Stmt, StmtId};

use super::Lowerer;
use crate::{CNode, CNodeType as Kind};

impl Lowerer<'_> {
    pub(super) fn statement(&self, id: StmtId) -> CNode {
        let span = self.ast.stmt_span(id);
        let (kind, children) = match self.ast[id] {
            Stmt::Empty => (Kind::Empty, Vec::new()),
            Stmt::Expr(expr) => return self.expression(expr),
            Stmt::Decl(decl) => (Kind::Declaration, self.declaration(decl)),
            Stmt::Compound(statements) => (
                Kind::Block,
                self.ast[statements]
                    .iter()
                    .map(|&id| self.statement(id))
                    .collect(),
            ),
            Stmt::Return(value, _) => (
                Kind::Return,
                value.into_iter().map(|id| self.expression(id)).collect(),
            ),
            Stmt::Break => (Kind::Break, Vec::new()),
            Stmt::Continue => (Kind::Continue, Vec::new()),
            Stmt::Goto(name) => return self.named(Kind::Goto, span, Some(name)),
            Stmt::Label { name, body, .. } => {
                let mut node = self.named(Kind::Label, span, Some(name));
                node.children.extend(body.map(|id| self.statement(id)));
                return node;
            }
            Stmt::Case { lo, hi, body } => {
                let mut children = vec![self.expression(lo)];
                children.extend(hi.map(|id| self.expression(id)));
                children.extend(body.map(|id| self.statement(id)));
                (Kind::Case, children)
            }
            Stmt::Default { body } => (
                Kind::Default,
                body.into_iter().map(|id| self.statement(id)).collect(),
            ),
            Stmt::GotoExpr(expr) => {
                let mut node = CNode::new(Kind::Opaque, span).named("computed goto");
                node.children.push(self.expression(expr));
                return node;
            }
            Stmt::Asm(_) | Stmt::LocalLabels(_) | Stmt::Error => {
                return CNode::new(Kind::Opaque, span).named("unsupported statement");
            }
            _ => return self.control_statement(id),
        };
        let mut node = CNode::new(kind, span);
        node.children = children;
        node
    }
}
