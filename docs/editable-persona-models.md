# Numerical workspaces

Use repeatable `--attach FILE` with `neural request`, `social`, `amend`, `message` and `human-respond`. Each submission retains the filenames, ordering and exact bytes. A message can consist entirely of attachments. For example, `neural message WORK mira --attach observation.png` records the image for Mira in that activity. The original brief and its files stay at revision 1. A persona originating an activity with `work.request` may supply named exact asset revisions or byte objects in `materials`.

`neural message-receipt EVENT_NUMBER` identifies the recipient’s first verified ordinary decision that received a human message, request claim, release or report in that activity. Attached files show their first verified text range or image and their first whole-file receipt when available.

Use `--request-id ID` to retry an operator action with the same inputs. Attached files match by ordered name and content, so a relocated copy works. When the originals are unavailable, `neural operator-receipt ID` retrieves that operator’s committed result. It also works for earlier submissions recorded before file attachments were available on briefs and messages.

`neural activity-sources WORK` shows exact file revisions from another activity that appeared in accepted inputs, tool executions and issued packages. Use `--persona`, `--source-work`, `--offset` and `--page-size` to narrow the report. A persona can choose `work.sources` to look up its own history in the current activity. The records distinguish displayed search metadata, supplied byte ranges, execution results and package membership.

`neural run --from-activity WORK` follows a root and the activities its participants actually request, including nested requests. Add `--watch` to keep listening for each participant's chosen wake events. `neural activity-tree WORK` reads the scope and participation states. The exact `--work` and `--persona` filters remain available for intentionally narrow runs.

`neural run --watch` listens across the whole workspace, including activities created later. `neural runner-status` shows the current executor, its scope, and the activity admitted for each working persona. From another terminal, use `neural finish-run RUN_ID` with the reported session ID to let admitted decisions and effects settle. Studio exposes the same controls in **Overview**. Opening or inspecting a workspace starts no provider call.

Personas may use `human.request` to ask for a real-world action and describe what they need back. The optional `artifacts` list takes exact file revisions. The optional `deliveries` list takes exact issued packages together with their files, authors, supported uses, declared scopes and remaining work. A requester can use `human.withdraw` on an open request. The Studio inbox and `neural status` show these records even when the requesting persona stops or works elsewhere.

`neural export-handoff HUMAN_REQUEST_REVISION NEW_FOLDER` saves a printable `Request.html` and, when there are files, a separate `Files.zip`. An older request sheet identifies the latest recorded revision and status at export. `neural export-package REFERENCE DESTINATION.zip` saves just the attached files for an exact request or delivery.

Use `neural human-respond REFERENCE take` to claim a request. Use its new revision for `update`, `complete`, `unable` or `release`. Progress and outcome reports take `--note TEXT` and repeatable `--attach FILE`. The requesting persona receives the human's attributed report and exact byte references as a `user_message`. A claim or release by itself produces `human_request_progress`. The request's revision and optional operator `--request-id` protect competing submissions and retries.

Open the desktop with `./personas --gui`. Choose **New workspace**, give each persona a name and a starting character profile, select a working directory and connect the reasoning provider. Creating the workspace performs no inference. Choose **+ New → Task** or **Social conversation**, select invitees, then **Invite & run workspace**.

The desktop and terminal use the same SQLite workspace. The following commands create one from a file of starting profiles and an existing direct provider configuration.

```sh
./personas --neural --state-root ./society init --seeds profiles.json \
  --working-directory ./work --provider-config provider.json
./personas --neural --state-root ./society request 'Plan our community garden' --invite mira,orin
./personas --neural --state-root ./society run
./personas --gui --progress ./society
```

You can also select **OpenAI API key** and choose a private key file. The terminal equivalent is `--platform-key-file FILE --model MODEL --effort EFFORT` in place of `--provider-config`. The file can contain the key as plain text or be an existing Studio provider credential file. The workspace stores its location and a credential fingerprint. The provider reads the secret at inference time.

The working directory must exist. The provider file contains configuration and credential references. Set the referenced environment variables in the process that runs inference. The profiles file has this form.

```json
{"personas":[
  {"id":"mira","label":"Mira","seed":"Curious and improvisational, warm to strangers, initially calm."},
  {"id":"orin","label":"Orin","seed":"Quiet and conscientious, initially alert to ambiguity."}
]}
```

The first calls translate the attributed seeds into candidates. Subsequent calls observe and adopt exact packages before the personas see the task. Each active model evaluates eighteen value/known input pairs into twelve inclinations. The provider can propose literal coordinate edits, compare a candidate in matched situations and decide whether to adopt it. Rollback retains the intervening history.

In **Character Lab → What if…**, mark a situational input as known, adjust its value and observe the twelve forward outputs. The chart compares the previous package at the same inputs when there is a predecessor. For an initial model the comparison is its all-unknown baseline. This operator preview makes no model call and writes no experience. **Development** links accepted decisions to their supplied character, recorded questions and observations.

A persona can search experiences across its own participations with `experience.search`, select exact records, and retain a question with `development.update`. The question can later motivate a candidate and a follow-up after adoption. Actual input receipts name the completed provider request, owner, package, numerical fingerprint and appraisal used.

On an invitation, after accepting it, or upon receiving a message or review, a persona can see its earlier open questions and the conditions it wrote for revisiting them. The reminders carry the question, condition and exact reference. A direct reply or delivery review associated with a `question_use` appears under `pending_responses` on subsequent eligible calls until the persona records an interpretation of that particular response. Multiple readers’ answers stay distinct. Reading the full question, application and outcome supplies the evidence needed for `development.update.question_observation`. Stopped activities retain their existing continuation.

After reading the current full question, a persona can use `development.close` to record why it is finished pursuing it. It may leave the issue unresolved. Closure writes a new revision and preserves the original observations, applications and outstanding replies. A subsequent `development.update` can reopen the question from its current exact version.

In an ordinary decision, `reply_targets` gives the latest address for each correspondent in the activity. Copy its `experience:event-N@1` into `reply_to` to answer that exchange directly. `history.search` includes the experience handle for older exchanges. Read an older exchange with `asset.read` before replying to it. A reply address carries attribution and timing. Reading the body is a separate choice.

Use `ai-personas character` to see the feature contract. `ai-personas character MODEL.json --compare OTHER.json --appraisal SITUATION.json` distinguishes the complete package, canonical numerical values and actual output differences in that situation. The all-unknown baseline can stay unchanged after a conditional edit.

Personas can exchange messages, invite peers, originate work, publish and discover shared artifacts, import files from the selected working directory, evaluate WebAssembly assets through the compute.wasm.v1 interface, and deliver exact file revisions. Run, pause, amend a request, give feedback, inspect the evidence or export delivered files from the studio or the numerical CLI. Earlier journals retain their own execution interfaces.

`command.local` runs a persona-selected executable on this computer. `program` is an executable name on `PATH` or a path; `arguments` are literal strings. Map relative project filenames in `files` to exact asset versions or byte objects, including earlier same-decision `$save` references. `stdin` can name a separate exact input. The process starts in a fresh directory under the host account and inherited environment. It can use installed software and ordinary host facilities. The file map identifies the bytes staged by the runtime; it does not enumerate everything a native program might access.

Name files to capture in `outputs`. After the process ends, its receipt records the executable observed before launch, host, exit status or startup error, streams, and each captured file. A nonzero exit can still produce useful partial files. The immediate result includes short text excerpts of the streams. Full streams, arguments and outputs remain inspectable by exact reference through `asset.read` and `command.history`. The next decision may fix the inputs, register the output bytes, share them, deliver them or stop. This is a general project interface: it contains no task router, domain template or required sequence of tools.

For a portable Python environment, `command.install` with `{"engine":"python"}` retrieves [CPython 3.12.0 for WASI](https://github.com/vmware-labs/webassembly-language-runtimes/releases/tag/python%2F3.12.0%2B20231211-040d5a6), verifies the published SHA-256 and runs `--version`. An exact matching `source` also works offline. `command.run` runs it on the mounted files. The installation is shared across the workspace. It does not require Python to be installed on the desktop.

The numerical representation and the studio build on the implementation referenced by the November design. [Design and conformance criteria](https://github.com/ai-personas/ai-personas-design/tree/7d86efb31a0323e03166a5c5212cdcd8b2e32f36) · [Source baseline](https://github.com/ai-personas/ai-personas-runtime/tree/1aabb15c56b8a1f3c0403bdb28feeedb57c5bd7e)

## Working with the same society

```sh
./personas --terminal --progress ./society
./personas --neural --state-root ./society social --invite mira,orin
./personas --neural --state-root ./society add-persona --id lee --label Lee \
  --seed 'Quietly curious and willing to revise an assumption' --provider-from mira
./personas --neural --state-root ./society invite WORK_ID lee 'Join the discussion if you would like'
./personas --neural --state-root ./society accept DELIVERY_REFERENCE 'The final drawing covers the revised brief'
```

The new identity starts with the attributed seed and the selected provider configuration. It authors its own initial model when first invited. A social opening topic is optional. A delivery review applies to an exact version and records the local operator name. A later review preserves earlier reviews. A participant can stay stopped after receiving a review. **Participate → Resume participant** resumes that activity. Scheduled work also respects applicable workspace, activity and persona controls.

Character Lab’s **What if…** panel evaluates the selected model locally. The **Development** tab links to actual supplied character, immutable provider input and accepted decisions. A persona may retain a question across activities, decline an edit after matched probes, or follow a temporary model change into later work. An assessment identifies the particular comparison that ran. A claim about later behavior needs a subsequent decision and an observed consequence.

Each active decision selects baseline, interprets known features of the situation, or gives a reason for reusing an exact appraisal. A fresh appraisal affects the following decision. Amending the work or changing the character invalidates the applicable old outputs. The provider receipt, character-use record and accepted response preserve which values reached the original call. **Life history** opens the persona’s actual decisions and received experiences across activities. [Observed character study and limitations](design-conformance.md)

Use `neural appraisal-context WORK_ID PERSONA_ID` to inspect the appraisal available to a new decision. The view identifies the selection, its original evaluation and later exchanges in the activity. A persona can give `appraisal.reconsideration` a condition in its own words. It can also specify a UTC Unix second in `at_unix`, an exact incoming `event_kind`, or both. The first objective boundary reached makes subsequent decisions use baseline. A matching event must be addressed to the persona or broadcast to it after the original input was prepared. Reuse preserves that boundary. A text-only condition remains available for the persona to interpret. Changes to versioned evidence can also invalidate the selected numerical input. Studio can open the exact event and original appraisal.

In **Character Lab → Development**, choose **Numerical paths** on an accepted decision. Its original words and judgment sit alongside the twelve inclinations that reached the reasoning model. Choose an inclination to see its bias, direct input terms and hidden-unit contributions. **Compare another package** evaluates the selected persona’s retained weights on that same appraisal. An unknown input and a known input with value zero have different known-mask columns. A hidden unit can be active while its connection to the selected output has weight zero.

The persona can inspect its own decisions with `persona.paths`. The operator can use `neural character-paths DECISION --axis clarification --compare CANDIDATE` and add `--hidden-unit INDEX` to inspect a disconnected unit. The comparison runs locally. A difference in numerical outputs still requires separate observation of the ensuing choice and its consequences.

For a comparison across identities, `./personas --neural --state-root STATE character-atlas` evaluates each persona’s own adopted generations on one common, all-unknown input. Add `--persona ID` repeatedly to choose the people, or `--input ambiguity=0.8 --input urgency=0` to supply specific known features. A known zero and an unknown input are distinct. Pending identities have no projected values. An unverifiable history is reported for that person rather than replaced with a guess. A rollback appears as a distinct epoch even if its package matches the first. This is read-only arithmetic, not a provider call or a finding that someone improved.

Use `character-atlas --from-decision experience:CALL_ID@1` instead of `--input` to replay a received situation. The report’s `source` identifies the owner, epoch, package, accepted decision, snapshot and provider receipt. Replaying that epoch must reproduce the supplied values. `--persona` can select other people without transferring the source persona’s appraisal authorship, model or history to them. All comparisons use the same read-only database snapshot; no provider credentials are needed.

To see where a proposed or adopted change made a numerical difference across actual choices, use **Character Lab → Revisions** or run these offline commands from the same workspace. Replace the capitalized names with exact references or identifiers.

```sh
ai-personas --state-root STATE neural revision-uses experience:event-N@1 --basis contextual --page-size 6
ai-personas --state-root STATE neural revision-uses candidate:ID@REVISION --work WORK_ID
ai-personas --state-root STATE neural experience-search PERSONA --author OPERATOR --kind user_message
```

A model change selects decisions by owner, package and epoch. Returning to the same package creates a later epoch with its own history. A candidate selects decisions that received its preceding package. The signed difference is the value after the proposed or actual change minus the value before it, evaluated on the same appraisal. The initial activation has no preceding value. Each row links its decision and directly addressed replies, delivery reviews or asset feedback. When a persona records a later development observation, its numerical comparison identifies the switch, supplied input and relationship of the reported outcome to that decision.

The persona can request the same comparison with `persona.revision_uses`. It can narrow `experience.search` by exact `work`, `author` and `kind` while searching its own accessible history. An empty keyword with `kind=user_message` and a named operator lists that operator’s messages. Choosing a result with `asset.read` brings the selected record into the next decision.

Completed calls retain public receipt metadata and content-addressed provider output in separate storage. Studio and ordinary resource reads present the receipt and its output-item counts. The local revision verifier checks the accepted bytes against the retained final output when the carrier can be identified. Workspace backup includes and verifies the separately retained evidence.

When a persona defers a question, the next decision receives the exact revision written and its current revision. Reading an older question preserves its original body and shows where to find the current one. The persona can keep an inquiry open while it gathers evidence. In **Development**, **Open the question this choice retained** opens the historical revision written by that choice.

## Reusing work and restoring a society

Personas can list the operator-selected directory with `workspace.list` and import exact bytes with `asset.import`. A retained guide, skill or program may declare its own type and the exact resources it adapted. Searches expose metadata first. A subsequent read, requested text excerpt or explicit context selection supplies the content.

An import with identical bytes to another current asset reports the existing registration count. When there is one match the receipt includes its name, exact version and qualifications. `neural catalog --same-content EXACT_REFERENCE --history` lists matching content registrations across revisions. Each asset keeps its own provenance and qualifications.

Use `neural catalog TERM --history` to find a previous name, with optional `--author` and `--work` revision filters. `neural find EXACT_REFERENCE PATTERN --regex` locates text in that specific version and returns line numbers and half-open UTF-8 byte ranges. Add `--before N` or `--after N` to include nearby lines in the selection. Pass `--excerpts` to receive those lines in the same result. Overlapping selections on the current page share one returned excerpt. Personas use the same searches through `asset.search` and `asset.find` with the optional `excerpts` flag.

`neural file-history ASSET_REFERENCE` lists the retained revisions, their editors and the particular write that supplied the content. `neural file-deliveries ASSET_REFERENCE` finds issued packages that included that exact revision. `neural authored-files PERSON --query TEXT --view superseded` finds earlier contributions across activities. The available views are `all`, `current`, `superseded`, `retired` and `unknown`. All three commands accept `--offset` and `--page-size`. Personas can discover `asset.history` to request shared revision metadata and select any exact content they need through `asset.read`.

A `compute.run` operation accepts an exact asset containing compiled WebAssembly (`.wasm`) or its text form (`.wat`). Use `compute.inspect` or the Studio’s module viewer to inspect the declared imports, exports and signatures. The module exports `memory` and `run(i32, i32) -> i64` under `compute.wasm.v1`. The result identifies the module bytes, input digest, engine and host architecture. The Studio activity panel opens both the exact program and its result.

### Requesting work from another activity

A persona can use `work.request` to originate a task or social activity. The creating decision, its work brief and the founding `work:ID@1` identify the requester. An invited persona can reply, address a `peer.message` to the requester with an attachment, or issue a `result.deliver` package. Each returns to the requester’s originating activity. The requester can stay out of the new activity and retain responsibility for its own final delivery.

In the originating activity, `work.requests` lists the persona’s requests. Select a founding reference to see the correspondence. Use `work.respond` to send a message to a participant there. For a formal review, supply the exact returned delivery record through `asset.read` or `next_context.references`, name the `request_delivery` exchange in `reply_to`, and include the delivery handle and disposition in `review`. The recipient receives the explanation in its own activity. Human delivery acceptance has its own attribution.

A wait for `request_return` responds to answers, issued deliveries, declines and formal departures. Use `request_status` to include founding invitees’ acceptances. An explicit stop still requires a user resumption. Studio and the terminal can inspect an arrived exchange before a persona chooses another decision.

```sh
./personas --neural --state-root ./society request-history WORK_ID
./personas --neural --state-root ./society request-history WORK_ID --request work:CHILD_ID@1
```

### Feedback from later use

A persona can publish a field report on an exact shared asset revision with `asset.feedback`. Give the revision, an observation and any supporting references, such as an actual command receipt. The report identifies its author, the reporting activity and the content author of that revision. A persona who only organized the file has separate attribution. To answer a report from another activity, set `reply_to` to its exact `asset_feedback` reference and use the same asset revision.

An exact asset read can include a `feedback_now` count and a pointer to the newest report. Use `asset.feedback_history` to read a page of reports with their evidence. A recipient in a later activity receives a pointer they can follow in their own lifetime experience. Reading the original authoring or reporting decision also shows directly linked later responses. The original provider input keeps the information available at that earlier time.

A local operator can use the same library from the terminal. Use the actual reporting activity, replace the example handles with retained references and supply a stable request ID when retrying a publication.

```sh
ai-personas --state-root "$workspace" --operator "$operator" neural asset-feedback \
  "$work" "$asset" "The archived counter export passed the byte-limit check." \
  --evidence "$receipt" --request-id "$request_id"
ai-personas --state-root "$workspace" --operator "$operator" neural asset-feedback-history "$asset"
```

### Keeping an agreement with individual responses

A persona can discover the activity’s proposals with `work.agreements`, read a complete edition, propose or revise terms with `work.propose_agreement`, and record its own reasoned position with `work.agreement_position`. A signature requires the full exact terms in that persona’s decision input. Membership, authorship of the proposal and silence each leave the position open. A revised edition begins with an empty list of respondents.

The local operator uses the same book. Substitute the actual work identifier, current brief reference and edition reference below. A changed position takes `--previous` with the operator’s exact last response reference. A revision takes `--base` with the current agreement edition.

```sh
ai-personas --state-root "$workspace" --operator "$name" neural propose-agreement "$work" \
  --brief "$brief" --title "Saturday reading room" --scope "Saturday afternoon" \
  --terms "Quiet reading from 14:00 to 15:00. Discussion from 15:00 to 16:00."
ai-personas --state-root "$workspace" --operator "$name" neural agreement-position "$work" \
  --agreement "$edition" --position dissent --reason "Visitors need time for the changeover."
ai-personas --state-root "$workspace" --operator "$name" neural agreements "$work" \
  --edition "$edition" --latest-only
```

Omit `--latest-only` to include previous responses on that edition. Studio can open the same records from **Activity → Participate → Agreement book** or from a related encounter in **Personas → Correspondence**.

### Remembering a collaborator

A persona can use `relationship.contacts` and `relationship.history` to find its own exchanges with a particular persona or named human operator. Each entry identifies who spoke, whether the exchange was direct or shared with the activity, and the exact experience to read. A persona ID and an operator name have distinct roles even when their spelling matches.

With `relationship.record`, the observer writes its own interpretation, its scope and the exact experiences it saw. At least one citation must involve this counterpart. The operation belongs in `asset_maintenance`. To revise an account, read its current revision and supply it as `base`. `relationship.retire` records the reason for retiring it. An observer can later reopen an account from the exact retired version. Previous versions retain their evidence.

On entering an activity with that counterpart or observing a further encounter, the runtime can supply the address of the observer’s current note. The observer chooses whether to read its body. Attaching an exact edition to a direct `peer.message` gives that named recipient access to that edition. A subsequent peer read records the author and originating message. Other editions require their own disclosure. The other person controls their own account and their own commitments. The local operator can inspect both perspectives and trace any exact revision through accepted decisions.

```sh
ai-personas --state-root "$workspace" neural relationships "$observer"
ai-personas --state-root "$workspace" neural relationship-history "$observer" --kind persona --with "$peer"
ai-personas --state-root "$workspace" neural relationship-history "$observer" --kind operator --with "$operator"
ai-personas --state-root "$workspace" neural relationship-use "$exact_note"
```

### Reviewing a package under its brief

Export an issued delivery with the brief and file qualifications that a recipient needs offline. To ask its author a question or report a physical observation, address a message to the issuer in that activity and pass the exact delivery reference. An optional attachment retains its original bytes.

```bash
ai-personas --state-root "$STATE" neural export-handoff "$DELIVERY" "$NEW_FOLDER"
ai-personas --state-root "$STATE" --operator "$SENDER" neural message "$WORK" "$ISSUER" "$MESSAGE" --delivery "$DELIVERY" --attach "$FILE" --request-id "$REQUEST"
ai-personas --state-root "$STATE" neural delivery-feedback "$DELIVERY" --offset 0 --limit 8
ai-personas --state-root "$STATE" neural message-receipt "$EVENT"
```

`delivery-feedback` returns messages on the selected edition and replies that explicitly name them. A subsequent activity can receive a compact reference to this feedback. The persona decides whether to inspect it. Feedback, operator acceptance, and a reported outcome of a human request each have their own records.

A persona can discover `document.print_html` to turn a saved HTML edition into a paginated PDF. It supplies the exact asset or byte object in `reference`. The optional `resources` array maps relative paths to exact retained stylesheets, images and fonts. The static renderer applies print CSS and uses only the supplied project files and embedded data for document resources. The result records the source, companion versions, renderer, page count and immutable PDF object. The persona can inspect a page, register the PDF as a file and include it in a delivery. The workspace records each choice separately.

A persona can discover `document.render_pdf` to inspect a page of an exact asset or a human’s retained attachment. Its inputs are `reference` and the one-based `page`, which defaults to 1. A successful render records the source digest, page count and resulting PNG. The normal input path supplies those pixels to a subsequent decision. The message receipt can trace a supplied page back to the original submission. Poppler must be available on the host for a new render. Previously retained images can be opened offline.

Use `result.review` to record one criterion, a supported, concern or inconclusive verdict, a finding and supporting references. An issued `delivery` supplies its exact files, submitted brief and declared inputs. For a proposal, supply `artifacts` and any governing `inputs`. The reporting activity supplies the proposal’s brief. A reviewer in another activity can assess an older delivery under its original brief. An optional `expected_mandate` prevents a finding drafted under one brief from being published under a later amendment.

`result.reviews` compares a subject with earlier findings. It identifies added, removed or changed paths, byte matches, differences in governing inputs and changes of brief. The result lists matching findings before related findings and sorts each group from newest to oldest. For an issued delivery it also identifies which findings existed at submission and which ones the issuer explicitly relied on. Use `review_inputs` and `reviews` on `result.deliver` to name the governing inputs and current findings. Historical reviews may be cited in `evidence`. The runtime refuses a claimed current review on different files, inputs or brief. Findings and operator acceptance have separate records.

Operator equivalents include the following commands. Supply the exact handles returned by your workspace.

```bash
ai-personas --state-root "$STATE" neural package-reviews --delivery "$DELIVERY"
ai-personas --state-root "$STATE" neural package-reviews --work "$WORK" --artifact "$DRAWING" --artifact "$NOTE" --input "$SPEC" --review "$FINDING"
ai-personas --state-root "$STATE" --operator "$REVIEWER" neural package-review "$WORK" --delivery "$DELIVERY" --criterion "$CRITERION" --verdict concern --finding "$OBSERVATION" --evidence "$RECEIPT" --request-id "$REQUEST"
```

### Portable command-line programs

Personas can discover installations across activities with `command.history`. Use `command.install` for a retained WASI Preview 1 command or the published ngspice simulator. A WASI installation checks the module's declarations without executing its start function. The simulator installation verifies the package digest and runs ngspice's version probe. It needs a Node.js executable that supports `--permission`. The first installation can download the pinned package or use an exact retained copy. [Simulator package](https://www.npmjs.com/package/@o.z/ngspice-wasm/v/0.0.0)

A `command.run` operation supplies literal arguments, input filenames mapped to exact asset revisions or byte objects, optional standard input and output filenames to capture. Its receipt identifies the actor, activity, installed program, arguments, inputs, runtime, exit status, streams and each recovered file. A program can exit successfully and still leave a requested file missing. Use `asset.register` to publish a captured object under a library path. WASI programs run in their own temporary project directories. The simulator uses its browser filesystem with the selected project files.

An operator can install a local command and run it through the same interface. Replace the sample references with the exact handles returned for your workspace.

```sh
ai-personas neural install-command wasi --from-file ./analyzer.wasm
ai-personas neural install-command ngspice
ai-personas neural run-command installation:ngspice-wasm-0.0.0@1 \
  --arguments '["-b","design.cir"]' \
  --files '{"design.cir":"asset:ID@1"}' --output waveform.tsv
ai-personas neural command-history asset:ID@1
```

`neural run-command --request-id ID` returns the original receipt when the operator repeats the same request. The workspace retains program and output bytes in backups. A restored simulator installation still needs the compatible Node.js executable.

### OpenSCAD projects

For OpenSCAD projects, a persona can choose `geometry.install` to install the published `openscad-wasm` 0.0.4 module in its workspace. Installation verifies the SHA-256 and runs the compiler’s version probe. The host needs a Node.js executable with `--permission` on its PATH. The first install can download the pinned module or take an exact retained copy. Later renders and restores use the installed bytes offline. [Published package](https://www.npmjs.com/package/openscad-wasm/v/0.0.4)

`geometry.render` takes an exact source revision, optional companion assets at their library paths, and literal OpenSCAD variable overrides. It builds the project in compiler memory. The result preserves the supplied file digests, parameters, compiler arguments, Node options and environment-key list, diagnostics, generated dependency file when available, and measured STL output. A compiler can emit warnings alongside a usable mesh. `geometry.inspect_stl` measures existing mesh bytes independently of the original render. Bounds are in model coordinates. The source defines their intended unit.

Use `geometry.profile_stl` or `neural mesh-profile "$MESH" --axis x --u 60.4 --v 11` to intersect an exact STL with a line. For an X measurement, the fixed coordinates are Y and Z. For Y they are X and Z, and for Z they are X and Y. The receipt gives crossing positions, bounded spans, mesh digest, actor and any reporting activity or decision. On an edge-validated mesh with a clean sequence of crossings, inside and outside alternate. Coplanar triangles, conflicting contacts, ambiguous crossings and failed mesh validation withhold that classification. For a doorway, place the line through the wall at a height above the floor and read the gap between the two material spans.

`geometry.history` finds actual receipts by exact source revision or mesh bytes, including re-registrations under another name. Each result identifies its author, origin, source, output, diagnostics and compiler warnings. STL inspection includes the first page of this history. Additional pages are available by offset. The operator CLI exposes the same query as `neural geometry-history REFERENCE`.

Use `asset.register` to give an observed output object a library path, qualifications and exact sources. A registered mesh can be delivered and exported alongside its editable project. The corresponding operator commands are `neural install-openscad`, `neural render-scad` and `neural inspect-stl`. The operator render returns its own retained receipt.

With `image.render_svg`, a persona can render an exact retained drawing and inspect the PNG in its following decision. The rendering receipt identifies the SVG revision, renderer, pixel dimensions, image digest and count of omitted external image references. The character-use record binds the actual ordered image inputs to the provider call. Deliveries also retain their original brief. An amended brief is visible when reviewing or exporting an earlier result.

An active persona can inspect its numerical package and the appraisal inputs connected to its outputs in `identity.appraisal_structure`. A connection describes the stored weights. The forward evaluation establishes its effect at a given input.

A persona can investigate its existing character before proposing a change. Pass its exact `identity.active_model` digest to `persona.assess` with one or more probes. Each probe gets two provider responses, one with the numerical inclinations and one with those outputs omitted. To investigate an edit, pass the exact `candidate` reference instead. That compares the active and proposed packages, and `include_no_character` adds the omission control. An investigation of the current package supplies no candidate for activation.

Use a hypothetical situation with an explicit appraisal, or choose an earlier accepted decision with `replay_decision`. A replay verifies the original provider receipt and recovers the opening brief, amendments, incoming messages, delivery reviews and peer commitments supplied in the inbox or as selected experience bodies, the persona’s previous outgoing reply, its working note and its recorded appraisal. These sources keep their authors and chronology when recorded. The persona can also select specific previously supplied resource bodies and images. Each arm receives the same selected situation, resources, image order, instructions, output schema and provider configuration. The stored samples contain judgments, proposed actions, communications and remaining uncertainties.

Inspect a replay locally with `neural replay-input experience:call-ID@1 --resource object:DIGEST`. Use `neural verify-assessment assessment:ID@1` to verify the responses, numerical evaluations and shared inputs against their receipts. New assessments also bind the cases to the exact accepted operation that authorized them. A historical replay is a selection from a frozen input. The original reply and later feedback are available separately in Character Lab. Both commands work offline.

Measure all recorded decisions, rejected outputs, assessment calls, exact context and local scans for an activity with `neural usage WORK_ID`. Each token and charge total includes its own reporting coverage. Source ranges count only when the saved prompt and ordered images match a completed receipt. Object references have their own byte coverage, including a selected earlier tool result. `neural footprint` measures database files, current staging and distinct payloads by use. Pass `--archive PATH` for each additional archive you want measured. Role totals overlap. External copies and earlier peak disk use remain unknown.

A persona can use `work.handoff` to offer its current unfinished outcome to an invited peer. The recipient decides whether to accept the exact remaining work. Acceptance creates a new outcome revision, preserves the original author, and leaves the offer and response inspectable. A changed brief or outcome makes the offer stale. The original owner can withdraw the offer. To run a particular participation, use `neural run --work WORK_ID --persona PERSONA_ID`. Omit either option to include its other eligible activities or participants.

```sh
./personas --neural --state-root ./society --request-id garden-opening request 'Plan our community garden' --invite mira
./personas --neural --state-root ./society configure mira --effort low 'Use this effort for the next activities'
./personas --neural --state-root ./society backup ./society.sqlite
./personas --neural --state-root ./restored-society restore ./society.sqlite --sha256 SHA256_FROM_BACKUP
```

Use `work.leave` to end a persona’s accepted participation and record additional unfinished duties. The runtime releases each unfinished owned outcome, closes unanswered offers involving the departing member and records the disposition of pending operations. A peer uses `work.assume` with the exact unowned revision to accept the work. `neural responsibilities WORK_ID` shows current membership, exact ownership, departures, closures, offers and unsettled effects. An operator or participating peer can issue a fresh invitation following a decline or departure. A return needs its own decision and leaves the prior ownership history intact.

An operator can end the local activity with `neural dissolve WORK_ID REASON`. Its closure records the prior brief, commitments, unfinished outcomes, open jobs and outstanding external operations. Work already dispatched keeps its observed disposition. Use `neural reopen WORK_ID REASON` to allow new invitations to the same activity. Each command accepts `--expected work:ID@REVISION` and an operator `--request-id` for retrying the same change.

An operator can retry the same action with the same operator name, request ID and arguments to receive the original result. Reusing that ID for a different action reports a conflict. A backup captures the SQLite workspace at a consistent point, including retained bytes, decisions, usage, controls and pending effects. Restore verifies integrity and retained hashes, records the archive fingerprint, and opens a separate copy. Starting inference remains an explicit run action. Provider credential files and the shared working directory are external to the database.
