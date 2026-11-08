# Argo architecture

> **Design, not an implementation report.** Drafted and revised on 2026-11-07. This revision incorporates the worker/swarm, private-model, and recursive-modernization discussion, defines the template-based block creation and merge pipeline (section 5), and absorbs the former block-generator specification as [Appendix A](#appendix-a-source-inventory-block-generator-specification). This checkout contains documentation, not an executable workspace. The crates, schemas, commands, screenshots, and runtime behavior below are proposed. This document owns the architecture and initial acceptance milestones; it does not claim that any migration has run.

## 1. Product and design basis

Argo progressively migrates a legacy program to Rust through a live, UE5-Blueprints-like node graph. First establish a behavior-preserving port, allowing temporary C-shaped interfaces and justified unsafe Rust. Then hill-climb that working Rust implementation toward native interfaces, clearer ownership, and maintainable structure through bounded, continuously tested refactorings. A functioning mixture of original C and generated Rust remains the first migration demonstration—not a translation revealed only after the entire project finishes.

**Product thesis:** make migration and modernization practical with small, privately hosted agents, rather than requiring a frontier model with the entire program in context. Nemotron Lightning and potentially Nano are intended worker candidates; their capability, hardware footprint, throughput and total cost are hypotheses to measure, not established results. A strong agent completing a direct port is a useful comparison baseline, not proof that private small-model orchestration is unnecessary. The system must demonstrate an advantage under the customer's deployment and confidentiality constraints.

**Golden project:** translate TinyEMU's C implementation needed for RV64 emulation into Rust, far enough to boot a RISC-V Linux **X11 desktop**. A console boot, an x86 guest, or Rust wrapping an unchanged C emulator does not meet that target. Small programs remain the first integration gate; the TinyEMU desktop is the requested end-to-end destination, not a silently optional stretch goal.

The product follows these user-originated decisions:

1. Create blocks by parsing the C AST and matching constructs against templates (functions, structs, enums, macros, `if`/`else` chains, loops, `switch` and others), not by token-level nodes. Each template turns its AST node into a piece of the graph. Boundary nodes (functions, types) take their pins from their signatures. Statement regions take pins from live-in/live-out and control-exit analysis. The result is a hierarchy that starts at a function/type view and opens into node groups. Cheap agents explain leaves and parents while retaining original source; publish the map as work completes (section 5).
2. Use architect guidance for grouping, dependencies and bounded refactors. Ultra 550B was the original larger-model option, not a mandatory external service or a claim of verified access. In private mode every participating model must run within the approved environment; do not conceal a frontier-model rescue behind a small-worker demonstration.
3. Represent source provenance, executable relationships and replacement/composite regions as distinct graph views. Declaration boundaries do not imply purity, independent ownership, or a permanent target architecture. A callable region declares its interface, control behavior and relevant effects.
4. First validate the graph-organized original-language program. Port implementations incrementally through temporary C ABI bridges, allowing faithful structure and justified unsafe Rust. Establish a validated Rust checkpoint before demanding idiomatic redesign; ABI compatibility is not behavioral correctness.
5. Translate bottom-up, one node at a time, then merge upward. Workers claim C nodes from SpacetimeDB, leaves first, and test each Rust node inside the otherwise-C program before submitting it. On each acceptance, SpacetimeDB detects neighbors that are now both Rust and parents whose children are all Rust. It dispatches narrowly scoped jobs that strip the C interop between them and collapse finished groups into one node. This recurses until a single Rust program node remains. Further modernization then improves the result through bounded, tested changes. The final root represents a program of ordinary Rust modules, not one enormous function or model prompt.
6. SpacetimeDB coordinates work **before ingestion begins** and throughout decomposition, translation, validation and modernization. Workers read repositories and execute tools/models; the server owns durable task dependencies, claims, revision checks and accepted state.
7. Start with one worker and a small fixture, but freeze swarm-safe ownership and revision semantics early. Demonstrate competing claims/recovery before enabling concurrent mutation. Private small-model swarms and bounded modernization are core design goals; automatic cluster provisioning and unrestricted whole-program redesign are not prerequisites.
8. Build the application frontend and backend in Rust. Legacy inputs and generated C shims are migration artifacts. Source, source-derived summaries/prompts, traces and diagnostic logs stay within the approved trust boundary; privacy applies to the database, harness, model endpoints and telemetry too.

Supporting mechanisms below are engineering proposals, not additional user requirements. In particular, job leases, report formats, and scope restrictions are means to ship the above design.

## 2. Hackathon scope

### Required first end-to-end slice

- One local SpacetimeDB instance, one Rust worker process, one Rust GUI client; one migration job/model interaction at a time. The worker may run in one prebuilt container. No cluster or VM provisioning service.
- A deliberately small, documented C input subset: a few functions, fixed-width integer arguments/results, bounded arithmetic, calls, conditionals, and a loop. Start with a supplied deterministic fixture rather than arbitrary repositories.
- Template-derived function/region hierarchy (section 5.3) with bottom-up explanations appearing live. Function nodes and at least one outlined region node (the fixture's loop body) are independently replaceable. Nodes below the work-unit floor stay visible for navigation and explanation without becoming separate ABI nodes.
- A generated same-language graph build that matches the original on the declared fixture/test domain.
- An actual agent-generated Rust replacement for at least one node, used by a mixed C/Rust program, with local and mixed-program checks recorded before acceptance.
- A GUI closely resembling UE5 Blueprints, connected to real database updates, with source/explanation inspection and visible validation evidence.
- An all-Rust fixture checkpoint with no original C dependency, reached through server-detected cleanup and collapse jobs (section 5.10): the outlined loop body is re-inlined, internal `extern "C"` boundaries are removed, and the unchanged regression domain is rerun. Then at least one accepted bounded modernization follows. A C-free checkpoint alone does not establish idiomatic modernization.

### Explicitly not required for the first slice

Arbitrary languages/repositories; every AST construct becoming its own work unit; templates beyond catalog v1; pointer-heavy C; heap aliasing; threads; macros beyond the selected fixture; exceptions; production external effects; hot library unloading; universal equivalence proofs; automatic remote worker provisioning; multi-machine deployment; collaborative semantic graph editing; sophisticated auto-layout; arbitrary target-language backends; or unrestricted architectural rewrites. This sequencing does not defer the swarm protocol or make bounded Rust modernization a stretch goal.

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

**Bounded interpreter work is required on the golden path:** the CPU interpreter is generated from a roughly 57 KB template function with extensive shared locals and `goto` exits. Declaration-level splitting alone does not make this a small-model task. Demonstrate a supported region-scale strategy early. The primary route is template-derived regions (`switch` case groups, loops, branches) outlined with explicit live-in/live-out frames, state handles and control-exit tags (sections 5.3–5.6). The fallback is staged bounded edits with defined C/Rust boundaries and compilable, tested checkpoints. Its `goto` exits are opaque in template catalog v1, so the interpreter needs either a `goto`-aware template or coarse opaque regions. Merely splitting text at `case` labels is not a semantic extraction rule. Start coarse enough not to add an ABI hop to every instruction. Broader arbitrary extraction remains stretch; solving the actual interpreter with small agents cannot be deferred until after claiming the golden result.

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

- `crates/argo-protocol/`: shared Rust DTOs, IDs, schema version, fixture messages. Part S maintains it; both clients review changes. This is a shared boundary, not a fourth product subsystem. **Created:** graph structure (nodes, pins, edges, layouts, provenance) kept apart from live status (claims, candidates, reports), freeze validation, canonical hash, a level view for the GUI, and the §5.6 fixture with a live transcript. Its crate docs own the pin-key conventions.
- `crates/argo-server/`: Rust SpacetimeDB module and server-side transition rules.
- `crates/argo-harness/`: S-owned native support library/runner for reproducible baselines, layout assertions, test execution, traces and captures. W invokes it locally; it is not a fourth deployed subsystem.
- `crates/argo-worker/`: Rust native worker, transport/model adapters, claimed task execution, generation/linking, harness invocation, modernization and export.
- `crates/blocks/`: Clang-free lossless source inventory specified in [Appendix A](#appendix-a-source-inventory-block-generator-specification). W integrates its records into the shared protocol; this is not a separate authoritative job system.
- `crates/argo-graphgen/`: W-owned configured AST, `Template` registry, interface derivation and outlining (sections 5.2–5.6).
- `crates/argo-ui/`: selected Rust UI toolkit, graph scene, inspectors, subscription adapter.
- `fixtures/`: small C inputs, fixed test domain, expected graph/protocol snapshots, and deliberately failing candidates.

Keep toolkit-specific types out of protocol/domain records. Freeze protocol v1 before independent implementations commit to incompatible meanings. W can begin against a fake server sink and shared transcript without waiting for a completed backend; the protocol and S-owned harness invocation/result contract are the early prerequisites.

Appendix A owns lossless source segmentation and edit points, not the executable graph, worker scheduler or modernization model. Its implementation references are not present in this checkout. Its records integrate with project/revision/configured-instance identities rather than forming an unrelated set of database tables. Lexical candidate references are useful context, not proven executable dependencies or safe replacement boundaries. Templates and the configured AST (section 5) decide nodes and pins.

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
| `Node` / `CompositeRegion` | Logical ID + revision + configured-instance identity, parent/children, source provenance, producing template ID and node kind, work-unit flag, executable/explanation/type/state role, external interface/effects, migration state (C / Rust candidate / Rust accepted / collapsed); a composite is not necessarily one function |
| `Pin` / `Edge` | Stable pin IDs, directions/types, input/output/in-out, exec/data/state/memory/control-exit semantics, signature- or analysis-derived origin; explicit call/callback/type-use relationships and whether an edge still crosses a C ABI |
| `TypeLayout` / `EffectContract` | Optional on scalar fixture, required where used: size/alignment/field offsets; state/resource accesses, indirect calls and control exits |
| `Annotation` | Summary, assumptions/unknowns, source/context/model attribution; never executable truth |
| `ArchitectPlan` / `NodeDirective` | Input semantic-map identity, architect model/run, updated plan, dependency decisions, parent directive chain, per-region instructions |
| `Implementation` / `ModernizationProposal` | Region/interface/build identity, source/hash, selected children or bounded changes, objective and affected callers/resources, artifacts and evidence |
| `Run` / `Job` / `Attempt` | Stage (import, explain, translate, edge cleanup, collapse, modernize, export …), prerequisites, canonical logical-task key, exact inputs/base revision, mutation scope, authenticated worker, lease/fence generation, lifecycle, request/batch IDs, progress and model/tool usage |
| `ValidationReport` | Exact baseline/candidate/full-manifest identities, build/toolchain/test-domain/harness identity, outcome, cases, counterexample, bounded logs and relevant performance/quality evidence |
| `Manifest` | Versioned region → implementation/composition mapping, frozen graph/interface/build identity, accepted base version |
| `Artifact` | Hash, kind, size, owner/location and optional bounded inline preview; report/source/boot evidence references |

A source block is not a configured executable instance. TinyEMU compiles/includes the same source under different width/XLEN settings, and one shared-source edit can affect several instances. Namespace records by project and immutable snapshot/revision; bind task/report identity to the relevant build, translation unit and template instance. File paths, names, short hashes or collision ordinals alone are not persistent logical identity. Retain provenance when an edit splits, merges or replaces a region.

At G0 confirm the configured-compilation route (section 5.2 recommends Clang's AST over Appendix A's lossless inventory). The lossless non-preprocessing scanner is the first inventory stage, but stored `#if` text or lexical identifier matching does not establish which definition the compiler uses. Validate active configurations with the actual toolchain; do not quietly implement an incomplete replacement preprocessor. RV64 targeting does not justify deleting all x32 paths or 32-bit floating-point behavior. Exclude x86/RV128 only through an explicit validated build configuration.

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
- `accept_manifest(project_id, expected_base_version, candidate_manifest, report_ids)` — W/owner → S; compare-and-swap acceptance under the checks below, not self-certified correctness. In the same transaction, S marks newly ready translation jobs (section 5.8) and enqueues edge-cleanup and collapse jobs (section 5.10).

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

## 5. Block creation and migration pipeline

This section owns how C source becomes graph nodes, and how those nodes move from C to Rust and merge upward until one Rust program remains. [Appendix A](#appendix-a-source-inventory-block-generator-specification) owns the lossless source inventory underneath it. Every crate, type, job and command named here is proposed, not implemented.

### 5.1 Pipeline overview

| Step | Stage | Runs in | Output |
| --- | --- | --- | --- |
| 0 | Source inventory | W (`crates/blocks`) | Lossless files, segments, top-level blocks, template instantiations and edit points (Appendix A) |
| 1 | Parse | W (`crates/argo-graphgen`) | Configured C AST per translation unit, mapped back to block ranges (5.2) |
| 2 | Template matching | W | One node per recognized construct, recursively; anything unrecognized becomes an opaque node (5.3) |
| 3 | Interface derivation | W | Pins: from the signature for boundary nodes, from live-in/live-out/exit analysis for region nodes (5.4) |
| 4 | Hierarchical graph | W publishes, S freezes | Graph revision with a containment hierarchy plus call, data, type, state and control-exit edges (5.5) |
| 5 | Bottom-up assignment | S schedules, W claims | Translation jobs for ready nodes, leaves first (5.8) |
| 6 | Translate and validate | W | A Rust node compiled into the otherwise-C program, a report, and a submission (5.8, 5.9) |
| 7 | Merge | S detects, W executes | Cleanup and collapse jobs that remove C interop between Rust neighbors and fold finished groups (5.10) |
| 8 | Root | — | One Rust program node; modernization continues from there (5.11) |

Steps 0–4 build the graph once per source snapshot. Steps 5–7 repeat until the root node is Rust. Bottom-up explanations and architect guidance (5.7) annotate nodes and jobs along the way; they never create, split or delete nodes.

### 5.2 Source inventory and parsing (steps 0–1)

Two layers with separate jobs:

- **Lossless inventory** (`crates/blocks`, Appendix A): a Clang-free lexical scanner over the unpreprocessed text. It owns exact bytes, top-level blocks, template and fragment file roles, instantiations, function-head edit points and byte-exact rebuild. Every source edit Argo makes, including outlining (5.6), lands as a segment replacement here.
- **Configured AST** (`crates/argo-graphgen`): parses each configured translation unit (one `.c` file plus its defines from the manifest in A.5) into a statement-level AST with resolved declarations and types. Every AST node maps to a block and a range within it. Code produced by a macro expansion maps to its macro call site. Code inside a template or fragment file maps to that file plus the instantiation that produced it.

Neither layer is enough alone. The inventory can't see inside function bodies, and it can't tell a local from a global or a macro constant. The AST can't drive edits, because preprocessing loses the exact source text.

**Parser choice (open; recommendation recorded):** use Clang's AST for the configured layer, either through libclang bindings or `clang -Xclang -ast-dump=json`, run with the same defines and include paths as the build.

- *Why:* it resolves every name to its declaration, which live-in/live-out analysis (5.4) needs. It provides types for pins. It expands TinyEMU's `glue()` names, template files and in-body fragment includes into separate subtrees per instance. It also reports spelling and expansion locations for mapping back to blocks.
- *Alternatives:* tree-sitter-c is lossless and error-tolerant, but purely syntactic. It would need a hand-written scope resolver, and it can't see the statements inside a fragment file included in a function body. A Rust C parser over preprocessed text loses source ranges through macros.
- *Cost:* Clang becomes a worker dependency. The `blocks` crate keeps its no-Clang rule and B8.
- *Falsifier:* if the G0 spike can't map Clang ranges back to block ranges for `riscv_cpu.c` with `MAX_XLEN=64` (templated functions, the in-body fragment include and `TARGET_READ_WRITE`), revisit the choice.

### 5.3 Templates (step 2)

A template recognizes one kind of C construct and turns that AST node into its piece of the graph. Templates implement one trait:

```rust
/// Proposed; not implemented.
pub trait Template: Send + Sync {
    /// Stable identifier, recorded on every node this template produces.
    fn id(&self) -> TemplateId;
    /// Structural test against one AST node.
    fn matches(&self, ast: &AstNode, cx: &MatchCx) -> bool;
    /// Build this construct's node and name the child regions to expand next.
    fn expand(&self, ast: &AstNode, cx: &mut ExpandCx) -> Result<Expansion, Unsupported>;
}

pub struct Expansion {
    pub node: Node,                 // the node for `ast`, with pins derived per 5.4
    pub children: Vec<ChildRegion>, // ordered sub-regions, each matched recursively
    pub edges: Vec<Edge>,           // relationships discovered at this level
}
```

**Driver:** walk every top-level declaration. At each AST node, try templates in registry priority order. The first match wins, and its children recurse. More specific templates come first, so an `if / else if / else` chain matches before a plain `if`. A template that returns `Unsupported` falls through to the next one. The registry always ends with an `Opaque` template that matches anything. An opaque node keeps its whole text as one leaf, gets a diagnostic, and is translated as a unit inside its parent's context. This is the template layer's version of fail-closed (B3): no template guesses a boundary.

**Catalog v1:**

| Level | Template | Node | Children |
| --- | --- | --- | --- |
| Top level | Function definition | Function (boundary) | Body statements, as regions |
| | `struct` / `union` | Type, fields as pins | — |
| | `enum` | Type, variants as pins | — |
| | `typedef` | Type alias | — |
| | Global or file-static variable | State | Non-trivial initializer |
| | Object-like or function-like macro | Macro (compile-time) | — |
| | Code-generating macro call (A.6.6) | Group | One node per generated symbol |
| | Prototype / `extern` declaration | No node; an edge target | — |
| Statement | `if`, `if/else`, `if / else if / else` | Branch region | One child per arm |
| | `for`, `while`, `do/while` | Loop region | Body; init, condition and step stay on the loop |
| | `switch` | Switch region | One child per case group; cases joined by fallthrough stay together |
| | Bare compound block `{ … }` | Scope region | Its statements |
| | Straight-line run between control statements | Sequence region | — (leaf) |
| | Fragment file included in a body (A.6.7) | Fragment region, one per instantiation | Its statements |
| | `goto`/labels, computed goto, inline asm, `setjmp`, variadic bodies | Opaque in v1 | — |

Additions to the original keyword list are switch/case, do/while, unions, typedefs, globals, code-generating macro calls, bare blocks, fragment includes and sequences. Sequences are required because every statement must belong to some node, or the hierarchy has holes. `return`, `break`, `continue` and `goto` aren't nodes. They're control exits of the region that contains them (5.4). Static locals are state, not inputs.

**Work-unit floor:** every matched construct becomes a node in the hierarchy, for viewing and explanation. Not every node becomes its own translation job. Function nodes are always work units. A region node becomes a work unit only above a size threshold (initial proposal: about 15 statements, or containing a loop). A node below the floor is translated together with its nearest work-unit ancestor. Without a floor, every small `if` arm would become a separate outlined FFI function, model call and regression run, and TinyEMU would produce tens of thousands of them. The threshold is a tunable scheduling policy, not a semantic rule. The toy fixture sets it to zero (5.6).

### 5.4 Interfaces (step 3)

Both derivations give every node separate input and output handles, Blender-style. When a node is expanded, its interior shows a group-input node on the left and a group-output node on the right.

As in UE5 Blueprints, pins come in two families that are never mixed:

- **Control (exec) pins** carry execution order. A node runs when control arrives at its exec input, then leaves through exactly one exec output. Exec wires inside a group are the C statement order. Branching and looping appear as several exec outputs, not as data.
- **Data pins** carry values, state handles and memory handles. Data wires show where a value comes from; they never decide when something runs.

Which nodes get which pins:

| Node | Control pins | Data pins |
| --- | --- | --- |
| Function, sequence, scope, fragment | One exec in; one exec out per control exit (normally just "then") | Parameters, return value, state/memory |
| Branch (`if` chain) | Exec in; inside the group, a Branch-style node with one exec out per arm (`true`/`false`, or one per `else if`) | Condition, plus each arm's live-in/live-out values |
| Loop | Exec in; inside the group, *body* and *completed* exec outs, with the body wired back to the loop | Loop-carried variables as in/out pins |
| Switch | Exec in; inside the group, one exec out per case group plus `default` | Selector value, plus each case group's live-in/live-out values |
| Type, enum, macro, global, side-effect-free expression | None (data-only, like UE5 pure nodes) | Fields, variants, parameters or value |

The exec outputs of a region are its control exits, listed below. Unlike a UE5 pure node, an Argo data-only node doesn't rerun for each wire that reads it. Values are computed once per declared evaluation (5.6). A function call whose effects are all `pure` may be drawn data-only. Any call with another effect stays on the exec chain, so its side effects keep their order.

**Boundary nodes** (functions, types, macros, globals) take their interface from the declaration:

- *Function:* exec in and exec out, one data input pin per parameter, and one data output pin for the return value. Pointer parameters are memory-handle pins. Body analysis decides whether they're read, written or both (`memory_read`/`memory_write`). Globals and static locals the body touches also become state pins, because the signature hides them.
- *Struct/union:* one pin per field, in the style of UE5 make/break nodes. Type-use edges connect it to the nodes that use it. *Enum:* one pin per variant. *Macro:* its parameters, plus use-site edges. *Global:* one state pin.

**Region nodes** (branch, loop, switch, scope, sequence, fragment) have no signature. Analysis of the region against its enclosing function computes:

- **Inputs (live-in):** each outer-scope variable the region reads whose value can come from before the region.
- **Outputs (live-out):** each outer-scope variable the region may write that is read after it. A variable that is both read and written is an in/out pin.
- **State and memory:** globals, static locals and memory reached through pointers become state or memory pins with the effect kinds of 5.6, not copied values. A local whose address escapes, for example `&x` passed to a call or stored, is a memory pin rather than a value pin.
- **Control (exec) pins:** one exec input and one exec output per distinct way out: fallthrough, `return` (with its value), `break` or `continue` to an enclosing loop or switch, and `goto` to an outer label. The parent dispatches on the exit taken.

Name resolution uses the configured AST's declaration references, never identifier text. Appendix A's lexical references remain hints. Outputs and exits matter as much as inputs. An `if` that assigns to a local, or returns from the function, isn't described by the values it reads.

### 5.5 Hierarchical graph and views (step 4)

The graph is hierarchical. The root view shows top-level items (functions, types, globals and macros) connected by call, type-use and state edges. Double-clicking a function opens its body as a subgraph, like a Blender node group or a UE5 collapsed or function graph. It shows a group-input node, a group-output node, and region nodes wired by exec and data edges between them. Each region can be opened in turn, and breadcrumbs lead back out (section 6).

Containment (parent/child) is one relation, and it forms a tree. Call, data, type-use, state and control-exit edges are separate relations, and they don't. The same hierarchy carries migration state: each node shows C, Rust candidate or accepted Rust. Merged groups (5.10) display collapsed unless expanded, exactly like a function.

Above the function level, the hierarchy is program → module → top-level items. The initial module grouping is the original source file. It's drawn as a frame in the root view and becomes a Rust module when its group collapses. An architect plan may regroup items before collapse (open decision in section 9).

A top-level item defined in a template file, such as `divu{XLEN}`, is one source node with one configured instance per instantiation (`divu32`, `divu64`, `divu128`). The graph shows the instances. Acceptance is per instance, though one candidate may cover all of them (for example a generic or `macro_rules!` implementation) when it is validated against each.

### 5.6 Executable graph and same-language validation

Keep three notions distinct: source and explanation regions, executable behavior, and replaceable or composite regions. The containment hierarchy isn't an execution schedule. Control, call, data and state relations are separate edges. The source graph is provenance, not an immutable template for the final Rust architecture.

**Outlining:** before any Rust exists, the graph must run as C. For each work-unit region node, `argo-graphgen` generates a C function `argo_<node>(inputs, output pointers, state handles) -> exit tag`. It replaces the region's text in the parent with a call, followed by a dispatch on the exit tag (fallthrough, `return`, `break`, `goto` …). Function nodes need no outlining; their call sites are rebound through generated link bindings, so the graph-declared target is always the one invoked. Edits go through the block generator's segments and edit points, so rebuilt files are exact except for the intended replacements. Bodies are compiled from retained original source, never reconstructed from summaries. Indirect calls appear as callback pins or dynamic-call nodes with known targets or an explicit unresolved marker.

Outlining generalizes Appendix A's identity swap (BT5) to region nodes. The fully outlined C program matching the original on the regression domain is gate G2's evidence. It also catches wrong interface derivations before any model sees the node: a missed live-out fails here.

**Evaluation:** materialize arguments and results once per declared call. Sharing a data pin doesn't execute a side effect again. Preserve sequencing, return destinations, loop backedges and exit behavior. The fixture avoids unspecified C argument evaluation and undefined behavior. Later imports must declare their compiler, configuration and defined-behavior assumptions rather than invent universal C semantics. Numeric operations preserve source widths, truncation/wrapping, shifts and exceptional cases explicitly. Emulator SoftFP behavior includes guest rounding and exception flags; it isn't replaced with host floating point.

**Minimal typed effects:** `pure`; `state_read/write(resource, fields)`; `memory_read/write(parameter)`; `indirect_call(callback)`; `host_io(resource)`; control exits/nonreturn; and `unknown`. Effects identify the state to snapshot and the dependencies to display. They don't prove that all behavior has been captured. The fixture permits pure computation only, with explicit call/return. An unknown or unsupported effect prevents a node from being advertised as validated. The golden track expands this envelope.

**Worked fixture contract (proposed, not implemented):** `compute(n)` starts `acc = 0`, loops `i = 0..n`, and calls `step(acc, i)`. `step` calls `normalize(acc + i)`, and `normalize(x)` returns `x & 255`. Values are unsigned 32-bit, with a declared test domain of `n = 0..32`. The fixture sets the work-unit floor to zero, so the loop body in `compute` is outlined as a region node (live-in `acc`, `i`; live-out `acc`; fallthrough exit). This exercises region translation as well as function translation. Executable calls bind `compute → step → normalize`. A manifest choosing C `compute`, Rust `step` and C `normalize` exercises an actual C → Rust → C path. Compare original C, the outlined all-C graph, the mixed manifest and the final Rust on the same cases, plus a deliberately incorrect `step` candidate.

### 5.7 Explanation and architect guidance

**Bottom-up explanation:** cheap model calls explain leaf nodes with bounded source and context. Parents are then explained from child explanations plus retained source. Explanations record interfaces, referenced definitions, state/effects, control exits and unresolved questions, and are published as batches complete. Source stays immutable. Showing a model summaries instead of source doesn't delete executable content. Explanations are attributed hints, never proof of independence or purity. Avoid token-level model calls. Parallelism comes from claimed task fan-out, not from each agent decomposing the whole repository.

**Top-down guidance:** an architect returns a versioned `ArchitectPlan` with grouping intentions, dependency and interface decisions, assumptions and bounded directives. Each work unit receives compact parent guidance, its directive, allowed dependencies, relevant source and evidence, and this lineage is persisted. The architect is a role, not necessarily a separate process or an always-required giant model. Ultra 550B remains an optional original candidate. Verify access, hardware/context budget and privacy policy before selecting any endpoint. Lightning/Nano worker capability is likewise unproven until measured. Private operation may not send source-derived context to an external architect. Public-demo endpoints must be disclosed and can't establish an in-house capability claim.

**Token discipline:** cache summaries by source, resolved context, and prompt/model identity. Batch small regions without losing IDs. Record actual models, tokens, retries, tool/test time and human interventions, and include validation and orchestration overhead in cost comparisons. Don't regenerate a validated child only because its parent's explanation changed.

### 5.8 Bottom-up assignment and translation (steps 5–6)

**Readiness (evaluated by S):** a work-unit node is ready when it meets all three conditions:

1. It has an accepted C implementation and no accepted Rust implementation.
2. Every work-unit descendant has accepted Rust.
3. Every type node it uses has a Rust mirror.

Callee-before-caller order in the call graph is a scheduling priority, not a prerequisite. A Rust function can call a C function through a binding, and recursion makes strict call-graph order impossible.

**Type mirrors:** in the first phase, a struct, union or enum's Rust implementation is a `#[repr(C)]` mirror. It is generated mechanically from the configured headers, with size, alignment and field-offset assertions on both sides (5.9), rather than written by an agent. Native redesign of a type happens only after all of its users are Rust (5.10).

**Worker loop:**

1. Claim a ready node through S (claim, lease and fence; section 4).
2. Assemble context: the node's C text, pins, exits and effects; the accepted Rust of its children; explanations; the architect directive; and relevant callers and callees. Agents may retrieve more source with tools. Summaries don't replace source that correctness depends on.
3. The model writes Rust for the node behind its current ABI: `extern "C"` with the declared or outlined signature and exit tag.
4. Build the program with that node swapped in by link-time manifest selection (5.9). Every other node uses its current accepted implementation, which is mostly C early on.
5. Run the quick regression tier (5.12). On failure, feed compiler and test output back within a bounded retry budget. On persistent failure, record the blocker and release the claim rather than escalating silently to a larger model.
6. Submit the candidate and its report. S accepts by compare-and-swap on the base manifest (section 4). Full-tier checks run at checkpoints.

A parent's own translation covers the code left after its work-unit children were outlined: the skeleton that sequences child calls and dispatches their exits. Its children are already Rust, so the Rust parent calls them, initially still through their C-shaped interfaces.

Budget affected semantic scope as well as context and diff size. If a node can't be decomposed safely or handled by the selected model, record the blocker, stage compatibility steps, or seek an explicitly allowed model or strategy change. Don't outsource it silently.

### 5.9 Mixed-language ABI

A region has a logical behavioral contract. Generated C ABI adapters serve the actual mixed-language boundary; they aren't the permanent identity of the node. These are **same-target** contracts, not portable wire formats. Pin the target triple, scalar widths, layout/alignment, ownership/allocation/freeing, callback lifetimes and panic/unwind policy. Use `extern "C"`, checked representations, and explicit scalar/buffer/handle/control-exit types where code crosses into C. Never assume `repr(C)` makes Rust containers, references or trait objects C-compatible.

For the golden shared-state ABI, generate mirrors and accessors from the same configured headers, and check size, alignment and field offsets on both sides. Initially C keeps ownership of state; Rust accesses it through raw pointers or accessors with explicit lifetimes. Don't hold exclusive Rust references across callbacks that can re-enter and mutate that state. This phase may need unsafe Rust and isn't advertised as a safe-Rust proof. Reject unsupported layouts and callbacks. Prevent panics from unwinding across FFI with an explicit policy, such as abort or caught-unwind-to-status, at supported entry points.

Keep both implementations available, but select them **at link time from the manifest**, rebuilding between runs. Don't put a generic runtime dispatcher in every guest-instruction hot path. Benchmark granularity against the measured baseline budget; this is another reason for the work-unit floor. No hot unloading is needed.

Before acceptance, run local cases and the actual mixed program containing the candidate and its selected neighbors. Reports identify the full build and manifest. A local pass doesn't certify every combination. Milestone desktop boots complement bounded instruction and device checks. ABI compatibility alone proves neither behavior nor ownership isolation.

The fixture has no irreversible effects and no shared aliased heap. Golden nodes require controlled state/event inputs and the effect contracts above. Never run two real external writes as a shadow comparison. Candidate tests run without the supervisor's database or provider credentials, with bounded paths, time and resources.

### 5.10 Merge (step 7)

S evaluates merge opportunities in the same transaction that accepts a node. This is pure table logic, with no compilation:

- **Edge cleanup:** for each edge (call, data or type-use) from the accepted node to a neighbor in the same parent group, if both ends now have accepted Rust and no C user of that interface remains, enqueue a cleanup job for the edge.
- **Collapse:** when a parent and all of its work-unit children have accepted Rust and the edges inside the group are clean, enqueue a collapse job for the parent.

Both job kinds use canonical logical-task keys, so two acceptances that detect the same opportunity create one job.

Cleanup and collapse jobs run under a dedicated, narrowly scoped worker skill.

- **Allowed:**
  - Remove `extern "C"`, `#[no_mangle]` and exported shims from items that no longer have a C caller. Keep them where a C caller or C-owned callback table remains.
  - Re-inline outlined region nodes into their parent function. Outlining was a migration device, not a design decision, and shouldn't survive into the Rust code.
  - Turn output pointers into return values or `&mut` parameters, and exit tags into native control flow (`return`, `break`, `?`).
  - Replace C scalar aliases and raw pointers with Rust types where layout and semantics are unchanged and every user is Rust. Drop `repr(C)` from types with no remaining C user.
- **Not allowed:** behavior changes, edits outside the claimed group, interface changes visible to the group's outer callers, or weaker tests.

The candidate runs the same regression tier. A failure leaves the previous accepted state in place. When a collapse is accepted, the group displays as one node, expandable to its children. Provenance keeps the original nodes, C source and reports.

The swarm can clean up and collapse disjoint groups concurrently. Parallelism narrows near shared interfaces and the root. Serialize conflicting edits. Use normal source integration plus tests, not model-based conflict resolution, as the default. Adjacency on the canvas is not a merge criterion; graph edges and the hierarchy are.

### 5.11 Root, export and modernization (step 8)

Collapse recurses: regions into functions, items into modules (5.5), and modules into the program. When the program node collapses, the result is one ordinary Rust project with no original C linkage and no internal C ABI. Two things may remain:

- **Legitimate external FFI:** host libraries such as SDL, or libc where the original used OS interfaces. This is disclosed and isn't migration scaffolding.
- **C-shaped architecture:** unsafe code, global state, pointer-heavy structures and manual memory management. This is modernization input, not a failed migration.

C-free export is a separate checked stage, not implied by green node badges. Treat evaluation order, errors and destruction timing as semantics. Rerun the declared tests and a clean source/link inventory.

**Modernization** hill-climbs the working Rust through accepted, behavior-preserving improvements. Inside an owned region, interfaces may evolve; outside it, the declared behavioral contract holds unless the migration explicitly includes the affected callers.

1. Claim a connected group of accepted Rust regions at an exact base manifest. Call cycles, shared state, callbacks and ownership may require a cluster.
2. Propose one bounded improvement: introduce a native interface, consolidate a type, clarify ownership, replace a dependency or restructure a small behavior. Use staged compatibility changes when one safe edit can't span the necessary callers.
3. Materialize an isolated candidate, run the regression gate, and inspect the intended quality improvement. Moving unsafe code around or reducing line counts isn't modernization evidence.
4. Atomically accept the tested full manifest and the new composition/provenance mapping. If the accepted base has moved, integrate and retest first.

Source and target need not stay one-to-one. A missing or incompatible Rust library means implementing the needed behavior, adapting a demonstrated alternative, or explicitly negotiating a scope change. A similarly named crate isn't evidence of equivalence. Introduce native types only with their semantic requirements accounted for; for example, replacing arbitrary bytes with `String` adds a UTF-8 constraint. Some global changes may stay out of reach of the selected model, because hierarchical bookkeeping doesn't guarantee that every change is locally solvable.

### 5.12 Continuous correctness and capability evidence

A fixed regression suite is the backbone of translation, merging and modernization. Use two tiers: a quick tier (focused unit, instruction and device cases) on every candidate, and a full tier (the pinned whole-program workload) at checkpoints and before export. Once a Rust workspace exists, `cargo test` should exercise the relevant paths and invoke the harness through an agreed entrypoint. No such command exists yet. Candidate agents may not silently weaken tests, golden outputs, supported domains or performance budgets to make a patch pass. Changes to that contract need separately reviewed, versioned evidence.

Keep evidence levels distinct: lossless source reconstruction; outlined original-language checks; actual C → Rust → C execution; C-free Rust behavior; and modernized full-program behavior. A C wrapper calling renamed C isn't Rust ABI validation. Count unsupported heads, opaque nodes, variants and skipped tests; never treat them as covered silently.

For TinyEMU, a controlled desktop workload is the primary end-to-end oracle: pinned guest and configuration, repeatable time and events, scripted interaction, and meaningful state/device/log checkpoints plus visual evidence. A final screenshot alone can hide divergence. Coverage and repeatability must be measured, not assumed. G0 sets the quick and full tier budgets, and each accepted artifact records the exact suite it passed.

Compare original C, faithful Rust and modernized Rust on the same declared workload. To support the product thesis, record the small model and serving hardware, context/concurrency limits, success rate, retries, test time, human or strong-model intervention, and total cost. A useful experiment compares a small model working directly with the same model using Argo. A strong-model port is an optional baseline. Fewer parameters don't by themselves prove lower total cost or an in-house deployment.

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
| **G0: contract and baselines** | Agree source/configured-instance identities, fixture/ABI/domain, parser choice (5.2) with a span-mapping spike, template catalog v1 and work-unit floor, import/claim/reclaim/modernize transcript and harness interface; investigate/pin original RV64 X11 baseline; test selected private-model endpoint/tool access | S owns protocol + harness; W owns importer/model and bounded-interpreter feasibility; U owns toolkit launch |
| **G1: visible shared state** | Claimed import produces the template-derived hierarchical graph and live explanations through real SDK bindings; function bodies open as subgraphs; reconnect restores state | S/W/U first vertical slice; Appendix A BM1–BM6 feed this gate; inventory is not a migration claim |
| **G1s: swarm ownership** | Two workers compete safely; duplicate task creation, lease expiry/reclaim, stale publication, overlapping edits and combined-manifest acceptance are tested | Can use two processes on one host; required before concurrent mutation, not after G6 |
| **G2: same-language baseline** | Original C and the fully outlined graph-organized C (5.6) match on the fixture domain; Appendix A's identity swap (BT5) passes on TinyEMU; expose exact report and skipped/unsupported cases | W invokes S's runner; S enforces evidence prerequisites |
| **G3: one Rust replacement** | Selected model produces Rust in real C → Rust → C execution; local/mixed cases pass and a bad candidate is rejected | Minimum honest migration demo; record actual model/interventions |
| **G4: fixture Rust checkpoint** | All toy work units are Rust, scheduled leaves-first by S; clean C-free build matches the fixed tests; internal C-shaped interfaces may remain | Faithful port complete, not yet the full first slice or golden acceptance |
| **G4m: merge to root and modernize** | S detects cleanup/collapse opportunities on acceptance; scoped jobs re-inline the outlined region, remove internal `extern "C"` boundaries and collapse groups up to one Rust program node; then one bounded modernization is accepted. External behavior and provenance are preserved; stale or failed jobs cannot advance state | Completes the first slice; demonstrate separate jobs rather than one large parent rewrite |
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

Open at implementation start: U ownership, toolkit/revision and demo OS; configured-AST parser (5.2 recommends Clang; tree-sitter-c is the main alternative); template catalog v1 coverage and whether `goto` gets a template; work-unit floor threshold (5.3); grouping above the function level (5.5 defaults to original source files as modules, with architect regrouping as the alternative); Appendix A's open questions; canonical task keys, claim scopes, lease/renewal parameters and conflict behavior; concrete SDK/schema/transcript and native harness interface; supported boundary/interpreter extraction or staged-edit rules; ABI/layout v1; private model variants/serving hardware and context/tool budgets; fixed regression domains, acceptance cadence and modernization objectives; TinyEMU source closure, guest/configuration hashes, offline storage/network/display choices and measured golden budget. W and S can start independently against the agreed transcript; full backend completion is not a prerequisite. G0 resolves or records blockers rather than silently broadening or shrinking acceptance.

## Appendix A. Source inventory: block generator specification

Status: draft 1, 2026-11-07; merged into this document from `docs/BLOCK_GENERATOR_SPEC.md` on the same date.
Target codebase: TinyEMU 2019-12-21 (tarball SHA-256 `be8351f2121819b3172fcedce5cb1826fa12c87da1b7ed98f269d3e802a05555`)
Current implementation: `src/split.rs` (`swapctl split`), which covers sections A.6.1 to A.6.3 and part of A.6.5. That code, `swap_harness/` and `kernelish.c` live outside this checkout.

**How this appendix fits the architecture.** This layer is step 0 of the pipeline in section 5. It owns lossless text, top-level blocks and edit points. It doesn't own statement-level nodes, interfaces, scheduling or acceptance. When merging it in, the following IDs were renamed to avoid clashing with the integration gates in section 7: guarantees G1–G8 → **B1–B8**, tests T1–T5 → **BT1–BT5**, milestones M1–M6 → **BM1–BM6**. These reconciliations with the rest of the document take precedence over the text below:

- **Identity:** a block ID (A.6.10) is unique within one source snapshot. Persistent node identity is project + snapshot + configured instance + block ID, with provenance carried across edits (section 4).
- **Storage:** the tables in A.7 are the storage form of the `SourceFile`, `SourceBlock` and `ConfiguredInstance` records in section 4, not a separate schema.
- **Large functions:** question 2 in A.12 is answered by the template layer (5.3–5.6). Large functions are split by statement templates with derived interfaces and control exits, not by cutting text at `case` labels.
- **Clang:** the no-Clang rule applies to `crates/blocks`. The configured AST (5.2) is a separate stage in the worker.
- **References:** lexical references (A.6.9) are context for models and the UI. Node pins and edges come from the configured AST's resolved declarations.

### A.1 Purpose

The block generator turns a C codebase into **blocks**: exact pieces of source text that a person or model can rewrite one at a time, and that the swap tool can replace with Rust while the rest of the C program still compiles and runs.

It is written in Rust, with no Clang dependency and no preprocessing step. It works on the source text exactly as written.

#### Non-goals

- **Type checking:** Types are kept as text. No C type system is implemented.
- **Full preprocessing:** Macros are only expanded in the limited cases in section A.6.6, to find names.
- **Rust generation:** Writing Rust is the model's job. This crate only provides the blocks, their context and the edit points.
- **Build system parsing:** Programs and compile units come from a manifest (section A.5), not from Makefiles.

### A.2 Terms

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

### A.3 Guarantees

Every guarantee has a test in section 10.

| ID | Guarantee |
|---|---|
| B1 | **Exact rebuild:** Joining a file's segments in order reproduces the file byte for byte. Output that fails this check is an error, never written. |
| B2 | **Full coverage:** Segments are in order, with no gaps or overlaps, from byte 0 to the end of the file. |
| B3 | **Fail closed:** Code that can't be split safely becomes an `Unparsed` block with a diagnostic. It's never guessed. |
| B4 | **Deterministic:** The same input always gives the same blocks, IDs and order. |
| B5 | **Stable IDs:** Editing one block doesn't change any other block's ID. |
| B6 | **No panics:** Any valid UTF-8 input produces a result. Invalid UTF-8 produces error `E001`. |
| B7 | **Exact edit points:** Every range reported inside a block, such as a function's name or parameters, points to exactly that text. |
| B8 | **Safe Rust:** The crate uses `#![forbid(unsafe_code)]`. |

### A.4 Data model

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
    pub segments: Vec<Segment>,     // in file order, covering the file (B1, B2)
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

### A.5 Input

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

Version 1 splits every `.c` and `.h` file under `root`, with or without a manifest. The manifest only adds program and compile-unit information, which section A.6.7's instantiation records and the identity-swap test (BT5) use.

### A.6 Pipeline

The stages run in this order. Stages A.6.1 to A.6.3 run per file. Stage A.6.4 needs all files' directives first, so the generator makes two passes.

#### A.6.1 Read
- **Path order:** Read files sorted by path (B4).
- **Encoding:** Reject invalid UTF-8 with `E001`, and skip that file.
- **Hash:** Record the file's SHA-256.

#### A.6.2 Lex (implemented)
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

#### A.6.3 Top-level scan (implemented, with new rules)
Bracket depth counts `()`, `[]` and `{}` in significant tokens, meaning everything except whitespace, comments and directives.

**Item boundaries:**
1. A directive at depth 0 between items is its own segment (`Macro` for `#define NAME`, otherwise `Directive`).
2. `extern "<string>" {` and its matching `}` are each `Directive` segments. The code between them is scanned as top-level.
3. A `{` at depth 0 directly after `)` starts a function body, and the item ends at the matching `}`. Comments and directives between the `)` and the `{` are skipped.
4. Otherwise the item ends at the first `;` at depth 0. This includes `struct`/`union`/`enum`/`typedef` definitions, which continue past their `}`.
5. **New: self-terminating macro calls.** An item that so far is only `M(...)`, where `M` is in the macro table (A.6.4) and its body ends with `}` or `;`, ends at its closing `)`. It becomes a `MacroCall` block.
6. A bare macro use (`M` or `M(...)` with no other tokens) ends before a following directive, before a closing `}` of an `extern` wrapper, or at the end of the file.
7. Otherwise a bare macro use joins the following item, so attributes like `__printf(1, 2)` stay with their function.

**Comments:** Comments directly above an item, with no blank line in between, belong to the item.

**Failure:** If depth goes negative or the file ends inside an item, the code from the item's start to the next `}` at the start of a line becomes one `Unparsed` block with `W010`. Scanning then continues after it. Without such a `}`, the `Unparsed` block runs to the end of the file.

**New: conditions.** While scanning, keep a stack of top-level `#if`/`#ifdef`/`#ifndef` groups, updated by `#elif`, `#else` and `#endif`. Each block records the stack at its start in `conditions`. An `#endif` with no open group, or a group still open at the end of the file, gives `W011`, but the scan continues. Directives inside a block don't change the stack.

#### A.6.4 Macro table (new)
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
- **Code-generating macros:** A function-like macro whose expanded body, scanned as in A.6.3, contains at least one `Function` or `Declaration`, such as `TARGET_READ_WRITE` in `riscv_cpu_priv.h`.

Macros defined more than once with different bodies are kept as alternatives, each with its own conditions.

#### A.6.5 Classify and name
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

#### A.6.6 Code-generating macro calls (new)
For each `MacroCall` block calling a code-generating macro:
1. **Expand:** Substitute the arguments into the macro body and apply `##`, but don't expand other macros. Paste macros and `#` stringizing are the only exceptions.
2. **Scan:** Run A.6.2 to A.6.5 on the expanded text.
3. **Record:** Add each resulting function or declaration name as a `Symbol` of the `MacroCall` block.

If expansion fails, record no symbols, add `W040`, and keep the block. Calls of `TARGET_READ_WRITE(8, uint8_t, 0)` must produce `target_read_u8` and `target_write_u8`.

#### A.6.7 File roles (new)
- **Fragment files:** A file is a fragment if it's included from inside a block in another file, or if its top level contains a statement keyword item (`case`, `default`, `if`, `else`, `for`, `while`, `do`, `switch`, `return`, `break`, `continue` or `goto`), or a `label:`. The whole file then becomes one `Fragment` block, plus leading and trailing `Text`, and gets `W020`. The block is attached to every block that includes it.
- **Template files:** A file is a template if it's included more than once from the same file, and at least one macro defined just before each `#include` is used in the template. Those macros are the template's parameters.
- **Instantiations:** Each `#include` of a template becomes an `Instantiation`. Its `bindings` are the parameter values defined just before it, and its `conditions` are the `#if` groups around it.
- **Template symbols:** Each `Templated` block in a template produces one `Symbol` per instantiation, with the bindings substituted. `divu{XLEN}` produces `divu32`, `divu64` and `divu128`.

Templates nested inside templates combine both files' bindings. For example, `softfp_template_icvt.h` is included three times by `softfp_template.h` with `ICVT_SIZE`, and `softfp_template.h` is included three times by `softfp.c` with `F_SIZE`, giving up to nine instantiations.

A file can meet both the fragment and template rules. `riscv_cpu_fp_template.h` is included three times with `F_SIZE`, but from inside a function. In that case the file is a `Fragment`, and its instantiations are still recorded so the including block knows which widths it contains.

#### A.6.8 Function heads (new)
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

#### A.6.9 Identifiers and references
- **Identifiers (implemented):** Identifier tokens in the block, excluding C keywords, sorted and deduplicated.
- **Paste-macro calls (new):** Before collecting, render each paste-macro call to its pattern, so a call `glue(divu, XLEN)(a, b)` records `divu{XLEN}` instead of `glue`, `divu` and `XLEN`.
- **Resolution (new):** For each identifier, find matching block names or symbols, in this order:
  1. A `static` block in the same file, or in a template or fragment file included by the same file
  2. Any block in a header the file includes, directly or indirectly
  3. Any `External` block in the project
  4. Otherwise `Target::External`

  If several blocks match at the same level, for example alternatives under different `#ifdef` branches, the result is `Target::Ambiguous`.

References are candidates, not proven uses. A local variable with the same name as a global still produces a reference.

#### A.6.10 IDs
A block ID has the form `{path}:{kind}:{name}`, with `kind` as `fn`, `decl`, `type`, `macro`, `call`, `dir` or `frag`, and `name` as the rendered pattern.

Duplicates within the same file are resolved in this order:
1. Add `@` followed by the first 8 hex characters of the SHA-256 of the block's conditions. This separates the two `cpu_to_be32` blocks in `cutils.h`.
2. If two blocks still collide, add `~2`, `~3` and so on, in file order, and add `W030`.

Directive and `Unparsed` blocks have no name, so their ID is `{path}:{kind}:{hash8}`, using the first 8 hex characters of the SHA-256 of their text. A fragment file's block is `{path}:frag`.

### A.7 Output

- **JSON Lines export:** `swapctl blocks [--manifest swap.toml] <root>` writes one record per file, block, symbol, instantiation, reference and diagnostic.
- **SpacetimeDB tables:** `file`, `file_segment`, `block`, `symbol`, `instantiation`, `reference` and `diagnostic`, with one column per field in section 4. `file_segment` stores `Text` content inline and a `BlockId` for blocks, so a file can be rebuilt from the database alone.
- **Rebuild:** `swapctl rebuild <file>` joins the stored segments. It's also how swapped code reaches the compiler.

### A.8 Diagnostics

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

### A.9 Code layout

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

### A.10 Tests

| ID | Test | Passes when |
|---|---|---|
| BT1 | Unit tests per rule in section A.6 | Every rule has a passing and a failing example |
| BT2 | Fixtures in `tests/fixtures/`, each with an `.expected.json` | Output matches exactly |
| BT3 | TinyEMU corpus, from the pinned tarball | The criteria below are all met |
| BT4 | Property and fuzz tests | No panics, and B1 holds for arbitrary UTF-8 input and for edited fixtures |
| BT5 | Identity swap on TinyEMU | The program builds and the boot output is identical |

**Fixtures (BT2):** At least one each for strings and comments containing braces, `extern "C"`, a leading comment, `SYSCALL_DEFINE`-style functions, the current `kernelish.c`, and these cut-down TinyEMU cases:
- Templated `glue(div, XLEN)` functions in a template included three times
- A fragment file included inside a `switch`
- `TARGET_READ_WRITE`-style code-generating macro calls
- Alternative `cpu_to_be32` definitions under `#ifdef`/`#else`
- Alias macros like `#define add_sf glue(add_sf, F_SIZE)`
- A template nested in a template, like `softfp_template_icvt.h` inside `softfp_template.h`

**TinyEMU corpus (BT3)** requires:
1. All 88 `.c` and `.h` files, including `slirp/`, rebuild byte for byte.
2. Zero `Unparsed` blocks.
3. `riscv_cpu_fp_template.h` is one `Fragment` block, with no `Declaration` or `Function` blocks.
4. `riscv_cpu_template.h` has a `Templated` block named `divu{XLEN}`. Its symbols across all instantiations are `divu32`, `divu64` and `divu128`.
5. `riscv_cpu_priv.h`'s `TARGET_READ_WRITE` calls produce `target_read_u8` through `target_read_u64` and the matching write functions, plus the `u128` ones under their `#if` condition.
6. The two `cpu_to_be32` blocks have different IDs and different conditions.
7. No `W030` diagnostics.
8. The output matches a reviewed snapshot, `tests/snapshots/tinyemu.jsonl`.

**Identity swap (BT5):** For every `Function` block in `temu` that has a `head`:
1. Rename it to `<name>__orig` using the `name` range.
2. Add a wrapper with the original head that calls `<name>__orig` with the same arguments.
3. Rebuild each file from its segments.
4. Build `temu` and run `swap_harness/run_workloads.sh`. The output must match the unmodified build.

Variadic functions are skipped and counted. Files are processed in batches, so a failure points to one file. This test checks block boundaries and function heads against a real compiler without using a model.

**Other checks:** `cargo clippy` with no warnings, and the whole TinyEMU tree processed in under 1 second in a release build.

### A.11 Milestones

| # | Work | Done when |
|---|---|---|
| BM1 | Move `split.rs` into `crates/blocks` with the section A.4 model, IDs, diagnostics and condition tracking | Current tests pass, BT3 criteria 1 and 6 pass |
| BM2 | Macro table, self-terminating macro calls, paste-macro and alias names | BT3 criteria 2 and 5 pass |
| BM3 | File roles and instantiations | BT3 criteria 3 and 4 pass |
| BM4 | Function heads and the identity swap | BT5 passes |
| BM5 | Reference resolution and the JSON Lines export | BT3 criteria 7 and 8 pass |
| BM6 | SpacetimeDB tables and `swapctl rebuild` | A file rebuilt from the database matches the original |

### A.12 Open questions

1. **Evaluating `#if`:** Conditions are stored as text. Evaluating them against a compile unit's defines would show which blocks are actually built, but needs a C constant-expression evaluator. This is planned after BM6.
2. **Splitting very large functions:** `riscv_cpu_interp_x` is 57 KB, too large for one model rewrite. Splitting it at `case` labels into sub-blocks is planned as a separate design.
3. **Non-UTF-8 files:** These are rejected for now. Storing them as bytes would need a different SpacetimeDB column type.
4. **Inlining across C and Rust:** Swapped `static inline` helpers lose inlining unless link-time optimization spans both languages. This belongs to the swap tool, not the block generator, but affects which blocks are worth swapping first.
