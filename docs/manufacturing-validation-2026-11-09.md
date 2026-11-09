# Manufacturing-artifact live validation — 9 November 2026

**Ongoing: the manufacturing-ready acceptance requirement is not met.** The editable-character runtime executes real Codex `gpt-6.1-sol` decisions and host operations, but the SMPS result remains an incomplete engineering candidate. The [implementation design](editable-persona-models.md) and [earlier preliminary-run report](live-validation-2026-11-09.md) describe the architecture and prior evidence. Neither a successful runtime operation nor a native CAD file establishes a good electrical design.

## Run conditions

The qualifying follow-up started with empty state and a neutral, empty host workspace. Its exact request is `design smps for pc`, with no amendments. Mira, Oren, Sana, Ivo and Lena received general character seeds, not assigned professions or a domain workflow. All use direct provider inference with requested model `gpt-6.1-sol` and `max` reasoning; the Codex helper supplies authentication only. The actual need was withheld throughout the 14 initialization calls, then supplied after activation without the initial seed.

The personas choose their approach, tools, contributions and collaboration. No engineering corrections from the independent review were sent to them or applied to their files. Runtime fixes were installed after draining admitted decisions and effects, retaining every earlier snapshot and receipt. This is an iterative live validation, not a controlled comparison of frozen releases. An earlier exploratory follow-up whose workspace path contained an evaluator cue remains retained and excluded from the qualifying cohort.

Core and desktop builds passed. No runtime unit tests were written or run. Validation uses actual provider calls, recorded decisions, host execution, independent checks of copied artifacts and real engineering tools. The personas also authored engineering verification programs as part of their design work.

## Frozen checkpoint before DecisionV4

Phase 5 ended on commit `403c0d5`, with these cumulative observations:

| Observation | Recorded result |
| --- | --- |
| Decisions | 383 accepted, 3 rejected, 1 provider failure |
| External effects | 470 completed |
| Reported tokens | 9,474,270 input + 1,370,966 output = **10,845,236** |
| Cached input included above | 123,520 tokens |
| Reported provider bindings | 386 `gpt-6.1-sol` / `max`; the refusal reported effort but no model or usage |
| Retained assets / deliveries | 84 assets; no deliveries |
| Character development | Five initial models, four contextual evaluations; no post-initialization parameter patches or assessments |

The failed request's charges are unknown. Token counts do not establish dollar billing. These cumulative numbers must not be added to earlier phase snapshots of the same run. Separate mechanics checks and earlier runs also consumed resources and remain recorded separately.

The team chose a provisional 300 W modular ATX-style supply using purchased isolated power modules and custom secondary circuitry. They produced circuit definitions, candidate bills of materials, native schematics, project libraries, calculations and harness construction records. Their collaboration exposed and revised substantive issues, including connector contact grouping, an enable-voltage assumption and logic-output compatibility. Those revisions are useful activity; they do not establish closure of the complete design.

Independent KiCad exports and ERC checks on exact copied snapshots found:

| Copied candidate | Exported connectivity | Remaining native-CAD findings |
| --- | --- | --- |
| Negative rail | 12 components; intended connections and isolated no-connect matched | All 12 components lacked footprints; 14 library warnings |
| Supervisor | 144 components, 466 pins; pin connection groups matched | Two power-input errors, two dangling-label warnings and 144 unresolved footprint links |
| Auxiliary rails | 105 components, 305 pins; pin connection groups matched | No ERC errors in this snapshot; 18 missing footprints and 87 unresolved footprint links |

These snapshots predate some subsequent persona revisions. The comparison checks representation against the authored circuit, not the electrical truth of that circuit. Visual inspection also found large grids of labeled components with limited circuit-flow explanation. Late in phase 5, Sana created the first native standby PCB placement. Independent inspection found 34 footprints, 93 pad objects with matching assigned nets, four copper layers and no tracks or vias. Actual DRC reported 71 unconnected items, two courtyard overlaps and a clearance error, alongside library and silkscreen warnings. This board predates the memory update below. There was no routed-board verification, complete fabrication export or integrated manufacturing package. Component selection, electrical margins, harness bounds and integration remained open. Physical qualification had not been performed.

## Runtime findings and actual checks

An actual command produced 4,899,297 bytes of output, and the provider refused the resulting input with `context_length_exceeded`. Recovery now supplies explicit metadata and immutable references so the persona can choose exact reads and a smaller next context. A separate live check on `570b967` exercised the automatic path: a 4,944,608-byte request was refused, the runtime scheduled a 35,594-byte recovery request without operator intervention, and the persona obtained byte-exact boundary excerpts, delivered its report and stopped. The command executed once; the retained output remained complete. Five accepted calls used 49,575 reported tokens, plus one refusal with unknown usage and charges. Other failure classes retain their explicit-review behavior.

Exact captured images can now remain visual inputs through explicit reference selection. A separate check on `403c0d5` captured an image, replaced its host file, and correctly reported the original label and three circles in two subsequent ordinary decisions. Both requests contained the same original bytes, without another capture. Seven accepted calls and one initialization rejection used 72,887 reported tokens. Sana and Lena subsequently selected retained manufacturer images themselves in the SMPS run. This establishes image continuity, not a causal improvement in design quality.

That initialization rejection exposed an overly restrictive receipt-access rule. A live check on `705fede` confirmed that a pending persona could read and retain its own initialization receipt while ordinary task content and the host path stayed hidden. Initialization prose remained absent after activation. The check finished with seven accepted calls, two later outcome-artifact rejections and 85,031 reported tokens. The persona recovered from those rejections on that build. A subsequent change clarified that outcome artifacts require exact asset references, while object receipts belong in evidence; that change is not credited for the earlier recovery.

## Current continuation

The phase-5 record also showed a consequential memory failure: Sana alternated between circuit, footprint and tool-environment inspections across ten consecutive accepted decisions while repeatedly selecting the same stale working note. Repeated statements of intent did not preserve the discoveries needed to act.

Phase 6, introduced by `7e255f2`, uses `DecisionV4`. Every next-context choice includes a persona-authored `working_note`; the next decision receives its exact accepted text without requiring a separate asset-save operation. The note can preserve useful facts, working tool invocations, failed approaches and evidence pointers. The runtime neither writes the summary nor decides which facts matter. Initialization notes remain separated from active work. The original SMPS need, five character models and persona continuations were unchanged at this upgrade.

A separate fresh DecisionV4 initialization check completed four accepted `gpt-6.1-sol` / `max` calls using 31,805 reported tokens. Its pending decision received the exact preceding initialization note; task and host path stayed hidden until activation, and the first active request contained neither that bootstrap continuity nor the initial seed. The persona fulfilled the brief greeting request and explicitly stopped.

Further live observations must establish whether the working-note mechanism improves sustained continuity and artifact production. Useful parameter learning, causal character benefit, lower overall cost and manufacturing readiness remain unproven. The default desktop onboarding also still creates the earlier fragment runtime; editable-model work uses the explicit neural entry points documented in the design.

Private evidence is retained outside the repository under `/home/dev/.local/state/ai-personas-validation/20261109-manufacture-artifacts`, including phase manifests, immutable state, provider receipts, independent snapshot reviews and exported mechanics reports. The implementation remains on `feat/editable-persona-models` in the separate editable checkout; the original checkout and prior deliveries are preserved.
