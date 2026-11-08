//! Convert rucc's index arenas into the public owned syntax tree.
//! Unsupported syntax remains source-located and opaque; no semantic guesses.

mod control;
mod declarations;
mod declarators;
mod expressions;
mod initializers;
mod statements;
mod types;

use rucc_ast::Ast as RawAst;
use rucc_base::{Interner, Symbol};
use rucc_diag::Span;

use crate::{Ast, CNode, CNodeType};

struct Lowerer<'a> {
    ast: &'a RawAst,
    interner: &'a Interner,
    source: &'a [u8],
}

pub(crate) fn lower(ast: &RawAst, interner: &Interner, source: &[u8]) -> Ast {
    let lowerer = Lowerer {
        ast,
        interner,
        source,
    };
    Ast::new(
        ast.top_level()
            .iter()
            .flat_map(|&id| lowerer.declaration(id))
            .collect(),
    )
}

impl Lowerer<'_> {
    fn text(&self, span: Span) -> String {
        let bytes = self
            .source
            .get(span.lo as usize..span.hi as usize)
            .unwrap_or_default();
        String::from_utf8_lossy(bytes).into_owned()
    }

    fn named(&self, node_type: CNodeType, span: Span, name: Option<Symbol>) -> CNode {
        let mut node = CNode::new(node_type, span);
        node.name = name.map(|name| self.interner.resolve(name).to_string());
        node
    }
}
