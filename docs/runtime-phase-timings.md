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
| `store.open_runtime` | Validate the historical settings chain, open SQLite and optionally recover an unowned Store |
| `store.encode_changed_object` | Encode one complete changed object to canonical JSON, borrowing its body |
| `store.fingerprint_changed_object` | Hash those complete encoded bytes for the returned exact reference |
| `store.validate_capacity_chain` | Read and validate the exact historical settings receipt chain |
| `store.open_connection` | Open SQLite, establish its schema/indexes and verify controlling metadata |
| `store.try_recovery_owner` | Try to acquire the execution owner for normal opening recovery |
| `store.recover_dispatched` | Run the existing recovery transaction, including scanning and applying any recorded transitions |
| `store.scan_recovery_invocations` | Read invocation records, collecting dispatched calls and handling recorded prelaunch rejections |
| `store.scan_recovery_intents` | Read intent records and collect dispatched intents |
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
| `store.read_object_bytes` | Read complete current object bytes and validate cell metadata |
| `store.fingerprint_object_bytes` | Hash all bytes of the current object record |
| `store.reuse_observed_object` | Charge the prior canonical result size for an identical current read |
| `store.decode_observed_object` | Decode a new or changed object record |
| `store.measure_observed_object` | Measure that decoded object's canonical navigation charge |
| `runtime.binding_head_unchanged` | Reuse this driver's validation of identical current bytes and compare its expected configuration |
| `runtime.validate_binding_head` | Fully decode and validate changed state before comparing configuration |

These are elapsed wall times, not CPU time. Nested phases overlap: do not add
`runtime.load` to its children or `runtime.save` to the serialization and commit
phases it contains. Events appear in completion order; sequence identifies start
order within one process. Serialization and decoding can also occur outside the
ordinary load/save methods. Separate atomic admission and adoption transactions
do not pass through `runtime.commit_state` merely because they save state.

Store opening and recovery phases precede the first ordinary runtime load.
An inspection-only connection does not recover; ordinary opening tries to acquire
the existing execution owner before recovery. An `Ok` owner attempt can mean
either acquisition or contention, so only subsequent scan events establish that
recovery ran. Scan events do not themselves establish that a record needed or
received a transition. They retain the existing recovery behavior and accounting.
The Store and runtime share one process-local timing switch and sequence.

Recovery streams each existing namespace in key order through one SQLite cursor,
instead of listing its keys and issuing a separate point read for every record.
Each current record still passes the same cell metadata, physical decoding and
typed JSON checks. Pending replacements take precedence over durable rows, and
visitor changes remain pending until the existing transaction commits. The scan
does not collect all request bodies in memory or skip completed records. Recorded
prelaunch handling, dispatched-candidate collection and later transitions retain
their existing order; the cursor does not create permission to replay an effect.

This is focused instrumentation of state handling, not a complete profiler of
scheduling, provider transport, host effects or storage. Use repeated loads and
their measured phases to locate costs in actual retained work before attributing
a slow session to the provider. Logging and unrelated concurrent work can affect
timings. No performance improvement or causal comparison follows from one run.

Live session and native-provider binding checks read the complete current object
bytes in a normal Store transaction, including cell metadata checks. Here,
object bytes mean the complete logical record returned after physical storage
decoding, before JSON parsing. Each read independently validates the physical
encoding and cell metadata; the identity does not hash the physical storage
envelope. A private read identity binds the lookup key, SHA-256 of every logical
record byte, and the canonical returned-byte charge measured when that object
was decoded. A driver retains it only after typed state and intake validation
succeed and configuration matches the driver's expected value.

An identical subsequent read can omit JSON parsing, canonical remeasurement and
typed state decoding. It still hashes all current bytes, checks current cell
metadata, compares the expected configuration and charges the complete canonical
result size once. Changes anywhere in the record require fresh decoding and
validation, even if the revision or configuration text is unchanged. A different
JSON encoding also takes that path. A physical representation change alone may
reuse validation if the newly validated, decoded logical bytes are identical.
Missing or malformed records and changed configuration retain their actual
errors or mismatch; no failed check replaces
the remembered successful validation. Each driver starts without this metadata.
It stores no retained body, adds no persistent index and shares nothing with a
different driver.

This reuse applies only to configuration binding checks. Ordinary state loads,
preparation, adoption, ownership and freshness checks retain their existing
behavior. The current read remains mandatory, so neither an old revision number
nor a cached success alone establishes current state. The navigation charge is
the existing logical canonical-result charge, not physical I/O, tokens or money.
Earlier builds emit `runtime.fingerprint_state` while parsing and fingerprinting
the complete stored object on every binding check; current timing phases separate
raw reads, byte hashing, decoding and reuse inside `runtime.check_configuration`.

Store transactions also count ordinary records and their history slots, checking
those totals against controlling metadata. A covering SQLite index on `lane` and
`slots` lets this query read the required fields without scanning record bodies.
The counts are recomputed in each transaction's current snapshot; they are not
cached across transactions. SQLite maintains the index with each cell change.

Opening a database creates the index if absent, including for an existing Store.
The index uses additional SQLite pages and has an initial construction cost. It
does not add logical Store records or change object bytes, canonical navigation
charges or accounting checks. Query plans and actual command timings can show
the indexed path's use; the index alone does not establish a whole-session speedup.

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

Runtime creation, atomic launch bootstrap and later state-object construction
take the packed JSON value directly from the same constructor used by State's
general serialization adapter. They do not serialize an already packed value
through another value serializer to rebuild the complete tree. The constructor
still performs expanded serialization and exposure packing, with the same phase
names, complete fields, pool identities and reference locations. General
serialization delegates to that constructor before writing to its requested
serializer; stored-object consumers take ownership of the constructed value.
Creation and atomic bootstrap release the temporary typed State once that value
has been constructed, before entering the Store transaction.

This removes a redundant value traversal and allocation, not the retained
history, Store cloning or distinct constructions required by later transactions.
Object revisions, exact fingerprints and commit checks still cover the complete
stored body. A successful construction event is not a commit receipt, and this
source change alone establishes neither a speedup nor a peak-memory reduction.

Store change validation borrows each immutable candidate from the caller's
change batch instead of cloning its complete body into a candidate map.
Dependency checks fingerprint that borrowed candidate directly. Dependencies
outside the batch still load the current stored object in the same transaction.
The borrowed view ends with the batch; it does not cache validation across
transactions or omit current-head, revision, duplicate or exact-reference
checks. This removes temporary copies without changing committed bytes, retained
history or accounting. Actual latency and memory effects require observation.

Store admission, adoption and evidence persistence no longer serialize JSON
bodies or complete objects merely to discard the bytes from historical size
checks. These inputs already contain typed JSON values. Required fingerprints
and durable record encoding still serialize the complete content, propagate
errors and participate in the same atomic transaction. Identity, revision,
dependency, terminal authentication and duplicate checks remain in place. This
removes obsolete quota preflights without changing canonical records or actual
allocation accounting; it does not establish a measured performance improvement.

Runtime snapshots and corpus views share each immutable file revision through
an `Arc<FileRevision>`. Copying a State or building its corpus copies those
handles instead of every retained file body. A new file revision gets a new
record; current heads, retirement, bindings and exposure remain separate state.
Each corpus construction still validates complete record identities, digests,
paths, qualifications and policy consistency, and measures the same logical
record bytes. Sharing is not a validation cache or a reason to skip current
binding and lineage checks.

The storage adapter writes the same compact file bytes and metadata. Public
file serialization and revision hashes retain their existing content. Each
state decode constructs its own records; no process-wide interning or persistent
pointer identity is introduced. This removes copies within an in-memory
snapshot and its derived views, while observation payloads, exposure manifests
and other State fields retain their existing ownership. Actual memory and
latency effects still require measurement.

Applying a Store object change encodes its complete canonical JSON once, borrowing
the body through the same serializer used by standalone object fingerprinting.
The immutable revision and current head retain separate copies of those bytes;
the returned exact reference hashes that encoding directly. The revision
conflict check still precedes encoding, and the head write still checks its
namespace, existence and allocation metadata. Each logical record retains its
own accounting and physical storage treatment within the same atomic commit.

The two change phases above measure encoding and hashing only. They exclude the
byte-buffer copy, cell accounting, physical encoding and database commit. Their
successful completion does not prove a commit. General record encoding and
other state processing remain separate; eliminating these repeated JSON-tree
copies and serializations alone establishes no whole-session performance gain.
