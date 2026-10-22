# Native work commands and supporting guides

Every ordinary admission supplies the complete current native work-command
interface in `CONTROL.work_capability.inputs_schema`. No work-command grant,
guide registration, selected guide or guide-body read is required to expose it.
`work_command.inputs.kind` names a supplied command; the `effect` form also
cites the admitted interface's exact `binding_revision`. Complete-response,
exact-record and current work-state checks still preserve meaningful outcomes.

Guides remain optional supporting assets under `runtime/work-commands/`.
Reading a guide supplies its actual text; discovering a reference supplies only
metadata. Neither action executes a command, establishes successful work or
changes a retained selection automatically. Primitive file, communication and
maintenance operations remain in the application Decision schema. Initialization
and selection-repair modes remain separate from ordinary work; their remaining
restrictions are tracked in [execution resources](execution-resources.md).

Current command instructions accompany the interface under
`command_instructions`. `native_command_readiness` separates actual guide-body
supply from actual schema supply. A current command can be supplied while its
guide body is absent. Historical inputs keep their original interfaces and
supply records; removal of today's gate does not rewrite those inputs.

Fresh setup installs the supporting guides. Installation and refresh append
exact versions and record actual operator provenance. Existing guides can be
kept for their text; an old statement that guide selection grants callability
is superseded by the current interface. Their protected-source mutation rules
and installation approval fields remain separate implementation work.

## Accepting a supplied responsibility offer

An ordinary `responsibility` operation names an exact offer from
`CONTROL.own_pending_offers` and the current work:

```json
{
  "kind": "responsibility",
  "id": "COPY_THE_SUPPLIED_OFFER_ID",
  "work_id": "COPY_CONTROL_WORK_ID",
  "disposition": "accept",
  "reason": "I accept this supplied responsibility."
}
```

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
not a prerequisite for native command use. The existing installation CLI still
uses the fields below; its approval and request-size restrictions remain pending
removal under UR-052.

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
