//! What one translation needs: the claimed row, where to check it, and, for a piece of a
//! function, the function around it and the name the piece is exported under.

use std::path::Path;

use argo_db_client::{CChunk, ChunkKind};

pub struct Job<'a> {
    pub chunk: &'a CChunk,
    /// The repository at the ingested commit (see [`crate::validate::Candidate::checkout`]).
    pub checkout: Option<&'a Path>,
    /// The project's preprocessor flags, as the ingest recorded them (relative to the checkout).
    pub flags: &'a [String],
    /// Set for a piece of a function (a `Region` row): it is outlined into its own function.
    pub piece: Option<Piece<'a>>,
}

pub struct Piece<'a> {
    /// The function the piece is inside, for context and for its name.
    pub function: &'a CChunk,
    /// The `extern "C"` function the piece becomes.
    pub symbol: String,
}

impl<'a> Job<'a> {
    /// A whole function's row (or any row without a function around it).
    pub fn function(chunk: &'a CChunk, checkout: Option<&'a Path>, flags: &'a [String]) -> Self {
        Job { chunk, checkout, flags, piece: None }
    }

    /// A row of either kind: a `Region` inside `function` becomes a piece.
    pub fn new(
        chunk: &'a CChunk,
        function: Option<&'a CChunk>,
        checkout: Option<&'a Path>,
        flags: &'a [String],
    ) -> Self {
        let piece = function
            .filter(|_| chunk.kind == ChunkKind::Region)
            .map(|function| Piece { function, symbol: piece_symbol(&function.name, chunk.id) });
        Job { chunk, checkout, flags, piece }
    }

    /// The function the Rust must export.
    pub fn symbol(&self) -> &str {
        self.piece.as_ref().map_or(&self.chunk.name, |p| &p.symbol)
    }
}

/// `compute` piece in row 22 → `compute__part22`: unique per row, and says where it came from.
pub fn piece_symbol(function: &str, row: u64) -> String {
    format!("{function}__part{row}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn piece_names_say_where_they_came_from() {
        assert_eq!(super::piece_symbol("compute", 22), "compute__part22");
    }
}
