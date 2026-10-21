# Native file write conflicts

`file_write.source` or `file_import.source` identifies a native file. Its `path` is unique among the
active sources belonging to that persona in the same scope. Creating a different
source at an occupied path does not overwrite the existing file. To revise a
file, reuse its source and exact current `base_revision`. To create a separate
file, choose an unused path. A missing or null `base_revision` means creation;
it does not request the latest revision of an existing source. Retiring a source
keeps its immutable evidence.

The runtime checks paths after staging the allowed writes and retirements for
the complete decision. A valid move, path swap, or retirement and replacement
therefore does not fail because of a temporary duplicate during staging. A final
conflict rejects the complete decision; none of its proposed actions is adopted.
Character changes retain their existing ordering and operation fences.

New rejections can retain a `native_write_correction` observation. A path conflict
describes the scope, path and two source identities, with proposed operation IDs
and accepted head revisions where present. A stale base describes the source,
write operation, proposed base and accepted head revision. The runtime only
reports an accepted base when the conflict was against accepted state; a head
changed by an earlier write in the same rejected response keeps its ordinary
failure classification. These are historical facts: a fresh decision must still
check current state. The observation does not supply rejected file bodies or
execute corrected writes.

Within an already authorized foreground run, an unchanged accepted ordinary
continuation can allow the failed invocation to retire and a fresh decision to
receive the correction. The new admission spends the remaining session and work
allowances. An initial-character failure, authored stop, consumed timed
opportunity, cancellation or incomplete transport gains no continuation.

The diagnostic retains exact request and terminal bindings and the relevant
accepted-source lineage within the existing failure-context byte limit. If that
lineage or bounded context cannot be established, the failure keeps its ordinary
recovery path. Older failures are not reclassified. Source availability is
checked again when supplying the observation.

Native files are separate from host filesystem files. Personas continue to have
[full host command access](host-commands.md), including package installation.
