# From a need to a local result

After one explicit setup, launch from the checkout with `./personas` and enter one need, or use `./personas '…'`. The [launcher](launcher.md) uses an existing installation and saved settings; it performs no automatic setup or access renewal. The underlying `ai-personas run '…'` interface remains available. Each command works in the foreground within finite limits, then returns a result or explains why it stopped. Later needs reuse the same personas and their retained state.

![Three scenes: a person gives a fictional game-night request at a desk; three distinct persona workspaces select bounded pages from separate own-voice fragment and supporting-asset shelves; a shared handoff reaches a local inbox, beside a bookmark for paused work. The figures are conceptual, not people with lived experience.](assets/need-to-result.svg)

[Open the full-size illustration](assets/need-to-result.svg). The scene is conceptual; it does not depict a recorded run or promise participation, delivery, or improvement.

## Once: verify the profile and save the cohort

Build with Rust 1.88 or later (development checks use 1.95.0):

```sh
cargo build --locked --release
./target/release/ai-personas doctor
```

The examples below assume `ai-personas` is on your `PATH`; otherwise use `./target/release/ai-personas` from the checkout.

First prepare a verified [`DirectSettings`](../src/provider.rs) JSON file. It selects one access mode, explicit credential references, immutable account and route capability evidence, exact model and effort, and finite request/stream limits. [Direct provider configuration](direct-provider.md) explains the three separate authentication routes. Configuration does not establish access by itself; setup neither resolves credentials nor performs inference.

Replace the two path placeholders below. The working directory must already exist and be separate from private state; it is compatibility metadata and is not read or transmitted by the provider. `--destination` names your approved processing destination. Read the [transport limits](direct-provider.md) before opting in.
```sh
ai-personas --operator local-operator setup \
  --provider-config /absolute/private/verified-provider.json \
  --working-directory /absolute/operator-approved/checkout \
  --destination approved-model-destination \
  --share-cohort --allow-direct-provider
```

Setup performs no inference. It saves three continuing personas by default and explicitly permits sharing within that cohort and with the requester. Add `--cohort 1` to work with a single continuing persona; the supported range is one to eight. Creation seeds assert no prior experience. State defaults to `$XDG_STATE_HOME/ai-personas`, or `$HOME/.local/state/ai-personas`; keep it outside repositories. If you choose another location with `--state-root`, pass that same flag on subsequent commands.

## Give a need, then read or continue

For example, a fictional planning request:

```sh
ai-personas run 'Draft a short guide for a fictional tabletop game night. Leave the date and guest count open.'
```

Every cohort member receives the same neutral invitation to consider or contribute voluntarily. The runtime assigns no owner or reviewer. Personas propose responsibilities and decision arrangements within the existing delegation; affected participants must accept their exact terms. The output prints a work ID. Substitute that actual ID for `WORK_ID`:

```sh
ai-personas result WORK_ID
ai-personas result WORK_ID --json
ai-personas run --resume WORK_ID
```

`result` performs no inference. It shows currently permitted accepted messages and local delivery content, with attribution and required qualifications. `run --resume` explicitly continues eligible work within the saved allowance. It does not replay a call, reset resources, or override a persona's Stop, work pause, failed call, unresolved transport, or character-change fence. A previously accepted ordinary continuation remains subject to its existing eligibility rules.

By default, one foreground command attempts at most 12 steps and considers at most 128 participant entries. Work, saved-cohort and storage limits also apply. A wait returns to the shell; there is no background daemon or automatic retry. The initial accepted character receives one recorded handover. Later character changes retain their fresh-decision fence and any earlier accepted ordinary continuation; stops and recovery boundaries still require their existing explicit controls. Inspect a held or uncertain attempt before deciding on recovery; another `run --resume` is not a universal repair command.

## Read the outcome literally

| JSON status | What it establishes |
| --- | --- |
| `delivered-unfinished` | A permitted local result exists; work remains unfinished. |
| `delivered-acceptance-unestablished` | Permitted evidence does not establish required human acceptance; it may be absent or inaccessible. |
| `completed-as-recorded` | A permitted completion record exists; current global support is not re-established. |
| `paused` / `ended-unfinished` | A lifecycle boundary, not successful completion. |
| `withheld` | Current permissions do not permit this result view. |
| `intake-incomplete` / `unfinished` | Intake or work remains incomplete. |

Local delivery means a deposit in this application's inbox, not external delivery, human attention, or independent verification of quality. Model-authored judgments remain attributed judgments. Elapsed time and repeated calls do not guarantee maturation; a justified no-change decision is valid.

Human acceptance is optional unless explicitly required when starting the need:

```sh
ai-personas run --require-acceptance 'Draft the fictional game-night guide for my explicit review.'
```

Required human acceptance gates the affected completion claim; it does not force independent authorized improvement to stop. A real human decision must be recorded separately. `result WORK_ID --json` supplies historical `human_decision_pointers`, not a complete authorized acceptance request. An operator must inspect the current work authority/version with `status`, bind the exact retained release and delivery, and supply the human's actual statement and acceptance or rejection. See the [bounded stakeholder input contract](../RUNTIME-REFERENCE.md#foreground-cohort-workflow) before using:

```sh
ai-personas --state-root /actual/saved/state --operator local-operator status
ai-personas --state-root /actual/saved/state --operator local-operator \
  stakeholder-decision exact-human-decision.json
```

This command neither invents consent nor resumes or completes work. Explicit `run --resume WORK_ID` can allow a fresh eligible consideration beyond the acceptance boundary; it does not supply acceptance itself.

## A text reading of the picture

1. **Request at the desk.** A human supplies a need and its limits. The runtime records that request without pretending the illustrative figures have lived experience.
2. **Three separate workspaces.** Each persona reasons with its accepted character and own voice. It can request bounded file discovery and choose exact permitted fragments and supporting assets for later calls. Tools, skills, knowledge, data, and history remain supporting assets. Stored, discovered, selected, supplied, and used are different states; shelves are not loaded wholesale. Useful revision and organization are discretionary, under the same authority and limits.
3. **Shared handoff, local inbox, saved bookmark.** Accepted contributions can inform a result. Local delivery, collective completion, human acceptance, and an individual's continuation remain separate. Pausing preserves the work's place; restarting the application is not permission to replay an uncertain call or erase responsibilities.

For the deeper design, see the [illustrated guide at canonical revision 17f165b](https://github.com/ai-personas/ai-personas-design/blob/17f165b156ee5962e290a7e2326bba3f81ad2069/VISUAL-GUIDE.md) and [detailed baseline runtime limits](../RUNTIME-REFERENCE.md). This quickstart describes the current bounded local workflow, not unattended production operation or full design conformance.
