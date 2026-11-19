# Numerical workspaces

Open the desktop with `./personas --gui`. Choose **New workspace**, give each persona a name and a starting character profile, select a working directory and connect the reasoning provider. Creating the workspace performs no inference. Choose **+ New → Task** or **Social conversation**, select invitees, then **Run controls → Continue eligible work**.

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

Use `neural appraisal-context WORK_ID PERSONA_ID` to inspect the appraisal that would be available to a new decision. When the persona selected a contextual appraisal, the view identifies that decision’s exact input and the latest later exchange from each correspondent in the same activity. Incoming correspondence leaves the old calculation intact until the persona chooses a new appraisal. A changed versioned dependency makes the next input use baseline. The Studio can open the later exchanges from the appraisal inspector.

In **Character Lab → Development**, choose **Numerical paths** on an accepted decision. Its original words and judgment sit alongside the twelve inclinations that reached the reasoning model. Choose an inclination to see its bias, direct input terms and hidden-unit contributions. **Compare another package** evaluates the selected persona’s retained weights on that same appraisal. An unknown input and a known input with value zero have different known-mask columns. A hidden unit can be active while its connection to the selected output has weight zero.

The persona can inspect its own decisions with `persona.paths`. The operator can use `neural character-paths DECISION --axis clarification --compare CANDIDATE` and add `--hidden-unit INDEX` to inspect a disconnected unit. The comparison runs locally. A difference in numerical outputs still requires separate observation of the ensuing choice and its consequences.

When a persona defers a question, the next decision receives the exact revision written and its current revision. Reading an older question preserves its original body and shows where to find the current one. The persona can keep an inquiry open while it gathers evidence. In **Development**, **Open the question this choice retained** opens the historical revision written by that choice.

## Reusing work and restoring a society

Personas can list the operator-selected directory with `workspace.list` and import exact bytes with `asset.import`. A retained guide, skill or program may declare its own type and the exact resources it adapted. Searches expose metadata first. A subsequent read, requested text excerpt or explicit context selection supplies the content.

An import with identical bytes to another current asset reports the existing registration count. When there is one match the receipt includes its name, exact version and qualifications. `neural catalog --same-content EXACT_REFERENCE --history` lists matching content registrations across revisions. Each asset keeps its own provenance and qualifications.

Use `neural catalog TERM --history` to find a previous name, with optional `--author` and `--work` revision filters. `neural find EXACT_REFERENCE PATTERN --regex` locates text in that specific version and returns line numbers and half-open UTF-8 byte ranges. Add `--before N` or `--after N` to include nearby lines in the selection. Pass `--excerpts` to receive those lines in the same result. Overlapping selections on the current page share one returned excerpt. Personas use the same searches through `asset.search` and `asset.find` with the optional `excerpts` flag.

A `compute.run` operation accepts an exact asset containing compiled WebAssembly (`.wasm`) or its text form (`.wat`). Use `compute.inspect` or the Studio’s module viewer to inspect the declared imports, exports and signatures. The module exports `memory` and `run(i32, i32) -> i64` under `compute.wasm.v1`. The result identifies the module bytes, input digest, engine and host architecture. The Studio activity panel opens both the exact program and its result.

For OpenSCAD projects, a persona can choose `geometry.install` to install the published `openscad-wasm` 0.0.4 module in its workspace. Installation verifies the SHA-256 and runs the compiler’s version probe. The host needs a Node.js executable with `--permission` on its PATH. The first install can download the pinned module or take an exact retained copy. Later renders and restores use the installed bytes offline. [Published package](https://www.npmjs.com/package/openscad-wasm/v/0.0.4)

`geometry.render` takes an exact source revision, optional companion assets at their library paths, and literal OpenSCAD variable overrides. It builds the project in compiler memory. The result preserves the supplied file digests, parameters, compiler arguments, Node options and environment-key list, diagnostics, generated dependency file when available, and measured STL output. A compiler can emit warnings alongside a usable mesh. `geometry.inspect_stl` measures existing mesh bytes independently of the original render. Bounds are in model coordinates. The source defines their intended unit.

`geometry.history` finds actual receipts by exact source revision or mesh bytes, including re-registrations under another name. Each result identifies its author, origin, source, output, diagnostics and compiler warnings. STL inspection includes the first page of this history. Additional pages are available by offset. The operator CLI exposes the same query as `neural geometry-history REFERENCE`.

Use `asset.register` to give an observed output object a library path, qualifications and exact sources. A registered mesh can be delivered and exported alongside its editable project. The corresponding operator commands are `neural install-openscad`, `neural render-scad` and `neural inspect-stl`. The operator render returns its own retained receipt.

With `image.render_svg`, a persona can render an exact retained drawing and inspect the PNG in its following decision. The rendering receipt identifies the SVG revision, renderer, pixel dimensions, image digest and count of omitted external image references. The character-use record binds the actual ordered image inputs to the provider call. Deliveries also retain their original brief. An amended brief is visible when reviewing or exporting an earlier result.

An active persona can inspect its numerical package and the appraisal inputs connected to its outputs in `identity.appraisal_structure`. A connection describes the stored weights. The forward evaluation establishes its effect at a given input.

For a matched comparison, `persona.assess` accepts either a hypothetical situation and appraisal or an exact earlier `replay_decision`. In a replay the runtime verifies the original provider receipt, recovers the brief and incoming exchanges, and uses the appraisal supplied at the time. The persona selects which previously supplied resource bodies to include. Each arm receives the same selected images. The optional `include_no_character` adds one provider call without numerical inclinations. The comparison retains each returned judgment, proposed action, communication and uncertainty with its own receipt.

Inspect a replay locally with `neural replay-input experience:call-ID@1 --resource object:DIGEST`. Verify the original responses, numerical forward evaluations and shared inputs of a completed comparison using `neural verify-assessment assessment:ID@1`. Both commands work offline.

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
