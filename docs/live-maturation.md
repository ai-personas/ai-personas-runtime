# Live persona maturation walkthrough

On 18 October 2026, continuing three-persona cohorts ran with GPT-5.6 Sol and
GPT-5.6 Luna through Codex OAuth. Inference used the runtime's native HTTPS
provider. Codex supplied authentication only. No automated tests were added or run.

These were supervised foreground sessions. Operator feedback, explicit resumes,
and navigation-only resource grants helped unfinished work continue. Rejected
responses remained unadopted; their failures were explicitly acknowledged before
fresh decisions. This is not evidence of an unattended recovery loop.

The working path is: actual feedback, a persona-authored characteristic
refinement, accepted storage and explicit selection, then the exact body supplied
to a later accepted invocation. Supporting reports remain separate assets.

## Sol: feedback becomes retained guidance and later action

Mira received attributed requester feedback that repeated attempts to recover
complete history were delaying a useful contribution. In `call-00000031`, she
authored an optional fragment describing a bounded evidence-gathering habit:

> I make one bounded attempt to retrieve the missing record most likely to change
> the judgment; then I work from the experience actually supplied, distinguish
> observation from interpretation, state remaining unknowns and duties, and
> deliver a scoped contribution.

The accepted source was `persona-0001-bounded-evidence-progress-refinement`,
revision `a6c7ac0bb5089c793c8077dc14b9fc6bd47019a0265882a4916ec4d91858f5e7`.
Mira explicitly selected it for the remaining reflection work. Its complete body
appeared in the qualified input of `call-00000034`, whose direct-provider receipt
matched the retained input digest and whose complete response was accepted.

In that later call, Mira wrote and registered a shared synthesis, proposed taking
responsibility for the result, and requested her peers' consent. She made no
further history query, although navigation allowance remained. Sol subsequently
released and locally delivered the reflection result, then recorded closure in
`call-00000040` after the exact delivery receipt reached a fresh admission.

This is observed subsequent action consistent with the retained refinement. The
run does not isolate that fragment's effect from the new feedback and peer accounts.

## Luna: a second accepted refinement and later supply

Luna's Rowan initially produced supporting reflection material without changing
characteristic guidance. In `call-00000046`, he authored and selected
`persona-0002/work-00000002/timeboxed-progress-guidance`, revision
`f1461102bdf6bc0962b329054a0af08f814c6a760a9d863703c8b18a3243c3c8`.

It describes moving from a stalled evidence search to a scoped contribution,
stating unknowns and remaining duties, and distinguishing an artifact from proof
of its later effect. Its complete body reached accepted `call-00000049`. The
provider receipt matched that input, and Rowan's reply preserved those distinctions.
This demonstrates later supply; sustained benefit remains unestablished.

## Returning to ordinary work

Both cohorts then continued their original four-bedroom house brief with their
existing identities and histories. Sol delivered the corrected concept in
`call-00000045` and recorded closure in `call-00000048`. The result contains four
bedrooms, an explicit sharing assumption, a separate five-bedroom alternative,
an enclosed office, quiet study, an accessibility-ready ground-floor route,
tradeoffs and the remaining family decisions. It is an early concept, with no
construction or engineering assurance claimed. The native result reports
`completed-as-recorded` and one local delivery.

The return also supplies a useful limit: Mira's house call `call-00000041`
contained her original own voice but did **not** contain her optional reflection
fragment. Her better house response therefore does not demonstrate that fragment
carrying across works. Optional selection is work-scoped; another work needs the
persona's own selection or a permitted character adoption. Retention alone does
not silently put a fragment into every future prompt.

Luna's house work reached a valid voluntary responsibility proposal after the
contributors judged their individual concepts sufficient. The arrangement
activated after the peers' own consent in `call-00000060` and `call-00000062`.
Formal shared release remained outstanding at that checkpoint. Its reflection
work also remained open.

## Inspect an actual accepted call

```sh
./personas --state-root /private/sol-state --operator demo-operator \
  --development persona-0001 --invocation call-00000034
```

For the Luna cohort, use its separate state directory, `persona-0002` and
`call-00000049`. The inspection makes no model call. It returns currently
permitted characteristic revisions and checks their full text against the named
call's retained qualified input. It also binds the accepted response to its
durable record and reports whether a completed direct-provider receipt matches
the input digest. Metadata, selected references and a saved file alone do not
satisfy that body check. Behavioral improvement is reported separately.

The original stores, full invocation records and credentials remain outside the
repository. This walkthrough reports the bounded live observations above; it is
not a claim of complete design conformance or general behavioral improvement.
