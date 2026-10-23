# From a need to a local result

Run `./personas` from the checkout to open the [interactive launcher](interactive-launch.md). It guides initial installation, sign-in, persona profiles and setup, or reopens saved work. For an existing setup, `./personas '…'` and `ai-personas run '…'` start foreground work directly. Later needs reuse the same personas and retained state. Calls, scheduling rounds, participant scans, context, storage and tool output have no application quota.

![Three scenes: a person gives a fictional game-night request at a desk; three distinct persona workspaces select material from separate own-voice fragment and supporting-asset shelves; a shared handoff reaches a local inbox, beside a bookmark for paused work. The figures are conceptual, not people with lived experience.](assets/need-to-result.svg)

[Open the full-size illustration](assets/need-to-result.svg). The scene is conceptual; it does not depict a recorded run or promise participation, delivery, or improvement.

## Explicit command-line setup

Build with Rust 1.88 or later (development checks use 1.95.0):

```sh
cargo build --locked --release
./target/release/ai-personas doctor
```

The examples below assume `ai-personas` is on your `PATH`; otherwise use `./target/release/ai-personas` from the checkout.

Prepare a [`DirectSettings`](../src/provider.rs) JSON file with the selected access mode, credential references, exact account, model and effort. Catalogue, effort and capability observations are optional; saved transport limits are accounting metadata. [Direct provider configuration](direct-provider.md) explains the three authentication routes. Setup validates the configuration without resolving credentials or performing inference.

Replace the path placeholders below. The working directory must exist; its canonical path is retained as setup metadata. It may be the same as, contain, or be inside the state directory. Relative paths and symlinks resolve to the actual directory. Host commands default to the foreground process's working directory and may select another directory. `--destination` records the processing destination.
```sh
ai-personas --operator local-operator setup \
  --provider-config /absolute/private/provider.json \
  --working-directory /absolute/working/directory \
  --destination selected-model-destination
```

Setup saves three continuing personas by default. Use `--cohort NUMBER` to choose a positive starting population; there is no application maximum. `--persona-seeds FILE` supplies attributed starting profiles. Sharing and Direct execution need no additional approval flags. Creation seeds assert no prior experience. State defaults to `$XDG_STATE_HOME/ai-personas`, or `$HOME/.local/state/ai-personas`. If you choose another location with `--state-root`, pass that same flag on subsequent commands.

## Give a need, then read or continue

For example, a fictional planning request:

```sh
ai-personas run 'Draft a short guide for a fictional tabletop game night. Leave the date and guest count open.'
```

Every cohort member receives a neutral invitation to consider or contribute voluntarily. Personas choose responsibilities, arrangements and peer requests; affected participants choose whether to accept their exact terms. Native work automatically supplies delegation and admits actual adopted peer requests, without a grant or selection file. Independent Direct calls can overlap across parent and descendant tasks. The output prints a work ID. Substitute that actual ID for `WORK_ID`:

```sh
ai-personas result WORK_ID
ai-personas result WORK_ID --json
ai-personas run --resume WORK_ID
```

`result` performs no inference. It shows accepted messages and local delivery content with attribution and qualifications. `run --resume` continues eligible work using the saved identities and provider route. Explicit Stop/Pause and unresolved calls retain their recorded meaning; use the corresponding controls when a participant needs resumption or a call needs recovery.

Foreground work continues while an eligible route or pending command can advance. There is no 12-step limit, 128-participant cutoff, lifetime call quota or delegation-depth cap. A wait with no current progress returns to the shell. The initial accepted character receives a recorded handover, and later characteristic revisions retain fresh-decision attribution. Actual provider and host outcomes, including unknown outcomes, are retained.

## Read the outcome literally

| JSON status | What it establishes |
| --- | --- |
| `delivered-unfinished` | A permitted local result exists; work remains unfinished. |
| `delivered-acceptance-unestablished` | Permitted evidence does not establish required human acceptance; it may be absent or inaccessible. |
| `completed-as-recorded` | A permitted completion record exists; current global support is not re-established. |
| `paused` / `ended-unfinished` | A lifecycle boundary, not successful completion. |
| `withheld` | A retained result is unavailable in this projection; inspect its stated reason. |
| `intake-incomplete` / `unfinished` | Intake or work remains incomplete. |

Local delivery means a deposit in this application's inbox, not external delivery, human attention, or independent verification of quality. Model-authored judgments remain attributed judgments. Elapsed time and repeated calls do not guarantee maturation; a justified no-change decision is valid.

Human acceptance is optional unless explicitly required when starting the need:

```sh
ai-personas run --require-acceptance 'Draft the fictional game-night guide for my explicit review.'
```

Required human acceptance governs that completion claim. Record the human's actual decision separately, binding the current work authority/version and exact retained release and delivery. `result WORK_ID --json` provides historical `human_decision_pointers`; `status` exposes current work. See the [operator reference](../RUNTIME-REFERENCE.md) before using:

```sh
ai-personas --state-root /actual/saved/state --operator local-operator status
ai-personas --state-root /actual/saved/state --operator local-operator \
  stakeholder-decision exact-human-decision.json
```

This command neither invents consent nor resumes or completes work. Explicit `run --resume WORK_ID` can allow a fresh eligible consideration beyond the acceptance boundary; it does not supply acceptance itself.

## A text reading of the picture

1. **Request at the desk.** A human supplies a need and its intended scope. The runtime retains that request.
2. **Three separate workspaces.** Each persona reasons with its accepted character and own voice. It discovers and selects exact fragments and supporting assets for later calls. Tools, skills, knowledge, data and history remain supporting assets. Stored, discovered, selected, supplied and used are different states. Useful revision and organization are the persona's choices.
3. **Shared handoff, local inbox, saved bookmark.** Accepted contributions can inform a result. Local delivery, collective completion, human acceptance, and an individual's continuation remain separate. Pausing preserves the work's place; restarting the application is not permission to replay an uncertain call or erase responsibilities.

See the [current design](https://github.com/ai-personas/ai-personas-design/tree/cdb82bf), [operator reference](../RUNTIME-REFERENCE.md) and [remaining implementation gaps](execution-resources.md#remaining-work) for further detail.
