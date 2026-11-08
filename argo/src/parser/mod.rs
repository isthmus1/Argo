use std::path::Path;

use rucc_ast::Ast as RawAst;
use rucc_base::Interner;
use rucc_diag::Diagnostic;
use rucc_parse::{Context, parse};

mod model;
mod summary;

pub use model::CSnippet;
pub use summary::{Report, parse_source, run};

use crate::ast::Ast;
use crate::lexer::Lexer;
use crate::lowering;

/// The C parser owns its lexer configuration and can parse independent inputs.
#[derive(Default)]
pub struct Parser {
    lexer: Lexer,
}

pub struct ParseResult {
    pub ast: Ast,
    pub source: Vec<u8>,
    pub diagnostics: Vec<Diagnostic>,
    pub pp_token_count: usize,
    pub token_count: usize,
    raw: RawAst,
    interner: Interner,
}

impl Parser {
    pub fn new(lexer: Lexer) -> Self {
        Self { lexer }
    }

    pub fn lexer(&self) -> &Lexer {
        &self.lexer
    }

    pub fn parse_file(&self, path: &Path) -> Result<ParseResult, String> {
        let source = std::fs::read(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        Ok(self.parse(&source))
    }

    pub fn parse(&self, source: &[u8]) -> ParseResult {
        let lexed = self.lexer.lex(source);
        let mut context = Context::new(&lexed.interner, self.lexer.standard);
        context.gnu = self.lexer.gnu;
        let parsed = parse(&lexed.tokens, context);
        let mut diagnostics = lexed.diagnostics;
        diagnostics.extend(parsed.diagnostics);
        let failed = diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity.is_fatal());
        let ast = if failed {
            Ast::default()
        } else {
            lowering::lower(&parsed.ast, &lexed.interner, source)
        };
        ParseResult {
            ast,
            source: source.to_vec(),
            diagnostics,
            pp_token_count: lexed.pp_token_count,
            token_count: lexed.tokens.tokens.len(),
            raw: parsed.ast,
            interner: lexed.interner,
        }
    }
}

impl ParseResult {
    pub fn failed(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity.is_fatal())
    }

    /// The original rucc pretty-printer, not a byte-exact source rebuild.
    pub fn as_c(&self) -> String {
        rucc_ast::print(&self.raw, &self.interner)
    }
}
