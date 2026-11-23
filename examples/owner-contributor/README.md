# Can a second person take over the outside step?

**Facilitator’s protocol.** Run once with two different people, one activity owner and one outside contributor. The earlier [pickup](../../docs/field-packet-trial.md) and [shift](../../docs/shift-intake.md) trials used evaluator-operated windows. This trial measures what people discover and where they need help.

**Current stage: waiting for participants.** The [station check](preflight.json) opened both native windows on the same workspace without sending input. All 114 events, 69 invocation records and 67 stored provider outputs were unchanged. The observer was armed at event 114; neither person has issued a brief or taken a request for this trial. Time to complete, difficulties and exported bytes have not been measured.

### Set the desks

Use packaged native Studio `8e956cf`, direct Codex OAuth, `gpt-6.1-sol`, medium effort. Restore the [BC-17 closing archive](../solar-bond-strip/decisions.json) with the two continuing residents into a separate workspace with an empty shared project. Start the [private epoch observer](../stitch-desk/watch_epochs.py) **before** opening either desk. Keep the task cards, study notes, exports and private archives outside the personas’ project.

The prepared local Linux station is linked at `target/owner-contributor-desk`. From the checkout, run `sh examples/owner-contributor/open-studio.sh owner` for the first person; give them only the [owner card](owner.md). At the shift change use `sh examples/owner-contributor/open-studio.sh contributor` for the other person and give them only the [contributor card](contributor.md). Keep the owner's listener window open. Both windows open the same workspace at its ordinary entry point; the launcher supplies only the local operator label and host settings. Two labels or two windows do not establish two participants.

### Observe one handoff

1. Start at Overview. The owner enters one brief in their own words. Record the actual invitees and brief. Do not coach the choice of menus, persona, collaborator, checks or deliverable.
2. When a persona opens a relevant outside request, record its edition, open time and lack of a claimant or report. Leave it unanswered for at least one minute before bringing in the contributor. Record any actual work, issue, wait or stop during that interval. If the request is withdrawn, never opens, or no file is issued, keep that result. Do not create the missing request or start another commission.
3. Observe the second person finding the request, taking it, reporting an intermediate milestone, exporting the issued files, and opening the PDF in another reader. Distinguish a progress update from a completion claim. Ask the owner to find the contributor’s update in Studio and say what remains. Do not resume a stopped persona to make it acknowledge the update.
4. Use the station’s `observation/attempts.tsv` (a copy of [the observation sheet](observation.tsv)), one row per attempt. Mark **unaided**, **self-help** (e.g. consulted a guide), **assisted**, **blocked**, or **not reached**. Record where help was wanted, what the person expected, the exact help actually given and what happened next. Ask each participant what was confusing, if anything. Distinguish direct observation from participant recollection. If a participant stops, keep the incomplete journey. Finish the listener after the session, or at 35 minutes from the invitation; record censoring and let admitted work settle.

### Check the record

- Match the native invitation, requesting persona’s accepted decision, unanswered interval, claimant and progress against `neural intake-history` and `neural request-correspondence`. Use one request ID across its revisions. Compare the owner’s readback with the report, including anything the contributor explicitly left pending. Distinguish a saved report from its receipt by a persona. Record whether the two people actually operated their own desks; actor labels cannot answer that question.
- Keep the contributor’s original **on-disk download** and issuing reference. Compare its length and SHA-256 with the immutable issued file. For a field packet, compare the enclosed original members at their issued paths and the printable directions against the exact request edition; the wrapper ZIP is a different object. Record the actual reader and visible rows. Independently inspect the unmodified PDF for one A4 page, the four named cases and space to enter expected and actual counts. A matching digest alone does not establish legibility.
- Audit `neural usage` and the [stored terminal response and input bindings](../stitch-desk/receipt_audit.py) for the new work and any activities it generated. Preserve rejected choices, partial delivery, waits and failures alongside successes.
- After draining, end the observer with the station’s `finish-observation` file. Check every switch. Only the first eligible voluntary post-initialization numerical change with **both** authentic active histories qualifies for the [already registered comparison](../stitch-desk/protocol.json); run its unseen brief once per state under the matched conditions. Otherwise keep it unused.

The observation concerns this handoff on one prepared desktop. Report where each person needed help even when the bytes and the persona’s work check out.
