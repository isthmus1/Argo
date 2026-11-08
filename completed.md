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

## Translation (`argo-worker`, `iterator`)
- `argo-worker --db <ws-uri>` claims one row at a time, sends its C to Nemotron (via `iterator`), and saves the Rust to `rust_chunk` under the same id as the C row.
- Each claim is locked to one worker with a lease and heartbeat, so no two workers translate the same row.
- **Worker pool:** `--jobs N` runs N workers in one process, each translating one row at a time. Starting more processes adds more workers.
- **Compile check:** each Rust reply is compiled with `rustc` before it's saved. If it fails, the compiler errors go into the next prompt, up to `--max-attempts` (default 3), then the row is marked `Failed`.
- Rust that compiles is saved as `Unvalidated`, because it isn't linked into the C program or tested yet.
- **Model proxy:** only `argo-server` holds the OpenRouter key and picks the model. Each worker attaches to the server's proxy (port 3101) and gets its own key and the model; the proxy forwards its requests as that model (optionally pinned to one provider) with the real key and logs requests, tokens and cost per worker and overall to `usage.log` in the data dir.

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
workers on port 3101 (`--proxy-port`), logging usage to `usage.log` in the data dir. It uses the
free `nvidia/nemotron-3.5-lightning:free` by default; for the demo, the paid model pinned to
Fireworks for speed:
```sh
./target/release/argo-server <repo> --listen 0.0.0.0 \
  --model nvidia/nemotron-3.5-lightning --model-provider fireworks
```

**2. Workers: translate rows with Nemotron**
```sh
./target/release/argo-worker --db ws://127.0.0.1:3100 --module argo --jobs 20
```
No API key or model needed: workers use the server's model proxy (the `--db` host, port + 1;
`--proxy URL` to change it) and its model. `--direct --model M` with `OPENROUTER_API_KEY` skips
the proxy, for a database set up with `--server`. Needs `rustc` on the machine. Start more processes to add workers.

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
- On/off switch for the workers from the UI, and control over who may use it.
- Changing the number of workers inside a process that's already running.
- Handing a failed row to a different worker instead of marking it `Failed`.
- Linking each Rust result into the full C program and testing it.
- Passing the byte-range struct to `iterator` in place of line numbers.
