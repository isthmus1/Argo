# Argo architecture

> **Design, not an implementation report.** Drafted and revised on 2026-11-07. This revision incorporates the worker/swarm, private-model, and recursive-modernization discussion. This checkout contains documentation, not an executable workspace. The crates, schemas, commands, screenshots, and runtime behavior below are proposed. This document owns the architecture and initial acceptance milestones; it does not claim that any migration has run.

## 1. Product and design basis

Argo progressively migrates a legacy program to Rust through a live, UE5-Blueprints-like node graph. First establish a behavior-preserving port, allowing temporary C-shaped interfaces and justified unsafe Rust. Then hill-climb that working Rust implementation toward native interfaces, clearer ownership, and maintainable structure through bounded, continuously tested refactorings. A functioning mixture of original C and generated Rust remains the first migration demonstration—not a translation revealed only after the entire project finishes.

**Product thesis:** make migration and modernization practical with small, privately hosted agents, rather than requiring a frontier model with the entire program in context. Nemotron Lightning and potentially Nano are intended worker candidates; their capability, hardware footprint, throughput and total cost are hypotheses to measure, not established results. A strong agent completing a direct port is a useful comparison baseline, not proof that private small-model orchestration is unnecessary. The system must demonstrate an advantage under the customer's deployment and confidentiality constraints.

**Golden project:** translate TinyEMU's C implementation needed for RV64 emulation into Rust, far enough to boot a RISC-V Linux **X11 desktop**. A console boot, an x86 guest, or Rust wrapping an unchanged C emulator does not meet that target. Small programs remain the first integration gate; the TinyEMU desktop is the requested end-to-end destination, not a silently optional stretch goal.

The product follows these user-originated decisions:

1. Use source syntax/AST structure as a proxy for recursive decomposition, not token-level nodes. Functions, structs/unions, enums, macros and globals provide practical initial regions. Nested explanation regions need not be independently executable. Cheap agents explain leaves and parents while retaining original source; publish the map as work completes.
2. Use architect guidance for grouping, dependencies and bounded refactors. Ultra 550B was the original larger-model option, not a mandatory external service or a claim of verified access. In private mode every participating model must run within the approved environment; do not conceal a frontier-model rescue behind a small-worker demonstration.
3. Represent source provenance, executable relationships and replacement/composite regions as distinct graph views. Declaration boundaries do not imply purity, independent ownership, or a permanent target architecture. A callable region declares its interface, control behavior and relevant effects.
4. First validate the graph-organized original-language program. Port implementations incrementally through temporary C ABI bridges, allowing faithful structure and justified unsafe Rust. Establish a validated Rust checkpoint before demanding idiomatic redesign; ABI compatibility is not behavioral correctness.
5. Modernize through recursive region coarsening: neighboring accepted Rust regions can form a composite, then agents improve bounded internal relationships while preserving the external contract. Repeatedly test and accept small improvements. The final root represents a program of ordinary Rust modules, not one enormous function or model prompt.
6. SpacetimeDB coordinates work **before ingestion begins** and throughout decomposition, translation, validation and modernization. Workers read repositories and execute tools/models; the server owns durable task dependencies, claims, revision checks and accepted state.
7. Start with one worker and a small fixture, but freeze swarm-safe ownership and revision semantics early. Demonstrate competing claims/recovery before enabling concurrent mutation. Private small-model swarms and bounded modernization are core design goals; automatic cluster provisioning and unrestricted whole-program redesign are not prerequisites.
8. Build the application frontend and backend in Rust. Legacy inputs and generated C shims are migration artifacts. Source, source-derived summaries/prompts, traces and diagnostic logs stay within the approved trust boundary; privacy applies to the database, harness, model endpoints and telemetry too.

Supporting mechanisms below are engineering proposals, not additional user requirements. In particular, job leases, report formats, and scope restrictions are means to ship the above design.

## 2. Hackathon scope

### Required first end-to-end slice

- One local SpacetimeDB instance, one Rust worker process, one Rust GUI client; one migration job/model interaction at a time. The worker may run in one prebuilt container. No cluster or VM provisioning service.
- A deliberately small, documented C input subset: a few functions, fixed-width integer arguments/results, bounded arithmetic, calls, conditionals, and a loop. Start with a supplied deterministic fixture rather than arbitrary repositories.
- AST-derived function/block hierarchy with bottom-up explanations appearing live. Function-level boundaries are independently replaceable in the first slice. Nested blocks can be visible explanation regions without falsely claiming that every region is already an independently executable ABI node.
- A generated same-language graph build that matches the original on the declared fixture/test domain.
- An actual agent-generated Rust replacement for at least one node, used by a mixed C/Rust program, with local and mixed-program checks recorded before acceptance.
- A GUI closely resembling UE5 Blueprints, connected to real database updates, with source/explanation inspection and visible validation evidence.
- An all-Rust fixture checkpoint with no original C dependency, followed by at least one accepted bounded modernization inside a composite Rust region. Demonstrate removal of an unnecessary internal compatibility boundary and rerun the unchanged regression domain. A C-free checkpoint alone does not establish idiomatic modernization.

### Explicitly not required for the first slice

Arbitrary languages/repositories; every AST block becoming a callable unit; pointer-heavy C; heap aliasing; threads; macros beyond the selected fixture; exceptions; production external effects; hot library unloading; universal equivalence proofs; automatic remote worker provisioning; multi-machine deployment; collaborative semantic graph editing; sophisticated auto-layout; arbitrary target-language backends; or unrestricted architectural rewrites. This sequencing does not defer the swarm protocol or make bounded Rust modernization a stretch goal.

Reject unsupported constructs with a source-located diagnostic. Do not quietly replace them with a model's prose. Test results establish agreement on their stated domain, not a proof for all executions.

### Golden-project track: TinyEMU RV64 → Linux X11

The small-subset restrictions above describe the bootstrap slice, not permission to omit TinyEMU features needed by the golden boot. Expand the supported surface deliberately after that slice.

- **Source baseline:** upstream [TinyEMU 2019-12-21 archive](https://bellard.org/tinyemu/tinyemu-2019-12-21.tar.gz), downloaded during planning; SHA-256 `be8351f2121819b3172fcedce5cb1826fa12c87da1b7ed98f269d3e802a05555`. Inspection only: it has not been built or booted in this task.
- **Guest starting point:** official [Buildroot RV64 X Window configuration](https://bellard.org/jslinux/buildroot-riscv64-xwin.cfg). The checked configuration specifies `machine: "riscv64"`, 256 MiB, `bbl64.bin`, `kernel-riscv64.bin`, VirtIO 9P root storage, `simplefb` at 1024×640, and VirtIO input. The official JSLinux page also lists an RV64 X Window guest. Native compatibility and local reproduction still need to be established.
- **Scope of “C portion”:** migrate the TinyEMU-owned C implementation closure needed by the chosen RV64 desktop configuration: CPU execution and machine/memory wiring, required devices, storage/filesystem paths, configuration/front-end glue, and needed helper code. Not just the instruction decoder. Guest firmware, Linux/Xorg/root filesystem, and external host libraries are inputs/dependencies, not programs to translate. The x86/KVM path and browser JavaScript/Emscripten frontend are out of scope. Any networking/SLIRP retained by the selected configuration must be accounted for explicitly, or removed from both baseline and candidate configurations before comparison.
- **First prerequisite:** boot the unmodified C baseline into the exact intended desktop on the demo host. Pin the guest firmware/kernel/filesystem and configuration by hashes; record licenses and required host libraries. Cache or prepare the guest's HTTP/9P content locally so judging does not depend on a live remote root filesystem. Do not assume the busybox console image alone supplies X11.
- **Migration strategy:** begin with bounded helpers/devices, then migrate coupled regions with explicit shared-state/ownership contracts. Faithful, temporarily C-shaped Rust is an acceptable checkpoint; native redesign follows through tested modernization jobs. Headers, preprocessor templates, pointer-rich state, callbacks and memory-mapped I/O mean the toy subset is insufficient. A large unported region stays an honest C node, not a claimed Rust result.
- **Evidence ladder:** original C desktop → graph-organized C desktop → mixed C/Rust desktop → C-free Rust desktop checkpoint → incrementally modernized Rust desktop. Use focused instruction/helper cases and bounded state/memory/device traces for quick feedback. The pinned whole-program workload remains the primary golden regression gate, not a screenshot-only substitute for behavioral comparison.
- **Golden acceptance:** the final artifact runs the RV64 guest, reaches a visible X server/window-manager desktop, accepts keyboard/pointer interaction, and demonstrates an application/window interaction. Record guest architecture identity, boot logs, source/build/config/guest hashes, screenshot/video, and a scripted interaction result. The final in-scope emulator implementation must not secretly link the original TinyEMU C; external host libraries remain disclosed dependencies. Preserve an original baseline capture for comparison.

**Baseline manifest (S workstream owns it; W executes it):** record native host OS/architecture, target triple, compiler/linker flags, enabled TinyEMU translation units and preprocessor variants, permitted external libraries, source/guest/configuration hashes, licenses, storage/network backend, display adapter, and measured boot budget. The upstream frontend uses the old SDL API; SDL2/SDL3 availability alone does not establish compatibility. Pin the compatible firmware/kernel rather than assuming a modern QEMU/OpenSBI image works.

**Storage is an explicit open choice, not a hidden scope cut:** the supplied HTTP-backed 9P root includes TinyEMU network-filesystem code plus its host dependencies. A prepared local 9P tree/block image might avoid that code, but must preserve the guest-visible filesystem and be selected for both baseline and candidate. At G0, the team records this choice and the resulting source closure before migration. Likewise, SLIRP/networking cannot remain unported behind an undisclosed exemption. Do not reclassify TinyEMU-distributed helpers as external libraries.

**Deterministic validation mode:** upstream native code enables real-time RTC; clock, input, filesystem/network completion and scheduling otherwise make repeated boots diverge. S's harness must provide controlled time/event inputs (for example, a disclosed cycle-based clock mode and scheduled input), applied equivalently to baseline and candidate. Record any harness patch separately from the pristine baseline. Use bounded architectural-state checkpoints, memory hashes and device events; normalize resource identities instead of comparing host pointer bytes, padding or incidental timestamps. Interactive desktop acceptance is a separate check, not a claim that arbitrary live boots are byte-identical.

**Bounded interpreter work is required on the golden path:** the CPU interpreter is generated from a roughly 57 KB template function with extensive shared locals and `goto` exits. Declaration-level splitting alone does not make this a small-model task. Demonstrate a supported region-scale strategy early: outlined chunks with explicit live-in/live-out frames, state handles and control-exit tags, or staged bounded edits with defined C/Rust boundaries and compilable, tested checkpoints. Merely splitting text at `case` labels is not a semantic extraction rule. Start coarse enough not to add an ABI hop to every instruction. Broader arbitrary extraction remains stretch; solving the actual interpreter with small agents cannot be deferred until after claiming the golden result.

TinyEMU → X11 is the requested north-star acceptance test, not a guarantee of hackathon completion. G3 is the minimum honest incremental-migration demo, G4 is the faithful toy Rust checkpoint, G4m completes its modernization slice, and G4b is the first TinyEMU-native replacement; none completes G6 or G6m. If the original desktop baseline cannot be reproduced, mark the golden gates blocked while continuing bootstrap development.

## 3. Three independently buildable parts

These are three code/workstream boundaries, not a requirement for three people or a claim of identical effort. W contains the highest technical uncertainty. To make the packages more comparable, S also owns the reusable validation specification/harness: baseline provenance, layout checks, deterministic runner, capture and comparison. **That native support code runs on W's machine, never inside the database server.** U owns the substantial custom Blueprint canvas rather than a dashboard shell.

Current implementation split: this workstream takes **W, the worker client**; the teammate takes **S, the backend**, including the native harness contract. U is a separate GUI responsibility, not what “worker client” means; its assignment remains to be agreed. Both builders freeze the shared protocol and assess the TinyEMU/model risks together. This is not a claim of three contributors or equal effort; no amount of server bookkeeping makes the full translator a small task.

| Part | Owns | Does not own | Standalone deliverable |
| --- | --- | --- | --- |
| **S — Server and SpacetimeDB** | Shared protocol/bindings; task dependencies, deduplication, claims/leases/fencing; structural checks, revision/report binding and acceptance; reconnect fixtures; native validation-harness specification/library | Repository scanning, model calls, native compilation/execution inside reducers, worker provisioning, canvas rendering | Import/claim/report/accept transcript including duplicate, stale and conflicting submissions; native baseline harness invoked on worker compute |
| **W — Worker client** | Claimed repository import/decomposition, task discovery proposals, model/tool loop, graph/ABI generation, mixed linking, harness invocation, bounded modernization and export | Authoritative global scheduling/acceptance, duplicate baseline/capture/comparison machinery, GUI state | Source → C graph → mixed graph → Rust checkpoint → tested modernization using a fake or real server adapter |
| **U — GUI control-plane client** | Blueprint-like scene/hit testing, hierarchy/navigation, source/plan/evidence inspectors, subscription projection, controls, captures and reconnect UX | Repository ingestion/parsing, agent prompts, compilation, acceptance decisions | Faithful seeded scene plus real live deltas, controls, counterexample and boot-evidence inspection |

Proposed layout (not created yet):

- `crates/argo-protocol/`: shared Rust DTOs, IDs, schema version, fixture messages. Part S maintains it; both clients review changes. This is a shared boundary, not a fourth product subsystem.
- `crates/argo-server/`: Rust SpacetimeDB module and server-side transition rules.
- `crates/argo-harness/`: S-owned native support library/runner for reproducible baselines, layout assertions, test execution, traces and captures. W invokes it locally; it is not a fourth deployed subsystem.
- `crates/argo-worker/`: Rust native worker, transport/model adapters, claimed task execution, generation/linking, harness invocation, modernization and export.
- `crates/blocks/`: source-inventory package proposed by [the block-generator draft](docs/BLOCK_GENERATOR_SPEC.md). W integrates its lossless records into the shared protocol; this is not a separate authoritative job system.
- `crates/argo-ui/`: selected Rust UI toolkit, graph scene, inspectors, subscription adapter.
- `fixtures/`: small C inputs, fixed test domain, expected graph/protocol snapshots, and deliberately failing candidates.

Keep toolkit-specific types out of protocol/domain records. Freeze protocol v1 before independent implementations commit to incompatible meanings. W can begin against a fake server sink and shared transcript without waiting for a completed backend; the protocol and S-owned harness invocation/result contract are the early prerequisites.

The block-generator draft owns lossless source segmentation and edit points, not the entire executable IR, worker scheduler or modernization model. Its implementation references are not present in this checkout. Integrate its records with project/revision/configured-instance identities rather than creating an unrelated set of database tables. Lexical candidate references are useful context, not proven executable dependencies or safe replacement boundaries.

## 4. Interface between the parts

### Transport and state ownership

Both clients use the Rust SpacetimeDB SDK. Commands are reducer calls; clients subscribe to durable tables for the project. Generated SDK bindings are the concrete transport interface; `argo-protocol` defines framework-independent meanings and local fixtures. Do not invent a second authoritative REST server.

Reducers validate identity, revisions, allowed transitions, and report metadata. They do not invoke models or compilers. All such I/O and CPU work runs in W. Network callbacks reach the clients' async adapters, which deliver messages to their own event loops.

### Coordinated ingestion and fan-out

SpacetimeDB starts with schema/reducers installed and no imported project data. It does not read a repository. The initial flow is:

1. An authorized user/client registers a project/import request in S, before expensive scanning or model work. Replaying that request returns the same import job; a distinct intentional snapshot uses a distinct request. Route local-path imports to a worker that actually has repository access.
2. One W claims the import job, resolves the requested revision and captures exact source/build inputs, including relevant uncommitted bytes. A moving branch name alone is not a snapshot identity.
3. That attempt uploads bounded UTF-8 source files and seals an immutable, content-hashed bundle. Other workers may read/cache the same bundle; duplicate reading is harmless. They must not independently schedule the entire import or mutate a shared checkout.
4. W proposes discovered translation-unit/decomposition tasks through reducers. S validates their identities/dependencies and creates each logical task once. Further discovery can fan out recursively; parent explanations wait for their required children.
5. Workers claim ready tasks, execute locally, and publish version-bound results. Only S advances durable lifecycle and accepted state.

Every source writer, including a UI upload path, participates in this import ownership protocol. S retains exact source bytes, node implementations, summaries and compact evidence. Source can be reconstructed from stored bytes/segments without an undeclared original checkout; a text-segment marker alone is insufficient. Binaries/build directories remain on W; local paths are not remotely downloadable artifacts. Artifact distribution and automatic remote provisioning are separate later capabilities.

Initial proposed limits: 1 MiB/source file, 16 MiB/source bundle, 128 records/graph batch, 128 KiB/report log excerpt, and 128 KiB/inline evidence preview. Confirm these against the pinned SDK and actual fixture at G0; they are design bounds, not measured service limits. Reject oversize submissions explicitly. Subscribe to graph/status metadata first and fetch source/log details on selection, not every inspector subscribing to every full text.

### Shared records: protocol v1 sketch

| Record | Essential fields/meaning |
| --- | --- |
| `Project` / `Import` / `SourceBundle` / `SourceFile` | Owner/policy, import request and owning attempt, sealed canonical manifest, relative paths, exact source bytes/hashes |
| `SourceBlock` / `ConfiguredInstance` | Lossless source range/kind and snapshot identity; separate identity for each translation-unit/template configuration, with original/expansion provenance |
| `BuildConfig` / `TranslationUnit` | Target triple, compiler/flags/defines, include inputs, selected translation unit and macro/template instantiation identity |
| `GraphRevision` | Project/source/build identity, parent, `draft`/`frozen`, canonical frozen-content hash; accepted history is immutable |
| `Node` / `CompositeRegion` | Logical ID + revision + configured-instance identity, parent/children, source provenance, executable/explanation/type/state role, external interface/effects; a composite is not necessarily one function |
| `Pin` / `Edge` | Stable pin IDs, directions/types, exec/data/state/control-exit semantics; explicit call/callback relationships |
| `TypeLayout` / `EffectContract` | Optional on scalar fixture, required where used: size/alignment/field offsets; state/resource accesses, indirect calls and control exits |
| `Annotation` | Summary, assumptions/unknowns, source/context/model attribution; never executable truth |
| `ArchitectPlan` / `NodeDirective` | Input semantic-map identity, architect model/run, updated plan, dependency decisions, parent directive chain, per-region instructions |
| `Implementation` / `ModernizationProposal` | Region/interface/build identity, source/hash, selected children or bounded changes, objective and affected callers/resources, artifacts and evidence |
| `Run` / `Job` / `Attempt` | Stage, prerequisites, canonical logical-task key, exact inputs/base revision, mutation scope, authenticated worker, lease/fence generation, lifecycle, request/batch IDs, progress and model/tool usage |
| `ValidationReport` | Exact baseline/candidate/full-manifest identities, build/toolchain/test-domain/harness identity, outcome, cases, counterexample, bounded logs and relevant performance/quality evidence |
| `Manifest` | Versioned region → implementation/composition mapping, frozen graph/interface/build identity, accepted base version |
| `Artifact` | Hash, kind, size, owner/location and optional bounded inline preview; report/source/boot evidence references |

A source block is not a configured executable instance. TinyEMU compiles/includes the same source under different width/XLEN settings, and one shared-source edit can affect several instances. Namespace records by project and immutable snapshot/revision; bind task/report identity to the relevant build, translation unit and template instance. File paths, names, short hashes or collision ordinals alone are not persistent logical identity. Retain provenance when an edit splits, merges or replaces a region.

At G0 select the source-segmentation and configured-compilation route. A lossless non-preprocessing scanner can be the first inventory stage, but stored `#if` text or lexical identifier matching does not establish which definition the compiler uses. Validate active configurations with the actual toolchain; do not quietly implement an incomplete replacement preprocessor. RV64 targeting does not justify deleting all x32 paths or 32-bit floating-point behavior. Exclude x86/RV128 only through an explicit validated build configuration.

Node positions, viewport and selection belong to U and are excluded from semantic hashes. Language and validation status are independent dimensions.

### Commands and lifecycle

These are proposed contracts, not existing Rust functions:

- `request_import(project_id, repository_locator, requested_ref, build_request, request_id)` — authorized client → S; register the import before execution, with access constraints and replay-safe identity.
- `begin_source_bundle`, `put_source_file`, `seal_source_bundle` — owning import attempt → S; idempotent bounded uploads and canonical immutable sealing.
- `request_run(project_id, stage, input_ids, request_id)` — authorized client → S; queues only stages whose prerequisites exist.
- `propose_tasks(job_id, attempt_id, fence, batch_id, task_specs)` — W → S; idempotently submit discoveries; S derives/checks canonical logical-task keys and dependency validity.
- `claim_job(job_id)` / `renew_claim(job_id, attempt_id, fence)` — W → S; atomically acquire/renew ownership using authenticated identity and server time. Return an attempt, lease expiry and monotonic fencing generation.
- `publish_graph_batch(job_id, attempt_id, fence, batch_id, draft_revision, records)` / `freeze_graph(job_id, attempt_id, fence, draft_revision)` — W → S; publish owned draft records, check structure and compute/check the canonical hash.
- `publish_plan`, `submit_candidate`, `submit_report` — owning attempt/fence → S; persist version-bound proposals/evidence, not automatic acceptance. Candidate metadata declares affected regions/callers and any interface changes.
- `report_progress`, `finish_attempt` — owning attempt/fence → S; progress and completion with required output/report references.
- `cancel_run` / `retry_job` — owner → S; abandon/cancel safely or create a new attempt; W observes cancellation and stops its owned execution.
- `accept_manifest(project_id, expected_base_version, candidate_manifest, report_ids)` — W/owner → S; compare-and-swap acceptance under the checks below, not self-certified correctness.

Lifecycle: `queued → running → succeeded | failed | abandoned`. Retry/reclaim creates a new attempt and fencing generation. Lease expiry allows recovery; an old worker may still be computing, but its stale writes cannot modify current draft/accepted state. Historical outputs can remain inspectable through an explicitly non-authoritative path. Cancellation is checked again when publishing and accepting, not only when claiming.

Distinguish request replay IDs, logical-task deduplication keys and attempt IDs. A logical task is identified by stage plus its exact relevant source/build/region/context inputs and model/prompt/tool versions where applicable; an intentional alternative candidate is explicit, not an accidental duplicate. Same-key replay is idempotent; conflicting same-key payloads are rejected. Parent tasks become runnable only when their declared inputs exist.

For concurrent mutation, S arbitrates overlapping region/interface scopes against the base revision; W uses an isolated per-attempt workspace. Broaden a claim before changing additional callers or shared contracts. Disjoint edits can still interact through shared state, so scope arbitration does not replace integrated validation. Specify these semantics in v1 and prove competing claims, expiry/reclaim and stale-publication rejection before enabling multiple writers. Server-side lease/fencing behavior is not deferred to a later redesign.

The guarantee is **one authoritative accepted result per logical task/revision transition**, not exactly-once computation. Failures and network partitions can cause duplicate execution. Never rely on a worker-generated ID, a shared filesystem, or cooperative model behavior to prevent conflicting acceptance.

**Freeze:** S validates pin existence/types, containment acyclicity, supported control structure, call/interface links and required implementation references. Supported loop cycles are not mistaken for invalid containment cycles. Hash stable sorted semantic records and exact source hashes, excluding layout/progress; S is responsible for canonicalization, not blindly accepting W's claimed digest. Structural validity is not behavioral correctness.

**Acceptance:** S binds required reports to the exact candidate source/build, frozen graph/composition, full selected manifest, unchanged accepted base, established baseline, toolchain/flags and declared test domain. A body/interface/build/fixture/harness change requires evidence for the new identity. Old evidence remains valid only for its old artifact. Final C-free export and modernization candidates have their own evidence requirements. The team trusts its local tool runner; hashes do not prove execution.

A worker publishes a candidate, never edits accepted code in place. Acceptance atomically advances the manifest and region mapping while preserving history. Parallel candidates passing against base N do not certify their combination. After another acceptance changes the base, rebase/materialize the combined candidate and run the required checks for that exact identity before acceptance; reject stale compare-and-swap requests. Ordinary source tooling handles textual integration. Do not routinely ask a small model to reconcile accumulated thousand-line patches.

### Subscription initialization and parallel handoff

U initializes from the SDK's applied subscription/cache, then processes deltas; do not query a separate snapshot and subscribe later with a missed-update gap. Reload durable state on reconnect. Render partial draft batches as pending; only a structurally frozen, report-backed selection may look accepted.

At G0 agree one source/graph/manifest example and a complete import/claim/update/accept transcript. W emits it through a fake sink; U renders it; S checks duplicate requests, competing claims, stale attempts and invalid submissions. Include a TinyEMU-shaped state handle, callback/control-exit example and a two-region modernization with a changed internal interface. G1 replays the same meanings through real SDK bindings. W also initializes/reconnects from durable job state rather than independently choosing repository work outside the protocol.

## 5. Worker pipeline and executable graph

### 5.1 Understand bottom-up

After claiming import/decomposition work, inventory meaningful source regions with exact spans and configured-instance provenance. Declaration-level functions/types/macros/globals are a valid starting point; AST nesting can add explanation regions. A macro is a compile-time dependency/generator, a struct/enum is a type node, and a global is shared state—not every box is callable. Preserve preprocessing provenance rather than treating expanded results as the original source.

Explain leaves with bounded source/context, then parents using child explanations plus retained source. Record interfaces, referenced definitions, state/effects, control exits and unresolved questions. Publish completed batches immediately. Source remains immutable; substituting summaries in a model's view is not deleting executable content. Lexical references and explanations are attributed hints, not proof of independence or purity.

The first worker executes sequentially against the same durable task protocol that later workers share. Avoid token-level model calls and duplicate global import loops; parallelism comes from claimed task fan-out, not each agent independently decomposing the entire repository.

### 5.2 Executable IR v0 and same-language validation

Keep three notions distinct: source/explanation regions, executable behavior, and replaceable/composite regions. The containment hierarchy is not an execution schedule; control edges, calls, data dependencies and state access are separate relations. Initial callable units are functions with generated C-compatible boundaries. Branches/loops remain in retained bodies and may be explanation-only. Coupled functions can form a larger replacement region when shared ownership or callbacks prevent independent changes.

Do not require a novel pure-node compiler before the first faithful port. Conversely, AST slicing does not make arbitrary blocks self-contained: intra-function extraction needs live-in/live-out, alias/state and control-exit rules, demonstrated for the supported TinyEMU strategy. A region can be faithful unsafe Rust before it is idiomatic; record that status rather than advertising safety. The source graph is provenance, not an immutable template for the final Rust architecture.

The graph is authoritative for supported call-site bindings and selected implementations: generated wrappers/link bindings must invoke the graph-declared target, not leave original symbols secretly in charge. Compile source bodies retained from the original, not reconstructed from summaries. Show indirect calls as callback pins/dynamic-call nodes with known targets or an explicit unresolved marker.

**Evaluation:** materialize arguments/results once per declared call; sharing a data pin does not implicitly execute a side effect again. Retain sequencing, return destinations, loop backedges and exit behavior. The fixture avoids unspecified C argument evaluation and undefined behavior; later imports must declare compiler/configuration and defined-behavior assumptions rather than invent universal C semantics. Numeric operations preserve source widths, truncation/wrapping, shifts and exceptional cases explicitly. Emulator SoftFP behavior includes guest rounding and exception flags, not a substitution with host floating point.

**Minimal typed effects:** `pure`; `state_read/write(resource, fields)`; `memory_read/write(parameter)`; `indirect_call(callback)`; `host_io(resource)`; control exits/nonreturn; and `unknown`. Effects identify the state to snapshot and the dependencies to display; they are not a theorem that all behavior has been captured. The fixture permits pure computation only, with explicit call/return. Unknown/unsupported effects prevent a local node from being advertised as validated; expand the envelope for the golden track.

**Worked fixture contract (proposed, not implemented):** `compute(n)` starts `acc = 0`, loops `i = 0..n`, and calls `step(acc, i)`; `step` calls `normalize(acc + i)`; `normalize(x)` returns `x & 255`. Use unsigned 32-bit values and a declared test domain `n = 0..32`. The hierarchy shows the loop inside `compute`, while executable calls bind `compute → step → normalize`. A manifest choosing C `compute`, Rust `step`, and C `normalize` exercises an actual C → Rust → C path. Compare original C, generated all-C graph, mixed manifest and final Rust export on the same cases, plus a deliberately incorrect `step` candidate.

### 5.3 Architect and translate top-down

An architect returns a versioned `ArchitectPlan`: grouping intentions, dependencies, interface decisions, assumptions and bounded directives. Walk the planned hierarchy top-down; each child receives compact parent guidance, its directive, allowed dependencies, relevant source and evidence. Persist this lineage. The architect is a role, not necessarily a separate process or an always-required giant model.

Start with faithful boundaries and permissive implementation structure; separately schedule modernization after a validated Rust checkpoint. Ultra 550B remains an optional original architect candidate. Verify actual access, hardware/context budget and privacy policy before selecting any endpoint. Lightning/Nano worker capability is likewise unproven until measured. Private operation may not export source-derived context to an external architect; public-demo endpoints must be disclosed and cannot establish an in-house capability claim.

**Bounded tasks:** supply a concrete objective, invariants, relevant interfaces/callers and compiler/test feedback. Let agents retrieve additional source with tools; summaries do not replace source needed for correctness. Budget affected semantic scope as well as context/diff size. A large parent may require many small jobs, not a larger rewrite prompt. If a task cannot be safely decomposed or handled by the selected model, record the blocker, stage compatibility steps or seek an explicit allowed model/strategy change. Do not silently outsource it.

**Token discipline:** cache summaries by source + resolved-context + prompt/model identity; batch small meaningful regions without losing IDs; reuse only genuinely equivalent template summaries. Record actual models, tokens, retries, tool/test time and human interventions. Include validation and orchestration overhead in cost/latency comparisons. Do not regenerate a validated child solely because its parent explanation changed.

### 5.4 Mixed-language incremental replacement

A region has a logical behavioral contract; generated C ABI adapters serve the actual mixed-language boundary, not the permanent identity of every node. These are **same-target** contracts, not portable wire formats. Pin target triple, scalar widths, layout/alignment, ownership/allocation/freeing, callback lifetimes and panic/unwind policy. Use `extern "C"`, checked representations and explicit scalar/buffer/handle/control-exit types where crossing C. Never assume `repr(C)` makes Rust containers, references or trait objects C-compatible.

For the golden shared-state ABI, generate mirrors/accessors from the same configured headers and check size/alignment/field offsets on both sides. Initially retain C-owned state identity; Rust accesses raw pointers/accessors with explicit lifetimes. Do not hold exclusive Rust references across callbacks that can re-enter and mutate that state. This phase may require unsafe Rust; it is not advertised as a safe-Rust proof. Reject unsupported layouts/callbacks. Prevent panics unwinding across FFI; use an explicit policy such as abort or caught-unwind-to-status for supported entry points.

Keep both implementations available, but select implementations **at link time from the manifest**, rebuilding between runs. Do not put a generic runtime dispatcher into each guest-instruction hot path by default; benchmark granularity against the measured baseline budget. C/Rust crossings use generated bindings; native Rust-to-Rust calls can replace adapters once contracts permit. Remaining C callers or C-owned callback tables may still require exported shims. Removing `extern "C"` alone does not recover ownership or remove hidden shared state. No hot unloading is needed.

Before acceptance, run local cases and the actual mixed program containing that candidate and selected neighbors. Reports identify the full build and manifest. A local pass does not certify every combination; milestone desktop boots complement bounded instruction/device checks. ABI compatibility alone proves neither behavior nor ownership isolation.

The fixture has no irreversible effects/shared aliased heap. Golden nodes require controlled state/event inputs and the explicit effect contracts above. Never run two real external writes as a shadow comparison. Candidate tests execute without the supervisor's database/provider credentials and with bounded paths, time and resources.

### 5.5 Reassemble and export

Once the in-scope migration units are Rust, emit an ordinary Rust project and establish a reproducible working checkpoint. This may initially retain C-shaped types, structure and justified unsafe code. Remove original C linkage and unnecessary adapters where supported; disclose remaining compatibility scaffolding as modernization work. Legitimate OS/external-library FFI is not an undisclosed original-emulator implementation.

Treat evaluation order, errors and destruction timing as semantics; do not concatenate snippets blindly. Rerun the declared tests and clean source/link inventory. C-free export is a separate checked stage, not implied by green node badges, and not proof that modernization is complete.

### 5.6 Recursive merging and Rust modernization

After a validated Rust checkpoint, hill-climb it through accepted, behavior-preserving improvements. Early Rust regions can also be composed without demanding immediate idiomatic redesign. The rule is: **inside an owned region, interfaces may evolve; outside it, preserve the declared behavioral contract unless the migration explicitly includes the affected callers.**

1. Select a connected, sensible group of accepted Rust regions and claim its mutation scope at an exact base manifest. Call cycles, shared state, callbacks and ownership can require a cluster; adjacency on the canvas is insufficient. Containment is hierarchical but dependencies need not form a tree.
2. Distinguish structural aggregation from code cleanup. A composite can contain accepted children without flattening their source. The common accepted workspace already contains those implementations; do not hand a model two accumulated patches and ask it to merge them.
3. Propose one bounded improvement: remove an internal shim, introduce a native interface, consolidate a type, clarify ownership, replace a dependency or restructure a small behavior. Preserve adapters still needed at the region's outer boundary. Use staged compatibility changes if one safe edit cannot span the necessary callers.
4. Materialize an isolated candidate, run the required regression gate and inspect the intended quality improvement. Reject/revert failures; preserve the last accepted checkpoint. Passing tests are necessary, but merely relocating unsafe code or reducing line counts is not modernization evidence.
5. Atomically accept the tested full manifest and new composition/provenance mapping. Keep original source, child regions, prior implementations and reports inspectable. If the accepted base moved, integrate and retest before acceptance.
6. Repeat over larger regions through bounded jobs. Larger ownership scope is not a requirement for larger context or diffs. A root composite represents the whole program as normal modules; it is neither one giant function nor proof that every possible refactor is finished.

Source and target need not remain one-to-one. A missing or incompatible Rust library requires implementing the needed behavior, adapting a demonstrated alternative, or explicitly negotiating a scope/behavior change. A similarly named crate is not evidence of equivalence. Native Rust types are introduced only with their semantic requirements accounted for; for example, replacing arbitrary bytes with `String` introduces a UTF-8 constraint.

The swarm can port/refactor disjoint regions and run independent checks, but parallelism naturally narrows near shared interfaces and the root. Serialize conflicting edits and use normal source integration plus tests, not model-based conflict resolution as the default. Some global changes may remain blocked by model capability; hierarchical bookkeeping does not guarantee they are locally solvable.

### 5.7 Continuous correctness and capability evidence

A fixed regression suite is the hill-climber's backbone. Once a Rust workspace exists, `cargo test` should exercise its relevant unit/integration paths and invoke the harness through an agreed entrypoint. No such command is implemented here yet. Candidate agents may not silently weaken tests, golden outputs, supported domains or performance budgets to make a patch pass; changes to that contract require separately reviewed, versioned evidence.

Keep evidence levels distinct: lossless source reconstruction; original-language identity/extraction checks; actual C → Rust → C execution; C-free Rust behavior; and modernized full-program behavior. A C wrapper calling renamed C is not Rust-ABI validation and is not universally identity-preserving when function identity is observable. Unsupported heads, variants and skipped tests must be counted, not silently treated as covered.

For TinyEMU, a controlled desktop workload is the primary end-to-end modernization oracle: pinned guest/configuration, repeatable time/events, scripted interaction, and meaningful state/device/log checkpoints plus visual evidence. It exercises many subsystems together; a final screenshot alone can hide divergence. Near-complete coverage and repeatability must be measured, not assumed. Use coverage findings and focused instruction/exception/device tests to fill gaps and localize failures. G0 establishes quick-feedback and full-acceptance budgets; record which exact suite each accepted artifact passed.

Compare original C, faithful Rust and modernized Rust on the same declared workload. To substantiate the product thesis, record the small model and serving hardware, context/concurrency limits, success rate, retries, test time, human/strong-model intervention and total cost. A useful experiment compares a small model working directly with that same model using Argo; a strong-model port is an optional baseline. Fewer parameters do not alone prove lower total cost or an in-house deployment.

## 6. GUI: a UE5 Blueprints-like control plane

The visual target is specifically UE5 Blueprints, not a generic force-directed code graph or a collection of dashboard cards. Use original application assets while matching the recognizable visual/interaction grammar.

### Required visual and interaction surface

- Large charcoal, pannable/zoomable grid canvas with a clear world-to-screen transform.
- Compact rounded rectangular nodes with colored title bars, labeled pins on opposing sides, and curved Bézier wires.
- Visually distinct execution and typed-data connections; consistent pin colors across nodes and legend.
- Nested graph navigation: double-click/expand a region, breadcrumb back out, and select a node in a hierarchy panel.
- Drag nodes, select/focus, frame the graph, inspect source and explanations; pointer hit testing must follow zoom/pan.
- Inspector showing original C/provenance, candidate Rust and diffs, architect guidance, current implementation, composite children and actual validation evidence.
- Toolbar for import, explain/build, migrate/check, bounded modernization and export. Show which actions are unavailable and why; requesting a job is not accepting its result.
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
| **G0: contract and baselines** | Agree source/configured-instance identities, fixture/ABI/domain, import/claim/reclaim/modernize transcript and harness interface; investigate/pin original RV64 X11 baseline; test selected private-model endpoint/tool access | S owns protocol + harness; W owns importer/model and bounded-interpreter feasibility; U owns toolkit launch |
| **G1: visible shared state** | Claimed import/decomposition produces live explanations through real SDK bindings; reconnect restores state | S/W/U first vertical slice; inventory is not a migration claim |
| **G1s: swarm ownership** | Two workers compete safely; duplicate task creation, lease expiry/reclaim, stale publication, overlapping edits and combined-manifest acceptance are tested | Can use two processes on one host; required before concurrent mutation, not after G6 |
| **G2: same-language baseline** | Original C and graph-organized C match on the fixture domain; expose exact report and skipped/unsupported cases | W invokes S's runner; S enforces evidence prerequisites |
| **G3: one Rust replacement** | Selected model produces Rust in real C → Rust → C execution; local/mixed cases pass and a bad candidate is rejected | Minimum honest migration demo; record actual model/interventions |
| **G4: fixture Rust checkpoint** | All toy units are Rust; clean C-free project matches the fixed tests | Faithful port complete, not yet the full first slice or golden acceptance |
| **G4m: recursive fixture modernization** | Compose accepted Rust children, remove an unnecessary internal ABI boundary through bounded changes, preserve external behavior and provenance; stale/failed refactors cannot advance state | Completes the first slice; demonstrate separate jobs rather than one large parent rewrite |
| **G4b: TinyEMU leaf replacement** | A bounded helper is Rust; focused comparisons and the selected mixed emulator's pinned desktop workload pass | First TinyEMU-native result; preserve SoftFP flags where applicable |
| **G5a: shared-state/devices** | Layout-checked state/callback ABI; required device/storage/input paths migrate with bounded traces and desktop checks | W translation plus S harness; U shows state/callback/evidence |
| **G5b: bounded interpreter migration** | Supported chunks/staged edits preserve shared frame state and exits with usable budgets; demonstrate work within the selected small model's limits | Whole-function one-shot output from a stronger model does not establish this capability |
| **G6: golden Rust checkpoint** | In-scope TinyEMU closure is Rust; clean source/link inventory and pinned interactive RV64 X11 acceptance pass | No console/x86/unchanged-C-wrapper substitution; mechanical Rust is a valid checkpoint |
| **G6m: golden modernization** | Accept bounded native-Rust improvements/compositions above that checkpoint while the full declared workload stays green; record quality, performance and model/cost evidence | Report specific improvements, not that a root node proves all code idiomatic |
| **G7: stretch expansion** | Automatic remote provisioning/artifact distribution, broader languages, arbitrary extraction and unrestricted architectural redesign | Version contracts explicitly; core swarm ownership and bounded modernization are already above |

Proposed G0 spike cutoffs (budgets, not observed runtimes): W gets 20 minutes for the selected model/tool check; U gets 45 minutes for its first toolkit scene/launch; S with W gets 60 minutes to expose native TinyEMU/guest blockers. Establish a separate bounded-interpreter experiment rather than claiming that model access alone proves feasibility. At each cutoff record the blocker and next action. Measure boot, comparison and interaction budgets only after the baseline runs.

Bootstrap G1–G4m and local swarm checks may proceed while golden readiness is blocked. G4b–G6m require the pinned desktop baseline; G5b's small-model experiment should be investigated early rather than waiting for every device port. These gates are dependency checks, not a requirement to serialize all work by table order.

Model identity/access, private hosting and context budgets remain explicit choices. An unavailable optional architect does not block protocol/fixture work or justify silently sending private source elsewhere. If golden migration or modernization is incomplete at judging, show the highest completed gates and label the remainder unfinished. Scope cuts change reportable completion, not what RV64 X11 or private small-model execution means.

## 8. Invariants, failure handling, and evidence

- Original source, configured-instance provenance, prior graph/composition revisions and accepted checkpoints remain recoverable. Explanation structure is not executable truth or the permanent target architecture.
- Model output is an untrusted candidate. Only matching reports for the actual full candidate can advance accepted state; unchanged tests and explicit quality objectives govern modernization.
- Ownership starts before import. Only current authenticated attempts/fences may mutate their scoped drafts; stale attempts may retain historical outputs but cannot advance state. Exactly-once computation is not promised.
- Private mode keeps source and derived context/evidence within approved database, worker, model and telemetry boundaries. Public-demo inference is disclosed separately and does not prove in-house deployment or lower total cost.
- UI controls and worker claims do not bypass server-side revision/identity checks. MVP may trust the team's worker, but does not claim to verify its CPU execution cryptographically.
- Failed builds, mismatches, unsupported constructs, tool timeouts, and unavailable models are distinct states with inspectable evidence.
- The credentialed worker supervisor invokes S's native harness with restricted child environments/paths and time/resource limits; generated/legacy programs do not inherit database/model credentials. A container alone is not this separation. No production sandbox or untrusted-remote-worker guarantee is established.
- SpacetimeDB holds orchestration and program state, not native FFI execution. Native linking and execution happen in W; they cannot be moved into a database reducer by adding an ABI attribute.
- Reproducible command lines, target/configuration, defined-behavior assumptions and toolchain versions accompany reports. Sanitizer/test passes do not prove C definedness. A port matching TinyEMU is not independent proof of conformance to every current RISC-V specification.
- Final export evidence includes a clean build and source/link-input inventory, not just `.rs` file counts or the executable name. No runtime, performance, security, or visual-fidelity claims are established by this documentation-only commit.

Implementation verification should include a failing candidate before repair; duplicate import/task replay; competing claims, reclaim/fencing and cancellation; reconnect; a real C → Rust → C path; same-language, Rust-checkpoint and modernization comparisons; and a failed or stale refactor that cannot advance the manifest. Test independently passing parallel candidates again as the actual combined artifact. GUI claims require a capture at a named viewport/revision. Do not invent existing commands or declare model, runtime, cost, privacy or visual acceptance from this documentation alone.

## 9. Source references and open decisions

Primary references checked during drafting; repository links below are pinned documentation snapshots, not dependency versions selected for Argo:

- [SpacetimeDB reducers](https://spacetimedb.com/docs/functions/reducers/), [procedures](https://spacetimedb.com/docs/functions/procedures/), and [subscriptions](https://spacetimedb.com/docs/clients/subscriptions/): transaction/I/O and applied-cache boundaries. W owns external execution in this design.
- [Rust FFI guidance](https://doc.rust-lang.org/nomicon/ffi.html): calling conventions, representation, ownership, and unwinding.
- [Epic Blueprint functions](https://dev.epicgames.com/documentation/en-us/unreal-engine/functions-in-unreal-engine): execution versus data-oriented function semantics; Argo must define its own evaluation rules.
- [iced README](https://github.com/iced-rs/iced/blob/8ff48c42b0d5188223501ea1ccc506fd5beaf36a/README.md#L27-L45) and [interactive Bézier example](https://github.com/iced-rs/iced/blob/8ff48c42b0d5188223501ea1ccc506fd5beaf36a/examples/bezier_tool/src/main.rs).
- [GPUI README](https://github.com/zed-industries/zed/blob/b5cd310b3b54dfc659fcc06453e6358843dfe216/crates/gpui/README.md#L1-L15) and [custom Elements](https://github.com/zed-industries/zed/blob/b5cd310b3b54dfc659fcc06453e6358843dfe216/crates/gpui/README.md#L75-L99).
- [Rune vector rendering/input](https://github.com/anomalyco/rune/blob/99c123d6b2127d1226c72dec37919e51019142ff/crates/rune/README.md#L56-L145).
- [Nemotron Ultra model card](https://huggingface.co/nvidia/NVIDIA-Nemotron-3-Ultra-550B-A55B-BF16): original optional architect candidate, not a selected hosting configuration. Lightning/Nano worker variants, local serving requirements and endpoint access still need to be pinned and tested; no capability/cost claim is verified here.
- [TinyEMU upstream](https://bellard.org/tinyemu/), [technical README](https://bellard.org/tinyemu/readme.txt), and [JSLinux guest list](https://bellard.org/jslinux/): RV64, VirtIO devices, SDL graphics, MIT licensing, and available RISC-V X Window configurations. A listed web guest is evidence of availability, not proof of this project's native baseline boot.

Open at implementation start: U ownership, toolkit/revision and demo OS; integration of the block-generator draft with source/configured-instance identities; canonical task keys, claim scopes, lease/renewal parameters and conflict behavior; concrete SDK/schema/transcript and native harness interface; supported boundary/interpreter extraction or staged-edit rules; ABI/layout v1; private model variants/serving hardware and context/tool budgets; fixed regression domains, acceptance cadence and modernization objectives; TinyEMU source closure, guest/configuration hashes, offline storage/network/display choices and measured golden budget. W and S can start independently against the agreed transcript; full backend completion is not a prerequisite. G0 resolves or records blockers rather than silently broadening or shrinking acceptance.
