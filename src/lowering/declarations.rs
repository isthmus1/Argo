use rucc_ast::{Decl, DeclId, DeclSpecsId, Derived, StorageClass};

use super::Lowerer;
use crate::{CNode, CNodeType as Kind};

impl Lowerer<'_> {
    pub(super) fn declaration(&self, id: DeclId) -> Vec<CNode> {
        let span = self.ast.decl_span(id);
        match self.ast[id] {
            Decl::Function {
                specs,
                declarator,
                params,
                body,
            } => {
                let mut node = self.named(Kind::Function, span, self.ast[declarator].name);
                node.children.push(self.declarator_type(specs, declarator));
                for &declaration in &self.ast[params] {
                    node.children.extend(self.declaration(declaration));
                }
                node.children.push(self.statement(body));
                vec![node]
            }
            Decl::Var { specs, declarators } => {
                let mut nodes = self.type_definition(specs);
                for init in &self.ast[declarators] {
                    let kind = self.binding_kind(specs, init);
                    let span = rucc_diag::Span::new(self.ast[specs].span.lo, init.span.hi);
                    let mut node = self.named(kind, span, self.ast[init.declarator].name);
                    node.children
                        .push(self.declarator_type(specs, init.declarator));
                    if let Some(initializer) = init.init {
                        node.children.push(self.initializer(initializer, init.span));
                    }
                    nodes.push(node);
                }
                if nodes.is_empty() {
                    nodes.push(self.type_reference(specs));
                }
                nodes
            }
            Decl::StaticAssert { cond, .. } => {
                let mut node = CNode::new(Kind::StaticAssert, span);
                node.children.push(self.expression(cond));
                vec![node]
            }
            _ => vec![CNode::new(Kind::Opaque, span).named("unsupported declaration")],
        }
    }

    fn binding_kind(&self, specs: DeclSpecsId, init: &rucc_ast::InitDeclarator) -> Kind {
        let specs = self.ast[specs];
        if specs.is_typedef() {
            return Kind::Typedef;
        }
        let derived = &self.ast[self.ast[init.declarator].derived];
        let prototype = matches!(derived.first(), Some(Derived::Function { .. }));
        let external = specs.storage == Some(StorageClass::Extern) && init.init.is_none();
        if prototype || external {
            Kind::Declaration
        } else {
            Kind::Variable
        }
    }

    pub(super) fn type_reference(&self, specs: DeclSpecsId) -> CNode {
        let span = self.ast[specs].span;
        CNode::new(Kind::Type, span).named(self.text(span))
    }
}
