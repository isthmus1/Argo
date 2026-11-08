use rucc_ast::{Expr, ExprId};

use super::Lowerer;
use crate::{CNode, CNodeType as Kind};

impl Lowerer<'_> {
    pub(super) fn expression(&self, id: ExprId) -> CNode {
        let span = self.ast.expr_span(id);
        let mut node = CNode::new(Kind::Expression, span);
        let (name, children) = match self.ast[id] {
            Expr::Name(symbol) | Expr::LabelAddr(symbol) => {
                (self.interner.resolve(symbol).to_string(), Vec::new())
            }
            Expr::Int(_)
            | Expr::Float(_)
            | Expr::Char(_)
            | Expr::Str(_)
            | Expr::Bool(_)
            | Expr::Nullptr => (self.text(span), Vec::new()),
            Expr::Unary { op, operand } => (format!("{op:?}"), vec![self.expression(operand)]),
            Expr::Binary { op, lhs, rhs } => (
                op.spelling().into(),
                vec![self.expression(lhs), self.expression(rhs)],
            ),
            Expr::Assign { op, lhs, rhs } => (
                op.map_or_else(|| "=".into(), |op| format!("{}=", op.spelling())),
                vec![self.expression(lhs), self.expression(rhs)],
            ),
            Expr::Index { base, index } => (
                "index".into(),
                vec![self.expression(base), self.expression(index)],
            ),
            Expr::Comma { lhs, rhs } => {
                (",".into(), vec![self.expression(lhs), self.expression(rhs)])
            }
            Expr::Call { callee, args } => {
                let mut children = vec![self.expression(callee)];
                children.extend(self.ast[args].iter().map(|&id| self.expression(id)));
                ("call".into(), children)
            }
            Expr::Member { base, name, arrow } => (
                format!(
                    "{}{}",
                    if arrow { "->" } else { "." },
                    self.interner.resolve(name)
                ),
                vec![self.expression(base)],
            ),
            Expr::Cond {
                cond,
                then,
                otherwise,
            } => {
                let mut children = vec![self.expression(cond)];
                children.extend(then.map(|id| self.expression(id)));
                children.push(self.expression(otherwise));
                ("?:".into(), children)
            }
            Expr::Cast { ty, operand } => (
                "cast".into(),
                vec![self.type_name(ty), self.expression(operand)],
            ),
            Expr::SizeofExpr(expr) => ("sizeof".into(), vec![self.expression(expr)]),
            Expr::AlignofExpr(expr) => ("alignof".into(), vec![self.expression(expr)]),
            Expr::SizeofType(ty) | Expr::AlignofType(ty) => {
                (self.text(span), vec![self.type_name(ty)])
            }
            Expr::CompoundLiteral { ty, init } => (
                "compound literal".into(),
                vec![self.type_name(ty), self.initializer(init, span)],
            ),
            Expr::StmtExpr(body) => ("statement expression".into(), vec![self.statement(body)]),
            Expr::Extension(expr) => ("__extension__".into(), vec![self.expression(expr)]),
            _ => return CNode::new(Kind::Opaque, span).named("unsupported expression"),
        };
        node.name = Some(name);
        node.children = children;
        node
    }
}
