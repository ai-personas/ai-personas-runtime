# Inspect runtime state costs

Set `AI_PERSONAS_RUNTIME_TIMINGS=1` for a native command to emit phase timings on
standard error. For example, inspect an existing runtime using its saved operator:

```sh
AI_PERSONAS_RUNTIME_TIMINGS=1 personas-session \
  --state-root /private/runtime --operator requester \
  inspect > /private/inspection.json 2> /private/runtime-timings.jsonl
```

The option is read once per process, at the first instrumented operation. It is
off unless its value is exactly `1`. It applies to the actual command: inspection
still performs its normal accounting, and a `run` command still performs work.
Enabling timings adds no model calls, state mutations, retries or limits of its
own. The events are host diagnostics; they are not inserted into persona input,
native receipts or stored state. Standard output keeps its normal format.

Each completed phase emits a JSON object with schema
`runtime-phase-timing-v1`, process `pid`, start-order `sequence`, `phase`,
`elapsed_ms`, and `ok`. `ok` reports whether that phase returned `Ok`; it is not a
claim about the whole command, provider completion or work acceptance. Other
diagnostics may also appear on standard error. A panic or process termination
can leave a phase without an event. Failure to write diagnostics is ignored and
does not replace the operation's result.

| Phase | Measured work |
| --- | --- |
| `runtime.load` | Read, decode and validate one runtime state load |
| `runtime.read_state` | Store read and its normal navigation accounting |
| `state.validate_exposure_pool` | Validate pooled manifests and index their exact typed-field uses |
| `state.deserialize_borrowed` | Decode typed state from the borrowed stored body and validated references |
| `runtime.validate_intake_state` | Validate retained intake episodes and bindings |
| `state.serialize_expanded` | Build expanded JSON from typed state |
| `state.pack_exposures` | Identify repeated exposures and build their retained pool |
| `runtime.save` | Construct and commit a state object through the ordinary save path |
| `runtime.commit_state` | Compare and swap that constructed state object |
| `runtime.check_configuration` | Read and verify the current runtime configuration for a live driver |
| `runtime.fingerprint_state` | Bind the complete current stored object to its exact reference |
| `runtime.binding_head_unchanged` | Reuse this driver's successful validation of that same exact reference |
| `runtime.validate_binding_head` | Fully decode and validate a new reference before comparing configuration |

These are elapsed wall times, not CPU time. Nested phases overlap: do not add
`runtime.load` to its children or `runtime.save` to the serialization and commit
phases it contains. Events appear in completion order; sequence identifies start
order within one process. Serialization and decoding can also occur outside the
ordinary load/save methods. Separate atomic admission and adoption transactions
do not pass through `runtime.commit_state` merely because they save state.

This is focused instrumentation of state handling, not a complete profiler of
scheduling, provider transport, host effects or storage. Use repeated loads and
their measured phases to locate costs in actual retained work before attributing
a slow session to the provider. Logging and unrelated concurrent work can affect
timings. No performance improvement or causal comparison follows from one run.

Live session and native-provider binding checks always read the current stored
runtime object and compare its configuration with the driver's original value.
They retain only the exact reference of a successfully decoded, validated and
matching head. The reference binds identity, revision, kind and the entire
canonical JSON body. Repeated checks of that identical reference can omit typed
state decoding. Every changed reference, including changes outside configuration,
requires full decoding and intake validation again. A failed decode or mismatched
configuration never updates the remembered reference. Each driver starts without
one; nothing is persisted or shared with another driver.

This reuse applies only to the binding check. Ordinary state loads, preparation,
adoption, ownership and freshness checks retain their existing behavior. Store
reads and their accounting still occur on every binding check. The new timing
phases distinguish actual reuse from validation of a changed head; both remain
nested within `runtime.check_configuration`.

Computing a stored object's exact reference streams canonical JSON into SHA-256
through a small buffer. It borrows the complete body instead of cloning another
JSON tree and allocating a complete serialized byte vector. Object keys are
sorted at every level, including builds using `serde_json/preserve_order`; array
order and scalar encoding are preserved. Identity, revision, kind and every body
field still contribute to the same digest. This changes fingerprint allocation,
not the reference format, storage encoding, validation or navigation accounting.

State decoding also borrows the stored JSON body. It validates each pooled
exposure's digest and typed shape, every reference, and use of every pool entry
before decoding. Packing and decoding share one explicit inventory of typed
exposure fields. Only those exact locations can resolve through the pool;
similarly shaped authored JSON, payloads and historical records remain literal.
The pool is omitted only from the root state view. Native JSON key, scalar and
enum decoding still applies, and the resulting State owns its data as before.

The borrowed view exists for one decode and never changes the stored body or
persists a cache. Direct state loads no longer create a cloned body or an expanded
JSON tree. General deserialization still materializes its incoming JSON once,
and state serialization still constructs and packs expanded JSON. Older builds
emit `runtime.clone_state_json`, `state.unpack_exposures`, and
`state.deserialize_expanded`; those phases are replaced by the two borrowed-state
phases above. This allocation change alone does not establish an overall speed
or peak-memory improvement.
