# Continuing a history query

History queries and exact reads cover retained records across personas and works
in the current runtime. Record ownership and original message recipients describe
what happened; they do not restrict who can request a historical read. The
original actor, work, sender, recipient, invocation and immutable identity remain
attached. Retrieval does not redeliver a message, satisfy its original recipient's
wait, resume participants or adopt another persona's character.

Choose the record corpus explicitly. `history_query` searches message text and
identity metadata. Work judgments and authored operations belong to
`history_records_query` with `record_kind: adopted_decision`; tool receipts belong
to `record_kind: observation`. Each query result identifies its `corpus_scope`
and `query_scope.record_kind`, including an unsuccessful query. Its completeness
and `no_match` describe only that corpus and matching mode. A message search
does not establish that a correction is absent from decisions, tool results or
files. These annotations do not search another corpus or supply a record body.

`CONTROL.history_retrieval` describes how to follow a citation with a literal
query. For example, an observation query can find the receipt containing a cited
host-result identifier. The query returns metadata, and the persona chooses an
exact record to read on a fresh call. Another record may merely cite the same
identifier, so a match alone does not identify the original source. Unknown or
different resource identifiers need their appropriate discovery method; the host
does not invent a record reference. Peer judgments supply no cited bodies
automatically, and reading a receipt does not verify the claim it supports.

An exact historical host-result read can itself exceed the provider's context.
After an actual context or input-text length rejection, its pending receipt may
be delivered with `status: provider_context_reference` and `body_supplied: false`.
The record retains historical attribution, execution metadata and current
qualifications, while exact field references, byte counts and hashes replace
command, literal stdin and captured output text. Those references are available
for ordinary file reads, measured ranges, next-context selection or host
`stdin_asset`. Repeating a complete history read can request the same large body
again. The immutable observation, failed input and authored selection remain
unchanged. This recovery belongs to the current reader's pending read, even when
the result came from another persona or work; it ends upon accepted consumption.

Prior judgments also carry `citation_lookup` for exact observation, message and
native tool-result identifiers. This matches native record identities rather
than text that merely quotes an identifier. A match offers an exact history
reference directly; an unmatched entry stays unresolved within that declared
corpus. Observation matches also enter the current `input_history_choices`
table, so a persona can choose a short handle instead of retyping a digest.
This lookup is captured for the current input. It establishes neither earlier
body supply nor support for the judgment, and it does not rewrite a bad citation
or inspect other resource namespaces.

The current record-history and message-query profiles use v2. A cursor from the
earlier ownership-filtered corpus requires a fresh query; its old membership
cannot establish coverage of the expanded corpus. Cursor ownership still binds
the reader's actual query observation and admitted continuation. It does not
filter authorship of the records that query can find.

Ordinary inputs can include `CONTROL.history_retrieval.recent_other_work`, a
compact locator for the persona's latest indexed accepted judgment in another
work. Its invocation, work, admission time and per-persona serial identify the
earlier activity. When the retained commit identifies its archive, `reference`
supplies that exact history identity and `input_history_choices` offers a handle.
The persona can read it directly with `history_record_read` or
`history_record_read_from_input`, saving a query and an intervening model call.
The read checks actual body availability and immutable identity; a commit
reference alone does not establish either body supply or current readability.

The literal query remains available when useful, including when commit metadata
is unavailable. It may also match decisions citing the invocation, so the persona
chooses the matching record from the returned metadata before reading its body.
Earlier judgments without recorded chronology are not ranked; an absent locator
does not establish that there is no history.

The locator is derived from the persona's own retained judgment attribution and
one indexed commit-metadata lookup, with actual navigation usage recorded. It
loads no archive, earlier task body or optional selection and makes no relevance,
completion or learning judgment. The persona decides whether to investigate prior
experience, what to retain for later context and whether any maintenance is useful.
Chronological discovery supports continuity without transferring another work's
private focus or resuming its participation.

When a query result, feedback replacement link or other native history metadata
already names an exact record, `CONTROL.input_history_choices` offers a short
handle for it. Use the handle from the current input:

```json
{
  "kind": "history_record_read_from_input",
  "id": "read-useful-earlier-record",
  "handle": "COPY_THE_CURRENT_HISTORY_HANDLE"
}
```

Each table entry includes the unchanged typed reference and `metadata_paths`
pointing to its existing occurrences in `CONTROL`. The table loads no additional
candidate or body, scans no retained collection, and imposes no entry quota.
Handles name observations and adopted Decisions, not file revisions, messages or
query cursors. The persona chooses which record is useful. Table order establishes
no relevance, and a handle from an earlier input is not valid in a later one.

A listed record can already have its body in the current input. In an actual
`history_record_read` observation, `payload.body_supplied: true` means the body at
`payload.record.historical_record` is supplied now. Use that body directly; the
handle does not require another read. A body received only in an earlier input
is not supplied again by listing its reference. Query descriptors and artifact
receipts have their own metadata contracts and do not establish body supply.

Exact qualified text inside a supplied historical record can share the request's
`QUALIFIED_CONTEXT.shared_texts` pool with identical qualified text elsewhere.
Each `text_ref` binds the same exact file reference and complete text; decoding
restores the original bundle. Observation qualifications, qualified-read results
and adopted-Decision accompaniment use this encoding only at their host-rendered
paths. Authored JSON, message text and arbitrary tool payloads remain unchanged.
Historical wrappers, original identities, attribution and qualification roles
remain in place. Sharing does not make historical material a current event or
add it to the operative file-supply set used for capability exposure.

A full read of an original [image capture](image-input.md) can also supply its
retained pixels. `CONTROL.image_inputs` identifies the actual image parts;
query metadata alone does not supply them. This reuses the exact captured bytes
without reading the current host file, and leaves the original receipt intact.

The host checks the handle against the sealed input and resolves the same exact
reference used by `history_record_read`. It then follows the ordinary read path,
including immutable identity checks and fresh body supply. The receipt records
the input handle and its invocation alongside the resolved reference. A direct
typed reference remains available for known records not listed in the table.
Neither route needs a new query merely to read a known record. Historical
metadata inside an actual historical observation can supply a fresh handle;
arbitrary JSON in authored files, messages or Decision bodies cannot.

Native development inspection exposes the actually supplied table and checks
its metadata paths and admitted bindings. That evidence establishes an offered
choice; the adopted operation, exact receipt and later supplied body establish
the separate read and supply events. Reading replaced feedback leaves its
historical status and original payload intact.

The Store archive adapter now reads complete inventories and exact archives
without fixed record-count or aggregate byte quotas. Runtime cursor retention,
result projection and context fitting also have no application count or byte
quota under [the current requirement](execution-resources.md).

Retained observations with unknown additional source lineage remain available
in pending context and history. Their projections explicitly report
`source_lineage_status: unknown`; the original observation and its digest do not
change. Known source references and required qualifications remain attached.
Terminal-bound rejection diagnostics retain their complete actual detail,
including quoted field names. They describe a failed decision, not adopted
instructions or permission to replay its operations.

History query observations include the exact query alongside their results:

```json
{
  "query": {
    "kind": "history_records_query",
    "record_kind": "adopted_decision",
    "mode": {"kind": "literal", "needle": "inventory"}
  },
  "next_cursor": "COPY_THE_RETURNED_CURSOR"
}
```

This excerpt omits the observation's other status and result fields. To request
another page, use that cursor with the same `kind`, `record_kind` and `mode`,
an operation ID and a positive requested `limit`. For message history, `query.kind` is
`history_query` and there is no `record_kind`. The page limit may change; the
query must remain exact. Changing a literal, regex or record kind requires a
new query with `cursor: null`.

The same rule applies to `retry_cursor`, which revisits an explicitly omitted
entry. A cursor must come from an observation actually supplied to the current
decision. Membership and current permission checks still apply. Adding query
parameters to an observation does not restore an expired cursor or change the
meaning of `stale_cursor`.

The original authored decision need not be supplied on the next call, so the
result carries these parameters directly. Their complete serialized bytes, including
JSON escaping, are accounted for without a result or context quota. Requested
page size controls pagination; no navigation allowance is increased or reset.

Query hits identify records; a separate exact read supplies their bodies. A
qualified file read includes all its required accompaniment, including accepted
qualification bindings. Complete qualified bodies are supplied without a local
read-byte cap. Historical `read_overflow` with `text_supplied: false` supplied no
body; those observations are not rewritten. An incomplete history page
establishes neither absence nor complete coverage.

Adopted Decision reads supply the exact authored JSON and the explicit required
qualifications of its original sources. Source ancestry still governs access;
it does not automatically bring every ancestor's text into the result. Required
qualifications retain their complete current closure without a configured
size ceiling. This projection is labeled `authored-decision-qualifications-v2` and does
not reconstruct the original model input or prove execution. Adopted-Decision
query cursors from the earlier projection require an explicit fresh query;
their original observations and archives remain unchanged.

Qualification metadata lookup and bundle expansion are reserved before that
work occurs. Listing returns descriptors without resolving their bodies; a
later read or body search pays its own lookup and projection cost within the
same accounting ledger without a quota.
