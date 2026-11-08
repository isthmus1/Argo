//! Models wrap code in markdown fences despite being told not to. The submitted source is the
//! fenced code alone, so prose around it never reaches the Rust table.

/// The code in `reply`: its Rust blocks (tagged `rust` or untagged) joined in order, every
/// block if none is Rust, or the whole reply when it has no fence. A reply cut off inside a
/// block (no closing fence) keeps what it has.
pub fn strip(reply: &str) -> String {
    let mut blocks: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut open: Option<(&str, Vec<&str>)> = None;
    for line in reply.lines() {
        let fence = line.trim_start().strip_prefix("```");
        match (&mut open, fence) {
            (None, Some(tag)) => open = Some((tag.trim(), Vec::new())),
            (None, None) => {}
            (Some((_, block)), None) => block.push(line),
            (Some(_), Some(_)) => blocks.extend(open.take()),
        }
    }
    blocks.extend(open);
    // A model quoting the C it was given must not get that C submitted as Rust.
    let rust = |tag: &str| tag.is_empty() || tag.eq_ignore_ascii_case("rust");
    let any_rust = blocks.iter().any(|(tag, _)| rust(tag));
    let code: Vec<String> = blocks
        .into_iter()
        .filter(|(tag, _)| !any_rust || rust(tag))
        .map(|(_, lines)| lines.join("\n"))
        .collect();
    if code.is_empty() {
        return reply.trim().to_string();
    }
    code.join("\n\n").trim().to_string()
}

/// The C in `reply`: its blocks tagged `c`, joined in order; `None` when there are none.
pub fn c_blocks(reply: &str) -> Option<String> {
    let mut out: Vec<String> = Vec::new();
    let mut open: Option<(bool, Vec<&str>)> = None;
    for line in reply.lines() {
        let fence = line.trim_start().strip_prefix("```");
        match (&mut open, fence) {
            (None, Some(tag)) => open = Some((tag.trim().eq_ignore_ascii_case("c"), Vec::new())),
            (None, None) => {}
            (Some((_, block)), None) => block.push(line),
            (Some((c, block)), Some(_)) => {
                if *c {
                    out.push(block.join("\n"));
                }
                open = None;
            }
        }
    }
    (!out.is_empty()).then(|| out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::strip;

    #[test]
    fn c_blocks_are_read_apart_from_the_rust() {
        let reply = "```rust\nfn f() {}\n```\n```c\nf();\n```";
        assert_eq!(super::c_blocks(reply).as_deref(), Some("f();"));
        assert_eq!(strip(reply), "fn f() {}");
        assert_eq!(super::c_blocks("```rust\nfn f() {}\n```"), None);
    }

    #[test]
    fn unfenced_reply_is_kept_trimmed() {
        assert_eq!(strip("\n fn f() {}\n"), "fn f() {}");
    }

    #[test]
    fn prose_around_a_fence_is_dropped() {
        let reply = "Here it is:\n```rust\n#[no_mangle]\npub extern \"C\" fn f() {}\n```\nDone.";
        assert_eq!(strip(reply), "#[no_mangle]\npub extern \"C\" fn f() {}");
    }

    #[test]
    fn several_blocks_are_joined_in_order() {
        let reply = "```\nuse core::ffi::c_int;\n```\nand\n  ```rust\nfn g() {}\n  ```";
        assert_eq!(strip(reply), "use core::ffi::c_int;\n\nfn g() {}");
    }

    #[test]
    fn non_rust_blocks_are_dropped_beside_rust_ones() {
        let reply = "Original:\n```c\nint f(void);\n```\n```rust\nfn f() {}\n```";
        assert_eq!(strip(reply), "fn f() {}");
        assert_eq!(
            strip("```rs\nfn f() {}\n```"),
            "fn f() {}",
            "no Rust tag: keep all"
        );
    }

    #[test]
    fn unclosed_fence_keeps_the_rest() {
        assert_eq!(strip("```rust\nfn h() {}\n"), "fn h() {}");
    }
}
