use main::templates;
use main::{Ast, CNode, CNodeType as Kind};
use rucc_diag::Span;

#[test]
fn every_keyword_recognizer_wins_before_the_opaque_fallback() {
    let expected = [
        (Kind::Function, "function"),
        (Kind::Struct, "struct"),
        (Kind::Union, "union"),
        (Kind::Enum, "enum"),
        (Kind::Typedef, "typedef"),
        (Kind::Variable, "variable"),
        (Kind::Declaration, "declaration"),
        (Kind::If, "if"),
        (Kind::Else, "else"),
        (Kind::For, "for"),
        (Kind::While, "while"),
        (Kind::DoWhile, "do"),
        (Kind::Switch, "switch"),
        (Kind::Case, "case"),
        (Kind::Default, "default"),
        (Kind::Block, "block"),
        (Kind::Return, "return"),
        (Kind::Break, "break"),
        (Kind::Continue, "continue"),
        (Kind::StaticAssert, "static_assert"),
        (Kind::Expression, "expression"),
        (Kind::Initializer, "expression"),
        (Kind::Type, "type"),
        (Kind::Field, "type"),
        (Kind::Parameter, "type"),
        (Kind::Enumerator, "type"),
        (Kind::Label, "opaque"),
        (Kind::Goto, "opaque"),
        (Kind::Empty, "opaque"),
        (Kind::Opaque, "opaque"),
    ];
    let ast = Ast::new(
        expected
            .iter()
            .map(|&(kind, _)| CNode::new(kind, Span::DUMMY))
            .collect(),
    );
    let matches = ast.match_templates(templates::all());
    assert_eq!(matches.len(), expected.len());
    for (matched, &(kind, template)) in matches.iter().zip(&expected) {
        assert_eq!(matched.node.node_type, kind);
        assert_eq!(matched.template, template, "{kind:?}");
    }
}
