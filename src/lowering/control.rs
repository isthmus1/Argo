use rucc_ast::{ForInit, Stmt, StmtId};

use super::Lowerer;
use crate::{CNode, CNodeType as Kind};

impl Lowerer<'_> {
    pub(super) fn control_statement(&self, id: StmtId) -> CNode {
        let span = self.ast.stmt_span(id);
        let (kind, children) = match self.ast[id] {
            Stmt::If {
                cond,
                then,
                otherwise,
            } => {
                let mut children = vec![self.expression(cond), self.statement(then)];
                if let Some(otherwise) = otherwise {
                    let branch_span = rucc_diag::Span::new(
                        self.ast.stmt_span(then).hi,
                        self.ast.stmt_span(otherwise).hi,
                    );
                    let mut branch = CNode::new(Kind::Else, branch_span);
                    branch.children.push(self.statement(otherwise));
                    children.push(branch);
                }
                (Kind::If, children)
            }
            Stmt::While { cond, body } => (
                Kind::While,
                vec![self.expression(cond), self.statement(body)],
            ),
            Stmt::DoWhile { body, cond } => (
                Kind::DoWhile,
                vec![self.statement(body), self.expression(cond)],
            ),
            Stmt::Switch { scrutinee, body } => (
                Kind::Switch,
                vec![self.expression(scrutinee), self.statement(body)],
            ),
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                let mut children = match init {
                    ForInit::None => Vec::new(),
                    ForInit::Expr(expr) => vec![self.expression(expr)],
                    ForInit::Decl(decl) => self.declaration(decl),
                };
                children.extend(cond.map(|id| self.expression(id)));
                children.extend(step.map(|id| self.expression(id)));
                children.push(self.statement(body));
                (Kind::For, children)
            }
            _ => return CNode::new(Kind::Opaque, span).named("unsupported control construct"),
        };
        let mut node = CNode::new(kind, span);
        node.children = children;
        node
    }
}
