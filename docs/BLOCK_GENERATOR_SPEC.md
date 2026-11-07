# Block generator specification

Status: draft 1, 2026-11-07
Target codebase: TinyEMU 2019-12-21 (tarball SHA-256 `be8351f2121819b3172fcedce5cb1826fa12c87da1b7ed98f269d3e802a05555`)
Current implementation: `src/split.rs` (`swapctl split`), which covers sections 6.1 to 6.3 and part of 6.5.

## 1. Purpose

The block generator turns a C codebase into **blocks**: exact pieces of source text that a person or model can rewrite one at a time, and that the swap tool can replace with Rust while the rest of the C program still compiles and runs.

It is written in Rust, with no Clang dependency and no preprocessing step. It works on the source text exactly as written.

### Non-goals

- **Type checking:** Types are kept as text. No C type system is implemented.
- **Full preprocessing:** Macros are only expanded in the limited cases in section 6.6, to find names.
- **Rust generation:** Writing Rust is the model's job. This crate only provides the blocks, their context and the edit points.
- **Build system parsing:** Programs and compile units come from a manifest (section 5), not from Makefiles.

## 2. Terms

| Term | Meaning |
|---|---|
| Source file | A `.c` or `.h` file in the project |
| Segment | A contiguous byte range of one file. A file's segments cover it exactly. |
| Block | A segment containing code, with an ID |
| Text segment | Whitespace and comments between blocks. Not a block. |
| Compile unit | One compilation of one `.c` file with a set of defines, such as `riscv_cpu.c` with `MAX_XLEN=64` |
| Program | A linked output, such as `temu`, made of compile units |
| Template file | A file included more than once with different parameter macros, such as `riscv_cpu_template.h` with `XLEN` |
| Instantiation | One `#include` of a template file, with its parameter values |
| Fragment file | A file that is only valid inside a function body, such as `riscv_cpu_fp_template.h` |
| Paste macro | A function-like macro that joins tokens, such as `glue` and `xglue` |
| Symbol | A name the compiler or linker sees, such as `divu64`, after templates and macros are applied |

## 3. Guarantees

Every guarantee has a test in section 10.

| ID | Guarantee |
|---|---|
| G1 | **Exact rebuild:** Joining a file's segments in order reproduces the file byte for byte. Output that fails this check is an error, never written. |
| G2 | **Full coverage:** Segments are in order, with no gaps or overlaps, from byte 0 to the end of the file. |
| G3 | **Fail closed:** Code that can't be split safely becomes an `Unparsed` block with a diagnostic. It's never guessed. |
| G4 | **Deterministic:** The same input always gives the same blocks, IDs and order. |
| G5 | **Stable IDs:** Editing one block doesn't change any other block's ID. |
| G6 | **No panics:** Any valid UTF-8 input produces a result. Invalid UTF-8 produces error `E001`. |
| G7 | **Exact edit points:** Every range reported inside a block, such as a function's name or parameters, points to exactly that text. |
| G8 | **Safe Rust:** The crate uses `#![forbid(unsafe_code)]`. |

## 4. Data model

All offsets are byte offsets. Ranges are half-open (`start..end`). Ranges inside a block are relative to the start of that block's text.

```rust
pub struct Project {
    pub files: Vec<SourceFile>,
    pub blocks: Vec<Block>,
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub instantiations: Vec<Instantiation>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct SourceFile {
    pub path: String,               // relative to the project root, using `/`
    pub hash: String,               // SHA-256 of the file
    pub role: FileRole,
    pub segments: Vec<Segment>,     // in file order, covering the file (G1, G2)
}

pub enum FileRole {
    Normal,
    Template { params: Vec<String> },      // e.g. ["XLEN"]
    Fragment { included_by: Vec<BlockId> },
}

pub struct Segment {
    pub range: Range<usize>,
    pub content: SegmentContent,
}

pub enum SegmentContent {
    Text,              // whitespace and comments
    Block(BlockId),
}

pub struct Block {
    pub id: BlockId,
    pub file: String,
    pub kind: BlockKind,
    pub name: Option<BlockName>,
    pub text: String,                // exactly the file's bytes in this range
    pub hash: String,                // SHA-256 of `text`
    pub conditions: Vec<Condition>,  // enclosing #if branches, outermost first
    pub linkage: Linkage,
    pub head: Option<FunctionHead>,  // for Function blocks, section 6.8
    pub identifiers: Vec<String>,    // section 6.9
}

pub enum BlockKind {
    Function,
    Declaration,  // globals, prototypes, top-level macro calls
    Type,         // struct, union, enum, typedef
    Macro,        // #define
    Directive,    // other preprocessor lines, and `extern "C" {` / `}`
    MacroCall,    // a top-level macro call that generates code, section 6.6
    Fragment,     // the whole body of a fragment file
    Unparsed,
}

pub enum BlockName {
    Plain(String),                                      // helper
    Templated { pattern: String, params: Vec<String> }, // "divu{XLEN}", ["XLEN"]
    MacroCall { macro_name: String, first_arg: Option<String> }, // TARGET_READ_WRITE(8)
}

pub struct Condition {
    pub directive: String,  // the opening line, e.g. "#ifdef WORDS_BIGENDIAN"
    pub branch: u32,        // 0 for the #if part, then 1, 2, ... for each #elif or #else
    pub branch_text: String,// the #elif or #else line, or the opening line for branch 0
}

pub enum Linkage { Static, External, Unknown }

pub struct Symbol {
    pub name: String,                   // e.g. "divu64"
    pub block: BlockId,
    pub instantiation: Option<InstantiationId>,
    pub linkage: Linkage,
}

pub struct Instantiation {
    pub id: InstantiationId,
    pub template_file: String,
    pub included_from: String,           // the including file
    pub include_block: BlockId,          // the #include Directive block
    pub bindings: BTreeMap<String, String>, // e.g. {"XLEN": "64"}
    pub conditions: Vec<Condition>,      // around the #include, e.g. "#if MAX_XLEN >= 64"
}

pub struct Reference {
    pub from: BlockId,
    pub name: String,                    // the name as used, after paste-macro rendering
    pub target: Target,
    pub ranges: Vec<Range<usize>>,       // every use within `from`'s text
}

pub enum Target {
    Unique(BlockId),
    Ambiguous(Vec<BlockId>),  // e.g. alternatives under different #ifdef branches
    External,                 // not defined in the project, e.g. printf
}
```

## 5. Input

```toml
# swap.toml
root = "tinyemu-2019-12-21"

[[program]]
name = "temu"
units = [
  { file = "riscv_cpu.c", defines = { MAX_XLEN = "32" } },
  { file = "riscv_cpu.c", defines = { MAX_XLEN = "64" } },
  { file = "riscv_cpu.c", defines = { MAX_XLEN = "128" } },
  { file = "virtio.c" },
  # ...
]
```

Version 1 splits every `.c` and `.h` file under `root`, with or without a manifest. The manifest only adds program and compile-unit information, which section 6.7's instantiation records and the identity-swap test (T5) use.

## 6. Pipeline

The stages run in this order. Stages 6.1 to 6.3 run per file. Stage 6.4 needs all files' directives first, so the generator makes two passes.

### 6.1 Read
- **Path order:** Read files sorted by path (G4).
- **Encoding:** Reject invalid UTF-8 with `E001`, and skip that file.
- **Hash:** Record the file's SHA-256.

### 6.2 Lex (implemented)
Produce tokens covering the file exactly:

| Token | Rule |
|---|---|
| Whitespace | Spaces, tabs, newlines and `\` + newline |
| Comment | `//` to the end of the line, or `/* ... */`. An unterminated `/*` is a lex error. |
| Directive | A `#` that is the first token on its line, through the end of the line, following `\` continuations and block comments |
| String | `"..."` with `\` escapes. A raw newline is a lex error. |
| Char | `'...'`, same rules as String |
| Identifier | `[A-Za-z_][A-Za-z0-9_]*` |
| Number | C preprocessing numbers, including `1e+5` and `0x1p-3` |
| Punct | Any other single byte |

On a lex error, everything from that point to the end of the file becomes one `Unparsed` block with `W001`.

### 6.3 Top-level scan (implemented, with new rules)
Bracket depth counts `()`, `[]` and `{}` in significant tokens, meaning everything except whitespace, comments and directives.

**Item boundaries:**
1. A directive at depth 0 between items is its own segment (`Macro` for `#define NAME`, otherwise `Directive`).
2. `extern "<string>" {` and its matching `}` are each `Directive` segments. The code between them is scanned as top-level.
3. A `{` at depth 0 directly after `)` starts a function body, and the item ends at the matching `}`. Comments and directives between the `)` and the `{` are skipped.
4. Otherwise the item ends at the first `;` at depth 0. This includes `struct`/`union`/`enum`/`typedef` definitions, which continue past their `}`.
5. **New: self-terminating macro calls.** An item that so far is only `M(...)`, where `M` is in the macro table (6.4) and its body ends with `}` or `;`, ends at its closing `)`. It becomes a `MacroCall` block.
6. A bare macro use (`M` or `M(...)` with no other tokens) ends before a following directive, before a closing `}` of an `extern` wrapper, or at the end of the file.
7. Otherwise a bare macro use joins the following item, so attributes like `__printf(1, 2)` stay with their function.

**Comments:** Comments directly above an item, with no blank line in between, belong to the item.

**Failure:** If depth goes negative or the file ends inside an item, the code from the item's start to the next `}` at the start of a line becomes one `Unparsed` block with `W010`. Scanning then continues after it. Without such a `}`, the `Unparsed` block runs to the end of the file.

**New: conditions.** While scanning, keep a stack of top-level `#if`/`#ifdef`/`#ifndef` groups, updated by `#elif`, `#else` and `#endif`. Each block records the stack at its start in `conditions`. An `#endif` with no open group, or a group still open at the end of the file, gives `W011`, but the scan continues. Directives inside a block don't change the stack.

### 6.4 Macro table (new)
Built from every `Macro` block in the project before any classification:

```rust
pub struct MacroDef {
    pub name: String,
    pub params: Option<Vec<String>>,  // None for object-like macros
    pub body: Vec<Token>,             // tokens after the name and parameters
    pub block: BlockId,
}
```

From the table, mark these:
- **Paste macros:** A function-like macro whose body is `a ## b` of its parameters, or a call to another paste macro with its parameters in order. In TinyEMU, `xglue` and `glue` (`cutils.h`).
- **Alias macros:** An object-like macro whose body is a single paste-macro call, such as `#define add_sf glue(add_sf, F_SIZE)` in `softfp_template.h`.
- **Code-generating macros:** A function-like macro whose expanded body, scanned as in 6.3, contains at least one `Function` or `Declaration`, such as `TARGET_READ_WRITE` in `riscv_cpu_priv.h`.

Macros defined more than once with different bodies are kept as alternatives, each with its own conditions.

### 6.5 Classify and name
Classification rules (implemented):

| Rule | Kind |
|---|---|
| Item ended by rule 3 | `Function` |
| First identifier is `typedef` | `Type` |
| A leading `struct`/`union`/`enum` with a `{` body and no `=` | `Type` |
| Item ended by rule 5 | `MacroCall` |
| Anything else | `Declaration` |

Name rules, in priority order:
1. **Paste-macro name (new):** If a function's name position holds a paste-macro call, render it. Each argument is rendered recursively, a template parameter becomes `{PARAM}`, and the parts are joined. `glue(divu, XLEN)` → `divu{XLEN}`, and `glue(glue(cvt_sf, F_SIZE), _sf32)` → `cvt_sf{F_SIZE}_sf32`. The result is `BlockName::Templated`.
2. **Alias name (new):** If the name is an alias macro, render the alias's body the same way. `add_sf` → `add_sf{F_SIZE}`.
3. **Function name (implemented):** The identifier before the first depth-0 `(` that follows a return type (an identifier or `*`).
4. **Macro-defined function (implemented):** If no `(` follows a return type, use `MACRO(first_arg)`, such as `SYSCALL_DEFINE2(add)`.
5. **Declarations, types and macros (implemented):** As in the current implementation. These are a function pointer's `(*name)`, a struct tag, the `typedef` name, the last depth-0 identifier before `=`, `;`, `[` or `:`, and a `#define` name.

**Linkage:** `Static` if `static` appears before the name at depth 0, `External` for functions and declarations without it, and `Unknown` when a macro hides the storage class.

### 6.6 Code-generating macro calls (new)
For each `MacroCall` block calling a code-generating macro:
1. **Expand:** Substitute the arguments into the macro body and apply `##`, but don't expand other macros. Paste macros and `#` stringizing are the only exceptions.
2. **Scan:** Run 6.2 to 6.5 on the expanded text.
3. **Record:** Add each resulting function or declaration name as a `Symbol` of the `MacroCall` block.

If expansion fails, record no symbols, add `W040`, and keep the block. Calls of `TARGET_READ_WRITE(8, uint8_t, 0)` must produce `target_read_u8` and `target_write_u8`.

### 6.7 File roles (new)
- **Fragment files:** A file is a fragment if it's included from inside a block in another file, or if its top level contains a statement keyword item (`case`, `default`, `if`, `else`, `for`, `while`, `do`, `switch`, `return`, `break`, `continue` or `goto`), or a `label:`. The whole file then becomes one `Fragment` block, plus leading and trailing `Text`, and gets `W020`. The block is attached to every block that includes it.
- **Template files:** A file is a template if it's included more than once from the same file, and at least one macro defined just before each `#include` is used in the template. Those macros are the template's parameters.
- **Instantiations:** Each `#include` of a template becomes an `Instantiation`. Its `bindings` are the parameter values defined just before it, and its `conditions` are the `#if` groups around it.
- **Template symbols:** Each `Templated` block in a template produces one `Symbol` per instantiation, with the bindings substituted. `divu{XLEN}` produces `divu32`, `divu64` and `divu128`.

Templates nested inside templates combine both files' bindings. For example, `softfp_template_icvt.h` is included three times by `softfp_template.h` with `ICVT_SIZE`, and `softfp_template.h` is included three times by `softfp.c` with `F_SIZE`, giving up to nine instantiations.

A file can meet both the fragment and template rules. `riscv_cpu_fp_template.h` is included three times with `F_SIZE`, but from inside a function. In that case the file is a `Fragment`, and its instantiations are still recorded so the including block knows which widths it contains.

### 6.8 Function heads (new)
For each `Function` block, find these ranges, relative to the block text:

```rust
pub struct FunctionHead {
    pub leading_comment: Option<Range<usize>>,
    pub prefix: Range<usize>,   // storage class, attributes, return type
    pub name: Range<usize>,     // the name as written, e.g. `glue(divu, XLEN)`
    pub params: Range<usize>,   // between the parameter list's parentheses
    pub suffix: Range<usize>,   // anything between `)` and `{`, such as attributes
    pub body: Range<usize>,     // from `{` to `}` inclusive
    pub param_list: Vec<Param>,
    pub variadic: bool,
}

pub struct Param {
    pub range: Range<usize>,      // one parameter's text
    pub name: Option<String>,     // None for unnamed or function-pointer parameters
}
```

- **Splitting parameters:** Split the parameter list at depth-0 commas. A parameter's name is its last identifier, unless it contains `(*`. `void` alone means no parameters.
- **When to leave it out:** If the head can't be found, for example in K&R-style definitions or macro-defined functions, `head` is `None` and the block gets `W050`.

These ranges replace the regular expression in `swap_harness/apply_swap.py`. Renaming a function to `<name>_c` becomes an exact replacement of the `name` range.

### 6.9 Identifiers and references
- **Identifiers (implemented):** Identifier tokens in the block, excluding C keywords, sorted and deduplicated.
- **Paste-macro calls (new):** Before collecting, render each paste-macro call to its pattern, so a call `glue(divu, XLEN)(a, b)` records `divu{XLEN}` instead of `glue`, `divu` and `XLEN`.
- **Resolution (new):** For each identifier, find matching block names or symbols, in this order:
  1. A `static` block in the same file, or in a template or fragment file included by the same file
  2. Any block in a header the file includes, directly or indirectly
  3. Any `External` block in the project
  4. Otherwise `Target::External`

  If several blocks match at the same level, for example alternatives under different `#ifdef` branches, the result is `Target::Ambiguous`.

References are candidates, not proven uses. A local variable with the same name as a global still produces a reference.

### 6.10 IDs
A block ID has the form `{path}:{kind}:{name}`, with `kind` as `fn`, `decl`, `type`, `macro`, `call`, `dir` or `frag`, and `name` as the rendered pattern.

Duplicates within the same file are resolved in this order:
1. Add `@` followed by the first 8 hex characters of the SHA-256 of the block's conditions. This separates the two `cpu_to_be32` blocks in `cutils.h`.
2. If two blocks still collide, add `~2`, `~3` and so on, in file order, and add `W030`.

Directive and `Unparsed` blocks have no name, so their ID is `{path}:{kind}:{hash8}`, using the first 8 hex characters of the SHA-256 of their text. A fragment file's block is `{path}:frag`.

## 7. Output

- **JSON Lines export:** `swapctl blocks [--manifest swap.toml] <root>` writes one record per file, block, symbol, instantiation, reference and diagnostic.
- **SpacetimeDB tables:** `file`, `file_segment`, `block`, `symbol`, `instantiation`, `reference` and `diagnostic`, with one column per field in section 4. `file_segment` stores `Text` content inline and a `BlockId` for blocks, so a file can be rebuilt from the database alone.
- **Rebuild:** `swapctl rebuild <file>` joins the stored segments. It's also how swapped code reaches the compiler.

## 8. Diagnostics

| Code | Severity | Meaning |
|---|---|---|
| E001 | Error | File is not valid UTF-8 |
| W001 | Warning | Lex error, rest of file is `Unparsed` |
| W010 | Warning | Unbalanced brackets or unterminated item, `Unparsed` block created |
| W011 | Warning | Unbalanced `#if`/`#endif` |
| W020 | Info | Fragment file detected |
| W030 | Warning | ID collision resolved by ordinal |
| W040 | Warning | Code-generating macro couldn't be expanded |
| W050 | Info | Function head not found |

Each diagnostic records its code, file, range, block ID if any, and a message.

## 9. Code layout

```text
crates/blocks/src/
  lib.rs           # generate(root, manifest) -> Project
  model.rs         # section 4
  lex.rs           # 6.2 (from split.rs)
  scan.rs          # 6.3 (from split.rs)
  conditions.rs    # 6.3 condition stack
  macros.rs        # 6.4 and 6.6
  classify.rs      # 6.5 (from split.rs)
  roles.rs         # 6.7
  head.rs          # 6.8
  refs.rs          # 6.9
  ids.rs           # 6.10
  diagnostics.rs   # section 8
```

The crate has no dependencies beyond `serde`, `sha2` and `toml`.

## 10. Tests

| ID | Test | Passes when |
|---|---|---|
| T1 | Unit tests per rule in section 6 | Every rule has a passing and a failing example |
| T2 | Fixtures in `tests/fixtures/`, each with an `.expected.json` | Output matches exactly |
| T3 | TinyEMU corpus, from the pinned tarball | The criteria below are all met |
| T4 | Property and fuzz tests | No panics, and G1 holds for arbitrary UTF-8 input and for edited fixtures |
| T5 | Identity swap on TinyEMU | The program builds and the boot output is identical |

**Fixtures (T2):** At least one each for strings and comments containing braces, `extern "C"`, a leading comment, `SYSCALL_DEFINE`-style functions, the current `kernelish.c`, and these cut-down TinyEMU cases:
- Templated `glue(div, XLEN)` functions in a template included three times
- A fragment file included inside a `switch`
- `TARGET_READ_WRITE`-style code-generating macro calls
- Alternative `cpu_to_be32` definitions under `#ifdef`/`#else`
- Alias macros like `#define add_sf glue(add_sf, F_SIZE)`
- A template nested in a template, like `softfp_template_icvt.h` inside `softfp_template.h`

**TinyEMU corpus (T3)** requires:
1. All 88 `.c` and `.h` files, including `slirp/`, rebuild byte for byte.
2. Zero `Unparsed` blocks.
3. `riscv_cpu_fp_template.h` is one `Fragment` block, with no `Declaration` or `Function` blocks.
4. `riscv_cpu_template.h` has a `Templated` block named `divu{XLEN}`. Its symbols across all instantiations are `divu32`, `divu64` and `divu128`.
5. `riscv_cpu_priv.h`'s `TARGET_READ_WRITE` calls produce `target_read_u8` through `target_read_u64` and the matching write functions, plus the `u128` ones under their `#if` condition.
6. The two `cpu_to_be32` blocks have different IDs and different conditions.
7. No `W030` diagnostics.
8. The output matches a reviewed snapshot, `tests/snapshots/tinyemu.jsonl`.

**Identity swap (T5):** For every `Function` block in `temu` that has a `head`:
1. Rename it to `<name>__orig` using the `name` range.
2. Add a wrapper with the original head that calls `<name>__orig` with the same arguments.
3. Rebuild each file from its segments.
4. Build `temu` and run `swap_harness/run_workloads.sh`. The output must match the unmodified build.

Variadic functions are skipped and counted. Files are processed in batches, so a failure points to one file. This test checks block boundaries and function heads against a real compiler without using a model.

**Other checks:** `cargo clippy` with no warnings, and the whole TinyEMU tree processed in under 1 second in a release build.

## 11. Milestones

| # | Work | Done when |
|---|---|---|
| M1 | Move `split.rs` into `crates/blocks` with the section 4 model, IDs, diagnostics and condition tracking | Current tests pass, T3 criteria 1 and 6 pass |
| M2 | Macro table, self-terminating macro calls, paste-macro and alias names | T3 criteria 2 and 5 pass |
| M3 | File roles and instantiations | T3 criteria 3 and 4 pass |
| M4 | Function heads and the identity swap | T5 passes |
| M5 | Reference resolution and the JSON Lines export | T3 criteria 7 and 8 pass |
| M6 | SpacetimeDB tables and `swapctl rebuild` | A file rebuilt from the database matches the original |

## 12. Open questions

1. **Evaluating `#if`:** Conditions are stored as text. Evaluating them against a compile unit's defines would show which blocks are actually built, but needs a C constant-expression evaluator. This is planned after M6.
2. **Splitting very large functions:** `riscv_cpu_interp_x` is 57 KB, too large for one model rewrite. Splitting it at `case` labels into sub-blocks is planned as a separate design.
3. **Non-UTF-8 files:** These are rejected for now. Storing them as bytes would need a different SpacetimeDB column type.
4. **Inlining across C and Rust:** Swapped `static inline` helpers lose inlining unless link-time optimization spans both languages. This belongs to the swap tool, not the block generator, but affects which blocks are worth swapping first.
