use rucc_base::Interner;
use rucc_diag::Diagnostic;
use rucc_lex::{Convert, Keywords, Options, Tokens, convert, tokenize};
use rucc_session::Std;
use rucc_target::{TargetInfo, Triple};

pub const DEFAULT_TARGET: &str = "x86_64-unknown-linux-gnu";

/// Reusable lexer configuration. Every call gets its own symbol interner.
/// Directives/macros still require a future preprocessing stage.
pub struct Lexer {
    pub(crate) standard: Std,
    pub(crate) gnu: bool,
    target: TargetInfo,
    triple: String,
}

pub struct Lexed {
    pub tokens: Tokens,
    pub interner: Interner,
    pub diagnostics: Vec<Diagnostic>,
    pub pp_token_count: usize,
}

impl Lexer {
    pub fn new(target: &str, standard: Std, gnu: bool) -> Result<Self, String> {
        let triple: Triple = target
            .parse()
            .map_err(|_| format!("the target triple {target} does not parse"))?;
        Ok(Self {
            standard,
            gnu,
            target: TargetInfo::new(triple),
            triple: target.into(),
        })
    }

    pub fn target(&self) -> &str {
        &self.triple
    }

    pub fn standard(&self) -> Std {
        self.standard
    }

    pub fn gnu(&self) -> bool {
        self.gnu
    }

    pub fn lex(&self, source: &[u8]) -> Lexed {
        let mut interner = Interner::new();
        // Keywords must precede source identifiers in the interner.
        let keywords = Keywords::new(&mut interner, self.standard, self.gnu);
        let (pp_tokens, mut diagnostics) = tokenize(source, 0, Options::new(), &mut interner);
        let context = Convert {
            keywords: &keywords,
            interner: &interner,
            target: &self.target,
            std: self.standard,
            gnu: self.gnu,
            pedantic: false,
        };
        let (tokens, conversion_diagnostics) = convert(&pp_tokens, &context);
        diagnostics.extend(conversion_diagnostics);
        Lexed {
            tokens,
            interner,
            diagnostics,
            pp_token_count: pp_tokens.len(),
        }
    }
}

impl Default for Lexer {
    fn default() -> Self {
        Self::new(DEFAULT_TARGET, Std::C11, true).expect("the built-in target triple is valid")
    }
}
