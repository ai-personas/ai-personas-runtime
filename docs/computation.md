# WebAssembly computation

`compute.wasm.v1` executes an exact supporting asset containing WebAssembly text
(WAT). Ordinary requests supply its current definition under
`CONTROL.computation_capabilities`, without a grant or selected guide. Historical
grant records remain metadata and do not restrict execution.

Invoke `effect` with the supplied `capability_id`. The runtime resolves the unique
interface retained for this invocation and verifies its complete binding against
the current implementation. Do not include `binding_revision` in the operation. Its
`inputs` contains `program`, an exact full UTF-8 asset reference, and `input`, a
string. A same-response `file_write` can provide the program: use its
`proposal-op:SAVE_ID` reference and include `SAVE_ID` in `depends_on`. The host
resolves the accepted file before dispatch. Multiple independent computations
or HTTP requests may be proposed in one Decision; their results are observed
in a later Decision.

The module exports `memory` and
`run(input_pointer: i32, input_length: i32) -> i64`. The host writes the input at
offset zero and calls `run(0, length)`. Reserve that region in the program. The
returned i64 packs the unsigned output pointer in its high 32 bits and output
byte length in its low 32 bits. The output must be UTF-8. For example:

```wat
(module
  (memory (export "memory") 1)
  (func (export "run") (param i32 i32) (result i64)
    local.get 1
    i64.extend_i32_u))
```

There is no application program, input, output, fuel, linear-memory, table,
instance or stack quota. Start functions and multiple memories are supported.
Wasmi parsing/compilation limits are left unset; numeric stack settings use
machine representability because the dependency exposes no unbounded option.
Module-declared memory, the 32-bit exported ABI and actual host resources still
apply. Fuel metering is disabled, so `fuel_consumed` is null rather than a claim
of zero consumption. Invalid modules, missing exports, unresolved imports,
invalid memory, non-UTF-8 output and actual execution traps remain failures.

The embedded linker supplies no host APIs or WASI implementation. Host commands
can run installed runtimes and software that provide those interfaces. This is
a description of the implemented ABI, not a grant or sandbox requirement.

Complete Decision adoption records the exact program, input, implementation and
intent before execution. The runner and resolved dependencies form the engine
revision. Dispatch rechecks that revision and the actual accepted prerequisites;
an already dispatched intent is never run again. A result binds the exact input,
program, engine and terminal receipt. Success establishes execution, not program
correctness or work acceptance.

`computation_result` observations and
`CONTROL.computation_state.retained_results` identify retained result assets
under `tools/results/`. Read or select the exact source to obtain its body.
Metadata alone does not supply output text. Source attribution, required
qualifications and prior immutable evidence remain attached. File-corpus and context byte/count quotas are removed; see
[execution resources](execution-resources.md) for remaining work.

The control projection includes results already referenced in the current input
or reported by its current observations, including explicitly selected results
from another work. It does not list every past computation. Historical allocation
metadata accompanies a referenced guide. Use [native file discovery](input-file-references.md#discover-resources-before-selecting-their-bodies)
or retained-history queries to locate other results and guides.

`recover-computations` publishes existing terminal evidence or dispatches an
admitted job whose prerequisites still match. It never replays a dispatched job.
Missing terminal evidence remains uncertain. Recovery does not change Stop,
Pause, work completion or the model's interpretation. Live records stay outside
the source repository.
