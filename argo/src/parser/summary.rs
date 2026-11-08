use std::fmt::{self, Write};
use std::path::Path;

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

/// What parsing one translation unit produced.
#[derive(Debug, Clone)]
pub struct Report {
    /// The human-readable report: header, declarations, diagnostics, and the
    /// tree printed back out as C.
    pub output: String,
    /// Whether any fatal diagnostic was reported.
    pub failed: bool,
}

/// Parse the C file at `path` and print its report to stdout.
pub fn run(path: &Path) -> Result<(), String> {
    let source = read_source(path)?;
    let name = path.display().to_string();
    let report = parse_source(&name, &source)?;
    print!("{}", report.output);
    if report.failed {
        Err(format!("{name} reported, no graph built"))
    } else {
        Ok(())
    }
}

/// Parse C `source` held in memory. `name` labels it in the report.
pub fn parse_source(name: &str, source: &[u8]) -> Result<Report, String> {
    let triple: Triple = TARGET
        .parse()
        .map_err(|_| format!("the target triple {TARGET} does not parse"))?;
    let target = TargetInfo::new(triple);
    let mut interner = Interner::new();
    // Keywords are interned before any source is read, which is what makes
    // "is this identifier a keyword" a range check rather than a hash lookup.
    let keywords = Keywords::new(&mut interner, STD, GNU);

    let (pp_tokens, lex_diagnostics) = tokenize(source, 0, Options::new(), &mut interner);
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

    let mut output = String::new();
    write_report(
        &mut output,
        name,
        source,
        pp_tokens.len(),
        tokens.tokens.len(),
        &diagnostics,
        &parsed.ast,
        &interner,
    )
    .map_err(|e| format!("cannot format the report: {e}"))?;

    Ok(Report {
        output,
        failed: parsed.failed(),
    })
}

#[allow(clippy::too_many_arguments)]
fn write_report(
    out: &mut String,
    name: &str,
    source: &[u8],
    pp_tokens: usize,
    tokens: usize,
    diagnostics: &[&Diagnostic],
    ast: &Ast,
    interner: &Interner,
) -> fmt::Result {
    report_header(out, name, source, pp_tokens, tokens, diagnostics)?;
    report_declarations(out, ast, interner, source)?;
    report_diagnostics(out, diagnostics, source)?;
    writeln!(
        out,
        "\n=== rucc_ast::print: the tree written back out as C ==="
    )?;
    write!(out, "{}", rucc_ast::print(ast, interner))
}

fn read_source(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn report_header(
    out: &mut String,
    name: &str,
    source: &[u8],
    pp_tokens: usize,
    tokens: usize,
    diagnostics: &[&Diagnostic],
) -> fmt::Result {
    let errors = diagnostics.iter().filter(|d| d.severity.is_fatal()).count();
    writeln!(out, "file:        {} ({} bytes)", name, source.len())?;
    writeln!(out, "dialect:     {STD:?} + gnu, target {TARGET}")?;
    writeln!(out, "tokens:      {pp_tokens} pp-tokens -> {tokens} tokens")?;
    writeln!(out, "diagnostics: {} ({} fatal)", diagnostics.len(), errors)?;
    Ok(())
}

/// One row per top-level declaration: the boundary nodes of the graph.
///
/// `DeclId` is four bytes, so this is also the identity a graph record would
/// store. The span column is where a source-located diagnostic or a canvas
/// selection comes from.
fn report_declarations(
    out: &mut String,
    ast: &Ast,
    interner: &Interner,
    source: &[u8],
) -> fmt::Result {
    let counts = ast.counts();
    let top_level = ast.top_level();
    writeln!(
        out,
        "\ntop level:   {} declarations, {} exprs, {} stmts, {} params",
        top_level.len(),
        counts.exprs,
        counts.stmts,
        counts.params
    )?;
    writeln!(
        out,
        "\n{:<4} {:<14} {:<34} {:<12} detail",
        "id", "kind", "name", "span"
    )?;
    for &id in top_level {
        let (kind, name) = describe(ast, interner, id);
        let span = ast.decl_span(id);
        let (line, column) = line_column(source, span.lo);
        writeln!(
            out,
            "{:<4} {:<14} {:<34} {:<12} line {line}, column {column}, {} bytes",
            id.index(),
            kind,
            truncate(&name, 34),
            format!("{}..{}", span.lo, span.hi),
            span.hi.saturating_sub(span.lo),
        )?;
    }
    Ok(())
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

fn report_diagnostics(out: &mut String, diagnostics: &[&Diagnostic], source: &[u8]) -> fmt::Result {
    if diagnostics.is_empty() {
        return Ok(());
    }
    writeln!(out, "\n--- diagnostics ---")?;
    for diagnostic in diagnostics {
        let (line, column) = line_column(source, diagnostic.span.lo);
        let code = diagnostic.code.map_or("", |code| code);
        writeln!(
            out,
            "{:<7} {:<6} {line}:{column}  {}",
            diagnostic.severity, code, diagnostic.message
        )?;
        for child in &diagnostic.children {
            let (line, column) = line_column(source, child.span.lo);
            writeln!(
                out,
                "{:<7} {:<6} {line}:{column}  {}",
                child.severity, "", child.message
            )?;
        }
    }
    Ok(())
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
