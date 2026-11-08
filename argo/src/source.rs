use rucc_diag::BytePos;

/// Translate an original-source byte offset to a 1-based line and byte column.
/// Multi-file and macro-expansion mapping will need rucc's SourceMap later.
pub fn line_column(source: &[u8], position: BytePos) -> (usize, usize) {
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

pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let kept: String = text.chars().take(width - 1).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_positions_cover_newlines_and_eof() {
        assert_eq!(line_column(b"ab\nc", 0), (1, 1));
        assert_eq!(line_column(b"ab\nc", 3), (2, 1));
        assert_eq!(line_column(b"ab\nc", 99), (2, 2));
        assert_eq!(line_column(b"", 0), (1, 1));
    }

    #[test]
    fn truncation_is_unicode_safe_and_respects_zero_width() {
        assert_eq!(truncate("éclair", 4), "écl…");
        assert_eq!(truncate("abc", 0), "");
        assert_eq!(truncate("abc", 1), "…");
        assert_eq!(truncate("abc", 3), "abc");
    }
}
