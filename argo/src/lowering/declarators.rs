use rucc_ast::{ArraySize, DeclSpecsId, DeclaratorId, Derived, TypeNameId};
use rucc_diag::Span;

use super::Lowerer;
use crate::{CNode, CNodeType as Kind};

impl Lowerer<'_> {
    /// Fold outward declarator steps onto the base type, retaining their order.
    /// Derived type nodes use the full specifier/declarator span, not a token span.
    pub(super) fn declarator_type(&self, specs: DeclSpecsId, id: DeclaratorId) -> CNode {
        let declarator = self.ast[id];
        let base_span = self.ast[specs].span;
        let span = Span::new(
            base_span.lo.min(declarator.span.lo),
            base_span.hi.max(declarator.span.hi),
        );
        let mut ty = self.type_reference(specs);
        for derived in self.ast[declarator.derived].iter().rev() {
            let mut node = CNode::new(Kind::Type, span);
            node.children.push(ty);
            match *derived {
                Derived::Pointer { quals, .. } => {
                    node.name = Some(format!("pointer {quals:?}"));
                }
                Derived::Array {
                    size,
                    quals,
                    has_static,
                } => {
                    node.name = Some(format!("array {size:?} {quals:?} static={has_static}"));
                    if let ArraySize::Expr(expr) = size {
                        node.children.push(self.expression(expr));
                    }
                }
                Derived::Function {
                    params,
                    variadic,
                    kind,
                } => {
                    node.name = Some(format!("function {kind:?} variadic={variadic}"));
                    node.is_variadic = variadic;
                    for param in &self.ast[params] {
                        let mut child = self.named(
                            Kind::Parameter,
                            param.span,
                            self.ast[param.declarator].name,
                        );
                        if let Some(specs) = param.specs {
                            child
                                .children
                                .push(self.declarator_type(specs, param.declarator));
                        }
                        node.children.push(child);
                    }
                }
            }
            ty = node;
        }
        ty
    }

    pub(super) fn type_name(&self, id: TypeNameId) -> CNode {
        self.declarator_type(self.ast[id].specs, self.ast[id].declarator)
    }
}
