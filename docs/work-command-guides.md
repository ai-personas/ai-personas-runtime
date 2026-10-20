# Selecting native work commands

Fresh native setup and bootstrap install small, runtime-authored supporting
files in a profile-specific directory under `runtime/work-commands/`. Personas discover them with the existing
bounded file list and search operations, then read or select relevant exact
UTF-8 revisions. There is no separate catalogue-fetch operation. The directory
locator in `CONTROL.work_guide_discovery` supplies no command schemas or bodies.
Related guide filenames provide search cues for prerequisites. An absent command
definition is a reason to discover relevant guides before concluding that the
capability is unavailable; source and work authority still need their own checks.
If supplied guide bodies are from an older profile or are incomplete, the
capability's `guide_supply` diagnostic identifies those paths and the correction
needed. It does not change selections; preserving the same unavailable bodies
and retrying cannot expose their commands.

Only complete registered guide bodies actually supplied in an admission expose
their command definitions in `CONTROL.work_capability.inputs_schema`. The
runtime prunes definitions that the selected commands do not reference and seals
the resulting interface in that admission. Discovery metadata, old receipts,
permission ancestry and proposed next selections do not expose commands. A read
or selection in one response takes effect only in a later admission; it cannot
authorize another operation in the same response.

`work_command.inputs.kind` must belong to the supplied interface. The legacy
`effect` encoding additionally echoes that interface's exact `binding_revision`.
Both encodings retain all existing work, evidence, source, consent and resource
checks. Reading a guide does not grant responsibility, perform an operation or
establish skill. Primitive file, communication and maintenance operations remain
in the application decision schema. Initialization and selection repair do not
offer work commands. External tool execution uses its separate supplied interfaces
and current source/work permissions and execution grants; the native work-command
interface neither grants it nor establishes its absence.

The guide files have recorded author `ai-personas-runtime`, operator-import
provenance and the launch's existing source policy. The first cohort identity is
their storage owner; this assigns no specialist role or authorship to that
persona. Protected guide sources cannot be rewritten by personas. They consume
ordinary existing storage and can be withheld by current source restrictions.
Withdrawing a source cannot be undone by replaying installation.
Guide source IDs are readable, such as `native-work-guide:put_artifact`. The
separate revision hash still identifies the exact immutable file. Copy the
complete reference returned by discovery; the source name alone exposes nothing.
When `CONTROL.input_file_choices` lists a desired exact guide, the persona can
instead choose its current handle through `next_assets.kind=replace_from_input`
and `handles`. This resolves to the listed immutable reference before the same
selection checks. The handle is valid only in that input. A registration label
in the table is metadata; the command schema still needs actual complete guide
body supply on a later admission. This avoids retyping hashes without selecting
resources automatically or changing the authority checks.

For a currently registered guide already present in that bounded table,
`native_command_readiness` reports whether its complete exact body and command
schema were actually supplied in this admission, and whether the persona has the
work-command grant. The row's reference identifies the exact guide revision.
Its next-action field distinguishes selecting that guide for a fresh admission,
using an already supplied schema under current work authority, reporting a
missing grant, and reporting an unavailable definition. Operation-specific
responsibility, consent, evidence and resource checks remain required; there is
no blanket `ready` or authorized-operation claim.

These details cover only the already offered, permission-filtered choices. They
load no new guide or catalogue, change no selection, and create no continuation.
They share the table's existing 128-entry and 32 KiB bounds and the actual prompt
limit. Initialization and restricted repair do not receive this ordinary table.
Current guide files retain their exact revisions; no guide refresh is needed.

An ordinary input with permitted pending requester feedback or supplied closure
records may also include `CONTROL.work_guide_discovery.control_guides`. These are
at most two exact file links for the commands those host contracts already name:
`resolve_requester_feedback` and `close_goal`. Current source restrictions apply
to the metadata. The links also qualify for current-input file handles, but the
persona still selects relevant resources and their complete bodies must reach a
later admission before any schema is exposed. This does not resolve feedback,
close work, assign duties or add an unselected command catalogue.

Detailed command instructions travel with the commands actually exposed in
`CONTROL.work_capability.command_instructions`. A shared section appears once and
names only exposed commands. For example, artifact-registration details accompany
`put_artifact`, and closure receipt rules accompany `close_goal`. The common
persona instructions retain shared authority, evidence, responsibility, character,
maintenance and complete-response requirements. Freshly installed guide files
also contain their applicable command rules.

Legacy full interfaces retain every command section. An existing registered guide
still exposes the current admitted interface and its current command instructions;
refreshing guide files updates their retained prose separately. The development
view reports `resource_use.supplied_native_work_instructions` from the saved input,
including section scopes and byte counts. Older inputs keep their original prose
and report no separately recorded command-instruction field.

## Existing roots

Existing roots keep their complete legacy interface until the operator explicitly
installs guides. Installation changes future definition supply without changing
current selections, grants, resource limits, spent attempts or continuing work.
It performs no inference. Use the updated native binaries for all operations
after installation: older binaries cannot read the new catalogue state.

```sh
ai-personas --state-root /private/state --operator requester \
  work-command-guides-snapshot
```

Copy the returned runtime and launch references into a request:

```json
{
  "id": "install-native-work-guides-1",
  "operator": "requester",
  "expected_runtime": {"id": "runtime-state-v1", "revision": 1, "sha256": "COPY_FROM_SNAPSHOT"},
  "expected_launch": {"id": "runtime-launch-v1", "revision": 1, "sha256": "COPY_FROM_SNAPSHOT"},
  "authorization_reference": "The operator's actual authorization record",
  "authorize_installation": true
}
```

Use the actual snapshot revisions and complete digests. The request must fit
16 KiB. Preview returns the proposed qualified guide bodies and reserves nothing.

```sh
ai-personas --state-root /private/state --operator requester \
  install-work-command-guides installation.json --preview
ai-personas --state-root /private/state --operator requester \
  install-work-command-guides installation.json --apply
```

Apply requires unchanged fences, execution ownership and terminal invocations.
The guide sources, catalogue, launch revision and receipt commit together. An
exact request replay returns its historical receipt without changing current
sources or resources. A different request under the same ID is rejected.
New guide definitions require a fresh installation request. A refresh uses a new
profile directory, preserving old files, exact references and their restrictions.
For a current named guide source, refresh appends an immutable revision and
advances its head. It cannot overwrite an unrelated source or rewind an older
revision. Historical sources with earlier opaque IDs remain retained too.
Only the current registered guide revisions expose commands. Preserved selections
of older guides may therefore need replacement through the persona's ordinary
next-context choices. Installation refuses to replace withdrawn or restricted
current guides. The host registry connects exact guide references to native definitions;
an arbitrary authored file cannot add a command.

## Inspect actual use

Settings report `selective_work_commands` and `work_guide_directory`. For an
accepted call, the development view's
`resource_use.supplied_native_work_interface` reports the interface actually
present in the retained input: its selection mode, selected command names,
binding and serialized schema bytes. The supporting-resource entries show the
exact guide bodies supplied through context or read observations. Compare this
with the accepted operations and work result to establish application.
`supplied_input_file_choices.native_command_readiness` counts the readiness
entries actually recorded in that input, including supplied bodies/schemas and
missing grants. Older registered choices without these entries remain explicitly
unrecorded; inspection does not reconstruct their readiness from current state.

Selective supply reduces the command catalogue included in an individual input
when only a subset is needed. Finding and reading guides can require additional
calls, and other required context remains. Schema size alone does not establish
lower total work cost or successful delivery.
