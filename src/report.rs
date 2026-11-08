use std::collections::BTreeMap;
use std::path::Path;

use crate::source::{line_column, truncate};
use crate::{CNode, ParseResult, Parser, TemplateMatch};

pub fn header(path: &Path, parser: &Parser, parsed: &ParseResult) {
    let errors = parsed
        .diagnostics
        .iter()
        .filter(|d| d.severity.is_fatal())
        .count();
    println!(
        "file:        {} ({} bytes)",
        path.display(),
        parsed.source.len()
    );
    println!(
        "dialect:     {:?}, gnu={}, target {}",
        parser.lexer().standard(),
        parser.lexer().gnu(),
        parser.lexer().target(),
    );
    println!(
        "tokens:      {} pp-tokens -> {} tokens",
        parsed.pp_token_count, parsed.token_count
    );
    println!(
        "diagnostics: {} ({} fatal)",
        parsed.diagnostics.len(),
        errors
    );
}

pub fn diagnostics(parsed: &ParseResult) {
    for diagnostic in &parsed.diagnostics {
        let (line, column) = line_column(&parsed.source, diagnostic.span.lo);
        eprintln!(
            "{:<7} {:<6} {line}:{column}  {}",
            diagnostic.severity,
            diagnostic.code.unwrap_or(""),
            diagnostic.message,
        );
        for child in &diagnostic.children {
            let (line, column) = line_column(&parsed.source, child.span.lo);
            eprintln!("{:<7} {line}:{column}  {}", child.severity, child.message);
        }
    }
}

pub fn tree(node: &CNode, depth: usize) {
    let name = node
        .name
        .as_deref()
        .map(|name| truncate(name, 60))
        .unwrap_or_default();
    println!(
        "{}{:?} {} [{}..{}]",
        "  ".repeat(depth),
        node.node_type,
        name,
        node.span.lo,
        node.span.hi,
    );
    for child in &node.children {
        tree(child, depth + 1);
    }
}

pub fn template_summary(matches: &[TemplateMatch<'_>]) {
    let mut counts = BTreeMap::new();
    for matched in matches {
        *counts.entry(matched.template).or_insert(0usize) += 1;
    }
    println!("\ntemplates: {} syntax nodes matched", matches.len());
    for (template, count) in counts {
        println!("  {template:<14} {count}");
    }
}
