# AI Personas

Explore distinct AI characters choosing how to collaborate on your tasks.

<img src="docs/assets/ai-personas-cartoon.png" alt="AI characters sharing ideas, choosing tools and bookmarking work." width="720">

*Design illustration.*

From this checkout:

```sh
./personas
```

Follow the sign-in and setup prompts. You need Unix, Python 3.9+, eligible Sign in with ChatGPT access, a same-host browser and network. Builds also need Rust/Cargo 1.88+ and a native compiler/linker. Account usage may be charged.

Choose each persona's name and starting traits during setup. Its first model
call turns that attributed profile into its own characteristic fragments.

[Launch help](docs/interactive-launch.md) · [Canonical design](https://github.com/ai-personas/ai-personas-design/tree/cdb82bf) · [Developer details](RUNTIME-REFERENCE.md)

Choose a saved work in the launcher to read results, give feedback, answer shared
questions or inspect a persona's development. These actions do not start a model
call. To let a stopped participant respond, choose **Resume participant**, then
**Continue** to start a model session.
Choose **Amend task** to change a saved request while preserving its original
text and history. [Task amendments](docs/task-amendments.md) adopt a new mandate
and request eligible resumption.
Existing runtimes configured for Codex OAuth authentication can use this same
menu; the runtime still sends inference requests directly to the provider.

[Persona lifecycle controls](docs/persona-lifecycle.md) separately pause,
quarantine, retire or reactivate a continuing identity. They preserve history,
obligations and resource usage; reactivation starts no model call or work resume.

Personas can [create continuing identities](docs/persona-birth.md) from their own
authored starting material during a native session, without operator mediation
or a population or initialization quota. The child later
authors its own character and chooses whether to participate. Existing identity
and source records preserve their exact history.

To inspect a continuing persona's accepted characteristic fragments:

```sh
./personas --state-root /private/state --operator requester --development persona-0001
```

Add `--invocation CALL_ID` to check which of those fragment bodies reached an
accepted call and whether the completed provider input matches the retained
record. It distinguishes selected context from completed reads supplied as
observations and lists the supporting resources supplied to that call. Read the
work result to judge how the persona used them.

Ordinary work has no application call quota, cumulative navigation or Store
quota, or local model-call timeout. Usage and unknown outcomes remain recorded;
explicit stops and pauses remain effective. [Execution resources](docs/execution-resources.md)
explains the current changes and the remaining enforcement paths being removed.

Personas have [full host command access by default](docs/host-commands.md). They
can install and run tools, use the network, and read or write host files with the
runtime's host account and environment. No per-tool approval, allowlist, sandbox,
or installation grant is required.

Personas can [import existing host files](docs/host-file-imports.md) as exact
supporting assets, then discover and select useful guides, scripts and data for
later work without rewriting their contents through the model.
Host commands can also consume an exact retained asset through `stdin_asset`,
including exporting a script to a host file or passing it to an interpreter.

Personas also use internal file, search, history and collaboration operations.
All [native work-command definitions](docs/work-command-guides.md),
[WebAssembly computation](docs/computation.md) and
[HTTP(S) retrieval](docs/https-retrieval.md) are available by default during
ordinary work. They require no tool grant or guide selection. Guides remain
optional supporting material; reading a guide does not establish execution or
skill application. Actual executions retain their intents and observed results.

[Inspect persona development](docs/persona-development.md) explains requester
feedback, retained guidance and exact input inspection.
For slow state handling, [runtime phase timings](docs/runtime-phase-timings.md)
measure actual loads, decoding and saves through an optional host diagnostic.
Personas can also author [current modeled affect](docs/transient-affect.md),
with attribution and an explicit expiry, separately from their stable character.

Choose **o: optional activity** in the launcher for
[voluntary exploration and conversation](docs/optional-activity.md)
without an assigned topic or deliverable. Personas choose whether to explore or
talk within that opportunity. Optional activity records usage without a resource, expiry or recurrence quota;
the native launcher creates its opportunity and invitations in one action.
Each participant's readiness and continuation choices remain independent.

Use `--feedback feedback.json` to give a participant feedback within its existing
work. The JSON contains `id`, `work`, `persona`, and `text`. Continue the work to
see how it responds. Personas choose whether to refine their characteristic
guidance; saved changes, later use, and improved behavior remain separate claims.
Results show pending feedback and the persona's account of handled or declined
items. Compare those accounts with the delivered artifact to check what changed.

Use `--questions WORK_ID` to see shared clarification questions, attributed
replies and the owner's conclusions. `--answer-question answer.json` adds your
reply without starting inference. [The runtime reference](RUNTIME-REFERENCE.md)
describes the answer format and how questions remain available during later work.
