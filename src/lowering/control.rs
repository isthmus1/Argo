use rucc_ast::{ForInit, Stmt, StmtId};

use super::Lowerer;
use crate::{CNode, CNodeRole as Role, CNodeType as Kind};

impl Lowerer<'_> {
    pub(super) fn control_statement(&self, id: StmtId) -> CNode {
        let span = self.ast.stmt_span(id);
        let (kind, children) = match self.ast[id] {
            Stmt::If {
                cond,
                then,
                otherwise,
            } => {
                let mut children = vec![
                    self.expression(cond).with_role(Role::Condition),
                    self.statement(then).with_role(Role::Then),
                ];
                if let Some(otherwise) = otherwise {
                    let branch_span = rucc_diag::Span::new(
                        self.ast.stmt_span(then).hi,
                        self.ast.stmt_span(otherwise).hi,
                    );
                    let mut branch = CNode::new(Kind::Else, branch_span).with_role(Role::Else);
                    branch
                        .children
                        .push(self.statement(otherwise).with_role(Role::Body));
                    children.push(branch);
                }
                (Kind::If, children)
            }
            Stmt::While { cond, body } => (
                Kind::While,
                vec![
                    self.expression(cond).with_role(Role::Condition),
                    self.statement(body).with_role(Role::Body),
                ],
            ),
            Stmt::DoWhile { body, cond } => (
                Kind::DoWhile,
                vec![
                    self.statement(body).with_role(Role::Body),
                    self.expression(cond).with_role(Role::Condition),
                ],
            ),
            Stmt::Switch { scrutinee, body } => (
                Kind::Switch,
                vec![
                    self.expression(scrutinee).with_role(Role::Selector),
                    self.statement(body).with_role(Role::Body),
                ],
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
                for child in &mut children {
                    child.role = Some(Role::Initializer);
                }
                children.extend(cond.map(|id| self.expression(id).with_role(Role::Condition)));
                children.extend(step.map(|id| self.expression(id).with_role(Role::Step)));
                children.push(self.statement(body).with_role(Role::Body));
                (Kind::For, children)
            }
            _ => return CNode::new(Kind::Opaque, span).named("unsupported control construct"),
        };
        let mut node = CNode::new(kind, span);
        node.children = children;
        node
    }
}
