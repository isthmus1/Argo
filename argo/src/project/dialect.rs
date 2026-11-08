//! Compiler-specific spellings that system headers use and the parser doesn't read, blanked out of
//! preprocessed text before it is parsed. Bytes are replaced by spaces, so offsets and line
//! numbers (and with them [`super::lines::Mapped::origin`]) stay exact.
//!
//! All come from clang (macOS's `cc`) and the headers it is used with:
//! - nullability qualifiers: `int (* _Nullable _close)(void *);` in the Apple SDK's `<_stdio.h>`;
//! - availability attributes, whose versions (`introduced=10.15.4`) aren't valid C numbers;
//! - glibc's `typedef __float128 _Float128;`, declared for compilers without the `_FloatN`
//!   types, which the parser has built in.
//!
//! None changes what the code does.

const NULLABILITY: [&[u8]; 5] = [
    b"_Nullable",
    b"_Nonnull",
    b"_Null_unspecified",
    b"_Nullable_result",
    b"__kindof",
];

const FLOAT_N: [&[u8]; 7] = [
    b"_Float16",
    b"_Float32",
    b"_Float64",
    b"_Float128",
    b"_Float32x",
    b"_Float64x",
    b"_Float128x",
];

/// Blank out nullability qualifiers, `__attribute__((...))` groups that mention availability, and
/// typedefs naming a `_FloatN` type.
pub fn neutralize(text: &mut [u8]) {
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == b'"' || c == b'\'' {
            i = skip_literal(text, i);
            continue;
        }
        if !is_ident_start(c) || (i > 0 && is_ident(text[i - 1])) {
            i += 1;
            continue;
        }
        let end = i + text[i..].iter().take_while(|&&b| is_ident(b)).count();
        let word = &text[i..end];
        if NULLABILITY.contains(&word) {
            text[i..end].fill(b' ');
        } else if (word == b"__attribute__" || word == b"__attribute")
            && let Some(close) = group_end(text, end)
            && mentions_availability(&text[end..close])
        {
            blank(&mut text[i..close]);
            i = close;
            continue;
        } else if word == b"typedef"
            && let Some(semi) = float_n_typedef(text, end)
        {
            blank(&mut text[i..=semi]);
            i = semi + 1;
            continue;
        }
        i = end;
    }
}

/// For `typedef <type> _FloatN;` starting after `typedef` at `from`: the index of its `;`.
fn float_n_typedef(text: &[u8], from: usize) -> Option<usize> {
    let rest = &text[from..text.len().min(from + 160)];
    let semi = rest.iter().position(|&b| b == b';')?;
    let body = &rest[..semi];
    if body.iter().any(|b| matches!(b, b'{' | b'(' | b'"')) {
        return None;
    }
    let last = body.split(|&b| !is_ident(b)).rfind(|w| !w.is_empty())?;
    FLOAT_N.contains(&last).then_some(from + semi)
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Index after the string or character literal starting at `i`.
fn skip_literal(text: &[u8], i: usize) -> usize {
    let q = text[i];
    let mut j = i + 1;
    while j < text.len() {
        match text[j] {
            b'\\' => j += 2,
            b'\n' => return j,
            b if b == q => return j + 1,
            _ => j += 1,
        }
    }
    text.len()
}

/// After `__attribute__` at `from`: the index just past its balanced `(...)`, if one follows.
fn group_end(text: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    while j < text.len() && text[j].is_ascii_whitespace() {
        j += 1;
    }
    if text.get(j) != Some(&b'(') {
        return None;
    }
    let mut level = 0usize;
    while j < text.len() {
        match text[j] {
            b'"' | b'\'' => {
                j = skip_literal(text, j);
                continue;
            }
            b'(' => level += 1,
            b')' => {
                level -= 1;
                if level == 0 {
                    return Some(j + 1);
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

fn mentions_availability(group: &[u8]) -> bool {
    group
        .split(|&b| !is_ident(b))
        .any(|w| w == b"availability" || w == b"__availability__")
}

/// Spaces for everything but newlines.
fn blank(s: &mut [u8]) {
    for b in s.iter_mut().filter(|b| **b != b'\n') {
        *b = b' ';
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &str) -> String {
        let mut b = s.as_bytes().to_vec();
        neutralize(&mut b);
        assert_eq!(b.len(), s.len(), "offsets are kept");
        String::from_utf8(b).unwrap()
    }

    #[test]
    fn apple_spellings_become_spaces() {
        assert_eq!(
            run("int (* _Nullable _close)(void *);"),
            "int (*           _close)(void *);"
        );
        let attr = "__attribute__((availability(macosx,introduced=10.15.4)))";
        assert_eq!(
            run(&format!(
                "int f(void) {attr}\n  __attribute__((__nonnull__));"
            )),
            format!(
                "int f(void) {}\n  __attribute__((__nonnull__));",
                " ".repeat(attr.len())
            )
        );
        // Not in literals, nor inside longer names.
        assert_eq!(
            run("const char *s = \"_Nonnull\"; int my_Nonnull, _Nonnull_x;"),
            "const char *s = \"_Nonnull\"; int my_Nonnull, _Nonnull_x;"
        );
        // A group spanning lines keeps its newlines.
        assert_eq!(
            run("int g(void) __attribute__((\navailability(ios,unavailable)));"),
            format!(
                "int g(void) {}\n{};",
                " ".repeat("__attribute__((".len()),
                " ".repeat("availability(ios,unavailable)))".len())
            )
        );
    }

    #[test]
    fn glibc_float_n_typedefs_under_clang_become_spaces() {
        let t = "typedef __float128 _Float128;";
        assert_eq!(
            run(&format!(
                "{t}\ntypedef float _Float32;\ntypedef double real;"
            )),
            format!(
                "{}\n{}\ntypedef double real;",
                " ".repeat(t.len()),
                " ".repeat("typedef float _Float32;".len())
            )
        );
        // Other typedefs, even mentioning a `_FloatN` type, stay.
        let kept = "typedef _Float128 quad; typedef struct { _Float64 x; } s;";
        assert_eq!(run(kept), kept);
    }
}
