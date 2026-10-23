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
| `runtime.load` | Read, clone, decode and validate one runtime state load |
| `runtime.read_state` | Store read and its normal navigation accounting |
| `runtime.clone_state_json` | Clone the stored JSON body for typed decoding |
| `state.unpack_exposures` | Validate and expand retained exposure references |
| `state.deserialize_expanded` | Deserialize the expanded JSON into typed state |
| `runtime.validate_intake_state` | Validate retained intake episodes and bindings |
| `state.serialize_expanded` | Build expanded JSON from typed state |
| `state.pack_exposures` | Identify repeated exposures and build their retained pool |
| `runtime.save` | Construct and commit a state object through the ordinary save path |
| `runtime.commit_state` | Compare and swap that constructed state object |

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
