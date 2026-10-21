# Exact file references from the current input

An ordinary initialized admission may supply `CONTROL.input_file_choices`.
Each entry pairs a short handle with an exact file reference already identified
in that input. A handle includes the invocation identity, for example
`call-00000042:f3`. It resolves only against that admission's retained table.
It never means a current head, a file path, or a similar source name.

The `input-file-choices-v2` interface supports these uses:

| Field | Result |
|---|---|
| `next_assets` or `next_fragments`, with `kind: replace_from_input` and `handles` | Select the exact references for later context under the normal scope, lifetime and admission checks. |
| `file_write.qualification_handles` | Add the chosen exact references to the write's literal `qualifications`. |
| `file_import.qualification_handles` | Add the chosen exact references to an imported asset's literal `qualifications`. |
| Host-command `inputs.stdin_asset` with `kind: input_file` and `handle` | Pass the exact full UTF-8 supporting asset as command stdin without copying its body into the response. This does not select model context. |
| `communicate.reference_handles` | Attach canonical `file_references` to the deposited message. |
| An authored `reply.reference_handles` | Attach the same chosen references to each authorized local reply recipient. |

The output fields are optional and default to empty. Prefer them when the
desired references are listed, rather than retyping source identities and
revision hashes in qualifications or prose. The host preserves authored message
text and stores the resolved references separately. JSON result views include
`message.file_references`; the text result renderer prints those references
under a separate metadata label. Persona recipients receive the same structured
metadata and may choose to read or select it in their own later admission.

A message can attach at most 32 references. Literal qualifications and
`qualification_handles` together retain the existing 256-reference bound, and
duplicate exact qualifications are rejected after resolution. Handles are unique
within each field. A selection lifetime does not become an output-reference
lifetime: attachments retain the exact source, revision, representation and
range, with `lifetime: null`. Source permission and expiry rules still apply.
The canonical qualifications already identify the file's supporting sources;
authored prose can refer to those qualifications without copying their hashes.
A supporting-file save does not itself require registration of a formal work
artifact, release, delivery or completion judgment.

Handles cannot identify a new file proposed in the same response. Such a file
still uses `proposal-op:<exact FileWrite operation id>` where that existing
reference form is supported. Dependent work still needs the appropriate accepted
save or execution observation; an attachment creates no receipt or result.

## Evidence and authority

An attachment is a persona-selected citation. It supplies no body and proves
neither reading, correct interpretation, factual support nor requester
acceptance. File qualifications retain their ordinary exact binding and current
permission checks. Messages retain their full input lineage, with current
recipient and source checks before deposit and later disclosure. The host never
silently repairs an unknown handle or malformed literal reference.

The table contains at most 128 entries and 32768 serialized bytes and is omitted
if it cannot fit beside required context. Candidates come only from current host
projections and qualified entries. These include owned tool allocation guides,
retained tool-result references and structured citations in received local
messages. Arbitrary document text, nested history and quoted JSON are not parsed
as reference offers. Offering metadata loads no additional file body and does
not choose context, execute a tool or add funding. Its bytes and source
restrictions enter the normal admission accounting.

For an exact retained tool-result reference, `tool_result_origins` repeats the
tool family, producing invocation, operation, intent and recorded state already
present in the current host projection. This puts the producing invocation next
to the handle so the persona can distinguish several retained results from the
same tool. Table order does not identify the newest or requested result. The
metadata supplies no result body, and absent historical origin metadata remains
absent during inspection.

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
