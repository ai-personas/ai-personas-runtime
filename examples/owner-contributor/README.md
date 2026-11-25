# Can a second person take over the outside step?

**Facilitator’s protocol.** Run once with two different people, one activity owner and one outside contributor. The earlier [pickup](../../docs/field-packet-trial.md) and [shift](../../docs/shift-intake.md) trials used evaluator-operated windows. This trial measures what people discover and where they need help.

**Waiting for two independent participants.** Neither person has issued a brief, declared a task role or taken a request. There are no participant timings, difficulties or exported bytes. Two facilitator-operated window checks are recorded below; they are not two participant sessions.

### Set the desks

[Prepare a private Linux station](station.md). From the same checkout, run the commands below **separately** on the visible desktop. Supply the actual station path in each terminal, or export `OWNER_CONTRIBUTOR_STATION` and omit the path. The launcher never writes the brief or starts a listener.

```sh
# Facilitator terminal — leave it open. Verify the selections before continuing.
sh examples/owner-contributor/open-studio.sh arm "/path/to/private station"
# Once armed, give the first person only the owner card and their own window.
sh examples/owner-contributor/open-studio.sh owner "/path/to/private station"
# At the shift change, open the other person's window. Leave the first open.
sh examples/owner-contributor/open-studio.sh contributor "/path/to/private station"
```

[Owner card](owner.md) · [Contributor card](contributor.md). Each window opens at the ordinary workspace entry point. A desk name is attribution, not authentication. Outside the study, [**+ New → Operator desk…**](../../docs/desktop.md#working-under-another-operator-name) also opens a named window; it does not check the study observer, declare a role or take a request.

### Station checks (no participant actions)

- The [earlier station](preflight.json) used packaged Studio `8e956cf` with direct Codex OAuth, `gpt-6.1-sol`, medium effort. Opening the windows left its 114 events, 69 invocations and 67 provider outputs unchanged. That is not an Astra run.
- On a **separate restore**, native Studio `ef24223` on display `:0` opened **Exhibit owner** and **Exhibit volunteer** together at Overview. Both pointed at the same workspace. The pre-admission archive recorded direct Codex OAuth with `gpt-6.2-astra` selected for Mara (`high`) and Oren (`xhigh`). The operator changed two future selections before arming. Notices 115–117 were addressed to Mara in three earlier activities; none was supplied in a new decision. All three participations remained stopped. Mara retained epoch 1 and Oren remained uninitialized.
- Between arming and closing the two windows, all eleven database tables matched, including rowids: 117 events, the same 69 historical invocations, 67 stored provider outputs, no new calls, no observed listener. A different workspace with a copy of the capture marker could not open a study desk. Neither the native window check nor the saved model selections establish a new provider response or human participation.
- On disposable copies, the prior observer could arm on event 117 and, after a folder replacement, finalize an apparently finished observation from a different database ending at **114**. It also accepted a replacement with identical bytes. The observer now retains the opening archive but refuses a completed report in both cases. Separate historical timing controls still distinguished Ren’s single switch from two switches between samples; neither was a new participant decision.

### Observe one handoff

1. Start at Overview. The owner enters one brief in their own words. Record the actual invitees, brief and whether the owner separately declared task-level responsibility. Do not coach the choice of menus, persona, collaborator, checks or deliverable.
2. When a persona opens a relevant outside request, record its edition, open time and lack of a claimant or report. Leave it unanswered for at least one minute before bringing in the contributor. Record any actual work, issue, wait or stop during that interval. If the request is withdrawn, never opens, or no file is issued, keep that result. Do not create the missing request or start another commission.
3. Observe the second person finding the request, taking it, reporting an intermediate milestone, exporting the issued files, and opening the PDF in another reader. Distinguish a progress update from a completion claim. Ask the owner to find the contributor’s update in Studio and say what remains. Do not resume a stopped persona to make it acknowledge the update.
4. Use the station’s `observation/attempts.tsv` (a copy of [the observation sheet](observation.tsv)), one row per attempt. Mark **unaided**, **self-help** (e.g. consulted a guide), **assisted**, **blocked**, **chose not to**, or **not reached**. Record where help was wanted, what the person expected, the exact help actually given and what happened next. Ask each participant what was confusing, if anything. Distinguish direct observation from participant recollection. If a participant stops, keep the incomplete journey. Finish the listener after the session, or at 35 minutes from the invitation; record censoring and let admitted work settle.

### Check the record

- Check separately who issued the brief, who declared a task role (`neural responsibilities`), who took the outside request, and who actually used the exported file. If a role was declared, use `neural task-role-receipts` for that exact edition before claiming it reached a persona decision. A claimant need not be a declared task contributor, and a requester need not be the declared owner. Record an absent or declined role as such.
- Match the native invitation, requesting persona’s accepted decision, unanswered interval, claimant and progress against `neural intake-history` and `neural request-correspondence`. Use one request ID across its revisions. Compare the owner’s readback with the report, including anything the contributor explicitly left pending. Distinguish a saved report from its receipt by a persona. Record whether the two people actually operated their own desks; actor labels cannot answer that question.
- Keep the contributor’s original **on-disk download** and issuing reference. Compare its length and SHA-256 with the immutable issued file. For a field packet, compare the enclosed original members at their issued paths and the printable directions against the exact request edition; the wrapper ZIP is a different object. Record the actual reader and visible rows. Independently inspect the unmodified PDF for one A4 page, the four named cases and space to enter expected and actual counts. A matching digest alone does not establish legibility.
- Audit `neural usage` and the [stored terminal response and input bindings](../stitch-desk/receipt_audit.py) for the new work and any activities it generated. Preserve rejected choices, partial delivery, waits and failures alongside successes.
- After draining, end the observer with the station’s `finish-observation` file. Check every switch. A provider setting, initialization, practice note or contextual appraisal is not an eligible revision. Only consider the [registered held-out comparison](../stitch-desk/protocol.json) for the first freely adopted post-initialization numerical change with **both** authentic active histories, under the matched conditions for that resident. Otherwise leave it unused. [How to distinguish changed values from beneficial behavior](../../docs/maturation-transfer.md).

The observation concerns this handoff on one prepared desktop. Report where each person needed help even when the bytes and the persona’s work check out.
