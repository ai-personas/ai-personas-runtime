# Exact file references from the current input

For a literal `ExactRef`, omit `start` and `end` (or set both to null) to use the
complete text. Partial ranges use measured, half-open UTF-8 byte offsets, including
for decoded JSON strings. Do not guess offsets for a newly authored file.
`lifetime` is `work` or `next_call`; omitted/null means `work`. Mandatory own-voice
references use `work`. The response schema lists those values explicitly.

An ordinary initialized admission may supply `CONTROL.input_file_choices`.
Each entry pairs a short handle with an exact file reference already identified
in that input. A handle includes the invocation identity, for example
`call-00000042:f3`. It resolves only against that admission's retained table.
It never means a current head, a file path, or a similar source name.

`context_state.exact_body_supplied` and
`context_state.preserve_keeps_selection` describe separate facts. A current
read or complete host-output field can supply text without selecting it for
the next call. Host-result text fields have individual JSON-string handles,
so a persona can carry a useful stdout or stderr excerpt alongside a later
result without keeping the whole command document. Select those handles with
`replace_from_input`; include any other references still needed because this
replaces the complete optional set. Preserving an empty selection keeps it
empty, and “already supplied” does not establish later body supply.
If no optional choice has been recorded yet, an explicit `preserve` records an
empty selection. Its reason remains attributable to the persona. It does not
load observed bodies, renew lifetimes or remove mandatory own-voice context.

The `input-file-choices-v2` interface supports these uses:

| Field | Result |
|---|---|
| `next_assets` or `next_fragments`, with `kind: replace_from_input` and `handles` | Select the exact references for later context under the normal scope, lifetime and admission checks. |
| `file_write.qualification_handles` | Add the chosen exact references to the write's literal `qualifications`. |
| `file_edit.qualification_handles` | Add the chosen exact references to the edited revision's literal `qualifications`. |
| `file_import.qualification_handles` | Add the chosen exact references to an imported asset's literal `qualifications`. |
| Host-command `inputs.stdin_asset` with `kind: input_file` and `handle` | Pass the exact supporting-asset text as command stdin, including a decoded JSON string or measured UTF-8 byte range, without copying it into the response. This does not select model context. |
| `communicate.reference_handles` | Attach canonical `file_references` to the deposited message. |
| An authored `reply.reference_handles` | Attach the same chosen references to each authorized local reply recipient. |

The output fields are optional and default to empty. Prefer them when the
desired references are listed, rather than retyping source identities and
revision hashes in qualifications or prose. The host preserves authored message
text and stores the resolved references separately. JSON result views include
`message.file_references`; the text result renderer prints those references
under a separate metadata label. Persona recipients receive the same structured
metadata and may choose to read or select it in their own later admission.

The sender's `local_message_deposit` observation lists these same resolved
attachments beside the message identity and hashes. It confirms the actual
local deposit without resupplying the message or attached file bodies. It does
not establish recipient attention or acceptance, or resume a stopped sender.

Message references and combined literal/handle qualifications have no fixed
count cap. Duplicate exact qualifications are rejected after resolution. Handles are unique
within each field. A selection lifetime does not become an output-reference
lifetime: attachments retain the exact source, revision, representation and
range, with `lifetime: null`. Source policies remain historical provenance.
The canonical qualifications already identify the file's supporting sources;
authored prose can refer to those qualifications without copying their hashes.
A supporting-file save does not itself require registration of a formal work
artifact, release, delivery or completion judgment.

Handles identify references from the current input. To select files saved in
the same response, use `next_assets` or `next_fragments` with
`kind: replace_from_saved_files`. Each `files` entry names the exact `operation`
ID of a `file_write`, `file_edit` or `file_import`, with an optional `lifetime`.
The host derives the source and accepted full UTF-8 revision. Use `input_handles`
to retain useful current-input references alongside those saves. The replacement
covers the complete optional set for that scope; imports always belong to assets.

To cite those saves in `communicate` or an authored `reply`, use
`reference_operations` with the exact save operation IDs. Citations attach the
accepted references without reading or selecting their bodies. Literal
`proposal-op:<exact file-save operation id>` references remain available where
supported, including selections of other representations or ranges. See
[ordinary file maintenance](persona-development.md#maintain-fragments-and-supporting-files-during-ordinary-work)
for the save and selection rules. Dependent work still needs the appropriate
accepted save or execution observation; an attachment creates no result.

## Evidence and authority

An attachment is a persona-selected citation. It supplies no body and proves
neither reading, correct interpretation, factual support nor requester
acceptance. File qualifications retain their exact bindings and required
accompaniment. Messages retain their full input lineage and recipient/source
attribution; source policies are provenance, not application access gates. The
host never silently repairs an unknown handle or malformed literal reference.

The complete table has no application entry or serialized-byte cap.
Candidates come only from current host
projections and qualified entries. These include explicitly discovered files,
referenced tool guides and results, and structured citations in received local
messages. The host does not add the complete retained fragment collection or
past tool-result catalogue to each request. Arbitrary document text, nested
history and quoted JSON are not parsed
as reference offers. Offering metadata loads no additional file body and does
not choose context, execute a tool or add funding. Its bytes and source
restrictions enter the normal admission accounting.

When a pending host result uses `result_context: "reference"`, its exact command,
literal stdin and output-string references also receive handles. Select these
to receive the corresponding decoded text in a later context, or read an exact
reference directly. The whole result document remains a separate choice.

For an exact retained tool-result reference, `tool_result_origins` repeats the
tool family, producing invocation, operation, intent and recorded state already
present in the current host projection. This puts the producing invocation next
to the handle so the persona can distinguish several retained results from the
same tool. Table order does not identify the newest or requested result. The
metadata supplies no result body, and absent historical origin metadata remains
absent during inspection.
Decoded fields and ranges of the same immutable result retain that document's
producing invocation and operation metadata.

## Discover resources before selecting their bodies

Use an already known exact reference directly when it is useful. The native
collection spans personas and work in this runtime; discovery is not restricted
to the requesting persona's files or current task. For an unknown fragment or
supporting resource, including a persona-created skill, request native file
discovery by path, literal text or regex. For example, this operation asks for a page of
active supporting files whose native paths contain `reviews/`:

```json
{
  "kind": "discover",
  "id": "find-review-notes",
  "request": {
    "scope": "assets",
    "surface": {"kind": "path"},
    "mode": {"kind": "literal", "needle": "reviews/"},
    "hidden_paths": "include",
    "retirement": "active_only"
  },
  "limit": 20,
  "cursor": null
}
```

The page size is the persona's request, not an application quota. The persona
can choose another size, follow a returned cursor, or change its query. Use
`scope: "fragments"` for characteristic material. Native host-command and
computation results use `tools/results/`; HTTP results use `tools/http-results/`.
Retained-history queries can also locate the original
observations and producing invocations. These operations do not run old tools.

Regex discovery can use `{"kind":"regex","pattern":"(?i)birth|creation"}`.
An omitted `dialect` uses `{"kind":"rust_regex1"}`. Explicit unsupported
dialects remain errors; the runtime does not reinterpret their patterns.

Discovery returns candidates with exact references. Those references can receive
current-input handles, after which the persona chooses a read or next-context
selection. A result is not a supplied body or an automatic endorsement. When
editing, use the exact current base from file-mutation metadata; an earlier
discovered revision does not silently become the current head.

The retained table and provider-bound input must agree before a handle resolves.
Unknown, omitted-table or older-invocation handles are rejected. The original
authored handles remain in the accepted decision; qualifications and messages
retain their exact resolved targets. Native development inspection reports the
actual supplied table version and whether output reference handles were offered.

## Historical compatibility

Historical `input-file-choices-v1` admissions remain readable and continue to
support their original next-context selections. They cannot use the new output
fields with nonempty handles. New helpers do not retrofit v2 authority or
references into old inputs or messages. Empty optional fields preserve the
original serialized forms. Use updated helpers for states containing v2 inputs
or messages with `file_references`; older helpers may reject those records.
