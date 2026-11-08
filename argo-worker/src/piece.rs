//! A piece of a function: a statement-level row (a `Region`) translated on its own. It becomes
//! an exported Rust function, and the C lines it was are replaced by a call to it, which the
//! agent writes beside the Rust.

use argo_db_client::{CChunk, ChunkKind};

/// What a piece's claim needs besides its own row.
#[derive(Debug, Clone)]
pub struct Piece {
    /// The function the piece is inside: its text is where the piece is found, and its context.
    pub function: CChunk,
    /// The `extern "C"` function the piece becomes.
    pub symbol: String,
}

impl Piece {
    /// For a `Region` row inside `function`; `None` for any other row.
    pub fn of(chunk: &CChunk, function: Option<CChunk>) -> Option<Piece> {
        if chunk.kind != ChunkKind::Region {
            return None;
        }
        let function = function?;
        let symbol = symbol(&function.name, chunk.id);
        Some(Piece { function, symbol })
    }
}

/// `compute`'s piece in row 22 → `compute__part22`: unique per row, and says where it came from.
pub fn symbol(function: &str, row: u64) -> String {
    format!("{function}__part{row}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn piece_names_say_where_they_came_from() {
        assert_eq!(super::symbol("compute", 22), "compute__part22");
    }
}
