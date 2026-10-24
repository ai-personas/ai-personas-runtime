# Exact native text edits

Use `file_edit` for a correction to an existing native fragment or supporting
asset. It saves a new immutable revision while preserving every byte outside the
authored replacements. Native files are separate from host filesystem files;
use `host.command.v1` for host edits.

The operation names `id`, `scope`, `source`, `path`, the required exact current
`base_revision`, `edits`, `qualifications`, and `policy_id`. Optional
`qualification_handles` use the current input's exact file table. Path, policy,
scope, maintenance accounting and conflicts follow the existing file-save rules.
The source must already exist. A missing, null, stale or same-response base is
not replaced with a newer head.

Choose the operation for the intended change. For a localized correction,
use a short unique passage with `file_edit`. For a complete rewrite, use
`file_write` with the same source, scope and path, the exact current
`base_revision`, and only the complete new `text`. Reproducing an entire old
document as `old_text` adds unnecessary copying and opportunities for a mismatch.

Each `edits` entry contains `old_text` and `new_text`:

- `old_text` must be nonempty and occur exactly once in the immutable base.
  Include unchanged surrounding text to distinguish repeated passages.
- `new_text` is the literal replacement. Empty text deletes the matched span.
  Include the surrounding text again when it should remain.
- All matches are located against the original base before any replacement.
  Edit order does not allow one replacement to become another edit's input.
- Matches must not overlap. Missing, repeated or overlapping matches reject
  the complete response; the runtime does not guess an intended occurrence.
- No whitespace, newline or Unicode normalization occurs. The operation must
  change the base bytes. Reuse an exact reference when no change is needed, or
  use `file_write` when replacing the full content or changing only metadata.

There is no edit-count or text-size quota. The exact-match rules establish which
bytes the persona requested to change. They do not establish that the correction
is useful or accurate. Errors identify the operation and edit index without
copying the match or replacement text into the diagnostic.

A missing-match diagnostic also reports when the replacement already occurs
somewhere in the base. That observation does not prove it is in the intended
location or that an earlier edit succeeded. Inspect the actual current body and
accepted save and communication receipts before retrying. A rejected complete
decision adopts none of its proposed saves, messages or maintenance dispositions;
previously accepted changes and messages keep their actual outcomes.

The revision's `native_file_edit` origin records the persona, invocation,
operation and exact base reference. The accepted response retains the original
edit list; the base remains immutable. Lineage and exposure include that base,
even when its complete body was not supplied to the current model call. Context
attribution describes persona-authored edits with inherited bytes, rather than
claiming that the persona newly authored the entire result. An accepted edit can
supply a [persona-creation proposal](persona-birth.md): creation verifies the
accepted authoring operation and recomputes its bytes from the retained exact
base. The same response can reference the edit's proposal alias with its save
dependency; an earlier accepted edit uses its exact stored revision. An import
alone does not establish primary authorship.

An edit participates in the same atomic save plan as `file_write` and
`file_import`. `replace_from_saved_files`, Communicate and authored Reply can
refer to its exact operation ID. The host binds its accepted full UTF-8 revision
after successful adoption. Other consumers can use `proposal-op:<edit-id>` with
the save dependency required by their interface. Citation metadata and optional
selection do not supply the edited body in the current call.

List the edit in the matching maintenance `change.operation_ids`. Editing a
currently designated own-voice source still requires a coherent `character_adopt`
proposal; later work receives the newly accepted character in a fresh decision.
The initialization-only schema continues to use fully authored `file_write`
operations. No edit bypasses the existing character-adoption boundary.
