# Native navigation and unknown provider usage

Each invocation reserves Store resources before dispatch. Native file searches,
reads and history operations have a separate bounded navigation allocation,
debited from the work when the invocation is prepared. Provider usage can remain
unknown after a complete response; reported tokens alone do not settle that hold.

For a newly accepted response, the runtime knows its native navigation charge:
the scan and result byte counters from its own completed plan. It charges those
bytes to consumed Store navigation and releases the unused navigation portion of
that invocation's Store reservation. This commits atomically with adoption.
Input/output exposure remains uncertain. The original reservation and a separate
`navigation_charge`, bound to the accepted response digest, remain inspectable.

This does not refund a work attempt or renew its native navigation allocation.
It creates no retry, continuation, permission or model call. Per-work allowances
still reflect the full debit made before planning. A rejected or interrupted
plan retains its conservative unknown hold because complete local accounting
was not accepted. Older unknown holds are not automatically revised.

These counters are logical native-operation charges, not physical disk reads,
model tokens or money. Privacy-preserving scan and qualification checks can use
conservative charges. Selection repair also charges its declared projection
envelope; only its remaining read budget uses the ordinary counters. Store
record reads are metered separately and are not included again. Character-only
adoption fences ordinary navigation and records a zero native-operation charge.

The reservation can remain `Uncertain` after its navigation charge is known:
its remaining input/output exposure is still held. Repeating the same accepted
accounting neither charges nor releases navigation twice. A later complete usage
measurement must agree with the known navigation charge and does not count it
again. Exhaustion, pending effects and uncertain provider outcomes retain their
existing controls.

The native [development view](persona-development.md) exposes the original
allowance, known native navigation charge and any complete measurement for an
accepted invocation. Missing accounting is not zero usage. Lower reserved
navigation alone proves neither lower whole-episode cost nor useful completion.
