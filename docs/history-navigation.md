# Continuing a history query

The Store archive adapter now reads complete inventories and exact archives
without fixed record-count or aggregate byte quotas. Runtime cursor retention,
result projection and context fitting also have no application count or byte
quota under [the current requirement](execution-resources.md).

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
