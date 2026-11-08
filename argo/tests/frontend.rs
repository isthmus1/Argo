use main::{CNodeType, Lexer, Parser};
use rucc_session::Std;

#[test]
fn all_frontend_fatal_errors_reject_the_public_tree() {
    let parser = Parser::default();
    for source in ["int x; @", "int x; /*", "int x = 08;", "int x = ;"] {
        let parsed = parser.parse(source.as_bytes());
        assert!(parsed.failed(), "expected rejection: {source}");
        assert!(
            parsed.ast.roots.is_empty(),
            "no tree after fatal errors: {source}"
        );
    }
}

#[test]
fn preprocessing_is_not_silently_accepted() {
    let parsed = Parser::default().parse(b"#define X 1\nint x = X;\n");
    assert!(parsed.failed());
    assert!(parsed.ast.roots.is_empty());
}

#[test]
fn a_parser_is_reusable_without_symbol_or_error_leaks() {
    let parser = Parser::default();
    assert!(parser.parse(b"@").failed());
    let first = parser.parse(b"int first;");
    let second = parser.parse(b"int second;");
    assert!(!first.failed());
    assert!(!second.failed());
    assert_eq!(first.ast.roots[0].name.as_deref(), Some("first"));
    assert_eq!(second.ast.roots[0].name.as_deref(), Some("second"));
    assert!(first.as_c().contains("first"));
    assert!(!first.as_c().contains("second"));
}

#[test]
fn the_ast_owns_its_names_and_source_after_input_and_parser_drop() {
    let parsed = {
        let source = String::from("int owned;");
        Parser::default().parse(source.as_bytes())
    };
    let cloned_tree = parsed.ast.clone();
    assert_eq!(cloned_tree.roots[0].name.as_deref(), Some("owned"));
    assert!(cloned_tree.roots[0].source(&parsed.source).is_some());
}

#[test]
fn lexical_configuration_is_explicit_and_invalid_targets_fail() {
    assert!(Lexer::new("not-a-valid-target", Std::C11, true).is_err());
    let lexer = Lexer::new("x86_64-unknown-linux-gnu", Std::C11, false).unwrap();
    assert!(!lexer.gnu());
    let parser = Parser::new(lexer);
    assert!(!parser.parse(b"int plain;").failed());
}

#[test]
fn lexer_owns_tokens_diagnostics_and_symbols_for_each_input() {
    let lexer = Lexer::default();
    let bad = lexer.lex(b"int bad = 08;");
    let good = lexer.lex(b"int good;");
    assert!(bad.diagnostics.iter().any(|d| d.severity.is_fatal()));
    assert!(good.diagnostics.is_empty());
    assert!(good.pp_token_count > 0);
    assert!(!good.tokens.tokens.is_empty());
}

#[test]
fn empty_input_and_unsupported_syntax_have_explicit_results() {
    let parser = Parser::default();
    let empty = parser.parse(b"");
    assert!(!empty.failed());
    assert!(empty.ast.roots.is_empty());
    let extension = parser.parse(b"int x = __builtin_choose_expr(1, 2, 3);");
    assert!(!extension.failed());
    let opaque = extension
        .ast
        .walk()
        .find(|n| n.node_type == CNodeType::Opaque)
        .unwrap();
    assert_eq!(
        opaque.source(&extension.source).unwrap(),
        b"__builtin_choose_expr(1, 2, 3)"
    );
}
