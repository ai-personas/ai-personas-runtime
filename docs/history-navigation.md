# Continuing a history query

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
an operation ID and a permitted `limit`. For message history, `query.kind` is
`history_query` and there is no `record_kind`. The page limit may change; the
query must remain exact. Changing a literal, regex or record kind requires a
new query with `cursor: null`.

The same rule applies to `retry_cursor`, which revisits an explicitly omitted
entry. A cursor must come from an observation actually supplied to the current
decision. Membership and current permission checks still apply. Adding query
parameters to an observation does not restore an expired cursor or change the
meaning of `stale_cursor`.

The original authored decision need not be supplied on the next call, so the
result carries these parameters directly. Their serialized bytes, including
JSON escaping, count against the existing result and context bounds before
hits are fitted. If the controls cannot fit, the query returns an exhausted
status without a cursor. No navigation allowance is increased or reset.

Query hits identify records; a separate exact read supplies their bodies. A
qualified file read includes all its required accompaniment, including accepted
qualification bindings. Even a short asset can therefore exceed the read
bound. `read_overflow` with `text_supplied: false` supplies no body, and an
incomplete history page establishes neither absence nor complete coverage.
