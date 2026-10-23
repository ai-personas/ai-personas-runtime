# Native work commands and supporting guides

Every ordinary admission makes all implemented native work commands callable
under `CONTROL.work_capability`. Its `invocation_contract` and `binding_revision`
bind the complete current command grammar. No grant, guide registration, selection
or read is required to use a known current command shape. `work_command.inputs.kind`
names that command; the `effect` form also cites the exact admitted binding.
Complete-response, exact-record and current work-state checks preserve meaningful
outcomes.

When native command deserialization fails, the rejection retains the original
error and adds the known command's allowed fields. Missing required fields and
immediate primitive or enum mismatches name their `inputs` field and schema-owned
expected shape or values. For example, an object-valued requester-feedback
disposition identifies `inputs.disposition` and its allowed string values. This
is a diagnostic from the current schema, not a corrected command or a successful
disposition. The complete response stays rejected; an independently admitted
fresh decision may use the hint or obtain a useful guide. Existing failure and
continuation rules remain unchanged, and historical errors are not rewritten.

Detailed definitions are separate context. A complete current registered guide
body, supplied through ordinary selection or an exact-read observation, selects
that command's schema and instructions. `inputs_schema` contains only these
definitions; `null` means none were supplied. `definition_commands` records their
names. An absent definition does not prevent a valid invocation. If the input
shape is uncertain, the primary chooses useful guide context through the ordinary
file interface. There is no semantic task router, fixed default command set or
command-selection quota.

Guides remain optional supporting assets under `runtime/work-commands/`.
Reading a guide supplies its actual text; discovering a reference supplies only
metadata. Neither action executes a command, establishes successful work or
changes a retained selection automatically. Primitive file, communication and
maintenance operations remain in the application Decision schema. Initialization
and selection-repair modes remain separate from ordinary work; their remaining
restrictions are tracked in [execution resources](execution-resources.md).

Selected command instructions accompany their definitions under
`command_instructions`. `native_command_readiness` separates actual guide-body
supply from schema supply and tells the primary it can use a known command or
select the guide for its definition. A metadata link, body-search match or future
selection does not supply a definition in the current call. Historical inputs
keep their original interfaces and supply records.

Fresh setup installs the supporting guides. Installation and refresh append
exact versions and record actual operator provenance. Existing guides can be
kept for their text; older statements that guide selection grants callability,
or that every definition is automatically supplied, are superseded by the
current interface. Refresh installs current explanatory text while preserving
earlier exact revisions and selections.

## Accepting a supplied responsibility offer

An ordinary `responsibility` operation names an exact offer from
`CONTROL.own_pending_offers` and the current work:

```json
{
  "kind": "responsibility",
  "id": "respond-to-maintenance-offer",
  "offer_id": "COPY_THE_SUPPLIED_OFFER_ID",
  "work_id": "COPY_CONTROL_WORK_ID",
  "disposition": "accept",
  "reason": "I accept this supplied responsibility."
}
```

`id` is the persona's unique operation label, as for other operations.
`offer_id` is a separate required reference to the chosen supplied offer.
The runtime does not infer it from the operation label or choose an offer when
it is absent. Earlier retained responses keep their original wire fields;
fresh responses use the current schema.

Omit `terms_revision` or use `null` to bind the exact terms actually supplied in
that invocation. The runtime verifies the retained input, offer identity, terms,
mandate revision and actor against current work. An explicit `terms_revision`
must match; a mistyped digest is not repaired. A guessed, unseen or changed offer
cannot be accepted this way. Declining uses the same binding with
`disposition: "decline"` and an authored reason.

This does not accept automatically or imply arrangement consent. When
`CONTROL.arrangements.current_exact` is present, joining still uses
`accept_arranged_offer` with the exact supplied offer, terms revision and
arrangement.

## Optional guide installation and refresh

Installation is useful for making explanatory source files discoverable. It is
not a prerequisite for native command use. The installation CLI retains the
fields below as attribution; `authorize_installation` is optional metadata and
adds no approval gate.

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

Use the actual snapshot revisions and complete digests. There is no application
request-byte cap. Preview returns the proposed qualified guide bodies and
reserves nothing.

```sh
ai-personas --state-root /private/state --operator requester \
  install-work-command-guides installation.json --preview
ai-personas --state-root /private/state --operator requester \
  install-work-command-guides installation.json --apply
```

Apply requires unchanged fences, execution ownership and quiescent invocations.
An older uncertain call may retain its own persona's lease; it does not block
guide installation when no transport or adoption owns an unfinished operation.
That old call is neither replayed nor reconciled by installation.
The guide sources, catalogue, launch revision and receipt commit together. An
exact request replay returns its historical receipt without changing current
sources or resources. A different request under the same ID is rejected.
New guide definitions require a fresh installation request. A refresh uses a new
profile directory, preserving old files, exact references and their restrictions.
For a current named guide source, refresh appends an immutable revision and
advances its head. It cannot overwrite an unrelated source or rewind an older
revision. Historical sources with earlier opaque IDs remain retained too.
Only complete current registered guide revisions select detailed definitions.
Preserved selections of older guides can be replaced through the persona's
ordinary next-context choices when current definitions are useful. Exact source
identities prevent installation from overwriting an unrelated source. The host
registry connects exact guide references to native definitions; an arbitrary
authored file cannot add a command.

## Inspect actual use

Settings report `selective_work_commands` and `work_guide_directory`. For an
accepted call, the development view's
`resource_use.supplied_native_work_interface` reports the interface actually
present in the retained input: its invocation contract, definition-selection mode,
selected command names, binding and serialized schema bytes. A null schema has
four serialized bytes and supplies no command definition. The supporting-resource entries show the
exact guide bodies supplied through context or read observations. Compare this
with the accepted operations and work result to establish application.
`supplied_input_file_choices.native_command_readiness` counts the readiness
entries actually recorded in that input, including supplied bodies/schemas and
historical grant metadata. Older registered choices without these entries remain explicitly
unrecorded; inspection does not reconstruct their readiness from current state.

Selective supply reduces the command catalogue included in an individual input
when only a subset is needed. Finding and reading guides can require additional
calls, and other required context remains. Schema size alone does not establish
lower total work cost or successful delivery.
