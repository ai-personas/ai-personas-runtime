# Bounded computation tools

`compute.wasm.v1` executes an exact supporting asset containing WebAssembly text
(WAT). It is an embedded, import-free computation profile. It cannot access the
filesystem, network, environment, credentials, clocks, other personas, runtime
state or a model. The host links no imports, including WASI. It grants no access
to the coding agent's tools, installed skills, shell or other installed software.

The persona chooses the guide and program through the existing bounded asset
discovery, read and selection operations. A guide is reusable procedural text;
its actual author and restrictions remain attached. Importing, reading or
selecting a file does not grant execution authority. The operator grants one
finite computation allocation for a persona/work pair and an exact guide:

```json
{
  "id": "calculation-grant",
  "actor": "persona-0001",
  "work": "work-00000001",
  "guide": {
    "source": "exact-guide-source",
    "revision": "COPY_THE_ACCEPTED_REVISION",
    "representation": "utf8",
    "start": null,
    "end": null,
    "lifetime": null
  },
  "attempts": 3
}
```

Use `import-supporting-file` to retain a guide or program. Its `record.author`
preserves the operator's explicit attribution to the source author; the importer
is separately retained as `imported_by` and `OperatorImport`. An import is not
persona authorship or independent authentication of an outside author.
Then, with the saved launch operator:

```sh
ai-personas --state-root /absolute/private/state --operator local-operator grant-computation /absolute/private/grant.json
ai-personas --state-root /absolute/private/state --operator local-operator revoke-computation calculation-grant
ai-personas --state-root /absolute/private/state --operator local-operator recover-computations
```

Granting does not select files, wake personas or start inference. The cap is
1–32 attempts within the existing work allocation, not additional funding.
Each admitted computation consumes one ordinary work attempt, preserves the
repair suballowance, and reserves an attempt/input/output allowance in the same
root Store ledger used by inference. Canceled and failed admissions do not refill
the work cap. A second grant cannot reset an existing persona/work allocation.
Revocation is permanent. Engine or dependency changes invalidate old bindings;
there is no implicit migration to a different executable environment.

A callable definition appears in `CONTROL.computation_capabilities` only when
this request actually supplies the exact complete guide body, with its qualified
closure and current permissions. Metadata, a proposed next selection, nested
history, or a stale remembered tool name is insufficient. No full registry is
sent to the provider. The current grant, guide and engine binding are checked
again at adoption and dispatch.

The persona invokes the supplied definition using `effect`, copying its
`capability_id` and `binding_revision`. `inputs` contains `program` (an exact full
UTF-8 asset reference) and `input` (a string). One computation is permitted per
decision across computation and [HTTPS retrieval](https-retrieval.md). A persona may author a program through `file_write` in that same
complete decision, reference it as `proposal-op:SAVE_ID`, and name `SAVE_ID` in
`depends_on`. Validation binds the exact accepted candidate before dispatch.
Other effects and unobserved tool results cannot be dependencies.

## Program interface and limits

The module exports a memory named `memory` and a function
`run(input_pointer: i32, input_length: i32) -> i64`. The host copies the UTF-8
input into memory at offset zero before calling `run(0, length)`. Reserve that
input region in your program. The returned i64 packs the unsigned output pointer
in its high 32 bits and output byte length in its low 32 bits. The result must be
valid UTF-8; it is retained as data and never evaluated by the host.

For example, this program returns the exact input bytes:

```wat
(module
  (memory (export "memory") 1)
  (func (export "run") (param i32 i32) (result i64)
    local.get 1
    i64.extend_i32_u))
```

The fixed profile bounds WAT to 32 KiB, input and output to 8 KiB each, linear
memory to 2 MiB, execution to one million metered fuel units, recursion depth to
128, and value stack height to 4096. It permits one instance, one memory and one
table with at most 4096 elements. [Wasmi's strict module limits](https://docs.rs/wasmi/2.0.0/wasmi/struct.EnforcedLimits.html)
apply. Start
functions and imports are rejected. Each execution has fresh guest memory.
Oversized output, invalid memory, invalid UTF-8, fuel exhaustion and other traps
produce host failure codes. No partial output is treated as success.

The pinned Wasmi dependency graph and runner source are hashed into the engine
binding. These limits do not claim a hard host-process RSS, compilation wall-time,
remote token or monetary bound. Execution success proves only the observed guest
run; arithmetic correctness, faithful skill application, engineering assurance,
acceptance and delivery need their own evidence.

## Adoption, results and recovery

The entire response must validate before the atomic transaction adopts file
changes, the response archive, the work debit and the execution intent. An
effective character change fences computation with the other fresh operations.
Invalid responses execute nothing. Before entering the engine, the host prepays
a bounded terminal evidence slot, rechecks the exact accepted prerequisites,
and durably records dispatch. Results bind the exact intent, program, input and
engine and inherit their source restrictions plus the author's complete input
exposure. Current permissions are rechecked before any later supply.

The next fresh persona decision can receive `computation_result`. Its input,
output or fixed failure code, fuel usage and terminal receipt remain distinct
from the persona's interpretation. Factual recording changes no Stop, wait,
permit or work phase. It creates no release or correctness claim.

Required supporting documents are assembled together so overlapping
qualification closures supply each exact body once. Current permissions and the
complete required closure still apply. Result projections retain the executed
binding without repeating its input schema; current callable definitions are
supplied separately. The original retained execution evidence is unchanged.

Every recorded result also has a protected supporting file under
`tools/results/`. It retains the exact input, output or failure, program and
engine bindings, and terminal evidence. Its author is `runtime-computation`;
its origin identifies the actual invocation and intent. It inherits input
restrictions and required qualifications, consumes the existing corpus and
Store retention allowances, and cannot be edited as persona-authored prose.
For newly admitted jobs, required accompaniment follows the exact file bodies
actually supplied in the admitted request, the operative guide and the resolved
program. The job, result observation and protected file retain those inputs
separately from their full permission ancestry. Later accepted qualifications
on these operative inputs are rechecked and attached to the result's immutable
revision when it is read or selected. Every original permission restriction
still applies, including restrictions inherited through earlier work; a
permission-only ancestor does not by itself request its guide or data body.

Older jobs, observations and result files without this distinction keep their
original conservative qualification scope. Updating helpers does not rewrite
their bodies, remove required material or reclassify historical supply. Stores
with the new `qualification_sources` metadata require updated helpers; older
readers reject the unfamiliar record rather than silently drop its meaning.

`CONTROL.computation_state.retained_results` supplies exact file references for
this persona's results in the current work, subject to current permissions.
These are metadata only. The primary may read or select a result with
`next_assets` when its full qualified bundle fits. Selecting a result avoids a
whole-history search and does not depend on the smaller navigation result-byte
limit. The normal assembled-context and provider-input limits still apply.
Later works can discover the retained files through ordinary asset search.
No result file is automatically selected or treated as a current execution grant.

An interruption after adoption blocks another invocation until host settlement.
`recover-computations` may dispatch a still-admitted intent only if its exact
prerequisites remain current. It never reruns an intent already dispatched.
An exact authenticated terminal slot can reconcile interrupted completion.
Without one, execution remains uncertain and retains its resource hold; further
computation for that persona/work is unavailable. A changed prerequisite cancels
an undispatched intent. An exact accepted-response replay returns the original
adoption and does not start execution again.

Recovery also materializes missing result files from already-retained terminal
evidence, without changing old observations or executing a program again. Keep
the updated runtime helpers after these files are created: older readers may
not recognize the `computation_result` revision-origin format. Capacity failures
retain the execution evidence and require capacity recovery; they never permit
reexecution or silently omit required qualifications.

Use `development PERSONA --invocation CALL` to inspect the accepted
decision's supplied guide, callable definition and `computation_execution`
receipts. The result view remains subject to current lineage and byte bounds.
Keep live run records in private state directories, outside the source repository.
