use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rucc_ast::{Ast, Decl, DeclId};
use rucc_base::Interner;
use rucc_diag::{BytePos, Diagnostic};
use rucc_lex::{Convert, Keywords, Options, convert, tokenize};
use rucc_parse::{Context, parse};
use rucc_session::Std;
use rucc_target::{TargetInfo, Triple};

const STD: Std = Std::C11;
const GNU: bool = true;
const TARGET: &str = "x86_64-unknown-linux-gnu";
fn main() -> ExitCode {
    let path = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("tests/fixture.c"), PathBuf::from);

    match run(&path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(path: &Path) -> Result<(), String> {
    let source = read_source(path)?;

    let triple: Triple = TARGET
        .parse()
        .map_err(|_| format!("the target triple {TARGET} does not parse"))?;
    let target = TargetInfo::new(triple);
    let mut interner = Interner::new();
    // Keywords are interned before any source is read, which is what makes
    // "is this identifier a keyword" a range check rather than a hash lookup.
    let keywords = Keywords::new(&mut interner, STD, GNU);

    let (pp_tokens, lex_diagnostics) = tokenize(&source, 0, Options::new(), &mut interner);
    let cx = Convert {
        keywords: &keywords,
        interner: &interner,
        target: &target,
        std: STD,
        gnu: GNU,
        pedantic: false,
    };
    let (tokens, convert_diagnostics) = convert(&pp_tokens, &cx);
    let parsed = parse(&tokens, Context::new(&interner, STD));

    let diagnostics: Vec<&Diagnostic> = lex_diagnostics
        .iter()
        .chain(convert_diagnostics.iter())
        .chain(parsed.diagnostics.iter())
        .collect();

    report_header(
        path,
        &source,
        pp_tokens.len(),
        tokens.tokens.len(),
        &diagnostics,
    );
    report_declarations(&parsed.ast, &interner, &source);
    report_diagnostics(&diagnostics, &source);

    println!("\n=== rucc_ast::print: the tree written back out as C ===");
    print!("{}", rucc_ast::print(&parsed.ast, &interner));

    if parsed.failed() {
        Err(format!("{} reported, no graph built", path.display()))
    } else {
        Ok(())
    }
}

/// The phase-7 input: the file's bytes.
///
/// `rucc-pp` replaces this. Until then a file with a directive in it comes back
/// as a lexer or parser diagnostic rather than as a wrong tree, which is the
/// failure mode we want while the fixture is small.
fn read_source(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn report_header(
    path: &Path,
    source: &[u8],
    pp_tokens: usize,
    tokens: usize,
    diagnostics: &[&Diagnostic],
) {
    let errors = diagnostics.iter().filter(|d| d.severity.is_fatal()).count();
    println!("file:        {} ({} bytes)", path.display(), source.len());
    println!("dialect:     {STD:?} + gnu, target {TARGET}");
    println!("tokens:      {pp_tokens} pp-tokens -> {tokens} tokens");
    println!("diagnostics: {} ({} fatal)", diagnostics.len(), errors);
}

/// One row per top-level declaration: the boundary nodes of the graph.
///
/// `DeclId` is four bytes, so this is also the identity a graph record would
/// store. The span column is where a source-located diagnostic or a canvas
/// selection comes from.
fn report_declarations(ast: &Ast, interner: &Interner, source: &[u8]) {
    let counts = ast.counts();
    let top_level = ast.top_level();
    println!(
        "\ntop level:   {} declarations, {} exprs, {} stmts, {} params",
        top_level.len(),
        counts.exprs,
        counts.stmts,
        counts.params
    );
    println!(
        "\n{:<4} {:<14} {:<34} {:<12} detail",
        "id", "kind", "name", "span"
    );
    for &id in top_level {
        let (kind, name) = describe(ast, interner, id);
        let span = ast.decl_span(id);
        let (line, column) = line_column(source, span.lo);
        println!(
            "{:<4} {:<14} {:<34} {:<12} line {line}, column {column}, {} bytes",
            id.index(),
            kind,
            truncate(&name, 34),
            format!("{}..{}", span.lo, span.hi),
            span.hi.saturating_sub(span.lo),
        );
    }
}

/// What a declaration is, in the vocabulary the block templates use.
fn describe(ast: &Ast, interner: &Interner, id: DeclId) -> (&'static str, String) {
    match &ast[id] {
        Decl::Function { declarator, .. } => {
            ("function", declarator_name(ast, interner, *declarator))
        }
        Decl::Var { declarators, .. } => {
            let names: Vec<&str> = ast[*declarators]
                .iter()
                .filter_map(|init| ast[init.declarator].name)
                .map(|name| interner.resolve(name))
                .collect();
            if names.is_empty() {
                // `struct ram { ... };` declares a tag and no object.
                ("type", "<tag only>".to_string())
            } else {
                ("variable", names.join(", "))
            }
        }
        Decl::StaticAssert { .. } => ("static_assert", String::new()),
        Decl::Asm(_) => ("asm", String::new()),
        Decl::Attributes(_) => ("attribute", String::new()),
        Decl::Error => ("error", "<poisoned>".to_string()),
    }
}

fn declarator_name(ast: &Ast, interner: &Interner, id: rucc_ast::DeclaratorId) -> String {
    match ast[id].name {
        Some(symbol) => interner.resolve(symbol).to_string(),
        None => "<abstract>".to_string(),
    }
}

fn report_diagnostics(diagnostics: &[&Diagnostic], source: &[u8]) {
    if diagnostics.is_empty() {
        return;
    }
    println!("\n--- diagnostics ---");
    for diagnostic in diagnostics {
        let (line, column) = line_column(source, diagnostic.span.lo);
        let code = diagnostic.code.map_or("", |code| code);
        println!(
            "{:<7} {:<6} {line}:{column}  {}",
            diagnostic.severity, code, diagnostic.message
        );
        for child in &diagnostic.children {
            let (line, column) = line_column(source, child.span.lo);
            println!(
                "{:<7} {:<6} {line}:{column}  {}",
                child.severity, "", child.message
            );
        }
    }
}

/// `BytePos` to a 1-based line and column.
///
/// The real mapping is `rucc_diag::SourceMap`, which knows about every file in
/// the translation unit and about macro expansions. One file with no
/// preprocessor is small enough to count here, and doing it by hand keeps this
/// example from depending on the session plumbing before it is needed.
fn line_column(source: &[u8], position: BytePos) -> (usize, usize) {
    let position = (position as usize).min(source.len());
    let before = &source[..position];
    let line = before.iter().filter(|byte| **byte == b'\n').count() + 1;
    let column = before
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(position, |newline| position - newline - 1)
        + 1;
    (line, column)
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}
