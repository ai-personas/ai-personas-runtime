# Native file write conflicts

The `source` on `file_write`, `file_edit` or `file_import` identifies a native file. Its `path` is unique among the
active sources belonging to that persona in the same scope. Creating a different
source at an occupied path does not overwrite the existing file. To revise a
file, reuse its source and exact current `base_revision`. To create a separate
file, choose an unused path. A missing or null `base_revision` means creation;
it does not request the latest revision of an existing source. Retiring a source
keeps its immutable evidence.

Discovery can return historical paths and revisions after a move. The supplied
`file_mutation_metadata.current_head` identifies the current accepted base for
an encountered source. It uses the same representation as path discovery:
UTF-8 for valid text, otherwise byte metadata. Current-head metadata neither
substitutes for an exact historical reference nor supplies the file body.

`file_edit` always requires an accepted base and cannot create a source. Its
exact matches are resolved only after that base passes the current-head check.
A stale base never triggers a fuzzy match or an automatic edit of the latest
head. See [native text edits](native-file-edits.md).

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
receive the correction. The new admission records actual usage without a session,
work or failure-context quota. An initial-character failure, authored stop, consumed timed
opportunity, cancellation or incomplete transport gains no continuation.

The diagnostic retains exact request and terminal bindings and the relevant
accepted-source lineage in full. The admitted request is referenced by its exact
digest; newly consulted sources and policies are retained once each. Unknown
lineage remains explicitly unknown, independently of its size. Rejection, its
receipt and the complete provenance commit together; persistence failure rolls
them back together. Older failures are not reclassified. Source availability is
checked again when supplying the observation.

Native files are separate from host filesystem files. Personas continue to have
[full host command access](host-commands.md), including package installation.
