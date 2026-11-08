use rucc_ast::{DeclSpecsId, Member, RecordKind, TypeSpec};

use super::Lowerer;
use crate::{CNode, CNodeType as Kind};

impl Lowerer<'_> {
    pub(super) fn type_definition(&self, specs: DeclSpecsId) -> Vec<CNode> {
        let specs = self.ast[specs];
        match specs.ty {
            TypeSpec::Record {
                kind,
                tag,
                fields: Some(fields),
                ..
            } => {
                let kind = match kind {
                    RecordKind::Struct => Kind::Struct,
                    RecordKind::Union => Kind::Union,
                };
                let mut node = self.named(kind, specs.span, tag);
                for member in &self.ast[fields] {
                    match *member {
                        Member::Field(field) => {
                            let name = field.declarator.and_then(|id| self.ast[id].name);
                            let mut child = self.named(Kind::Field, field.span, name);
                            child.children.push(match field.declarator {
                                Some(id) => self.declarator_type(field.specs, id),
                                None => self.type_reference(field.specs),
                            });
                            child.children.extend(self.type_definition(field.specs));
                            if let Some(bits) = field.bits {
                                child.children.push(self.expression(bits));
                            }
                            node.children.push(child);
                        }
                        Member::StaticAssert { cond, span, .. } => {
                            let mut child = CNode::new(Kind::StaticAssert, span);
                            child.children.push(self.expression(cond));
                            node.children.push(child);
                        }
                    }
                }
                vec![node]
            }
            TypeSpec::Enum {
                tag,
                enumerators: Some(enumerators),
                ..
            } => {
                let mut node = self.named(Kind::Enum, specs.span, tag);
                for enumerator in &self.ast[enumerators] {
                    let mut child =
                        self.named(Kind::Enumerator, enumerator.span, Some(enumerator.name));
                    if let Some(value) = enumerator.value {
                        child.children.push(self.expression(value));
                    }
                    node.children.push(child);
                }
                vec![node]
            }
            _ => Vec::new(),
        }
    }
}
