# Historical tool declarations

Computation and HTTP(S) tools are available by default in ordinary requests.
Saved grants, guide bindings, attempt declarations, origin sets and expiry dates
remain historical metadata. They do not gate current execution. New work does
not need a grant, authorization update or selected guide.

The existing `grant-computation`, `grant-http-fetch`, `tool-grant-snapshot`,
`update-tool-grant` and revoke commands inspect or append these declarations.
Updates preserve original requests, prior receipts and recorded used counts.
Their exact runtime, declaration and implementation references prevent applying
an update to a different record. Matching update identities return the existing
receipt, without executing anything or resuming a persona. Revocation changes
the historical declaration and does not disable default tool access.

No application update-count, request-size, receipt-size, attempt-increase,
origin-count or seven-day-expiry cap is enforced. The `authorize_update` field
is optional historical metadata. Existing actor, work, exact-reference and
record-shape checks still apply to these metadata operations.

Use the current supplied callable definitions for execution. See
[computation](computation.md) and [HTTP(S) retrieval](https-retrieval.md).
