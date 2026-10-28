# Native work commands and supporting guides

Every ordinary admission makes all implemented native work commands callable
under `CONTROL.work_capability`. Its `invocation_contract` and `binding_revision`
bind the complete current command grammar. No grant, guide registration, selection
or read is required to use a known current command shape. `work_command.inputs.kind`
names that command. The `effect` form selects `capability_id: "work.command.v1"`
and resolves the same retained interface. Neither form includes an authored
`binding_revision`; the runtime checks the admitted binding itself.
Complete-response, exact-record and current work-state checks preserve meaningful
outcomes.

Model evidence, findings, assessments, judgments, shared decisions and releases
have immutable identities within their record type. Their `inputs.id` differs
from the response's operation ID. A later creation using an existing identity is
rejected with the record type and exact supplied ID; it does not replace the old
record. A new judgment retains its own current arrangement and acceptance. The
selected command instructions explain these identities, while existing records
remain available for reference when applicable. A duplicate rejects the whole
response, including any earlier staged commands.

Arrangement assent follows the actual changed scope. Under
`AffectedPartyAssentV2`, changed commitments require their owners' consent,
artifact transfers require both parties, and a result-rule change requires every
old and new decision holder. Contributors with unchanged duties and authority do
not acquire an approval duty merely from participating. A persona can volunteer
for an unowned result and its initially unassigned decision role through its own
exact proposal and consent when no other party is affected. Existing duties,
artifact ownership and decision rights cannot be transferred by naming a peer.
The accepted arrangement records its actual endorsers; it neither claims
unanimity about the artifact nor completes the task. This first-release policy
replaces the earlier development policy; old records are not reinterpreted or
automatically migrated.

`CONTROL.arrangements.outcome_accountability` explicitly lists every current
required outcome with an accepted owner, an ownership gap, or unavailable
acceptance provenance. Owners reference the supplied current commitment's persona
and acceptance ID. An invitation accepted without outcome responsibilities leaves
the outcome unowned. Editing files, contributing work and judging it sufficient
do not change that state. This projection supplies current metadata; it assigns
no role, requires no workflow and establishes neither quality nor completion.

The supplied arrangement metadata links its command guides through
`work_guide_discovery.control_guides`: an exact formation delegation links
`propose_arrangement`, supplied proposals link `accept_arrangement` and
`decline_arrangement`, and an exact current arrangement links
`accept_arranged_offer`. Each link identifies its referring field, exact
registered revision and native file path. It supplies no guide body or schema
and changes no selection, responsibility, consent or decision right. The persona
can read or select a useful guide through the ordinary file interface. An
ownership gap remains a matter for voluntary coordination, not a requirement to
wait for a new user request or a designated external coordinator.

`decide_shared` uses current formal `judge` records supporting its exact candidate
under the adopted decision rule. An ordinary `Decision.work` sufficiency account,
a message, or acceptance of outcome ownership does not create a formal judgment.
A `judge` earlier in the same complete Decision can supply that record. When the
adopted rule lacks support, rejection identifies the shared-decision ID, rule and
qualifying judgment IDs, instead of conflating this with missing ownership,
reviews, delivery or requester acceptance. The response remains rejected as a
whole; the diagnostic creates no evidence or judgment and makes no quality claim.

`put_artifact` registers the complete original bytes of a supporting asset named
by a `utf8` or `bytes_metadata` reference. `record_model_evidence` requires the
artifact's exact supplied text or byte-metadata view and current qualifications.
The evidence receipt identifies that view and sets `body_was_supplied` to false
for metadata. Neither view establishes truth or independent assurance. Binary
inspection can use persona-chosen host tools and images; the original bytes need
not be encoded into a text wrapper or loaded into model context for delivery.

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
first establishes the accepted character, then a fresh decision receives it.
New calls have no restricted selection-repair mode; see
[execution resources](execution-resources.md).

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

## Selective question context

`CONTROL.shared_questions` supplies complete threads for the current work,
outstanding owner review and unanswered questions addressed to the persona.
Older related threads from other work supply their question text, attribution,
status and counts as metadata. `body_supplied: false` identifies this metadata
view. A full view sets `body_supplied: true` for the question itself and reports
`thread_complete` separately. Unavailable replies or conclusions retain explicit
placeholders, set `thread_complete: false` and prevent a current review revision.

When that history is useful, copy the metadata item's `read_command` into a
`work_command` operation's `inputs`. The `read_question` command names the exact
`question_work` and `question` IDs. Its receipt selects the complete current
thread for the next input, including available replies and conclusions and any
unavailability markers. The request
remains pending until an accepted decision receives it; it does not permanently
load that history into later contexts. A new reply requiring owner review still
becomes current automatically.

Reading does not answer the question, conclude it, accept responsibility or
resume its work. Answering requires the actual complete question body;
concluding also requires the exact current review and an account for every
reply. Metadata alone satisfies neither check. Requester inspection continues
to return the complete retained thread, and no historical record is removed.

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

A rejected explicit revision identifies `responsibility.terms_revision` as the
mismatch and explains the existing omitted/null binding option. An offer for a
different persona has its own diagnostic. Neither diagnostic replaces the
submitted value, accepts the offer or executes other proposed operations.

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
