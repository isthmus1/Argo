# Completed so far

## Parsing (`argo`)
- Parses C into an owned AST (`CNode`). Every node records its exact byte range (`span`) in the parsed source.
- `parser::CSnippet` (`argo/src/parser/model.rs`) holds a node's exact C text and its byte range.
- Whole projects (a folder or a git/GitHub link) are preprocessed with `gcc -E` and parsed into one graph.
- Test `prints_the_ast_of_a_small_c_snippet` (`argo/tests/integration.rs`) prints the AST of a small C function.

## Database (`argo-server`, `argo-db`)
- `argo-server <github-url | folder>` clones the repository, parses it, and stores one `c_chunk` row per function or region. Each row has its byte range (`byte_lo..byte_hi`), its exact C text (`source`), and its original file and lines.
- **Runs locally or connects to a remote database:**
  - default: starts its own SpacetimeDB on `127.0.0.1:3100` and keeps serving it;
  - `--listen 0.0.0.0`: lets other machines connect to that local host;
  - `--server <http(s)://host | maincloud>`: publishes to and loads an existing SpacetimeDB (another machine, or SpacetimeDB's cloud), then exits.
- Running it again with an unchanged repository sends nothing.
- `argo-server` builds on macOS as well as Linux.

## Translation (`argo-worker`)
- `argo-worker --db <ws-uri>` claims one row at a time and runs a Nemotron **agent** on it, then saves the accepted Rust to `rust_chunk` under the same id as the C row.
- Each claim is locked to one worker with a lease and heartbeat, so no two workers translate the same row.
- **Worker pool:** `--jobs N` runs N workers in one process, each translating one row at a time. Starting more processes adds more workers.
- **The agent** is a multi-turn tool-calling conversation (OpenAI-compatible chat completions) with exactly four tools:
  - `read`, `grep`, `glob`: read-only, over the worker's clone and the claim's session directory (the target's preprocessed translation unit, each submission and its logs). Paths can't leave those two places, symlinks included. There is no write or shell tool: the clone is shared and must stay the ingested commit.
  - `submit_rewrite(rust_source)`: takes the raw `.rs` text (markdown is rejected, not stripped). The submission is compiled with `rustc`, checked against the C program's symbols (not exported, redefines a C function, calls a `static` one), linked into the C program in place of the C definition (link errors the C program already has are ignored), and its exported signature is compared with the C function's ABI from both compilers' debug info. An accepted submission ends the agent. A rejected one gets back each failing check, why, and the paths of the full rustc/linker/ABI logs, and the agent keeps going in the same conversation.
- **Budgets:** `--max-attempts` rejected submissions (default 5) and `--max-turns` model turns (default 40) per row, then the row is marked `Failed`. Every turn uses the same model: the strong one from the start.
- At startup the worker checks that the endpoint accepts tool calls, so a server without them can't fail rows.
- Rust that passes every check is saved as `Unvalidated`, because nothing runs the program or its tests yet.
- **Model proxy:** only `argo-server` holds the OpenRouter key and picks the model. Each worker attaches to the server's proxy (port 3101) and gets its own key and the model; the proxy forwards its requests as that model (optionally pinned to one provider) with the real key and logs requests, tokens and cost per worker and overall to `usage.log` in the data dir.

## Recording and replay
- The model proxy records every request it forwards and the reply in SpacetimeDB (`model_exchange`):
  worker, chunk, claim generation, times, tokens, cost. A request keeps only the messages added since
  the previous turn of its conversation.
- `argo-server --replay argo --module argo-replay [--speed 100]` plays a recorded run (program, claims,
  results, model exchanges) into a fresh database at 100x by default, for the UI to watch with
  `--module argo-replay`. Nothing is translated or compiled again. Runs from before recording existed
  replay their claims and results without exchanges.

## Compile helpers (`argo/src/comp`)
- `comp::compiles(rust)`: compiles one Rust snippet on its own.
- `comp::run_with_c(c, &Replacement)`: replaces a C function, at its byte range, with Rust, then links and runs the program.
- `Replacement` (`comp/model.rs`): a byte range plus the Rust that replaces it.

## How to run
Run from the repository root. Build everything once with `cargo build --release`; the binaries
are in `target/release/`.

**1. Server: load a repository into SpacetimeDB and serve it**
```sh
# its own database on this machine (ws://127.0.0.1:3100, database `argo`)
./target/release/argo-server https://github.com/fernandotcl/TinyEMU.git
# same, reachable from other machines (put a TLS proxy in front on untrusted networks)
./target/release/argo-server https://github.com/fernandotcl/TinyEMU.git --listen 0.0.0.0
# into an existing SpacetimeDB instead (another host, or the cloud after `spacetime login`), then exit
./target/release/argo-server https://github.com/fernandotcl/TinyEMU.git --server http://host:3000
./target/release/argo-server https://github.com/fernandotcl/TinyEMU.git --server maincloud --module <unique-name>
```
Add `--fresh` to replace a different repository already in the database. The local database
keeps its data in `~/.local/state/argo-server/3100`.

Give the server an OpenRouter key (`OPENROUTER_API_KEY=...`) and it also runs the model proxy for
workers on port 3101 (`--proxy-port`), logging usage to `usage.log` in the data dir. By default it
sends every request to `nvidia/nemotron-3.5-lightning` pinned to DeepInfra (`--model`,
`--model-provider SLUG`, or `any` for OpenRouter's choice). The worker's agent needs tool calling, which OpenRouter lists the Fireworks endpoint
for this model as not supporting (DeepInfra, CoreWeave, Io Net and Darkbloom do), so pin one of
those:
```sh
./target/release/argo-server <repo> --listen 0.0.0.0 \
  --model nvidia/nemotron-3.5-lightning --model-provider deepinfra
```

**2. Workers: translate rows with Nemotron**
```sh
./target/release/argo-worker --db ws://127.0.0.1:3100 --module argo --jobs 20
```
No API key or model needed: workers use the server's model proxy (the `--db` host, port + 1;
`--proxy URL` to change it) and its model. `--direct --model M` with `OPENROUTER_API_KEY` skips
the proxy, for a database set up with `--server`. Needs `rustc` on the machine, and a C compiler
(`cc`, or `$CC`) for the link and ABI checks. The model's provider must support tool calling.
Start more processes to add workers.

**3. UI: watch the database live**
```sh
./target/release/argo-ui --db ws://127.0.0.1:3100 --module argo
```
Other UI modes: `./target/release/argo-ui` (sample data), or `./target/release/argo-ui <git-url>`
to open a project locally without the database.

**Parser on its own, and the tests**
```sh
cargo run -p Argo -- path/to/file.c                  # parse report
cargo run -p Argo -- --protocol-json path/to/file.c  # protocol snapshot as JSON
cargo test -p Argo --test integration prints_the_ast -- --nocapture   # print an AST
cargo test --workspace
```

## Not done yet
- Running the C program (or its tests) with each Rust result linked in (in progress).

A `Failed` row stays C, by design: the goal is a mostly-Rust program that may still call C
where a part doesn't port easily.

## Stretch goals
- Worker orchestration: turning workers on and off from the UI and controlling who may use
  them, resizing a running pool, and spawning containers load-balanced across connected
  machines. For the demo, workers are started by hand.
- Merging neighbouring Rust nodes upward, removing the C glue between them.
- Cleaning up the resulting Rust.

## Out of scope
- C static analysis (name/type resolution, call/data/control edges, values flowing into and out
  of a region). The agent looks these up itself with `read`, `grep` and `glob`.
- An explanation stage (leaves, then parents, with an architect sending guidance down): a small
  hallucination low down could steer the parents wrong, and unlike code there's no compiler to
  catch it.
