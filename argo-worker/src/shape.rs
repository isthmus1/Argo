//! A cheap structural check on a reply before validation. It proves nothing about behavior; it
//! only rejects replies that cannot be what was asked for, so the model is told why and retries.

use argo_db_client::{CChunk, ChunkKind, MAX_RUST_BYTES};

/// `symbol`: the function the Rust must export (the C function's name, or a piece's).
pub fn check(chunk: &CChunk, symbol: &str, source: &str) -> Result<(), String> {
    if source.is_empty() {
        return Err("the reply contained no code".into());
    }
    if source.len() > MAX_RUST_BYTES {
        return Err(format!(
            "the reply is {} bytes; limit {MAX_RUST_BYTES}",
            source.len()
        ));
    }
    if !source.contains("extern \"C\"") || !source.contains("no_mangle") {
        return Err("no exported function: write it as `#[no_mangle] pub extern \"C\" fn`".into());
    }
    if matches!(chunk.kind, ChunkKind::Function | ChunkKind::Region) && !declares(source, symbol) {
        return Err(format!("no `fn {symbol}(`: export the function under exactly that name"));
    }
    Ok(())
}

/// Whether `source` declares `fn name`, read as tokens rather than one spelling: any whitespace
/// between them and before the parameters, and the raw identifier `r#name` a C name that is a
/// Rust keyword (`type`, `match`) needs; `no_mangle` still exports it as `name`.
fn declares(source: &str, name: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    source.match_indices("fn").any(|(at, _)| {
        let rest = &source[at + 2..];
        if source[..at].ends_with(ident) || !rest.starts_with(char::is_whitespace) {
            return false;
        }
        let rest = rest.trim_start();
        let rest = rest.strip_prefix("r#").unwrap_or(rest);
        rest.strip_prefix(name)
            .filter(|after| !after.starts_with(ident))
            .is_some_and(|after| after.trim_start().starts_with(['(', '<']))
    })
}

#[cfg(test)]
mod tests {
    use super::declares;

    #[test]
    fn declarations_are_read_as_tokens() {
        for source in [
            "pub extern \"C\" fn add(a: i32) -> i32 { a }",
            "pub extern \"C\" fn add (a: i32) -> i32 { a }",
            "pub extern \"C\" fn\n    add(\n a: i32) -> i32 { a }",
        ] {
            assert!(declares(source, "add"), "{source}");
        }
    }

    #[test]
    fn a_keyword_name_may_be_a_raw_identifier() {
        assert!(declares(
            "pub extern \"C\" fn r#type(x: i32) -> i32 { x }",
            "type"
        ));
    }

    #[test]
    fn other_names_do_not_count() {
        for source in [
            "pub extern \"C\" fn add_two(a: i32) -> i32 { a }",
            "pub extern \"C\" fn my_add(a: i32) -> i32 { a }",
            "pub extern \"C\" fn sub(a: i32) -> i32 { add(a) }",
            "// fnadd(\npub extern \"C\" fn sub() {}",
        ] {
            assert!(!declares(source, "add"), "{source}");
        }
    }
}
