//! Worker logs are bounded explicitly: the module rejects (never truncates) oversize logs, so
//! the worker cuts them itself and says so in the text.

pub use argo_db_client::MAX_LOG_BYTES;

/// The last `max` bytes of `text` (whole characters), prefixed with how much was cut. The tail
/// is kept because the latest attempt's failure is the one worth reading. A `max` too small for
/// the marker keeps no text, only the marker.
pub fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let marker = |cut: usize| format!("[{cut} earlier bytes cut]\n");
    let wanted = (text.len() - max).saturating_add(marker(text.len()).len());
    let mut start = wanted.min(text.len());
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("{}{}", marker(start), &text[start..])
}

#[cfg(test)]
mod tests {
    use super::tail;

    #[test]
    fn short_text_is_untouched() {
        assert_eq!(tail("abc", 3), "abc");
    }

    #[test]
    fn long_text_keeps_the_tail_within_the_bound() {
        let text = format!("{}é{}", "a".repeat(200), "z".repeat(50));
        let cut = tail(&text, 80);
        assert!(cut.len() <= 80, "{} bytes", cut.len());
        assert!(cut.starts_with('[') && cut.ends_with(&"z".repeat(50)));
    }

    #[test]
    fn a_bound_below_the_marker_keeps_only_the_marker() {
        assert_eq!(tail("abcdef", 2), "[6 earlier bytes cut]\n");
    }
}
