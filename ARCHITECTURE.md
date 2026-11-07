# Argo architecture

> **Design, not an implementation report.** The repository was empty when this document was drafted on 2026-11-07. The crates, schemas, commands, screenshots, and runtime behavior described below are proposed. This document owns the architecture and initial acceptance milestones; it does not claim that any migration has run.

## 1. Product and design basis

Argo progressively migrates a legacy program to Rust through a live, UE5-Blueprints-like node graph. The graph is both a navigable semantic map and a set of replaceable implementation boundaries. A functioning mixture of original C and generated Rust nodes is the central demonstration—not a translation revealed only after the entire project finishes.

**Golden project:** translate TinyEMU's C implementation needed for RV64 emulation into Rust, far enough to boot a RISC-V Linux **X11 desktop**. A console boot, an x86 guest, or Rust wrapping an unchanged C emulator does not meet that target. Small programs remain the first integration gate; the TinyEMU desktop is the requested end-to-end destination, not a silently optional stretch goal.

The product follows these user-originated decisions:

1. Use an AST as a proxy for recursive decomposition at meaningful block resolutions, not token-level nodes. Cheap agents explain leaves and recursively explain their parents, retaining the original source behind the explanations. Publish the semantic map into SpacetimeDB as work completes.
2. Give a larger Nemotron model—550B Ultra is the intended architect—the semantic map to propose dependencies, architectural decisions, and structural refactors. Pass those decisions down to agents implementing the target nodes.
3. Represent composition with a node-based, Blueprint-style IR. Each executable chunk has explicit inputs, outputs, control flow, and relevant effects, with original-language and target-language implementations.
4. First reorganize the program into the graph **without changing languages**, then check it against the original. Replace implementations incrementally through C ABI boundaries, allowing C and Rust to execute together. Reassemble the accepted graph into a regular Rust project.
5. Use SpacetimeDB for shared program state and agent coordination; compilation and correctness checks execute on worker machines, not the database server.
6. Ship one agent worker on simple programs first. Many workers attaching from other machines, broad language support, and deep refactoring are stretch goals.
7. Build the application frontend and backend in Rust. Existing legacy inputs and generated C interoperability shims are migration artifacts, not a non-Rust application backend.

Supporting mechanisms below are engineering proposals, not additional user requirements. In particular, job leases, report formats, and scope restrictions are means to ship the above design.

## 2. Hackathon scope

### Required first end-to-end slice

- One local SpacetimeDB instance, one Rust worker process, one Rust GUI client; one migration job/model interaction at a time. The worker may run in one prebuilt container. No cluster or VM provisioning service.
- A deliberately small, documented C input subset: a few functions, fixed-width integer arguments/results, bounded arithmetic, calls, conditionals, and a loop. Start with a supplied deterministic fixture rather than arbitrary repositories.
- AST-derived function/block hierarchy with bottom-up explanations appearing live. Function-level boundaries are independently replaceable in the first slice. Nested blocks can be visible explanation regions without falsely claiming that every region is already an independently executable ABI node.
- A generated same-language graph build that matches the original on the declared fixture/test domain.
- An actual agent-generated Rust replacement for at least one node, used by a mixed C/Rust program, with local and mixed-program checks recorded before acceptance.
- A GUI closely resembling UE5 Blueprints, connected to real database updates, with source/explanation inspection and visible validation evidence.
- An all-Rust output project for the fixture after all its migration units pass. Remove C dependency/glue during final export and rerun the same declared tests.

### Explicitly not required for the first slice

Arbitrary languages/repositories; every AST block becoming a callable unit; pointer-heavy C; heap aliasing; threads; macros beyond the selected fixture; exceptions; production external effects; hot library unloading; universal equivalence proofs; automatic remote worker provisioning; multi-machine swarms; collaborative semantic graph editing; sophisticated auto-layout; arbitrary target-language backends; or broad architectural rewrites.

Reject unsupported constructs with a source-located diagnostic. Do not quietly replace them with a model's prose. Test results establish agreement on their stated domain, not a proof for all executions.

### Golden-project track: TinyEMU RV64 → Linux X11

The small-subset restrictions above describe the bootstrap slice, not permission to omit TinyEMU features needed by the golden boot. Expand the supported surface deliberately after that slice.

- **Source baseline:** upstream [TinyEMU 2019-12-21 archive](https://bellard.org/tinyemu/tinyemu-2019-12-21.tar.gz), downloaded during planning; SHA-256 `be8351f2121819b3172fcedce5cb1826fa12c87da1b7ed98f269d3e802a05555`. Inspection only: it has not been built or booted in this task.
- **Guest starting point:** official [Buildroot RV64 X Window configuration](https://bellard.org/jslinux/buildroot-riscv64-xwin.cfg). The checked configuration specifies `machine: "riscv64"`, 256 MiB, `bbl64.bin`, `kernel-riscv64.bin`, VirtIO 9P root storage, `simplefb` at 1024×640, and VirtIO input. The official JSLinux page also lists an RV64 X Window guest. Native compatibility and local reproduction still need to be established.
- **Scope of “C portion”:** migrate the TinyEMU-owned C implementation closure needed by the chosen RV64 desktop configuration: CPU execution and machine/memory wiring, required devices, storage/filesystem paths, configuration/front-end glue, and needed helper code. Not just the instruction decoder. Guest firmware, Linux/Xorg/root filesystem, and external host libraries are inputs/dependencies, not programs to translate. The x86/KVM path and browser JavaScript/Emscripten frontend are out of scope. Any networking/SLIRP retained by the selected configuration must be accounted for explicitly, or removed from both baseline and candidate configurations before comparison.
- **First prerequisite:** boot the unmodified C baseline into the exact intended desktop on the demo host. Pin the guest firmware/kernel/filesystem and configuration by hashes; record licenses and required host libraries. Cache or prepare the guest's HTTP/9P content locally so judging does not depend on a live remote root filesystem. Do not assume the busybox console image alone supplies X11.
- **Migration strategy:** begin with bounded helpers/devices, then migrate larger coupled regions with explicit shared-state/ownership contracts. Headers, preprocessor templates, pointer-rich state, callbacks, and memory-mapped I/O mean the toy subset is insufficient. A large unported region stays an honest C node; it is not disguised as translated Rust.
- **Evidence ladder:** original C desktop → graph-organized C desktop → mixed C/Rust desktop at accepted milestones → Rust implementation of the in-scope emulator closure booting that same guest. Use focused instruction/helper cases and bounded CPU state/memory/device traces to localize failures rather than rerunning a complete desktop boot for every tiny edit.
- **Golden acceptance:** the final artifact runs the RV64 guest, reaches a visible X server/window-manager desktop, accepts keyboard/pointer interaction, and demonstrates an application/window interaction. Record guest architecture identity, boot logs, source/build/config/guest hashes, screenshot/video, and a scripted interaction result. The final in-scope emulator implementation must not secretly link the original TinyEMU C; external host libraries remain disclosed dependencies. Preserve an original baseline capture for comparison.

**Baseline manifest (S workstream owns it; W executes it):** record native host OS/architecture, target triple, compiler/linker flags, enabled TinyEMU translation units and preprocessor variants, permitted external libraries, source/guest/configuration hashes, licenses, storage/network backend, display adapter, and measured boot budget. The upstream frontend uses the old SDL API; SDL2/SDL3 availability alone does not establish compatibility. Pin the compatible firmware/kernel rather than assuming a modern QEMU/OpenSBI image works.

**Storage is an explicit open choice, not a hidden scope cut:** the supplied HTTP-backed 9P root includes TinyEMU network-filesystem code plus its host dependencies. A prepared local 9P tree/block image might avoid that code, but must preserve the guest-visible filesystem and be selected for both baseline and candidate. At G0, the team records this choice and the resulting source closure before migration. Likewise, SLIRP/networking cannot remain unported behind an undisclosed exemption. Do not reclassify TinyEMU-distributed helpers as external libraries.

**Deterministic validation mode:** upstream native code enables real-time RTC; clock, input, filesystem/network completion and scheduling otherwise make repeated boots diverge. S's harness must provide controlled time/event inputs (for example, a disclosed cycle-based clock mode and scheduled input), applied equivalently to baseline and candidate. Record any harness patch separately from the pristine baseline. Use bounded architectural-state checkpoints, memory hashes and device events; normalize resource identities instead of comparing host pointer bytes, padding or incidental timestamps. Interactive desktop acceptance is a separate check, not a claim that arbitrary live boots are byte-identical.

**Block-level migration is required on the golden path:** the CPU interpreter is generated from a large template with extensive `goto` control flow. A whole-function-only port would collapse it into one giant migration. Outline supported regions with an explicit live-in/live-out frame, shared-state handles and control-exit tags; the caller reconstructs branches/loops/returns. Start coarse enough not to add an ABI hop to every instruction. Broader arbitrary block extraction remains stretch, but the supported outlining needed for TinyEMU is not deferred until after the golden boot.

TinyEMU → X11 is the requested north-star acceptance test, not a guarantee of hackathon completion. G3 is the minimum honest incremental-migration demo, G4 finishes the toy slice, and G4b is the first TinyEMU-native replacement; none completes G6. If the original desktop baseline cannot be reproduced, mark the golden gate blocked while continuing bootstrap development.

## 3. Three independently buildable parts

These are three code/workstream boundaries, not a requirement for three people or a claim of identical effort. W contains the highest technical uncertainty. To make the packages more comparable, S also owns the reusable validation specification/harness: baseline provenance, layout checks, deterministic runner, capture and comparison. **That native support code runs on W's machine, never inside the database server.** U owns the substantial custom Blueprint canvas rather than a dashboard shell.

Proposed two-person allocation: one builder leads W; the other owns S + U initially. Both freeze the contract and assess the TinyEMU baseline risk together. Once G1 connects real clients, split the remaining S-owned harness work explicitly and reassess the worker bottleneck. These are proposed assignments for the team to choose, not an invented third contributor; no amount of server bookkeeping makes the full TinyEMU translator a small task.

| Part | Owns | Does not own | Standalone deliverable |
| --- | --- | --- | --- |
| **S — Server and SpacetimeDB** | Shared protocol/bindings, structural graph checks, stage/job lifecycle, revision/idempotency/report binding, reconnect fixtures; validation-harness specification and support library | Model calls, worker scheduling infrastructure, execution inside reducers, canvas rendering | Seed/claim/report/accept transcript with negative contract tests; independently runnable golden-baseline harness invoked on worker compute |
| **W — Worker client** | Source/build import, semantic map and architect/model loop, executable graph and ABI generation, mixed linking, invocation of S's harness, export | Authoritative server transitions, duplicate baseline/capture/comparison machinery, GUI state | Source → C graph → mixed graph → Rust export using a fake or real server client |
| **U — GUI control-plane client** | Blueprint-like scene/hit testing, hierarchy/navigation, source/plan/evidence inspectors, subscription projection, controls, captures and reconnect UX | Agent prompts, compilation, acceptance decisions | Faithful seeded scene plus real live deltas, controls, counterexample and boot-evidence inspection |

Proposed layout (not created yet):

- `crates/argo-protocol/`: shared Rust DTOs, IDs, schema version, fixture messages. Part S maintains it; both clients review changes. This is a shared boundary, not a fourth product subsystem.
- `crates/argo-server/`: Rust SpacetimeDB module and server-side transition rules.
- `crates/argo-harness/`: S-owned native support library/runner for reproducible baselines, layout assertions, test execution, traces and captures. W invokes it locally; it is not a fourth deployed subsystem.
- `crates/argo-worker/`: Rust native worker, model adapter, generation pipeline, linking, harness invocation and export.
- `crates/argo-ui/`: selected Rust UI toolkit, graph scene, inspectors, subscription adapter.
- `fixtures/`: small C inputs, fixed test domain, expected graph/protocol snapshots, and deliberately failing candidates.

Keep toolkit-specific types out of protocol/domain records. Freeze protocol v1 before parallel implementation; subsequent changes are explicit coordinated revisions.

## 4. Interface between the parts

### Transport and state ownership

Both clients use the Rust SpacetimeDB SDK. Commands are reducer calls; clients subscribe to durable tables for the project. Generated SDK bindings are the concrete transport interface; `argo-protocol` defines framework-independent meanings and local fixtures. Do not invent a second authoritative REST server.

Reducers validate identity, revisions, allowed transitions, and report metadata. They do not invoke models or compilers. All such I/O and CPU work runs in W. Network callbacks reach the clients' async adapters, which deliver messages to their own event loops.

U or W uploads bounded UTF-8 source files individually and seals a source-bundle manifest, rather than sending a whole repository in one reducer call. S retains source, node implementations, summaries and compact evidence for U. Binaries/build directories remain on W; a local binary path is not a remotely downloadable artifact. Cross-machine distribution is stretch.

Initial proposed limits: 1 MiB/source file, 16 MiB/source bundle, 128 records/graph batch, 128 KiB/report log excerpt, and 128 KiB/inline evidence preview. Confirm these against the pinned SDK and actual fixture at G0; they are design bounds, not measured service limits. Reject oversize submissions explicitly. Subscribe to graph/status metadata first and fetch source/log details on selection, not every inspector subscribing to every full text.

### Shared records: protocol v1 sketch

| Record | Essential fields/meaning |
| --- | --- |
| `Project` / `SourceFile` | Owner, languages, current IDs; sealed bundle, relative path, exact source bytes/hash |
| `BuildConfig` / `TranslationUnit` | Target triple, compiler/flags/defines, include inputs, selected translation unit and macro/template instantiation identity |
| `GraphRevision` | Source/build identity, parent, `draft`/`frozen`, canonical frozen-content hash |
| `Node` | Logical ID + revision + translation-unit/instantiation identity, parent region, kind, source/expansion spans, executable vs explanation-only status, interface hash |
| `Pin` / `Edge` | Stable pin IDs, directions/types, exec/data/state/control-exit semantics; explicit call/callback relationships |
| `TypeLayout` / `EffectContract` | Optional on scalar fixture, required where used: size/alignment/field offsets; state/resource accesses, indirect calls and control exits |
| `Annotation` | Summary, assumptions/unknowns, source/context/model attribution; never executable truth |
| `ArchitectPlan` / `NodeDirective` | Input semantic-map identity, architect model/run, updated plan, dependency decisions, parent directive chain, per-region instructions |
| `Implementation` | Node/interface/build identity, C or Rust source/hash, artifact references, validation status |
| `Run` / `Job` / `Attempt` | Stage and prerequisites, exact inputs, authenticated worker, lifecycle, idempotency keys, progress, model/token usage |
| `ValidationReport` | Exact baseline/candidate/full-manifest identities, build/toolchain/test-domain/harness identity, outcome, cases, counterexample, bounded logs |
| `Manifest` | Versioned node → implementation mapping, frozen graph/interface/build identity, accepted base version |
| `Artifact` | Hash, kind, size, owner/location and optional bounded inline preview; report/source/boot evidence references |

Nodes cannot be identified by source span alone: TinyEMU compiles/includes the same source under different width/XLEN settings. At G0 select the parsing/preprocessing route and retain both expansion and original-source provenance. RV64 targeting does not justify deleting all x32 paths or 32-bit floating-point behavior; audit the variants required by the pinned guest. Exclude x86/RV128 only through an explicit validated build configuration.

Node positions, viewport and selection belong to U and are excluded from semantic hashes. Language and validation status are independent dimensions.

### Commands and lifecycle

These are proposed contracts, not existing Rust functions:

- `begin_source_bundle`, `put_source_file`, `seal_source_bundle` — U/W → S; idempotent file upload and canonical manifest sealing.
- `request_run(project_id, stage, input_ids, request_id)` — U → S; queues only stages whose prerequisites exist.
- `claim_job(job_id)` — W → S; atomically starts an attempt using the caller's authenticated identity, not an authority supplied in `worker_id`.
- `publish_graph_batch(job_id, attempt_id, batch_id, draft_revision, records)` — W → S; idempotent upserts by stable record IDs.
- `freeze_graph(job_id, attempt_id, draft_revision)` — W → S; checks structure, then computes/checks the canonical revision hash.
- `publish_plan`, `submit_candidate`, `submit_report` — W → S; persist version-bound proposals and tool evidence, not automatic acceptance.
- `report_progress`, `finish_attempt` — W → S; update progress and close as succeeded/failed with required output/report references.
- `cancel_run` / `retry_job` — owner → S; abandon/cancel safely or create a new attempt; W observes cancellation and stops only its owned execution.
- `accept_manifest(project_id, expected_base_version, candidate_manifest, report_ids)` — W/owner → S; request acceptance under the checks below, not self-certified correctness.

Lifecycle: `queued → running → succeeded | failed | abandoned`. Retry creates a new attempt; late results from the old attempt remain inspectable but cannot advance current state. Use request/batch IDs for replay safety even with one worker. Distributed lease expiry/fencing and capability routing are deferred until multi-worker operation.

**Freeze:** S validates pin existence/types, containment acyclicity, supported control structure, call/interface links and required implementation references. Supported loop cycles are not mistaken for invalid containment cycles. Hash stable sorted semantic records and exact source hashes, excluding layout/progress; S is responsible for canonicalization, not blindly accepting W's claimed digest. Structural validity is not behavioral correctness.

**Acceptance:** S binds passing local and mixed-program reports to the exact candidate source/build, frozen graph, full selected manifest, unchanged accepted base, established same-language baseline, toolchain/flags and declared test domain. A body/interface/build/fixture/harness change requires evidence for the new identity. Old evidence is retained for the old artifact; it does not silently certify the new one. Final C-free export has its own report. The team trusts its local tool runner; hashes do not prove a worker actually executed tests.

### Subscription initialization and parallel handoff

U initializes from the SDK's applied subscription/cache, then processes deltas; do not query a separate snapshot and subscribe later with a missed-update gap. Reload durable state on reconnect. Render partial draft batches as pending; only a structurally frozen, report-backed selection may look accepted.

At G0 agree one source/graph/manifest example and a complete command/update transcript. W emits it through a fake sink; U renders it; S checks it, including stale/duplicate/invalid submissions. Include a TinyEMU-shaped example with a state handle, callback and control-exit pin, not only scalar nodes. G1 must replay the same meanings through real generated SDK bindings.

## 5. Worker pipeline and executable graph

### 5.1 Understand bottom-up

Parse the fixture into meaningful regions with exact source spans. Explain leaves with bounded source/context, then explain their parents using child explanations plus access to the retained source. Source remains immutable; replacing a chunk with an explanatory comment is a contextual/view transformation, not deletion of executable content.

Record inputs/outputs, referenced definitions, state/effects, control exits, and unresolved questions where relevant. Publish each completed annotation and node batch immediately. Avoid a separate model call for every token-level AST node.

The first worker performs these steps sequentially. Small-model summarization and a larger architect are roles in that worker, not a requirement for concurrent agents.

### 5.2 Executable IR v0 and same-language validation

The containment tree organizes explanations; it is not an execution schedule. Executable control edges, call dependencies, data dependencies and state access are separate graph relations. Initial callable units are functions with generated C signatures. Internal branches/loops remain in their retained C bodies and can appear as explanation regions; the UI does not pretend those regions are independently replaceable yet. The golden track adds supported block outlining with explicit frames and control exits.

The graph is authoritative for supported call-site bindings and selected implementations: generated wrappers/link bindings must invoke the graph-declared target, not leave original symbols secretly in charge. Compile source bodies retained from the original, not reconstructed from summaries. Show indirect calls as callback pins/dynamic-call nodes with known targets or an explicit unresolved marker.

**Evaluation:** materialize arguments/results once per declared call; sharing a data pin does not implicitly execute a side effect again. Retain sequencing, return destinations, loop backedges and exit behavior. The fixture avoids unspecified C argument evaluation and undefined behavior; later imports must declare compiler/configuration and defined-behavior assumptions rather than invent universal C semantics. Numeric operations preserve source widths, truncation/wrapping, shifts and exceptional cases explicitly. Emulator SoftFP behavior includes guest rounding and exception flags, not a substitution with host floating point.

**Minimal typed effects:** `pure`; `state_read/write(resource, fields)`; `memory_read/write(parameter)`; `indirect_call(callback)`; `host_io(resource)`; control exits/nonreturn; and `unknown`. Effects identify the state to snapshot and the dependencies to display; they are not a theorem that all behavior has been captured. The fixture permits pure computation only, with explicit call/return. Unknown/unsupported effects prevent a local node from being advertised as validated; expand the envelope for the golden track.

**Worked fixture contract (proposed, not implemented):** `compute(n)` starts `acc = 0`, loops `i = 0..n`, and calls `step(acc, i)`; `step` calls `normalize(acc + i)`; `normalize(x)` returns `x & 255`. Use unsigned 32-bit values and a declared test domain `n = 0..32`. The hierarchy shows the loop inside `compute`, while executable calls bind `compute → step → normalize`. A manifest choosing C `compute`, Rust `step`, and C `normalize` exercises an actual C → Rust → C path. Compare original C, generated all-C graph, mixed manifest and final Rust export on the same cases, plus a deliberately incorrect `step` candidate.

### 5.3 Architect and translate top-down

The intended architect is Nemotron Ultra 550B. It receives the complete semantic map for the fixture and returns a versioned `ArchitectPlan`: updated module/grouping intentions, dependencies, interface decisions, assumptions and region directives. Walk that planned hierarchy top-down; each child receives its parent's architectural prompt/decisions, its own directive, allowed dependencies, original source and evidence. Persist the lineage so U can inspect what actually conditioned the rewrite.

For the first migration, keep executable interfaces stable; broad structural refactoring/dependency replacement is stretch. Verify endpoint access before depending on Ultra. Persist the provider/model actually used; another model is a disclosed choice, not a silent claim that Ultra ran. For TinyEMU, measure the full semantic-map context budget first. If it does not fit the actual endpoint, report that constraint and discuss hierarchical/retrieval adaptations rather than claiming the entire map was submitted when it was not.

**Token discipline:** cache summaries by source + resolved-context + prompt/model identity; batch small meaningful regions without losing node IDs; reuse only genuinely equivalent template summaries; record estimated/actual tokens by stage and cap retry budgets. Keep parent guidance compact, and make retained source available for drill-down. Do not repeatedly regenerate a validated child because its parent explanation changed.

### 5.4 Mixed-language incremental replacement

Each migration node exposes a generated C-compatible ABI. This is a **same-target** contract, not a portable wire format. Pin target triple, scalar widths, layout/alignment, ownership/allocation/freeing, callback lifetimes and panic/unwind policy. Use `extern "C"`, checked representations and explicit scalar/buffer/handle/control-exit types. Never assume `repr(C)` makes Rust containers, references or trait objects C-compatible.

For the golden shared-state ABI, generate mirrors/accessors from the same configured headers and check size/alignment/field offsets on both sides. Initially retain C-owned state identity; Rust accesses raw pointers/accessors with explicit lifetimes. Do not hold exclusive Rust references across callbacks that can re-enter and mutate that state. This phase may require unsafe Rust; it is not advertised as a safe-Rust proof. Reject unsupported layouts/callbacks. Prevent panics unwinding across FFI; use an explicit policy such as abort or caught-unwind-to-status for supported entry points.

Keep both implementations available, but select implementations **at link time from the manifest**, rebuilding between runs. Do not put a generic runtime dispatcher into each guest-instruction hot path by default; benchmark granularity against the measured baseline budget. C nodes call Rust and vice versa through generated declarations/bindings. No hot unloading is needed.

Before acceptance, run local cases and the actual mixed program containing that candidate and selected neighbors. Reports identify the full build and manifest. A local pass does not certify every combination; milestone desktop boots complement bounded instruction/device checks. ABI compatibility alone proves neither behavior nor ownership isolation.

The fixture has no irreversible effects/shared aliased heap. Golden nodes require controlled state/event inputs and the explicit effect contracts above. Never run two real external writes as a shadow comparison. Candidate tests execute without the supervisor's database/provider credentials and with bounded paths, time and resources.

### 5.5 Reassemble and export

Once all migration units for the fixture are Rust, emit an ordinary Rust project organized by the accepted architecture. Replace compatibility adapters with native Rust calls where supported. Treat scope, evaluation order, error handling, and destruction timing as semantics; do not concatenate snippets blindly. Rerun the declared tests against the final artifact. C-free export is a separate checked stage, not implied by green node badges.

## 6. GUI: a UE5 Blueprints-like control plane

The visual target is specifically UE5 Blueprints, not a generic force-directed code graph or a collection of dashboard cards. Use original application assets while matching the recognizable visual/interaction grammar.

### Required visual and interaction surface

- Large charcoal, pannable/zoomable grid canvas with a clear world-to-screen transform.
- Compact rounded rectangular nodes with colored title bars, labeled pins on opposing sides, and curved Bézier wires.
- Visually distinct execution and typed-data connections; consistent pin colors across nodes and legend.
- Nested graph navigation: double-click/expand a region, breadcrumb back out, and select a node in a hierarchy panel.
- Drag nodes, select/focus, frame the graph, inspect source and explanations; pointer hit testing must follow zoom/pan.
- Inspector showing original C, candidate Rust, architect guidance, current implementation, and actual validation evidence.
- Toolbar for import, explain/build, migrate/check, and export. Show which actions are unavailable and why.
- Clear independent indicators for C/Rust selection, running work, unvalidated candidate, passing evidence, and counterexample. Never use decorative green as a substitute for a report.
- Live population from Spacetime subscriptions without resetting selection or moving the whole graph on every update. Use simple stable hierarchical placement initially.

Render one hierarchy level at a time, cull off-screen nodes, and fetch inspector detail on demand so TinyEMU does not require drawing every expanded block at once. Define a consistent visual legend—for example white execution wedges, red booleans, teal integers, blue state handles and distinct callback pins—without claiming every Argo type is an exact Unreal type.

**Visual acceptance:** choose a named team-captured UE5 Blueprint reference, viewport and zoom. Later compare an actual Argo capture for grid density, compact title bars, typography, execution/data pin shapes, spacing, labels, wire curvature, selection and surrounding panels. A toolkit example or generic dark boxes is not evidence of fidelity. The host Argo GUI is distinct from the guest X11 desktop; embedding the guest display is not required.

A full Blueprint editor, arbitrary rewiring, minimap, execution-wire animation, multiplayer cursors, and sophisticated automatic layout are stretch. Basic node dragging/navigation and faithful node/pin/wire appearance are core.

### Toolkit options: decision remains open

All three are legitimate candidates. None is assumed to include a ready-made Blueprint editor. The domain graph and subscription adapter must survive the toolkit choice.

| Option | Relevant strengths | Hackathon risk / question |
| --- | --- | --- |
| **iced** | Rust reactive/message architecture, custom widgets, async integration, cross-platform renderers | Custom graph canvas, text/pin composition, hit testing, and caching still need work. Its own README calls it experimental. Likely the conservative option given the team's initial preference, not a proven winner. |
| **GPUI** | GPU-accelerated editor-style framework, low-level custom Elements, retained state and integrated async facilities | Pre-1.0 API churn, platform setup, learning cost, and standalone build friction. Verify on the actual demo machine. |
| **Rune** (`anomalyco/rune`) | Native Rust UI framework with vector paths/cubic Béziers, drag input, animation and renderer control | New/pre-release ecosystem; pin an exact revision and validate platform/build stability. This is the UI toolkit, not the unrelated Rune scripting language. |

**Selection gate:** U spends a bounded initial spike on its strongest candidate: draw draggable nodes, labeled pins and a curved wire; pan/zoom; update a node from an async message; launch on the demo OS. Keep a screenshot and the exact dependency revision. If that fails, try the next candidate. Do not implement three complete GUIs or call an unrun spike successful.

### Reviewer opinions

Independent panel completed on 2026-11-07 against the user's own design contributions and latest three-part/TinyEMU updates, not the coordinator's previous conversational advice. The panel inspected documentation/source; neither reviewer built a toolkit or booted TinyEMU.

| Reviewer | iced | GPUI | Rune (`anomalyco/rune`) | Recommendation |
| --- | --- | --- | --- | --- |
| **Opus 5.5 / high** | Conservative default: cross-platform ecosystem, message/delta fit, custom drawing and renderer fallback; zoomed text/hit testing still need a spike | High editor ceiling but the riskiest newcomer path; choose only with existing owner expertise | Closest documented canvas fit; conditional upgrade if a pinned-revision demo-host spike passes; young/macOS-leaning ecosystem is a concern | **iced by default**, Rune conditional, GPUI only with familiarity; moderate overall confidence, lower confidence on iced-vs-Rune |
| **GPT 6.1 Sol / xhigh** | Conditional default absent stronger team expertise; documented Canvas/async building blocks, not a finished graph editor | Strong alternative with familiar custom Elements and verified host setup; no unsupported claim that it is macOS-only | Serious challenger: transformed hit testing, vector paths and headless tooling fit this project; can become preferred after a successful spike, but explicitly pre-release | **Conditionally iced**, moderate confidence; let demonstrated Rune/GPUI advantage override, without building three GUIs |

The panel's overlapping iced preference is not a benchmark, launch result, final user choice or dismissal of Rune. Keep all three options until U's bounded selection gate passes. Adopted review corrections include a shared-state/build-config ABI, explicit IR/effects, durable top-down directives, balanced harness ownership, reliable subscription initialization, and intermediate TinyEMU migration gates.

## 7. Initial integration milestones

| Gate | Deliverable and acceptance | Parallel handoff |
| --- | --- | --- |
| **G0: contract and baselines** | Agree fixture/ABI/test domain and one protocol transcript; independently reproduce/pin original TinyEMU RV64 X11 build/guest and deterministic harness conditions | S owns contract + baseline harness; W owns model access/import; U owns toolkit launch |
| **G1: visible shared state** | Real AST/explanation progress reaches the Blueprint-like canvas through actual SDK bindings; reconnect restores it | S/W/U first real vertical slice, still no migration claim |
| **G2: same-language baseline** | Original C and generated graph C match on the fixture domain; U exposes the report | W generates/invokes S's runner; S enforces evidence prerequisites |
| **G3: one Rust replacement** | Model-generated Rust participates in real C → Rust → C execution; local/mixed cases pass; a bad candidate is rejected | Minimum honest demo, not complete toy/golden acceptance |
| **G4: complete fixture export** | All toy units become Rust; clean exported project builds without original C and matches tests | First slice complete; not TinyEMU completion |
| **G4b: TinyEMU leaf replacement** | Port a bounded helper inside actual C TinyEMU; focused comparison passes and selected mixed emulator still boots the pinned desktop | First TinyEMU-native result; choose a suitable utility or bounded SoftFP operation preserving flags |
| **G5a: shared-state/devices** | Layout-checked state/callback ABI; required device/storage/input paths migrate with bounded traces and periodic desktop checks | W translation plus S harness; U shows state/callback/evidence details |
| **G5b: interpreter regions** | Supported outlined CPU blocks preserve frame state and control exits, selected by manifest with usable boot budgets | Block-level migration on the golden path, not deferred stretch |
| **G6: golden Rust desktop** | In-scope TinyEMU closure is Rust; clean export/source-link inventory and pinned interactive RV64 X11 acceptance pass | No console/x86/unchanged-C-wrapper substitution |
| **G7: stretch expansion** | Additional workers, leases/recovery, remote artifacts, arbitrary block extraction, aggressive architectural refactors and broader languages | Version contracts explicitly; do not build a cluster first |

Proposed G0 spike cutoffs (budgets, not observed runtimes): W gets 20 minutes for an authenticated model/tool check; U gets 45 minutes for its first toolkit scene/launch; S with W gets 60 minutes to expose the original native TinyEMU/guest blockers. At a cutoff record the blocker and agree the next action rather than pretending readiness. Measure boot, short-comparison and interactive-test budgets only after the baseline runs. Bootstrap G1–G4 may proceed while golden readiness is blocked; G4b–G6 require that baseline.

If Ultra is unavailable, seek an explicit model choice and preserve other progress. If the golden project is incomplete at judging, demonstrate the highest completed gate and label G6 unfinished. Scope cuts change the reportable completion state, not what RV64 X11 means.

## 8. Invariants, failure handling, and evidence

- The original source and frozen graph identity remain recoverable. Explanations are attributed claims, not the runtime semantics.
- Model output is an untrusted candidate. Compiler/test tools produce reports; only matching accepted reports can advance a manifest.
- UI controls and worker claims do not bypass server-side revision/identity checks. MVP may trust the team's worker, but does not claim to verify its CPU execution cryptographically.
- Failed builds, mismatches, unsupported constructs, tool timeouts, and unavailable models are distinct states with inspectable evidence.
- The credentialed worker supervisor invokes S's native harness with restricted child environments/paths and time/resource limits; generated/legacy programs do not inherit database/model credentials. A container alone is not this separation. No production sandbox or untrusted-remote-worker guarantee is established.
- SpacetimeDB holds orchestration and program state, not native FFI execution. Native linking and execution happen in W; they cannot be moved into a database reducer by adding an ABI attribute.
- Reproducible command lines, target/configuration, defined-behavior assumptions and toolchain versions accompany reports. Sanitizer/test passes do not prove C definedness. A port matching TinyEMU is not independent proof of conformance to every current RISC-V specification.
- Final export evidence includes a clean build and source/link-input inventory, not just `.rs` file counts or the executable name. No runtime, performance, security, or visual-fidelity claims are established by this documentation-only commit.

Implementation verification should include a failing candidate before repair, a stale-result rejection test, reconnect behavior, a real C → Rust → C call path, same-language and mixed/final comparisons, and a captured GUI view at a named viewport/revision. Do not invent existing build/test commands before the workspace exists.

## 9. Source references and open decisions

Primary references checked during drafting; repository links below are pinned documentation snapshots, not dependency versions selected for Argo:

- [SpacetimeDB reducers](https://spacetimedb.com/docs/functions/reducers/), [procedures](https://spacetimedb.com/docs/functions/procedures/), and [subscriptions](https://spacetimedb.com/docs/clients/subscriptions/): transaction/I/O and applied-cache boundaries. W owns external execution in this design.
- [Rust FFI guidance](https://doc.rust-lang.org/nomicon/ffi.html): calling conventions, representation, ownership, and unwinding.
- [Epic Blueprint functions](https://dev.epicgames.com/documentation/en-us/unreal-engine/functions-in-unreal-engine): execution versus data-oriented function semantics; Argo must define its own evaluation rules.
- [iced README](https://github.com/iced-rs/iced/blob/8ff48c42b0d5188223501ea1ccc506fd5beaf36a/README.md#L27-L45) and [interactive Bézier example](https://github.com/iced-rs/iced/blob/8ff48c42b0d5188223501ea1ccc506fd5beaf36a/examples/bezier_tool/src/main.rs).
- [GPUI README](https://github.com/zed-industries/zed/blob/b5cd310b3b54dfc659fcc06453e6358843dfe216/crates/gpui/README.md#L1-L15) and [custom Elements](https://github.com/zed-industries/zed/blob/b5cd310b3b54dfc659fcc06453e6358843dfe216/crates/gpui/README.md#L75-L99).
- [Rune vector rendering/input](https://github.com/anomalyco/rune/blob/99c123d6b2127d1226c72dec37919e51019142ff/crates/rune/README.md#L56-L145).
- [Nemotron Ultra model card](https://huggingface.co/nvidia/NVIDIA-Nemotron-3-Ultra-550B-A55B-BF16): intended architect; event-account inference access is still a prerequisite, not verified here.
- [TinyEMU upstream](https://bellard.org/tinyemu/), [technical README](https://bellard.org/tinyemu/readme.txt), and [JSLinux guest list](https://bellard.org/jslinux/): RV64, VirtIO devices, SDL graphics, MIT licensing, and available RISC-V X Window configurations. A listed web guest is evidence of availability, not proof of this project's native baseline boot.

Open at implementation start: team ownership of S/W/U; GUI toolkit/revision and demo OS; fixture parser and golden compiler-backed AST/preprocessing route; ABI/configuration/layout v1; actual small-model/Ultra endpoints and context budget; concrete SpacetimeDB SDK/schema; test domains and timeouts; TinyEMU baseline, complete source closure, pinned guest assets, offline storage/network/display choices and measured golden-boot budget. G0 resolves or records blockers; it does not silently broaden or shrink acceptance.
