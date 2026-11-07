# Argo

**Explode a legacy program into a live Blueprint-style graph. Replace its nodes with Rust. Keep checking the running program as you go.**

Argo is a Rust-first legacy-modernization project built around SpacetimeDB, agent-assisted understanding, and incremental C/Rust interoperability.

> **Status: architecture and planning only.** There is no implemented application, executable workspace, successful migration, or recorded boot yet. See [ARCHITECTURE.md](ARCHITECTURE.md) for the detailed design, interfaces, milestones, and independent reviewer opinions.

## The idea

1. **Build the graph from templates.** Parse the C AST and match constructs (functions, structs, enums, macros, `if`/`else` chains, loops, `switch` …) against templates. Each template emits its piece of a hierarchical node graph. Functions and types get pins from their signatures. Statement regions get pins from the values they inherit from and hand back to the enclosing scope, plus their control exits.
2. **Navigate it like Blueprints.** The top view shows functions, types and globals. Any function opens into its internal node groups, like a Blender node group or a UE5 collapsed graph.
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

No setup commands are published yet because the workspace does not exist. Implementation starts from the contracts and acceptance gates in [ARCHITECTURE.md](ARCHITECTURE.md).
