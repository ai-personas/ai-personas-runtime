# Native navigation and unknown provider usage

Current execution follows [observational resource accounting](execution-resources.md).
Saved work and Store allocations no longer stop ordinary work. Known navigation
may exceed an earlier reservation, and unknown provider usage remains recorded
without acting as a cumulative admission ceiling. The original accounting
mechanism and historical meanings below do not reinstate the removed quotas.


Each invocation reserves Store resources before dispatch. Native file searches,
reads and history operations record navigation use against a retained estimate.
Saved finite allocations do not limit those operations. Provider usage can remain
unknown after a complete response; reported tokens alone do not settle that hold.

For a newly accepted response, the runtime knows its native navigation charge:
the scan and result byte counters from its own completed plan. It charges those
bytes to consumed Store navigation and releases the unused navigation portion of
that invocation's Store reservation. This commits atomically with adoption.
Input/output exposure remains uncertain. The original reservation and a separate
`navigation_charge`, bound to the accepted response digest, remain inspectable.

This does not create a retry, continuation or model call. Original per-work
allowance metadata and prior logical debits remain historical records. A rejected or interrupted
plan retains its conservative unknown hold because complete local accounting
was not accepted. Older unknown holds are not automatically revised.

These counters are logical native-operation charges, not physical disk reads,
model tokens or money. Some graph projections retain conservative charges.
File reads and discovery now account for the complete retained corpus used to
construct their view; subject scans and returned bytes are additional charges.
Selection repair also records its declared projection envelope. Store
record reads are metered separately and are not included again. Character-only
adoption fences ordinary navigation and records a zero native-operation charge.

The reservation can remain `Uncertain` after its navigation charge is known:
its remaining input/output exposure is still held. Repeating the same accepted
accounting neither charges nor releases navigation twice. A later complete usage
measurement must agree with the known navigation charge and does not count it
again. Pending effects and uncertain provider outcomes retain their exact records.
An uncertain persona call holds that actor without blocking eligible peers.

The native [development view](persona-development.md) exposes the original
allowance, known native navigation charge and any complete measurement for an
accepted invocation. Missing accounting is not zero usage. Lower reserved
navigation alone proves neither lower whole-episode cost nor useful completion.
