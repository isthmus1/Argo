# Argo

**Explode a legacy program into a live Blueprint-style graph. Replace its nodes with Rust. Keep checking the running program as you go.**

Argo is a Rust-first legacy-modernization project built around SpacetimeDB, agent-assisted understanding, and incremental C/Rust interoperability.

> **Status: parser/syntax-graph prototype.** A standalone Rust CLI builds an owned C syntax tree and expands templates into a source-retaining containment graph, exportable as JSON. The application, executable graph, migrations, and golden boot remain unimplemented. See [ARCHITECTURE.md](ARCHITECTURE.md) for the broader design and acceptance gates.

## The idea

1. **Build the graph from templates.** Parse the C AST and match constructs (functions, structs, enums, macros, `if`/`else` chains, loops, `switch` …) against templates. Each template emits its piece of a hierarchical node graph. Functions and types get pins from their signatures. Statement regions get pins from the values they inherit from and hand back to the enclosing scope, plus their control exits.
2. **Navigate it like Blueprints.** The top view shows functions, types and globals. Any function opens into its internal node groups, like a Blender node group or a UE5 collapsed graph. As in Blueprints, nodes have separate control (exec) pins for execution order and data pins for values.
3. **Understand and plan.** Cheap agent calls explain leaves, then parents. An architect role sends grouping and interface guidance back down. Model access must be verified, and fallbacks must be disclosed.
4. **Validate the graph as C.** Outline the graph's regions in C and confirm that the reorganized program still passes its tests before any Rust exists.
5. **Translate bottom-up.** Workers claim C nodes from SpacetimeDB in real time, leaves first. They rewrite one node in Rust and test it inside the otherwise-C program before submitting it.
6. **Merge upward.** When neighboring nodes are both Rust, SpacetimeDB dispatches narrowly scoped jobs to strip the C interop between them and collapse the group into one node. This recurses until a single Rust program node remains.
7. **Modernize.** Improve the resulting Rust, which may still be unsafe or C-shaped, through bounded, continuously tested changes.

The graph is not merely an explanation visualization: it defines the replaceable boundaries and carries the evidence for each migration step. Passing tests demonstrate agreement on their stated inputs/domain, not universal correctness.

## Golden project: TinyEMU RV64 → an X11 desktop

The end-to-end target is to rewrite the **C portion of TinyEMU needed for RV64 emulation** in Rust until it boots a **RISC-V Linux X11 desktop**, including working keyboard/pointer interaction.

That means the required emulator/device/support-code closure—not just the CPU decoder. Guest firmware, Linux, Xorg, and the root filesystem remain inputs rather than translation targets. x86/KVM and the browser JavaScript frontend are out of scope.

A console login, an x86 desktop, or a Rust wrapper still executing the original C emulator does **not** meet the golden target.

Start by reproducing the original C baseline and pinning its guest/configuration. The [official TinyEMU project](https://bellard.org/tinyemu/) and [RV64 X Window configuration](https://bellard.org/jslinux/buildroot-riscv64-xwin.cfg) provide the starting references; native reproduction has not been performed here.

## Three buildable parts

| Part | Responsibility |
| --- | --- |
| **Server / SpacetimeDB** | Authoritative graph/jobs/reports, structural and acceptance checks, shared protocol, and the reusable baseline/validation-harness specification |
| **Worker client** | Agent/tool loop, parsing, graph/ABI generation, mixed linking, execution of the harness, and export on the worker's compute |
| **UI control-plane client** | A strongly UE5-Blueprints-like graph canvas, controls, source/explanation inspectors, and live migration/validation evidence |

The shared Rust protocol defines graph records, pin/edge types, versioned job inputs, implementation candidates, and validation reports. Clients issue reducer commands and subscribe to durable SpacetimeDB state. The server coordinates; it does not run the compilers or emulated guest.

The server workstream also authors a native harness for baseline builds, controlled traces and guest captures; **the worker runs it**, not SpacetimeDB. A proposed two-person start is W for one builder and S + U for the other, with shared contract/baseline decisions and explicit reassignment after the first integration gate. The worker remains the highest-risk package.

The first version uses **one worker and simple programs**. Many container/VM workers attaching from separate machines are a stretch goal, not a prerequisite for the first useful migration.

## The GUI

The visual target is specifically Blueprints: a dark grid, colored node title bars, typed pins, curved wires, nested graphs, pan/zoom, node dragging, and an inspector. Live updates must preserve the user's navigation and selection. The application uses its own assets, not Epic's.

**iced, GPUI, and the new [Rune UI framework](https://github.com/anomalyco/rune) are all options.** Rune here is not the unrelated scripting-language crate.

The requested panel reviewed the architecture against the user's own design: **Opus 5.5 / high** favors iced by default, Rune after a successful host-platform spike, and GPUI with existing expertise. **GPT 6.1 Sol / xhigh** conditionally favors iced but considers Rune a serious challenger that could win the spike, or GPUI when its owner already knows it. [Their separate opinions and conditions](ARCHITECTURE.md#reviewer-opinions) are recorded in full context.

A real node/pin/wire, input, async-update, and demo-platform spike is required before final toolkit selection. These are source-based opinions; no toolkit has been benchmarked or launched for this repository yet.

## Delivery ladder

- Agree interfaces and reproduce the TinyEMU baseline; verify model access and launch the GUI spike.
- Show a simple program's AST/explanations live in SpacetimeDB and the graph viewer.
- Validate its same-language graph build.
- Replace one node with Rust and check the mixed application, including a deliberately failing candidate.
- Finish and export the small fixture, then replace a bounded helper inside a still-booting TinyEMU.
- Expand through layout-checked shared CPU/device state and supported outlined interpreter blocks; preserve preprocessor/build identity and explicit control exits.
- Reach the RV64 X11 golden boot with the migrated implementation and recorded provenance/evidence.
- Only then expand into remote swarms, broader languages, and aggressive architectural refactoring.

The golden target is ambitious; an incomplete golden gate must be reported as incomplete rather than silently replaced by a toy demonstration.

## Why this stack

- **Rust:** the application frontend/backend and the migration target.
- **SpacetimeDB:** load-bearing shared graph state, job coordination, and realtime visibility—not an ornamental chat database.
- **Nemotron:** the intended understanding/planning/implementation workflow, beyond a chatbot.
- **Legacy modernization:** a natural fit for the 8090 challenge alongside SpacetimeDB and NVIDIA.

## Run it

Needs the `spacetime` CLI (2.0.x, matching `spacetimedb-sdk`), git, a C preprocessor, and the
`wasm32-unknown-unknown` Rust target. `argo-server` is Unix-only (Linux, macOS): it manages its
children through process groups and signals. Only on Linux are they also killed if the server
itself is SIGKILLed; elsewhere a host left behind is reported on the next start. Use one
terminal per process:

```sh
# Server: starts SpacetimeDB on 127.0.0.1:3100, publishes argo-db, ingests the repository's C
# chunks, and stops the database on Ctrl-C. `--fresh` replaces a different ingested commit.
# With an OpenRouter key it also runs the model proxy on port 3101, on the free Nemotron 3.5
# Lightning by default; for the demo, the paid one pinned to one provider as shown. The worker's
# agent needs tool calling, which OpenRouter lists Fireworks' endpoint for it as lacking.
export OPENROUTER_API_KEY=...
cargo run --release -p argo-server -- ~/src/project            # or a git URL[#ref]; -D/-I flags
#   [--model nvidia/nemotron-3.5-lightning --model-provider deepinfra]

# Workers (any number): each clones the recorded commit, claims chunks, and runs a read-only
# tool-using agent (read, grep, glob, submit_rewrite) that submits Rust once it compiles, links
# into the C program and matches the C function's ABI; needs rustc and cc. No key
# and no model: each attaches to the server's model proxy (the --db host, port + 1) for a key of
# its own and the server's model. `--direct --model M` uses OPENROUTER_API_KEY instead (no proxy,
# e.g. with --server; only then --escalation-model M --escalate-after 2).
cargo run --release -p argo-worker -- --db ws://127.0.0.1:3100 --module argo \
  [--jobs N] [--max-attempts 5] [--max-turns 40] [--once]

# GUI: watches the rows live, read-only; it never clones or parses the repository. The database
# is its only data source: without --db (or ARGO_DB) it prints its usage and exits.
cargo run --release -p argo-ui -- --db ws://127.0.0.1:3100 --module argo
# Headless capture: add `--snap out.png`; ARGO_FUNCTION=name picks the function shown,
# ARGO_OPEN=all opens its constructs, ARGO_HOVER=title hovers a node.
```

`scripts/render-mocks.sh`, `render-review.sh` and `render-understand.sh` capture the GUI the same
way: they serve a small sample project (`scripts/sample-db.sh`) through `argo-server` on their own
port, with its data under `target/`.

**Model proxy.** `argo-server` holds the only provider key (`OPENROUTER_API_KEY`) and picks the
model (`--model`, optionally `--model-provider SLUG` to pin one OpenRouter provider with no
fallback). A worker `POST`s `/attach` to the proxy and gets a key minted for it alone, kept in
memory, never in its environment, plus the model name it records with its results. It sends its
completions to the proxy, which checks the key, rewrites the request to the server's model and
provider, forwards it with the real key, and appends one line per attach and per request to
`usage.log` in the data dir. Each line carries the provider that served it and that worker's and
the overall request/token/cost totals. Worker keys die with the server; a worker whose key is
refused attaches again. Attaching is open to anyone who can reach the port, like the database
itself. `--proxy-port` moves it, `--model-url` points it at another OpenAI-compatible API; it is
off (with a message) when `OPENROUTER_API_KEY` is unset.

Candidates are submitted as *unvalidated* until the compile/link/C-test `Validator` lands.

## Parser and graph prototype

The parser lives in `argo/` (package `Argo`, library and binary `main`). Run
`cargo run -p Argo -- argo/tests/fixture.c` (or supply another C file). The CLI prints the
owned syntax tree, a template-match summary, and rucc's C pretty-print. Fatal
lexer, conversion, or parser errors exit unsuccessfully without emitting a tree;
diagnostics go to stderr.

- `argo/src/lexer.rs`: reusable `Lexer`, defaulting to C23 + GNU on x86_64 Linux.
- `argo/src/parser.rs`: reusable `Parser` and an owned `ParseResult`.
- `argo/src/ast.rs`, `argo/src/c_node.rs`: `Ast` owns a forest of `CNode` values, each with a
  `CNodeType`, name, original-source byte span, and owned children. These are
  syntax nodes, **not** UI or executable graph nodes.
- `argo/src/lowering/`: converts rucc arenas into that tree, including declarator
  shapes, expressions, initializers, and control statements. Unsupported forms
  remain source-located opaque nodes rather than guessed structures. Some
  synthesized type/initializer nodes use enclosing declaration spans.
- `argo/src/templates/`: one template per supported keyword/construct, all re-exported
  from the module. `templates::all()` supplies the ordered built-ins.
- `argo/src/c_node_role.rs`: explicit child roles distinguish loop init/condition/step,
  function signatures, branch arms, switch selectors, and case values. Omitted
  clauses never shift positional meanings.
- `argo/src/graph/`: independent `Graph`, `GraphNode`, and `GraphEdge` records, expansion
  contracts, recursive driver, source-range validation, and JSON serialization.

`Ast::match_templates(Vec<impl Template>)` visits the tree in preorder, choosing
the first matching template per node. Concrete homogeneous lists and mixed
`Vec<Box<dyn Template>>` lists both work. Results borrow the owned syntax, not the
template list. The built-ins end with an opaque fallback; caller-supplied lists
may leave nodes unmatched without removing them from the AST.

Source modules stay below 100 lines; simple keyword recognizers are intentionally
shorter. Focused checks: `cargo fmt -p Argo --check`, `cargo test --locked -p Argo
--lib --tests`, and `cargo clippy --locked -p Argo --all-targets -- -D warnings`.

### Open a whole C project

```sh
# A local directory (argo-server also takes a git URL[#ref] and -D/-I flags, see "Run it"):
cargo run --release -p Argo -- --protocol-json ~/src/project > target/project.json
```

The parser reads plain C, so `argo/src/project/` runs gcc's preprocessor (`gcc -E`, in C23
mode like the parser) first: includes are pasted in and macros expanded. It is always gcc,
whatever `cc` or `$CC` is: the parser, [rucc](https://github.com/isthmus1/rucc) from our fork,
reads gcc's dialect, and clang's output carries spellings it doesn't (macOS support comes later).
Each `.c` file is its own translation unit, preprocessed, parsed and built on its own as a
compiler would, then merged into one graph; line markers put every function back under its
original file, and a shared header's items are kept once. Files that fail (missing headers,
undefined build macros) are listed with the reason instead of failing the project. `-D`/`-I` flags come from the Makefile's `CFLAGS`/`CPPFLAGS`/`DEFINES` lines,
then from the command line.

### Export a syntax graph

```sh
cargo run --locked -p Argo -- --graph-json argo/tests/fixture.c > target/fixture.graph.json
```

Graph mode writes only JSON to stdout; frontend diagnostics stay on stderr and
fatal errors produce no graph. The existing human-readable mode is unchanged.

`parsed.ast.build_graph(&parsed.source, templates::all())` returns an owned graph.
Supply the syntax tree's original source bytes. `Template::expand()` returns an
optional unnumbered node and ordered child regions; `Unsupported` tries the next
matching template. The driver assigns revision-local preorder IDs and containment
edges. An unmatched custom registry returns an error, never a partial graph.

- Functions contain bodies; branches contain arms; loops contain bodies while
  retaining init/condition/step spans as header parts.
- Compound scopes partition statements into straight-line sequence leaves and
  control regions. `return`/`break`/`continue` are lexical exit records on their
  sequences, not standalone graph nodes. Exit targets remain unresolved.
- Switches group stacked labels and possible fallthrough until an unconditional
  direct exit. Uncertain fallthrough stays joined; nested switches own their
  labels. Unsupported switch shapes stay whole rather than guessing boundaries.
- Types and globals remain coarse leaves. Prototypes/extern declarations stay in
  the original AST/source, without graph nodes or resolved edge targets yet.
- Gotos/labels, variadic function bodies, unsupported headers, and other unknown
  regions become opaque leaves with diagnostics. No opaque region is expanded.

JSON schema version 1 declares `analysis: "syntax_only"`. It retains the complete
original source as an array of unsigned bytes (including non-UTF-8 bytes), with
half-open byte ranges on nodes, header parts, and lexical exits. IDs are stable
for repeated expansion of the same input, not persistent identities across edits.
`argo/tests/fixtures/regions.c` and its hierarchy snapshot exercise the region model.

This graph is **provenance/navigation, not an execution schedule or a safe
replacement boundary**. Only containment edges exist. Preprocessing, configured
name/type resolution, live-in/live-out/effect analysis, trustworthy pins,
call/data/control edges, safe outlining, and migration remain future work. This
local JSON schema is not the shared SpacetimeDB protocol. Implementation must
continue to respect [ARCHITECTURE.md](ARCHITECTURE.md)'s contracts and gates.
