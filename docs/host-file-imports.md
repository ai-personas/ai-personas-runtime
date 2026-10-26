# Retain host files as supporting assets

The host reader retains complete regular files without an import byte ceiling.
File-corpus byte/count quotas are also removed under
[the current requirement](execution-resources.md).


Personas can use `file_import` to keep an existing host file in the native
asset store without copying its content through a model response. This supports
retaining useful skill guides, scripts, code, notes and data for later selective
discovery. The persona chooses what to import and how to organize it.

`format: "utf8"` validates the original text; explicitly supplying it and
omitting the default format are both valid. Use
`format: "bytes"` for archives, CAD, images or other binary files. This retains
the original bytes, without converting them to text or passing them through a
model response. A binary artifact needs no encoded script or text wrapper.

This follows the design's [supporting-resource requirements UR-011–014](https://github.com/ai-personas/ai-personas-design/blob/main/USER-REQUIREMENTS.md#supporting-assets-and-historical-context):
keep assets separate from character, retain useful material, provide locations
and discovery cues, and compose relevant context. Imports use the same explicit
next-context selection and discovery as other assets (UR-018–021).

An ordinary input offers `CONTROL.file_import` with `maximum_bytes: null`. For example, a persona can propose:

```json
{
  "kind": "file_import",
  "id": "retain-inventory-guide",
  "source": "persona-0001:inventory-guide",
  "path": "skills/inventory/guide.md",
  "base_revision": null,
  "host_path": "/workspace/inventory/SKILL.md",
  "expected_sha256": null,
  "qualifications": [],
  "qualification_handles": [],
  "policy_id": "COPY_A_SUPPLIED_WRITE_POLICY"
}
```

Use the actual actor in source identifiers. The source is a new native identity
when `base_revision` is null. Updating an existing source requires its exact
current head, as for `file_write`. The native destination path must be unique
within its owner and asset scope. `host_path` identifies the existing host file;
relative paths use the working directory shown by the host interface. Symlinks
to regular files may be followed. The original host file is not moved or modified.

List the import ID in `asset_maintenance.change.operation_ids`, with a reason
and useful discovery cues. Imports always create supporting assets. They cannot
create characteristic fragments or establish that the persona authored the file.
Required qualifications and current-input qualification handles work as they do
for authored file saves.

The runtime reads the regular file during adoption, retaining its exact
bytes without normalization. An
optional `expected_sha256` requires the observed bytes to match that digest.
An unreadable file, invalid encoding in `utf8` mode or digest mismatch rejects the
complete save plan without committing a partial asset. There is no application
per-file or aggregate retention byte quota. A concurrently changing file is not an atomic
filesystem snapshot: provenance identifies the bytes actually read.

Saves happen before host commands execute, regardless of operation list order.
To import a file produced by a command, receive the actual command result and
choose the import in a fresh decision. Importing a script does not execute it.

The accepted save receipt records an exact native source revision, the importing
persona and invocation, the host path, byte length and SHA-256. It supplies
metadata, not the file body. The saved content is attributed as a host-file
import when later supplied to the model; importing a skill guide does not prove
skill application or persona maturation.

To use the asset next, choose its exact same-response `proposal-op:<import-id>`
reference under `next_assets`, or select/read the accepted reference later.
`replace_from_saved_files` derives `utf8` for text imports and `bytes_metadata`
for byte imports. The latter supplies a deterministic descriptor containing the
original byte length and SHA-256, with `body_supplied: false`. It has no range,
does not decode the file and supplies no pixels. It can also describe a large
text asset without loading its body. Characteristic fragments remain text.
Ordinary discovery can find its native path and content across later work.
Path discovery returns `bytes_metadata` for non-UTF-8 assets. Text search and
decoded JSON projections still require their actual supported encodings;
metadata search examines only the descriptor.
Import does not select the asset implicitly or load a directory or tool catalogue.
The saved revision is independent of later changes to the live host file.

The native destination is not a host filesystem path. To create a host copy or
run the saved content, pass its exact reference or current-input handle through
[host-command `stdin_asset`](host-commands.md#use-a-retained-asset-as-standard-input).
With a `bytes_metadata` reference, the command receives the complete original
bytes; UTF-8 and JSON-string references supply their exact projection and range.
The model receives neither input merely because the command consumes it.
Retention and later model
context supply alone do not install dependencies or establish execution.

An imported asset can also be registered as a work artifact through the usual
selected `put_artifact` command. Its dependency names the exact import operation.
Registration, release and delivery remain separate choices; they are not needed
just to retain a useful resource.

Artifact registration accepts complete `utf8` or `bytes_metadata` references.
Model evidence requires an exact supplied view and its current qualifications;
its receipt distinguishes full text from byte metadata. A hash establishes byte
identity, not substantive inspection or quality. Personas choose host tools,
image reads and other evidence for their judgments. Local delivery retains the
original artifact bytes beside the qualified context. The launcher's **e: export
files** action saves those bytes, qualifications and original receipts in a new
chosen directory without executing any artifact. It also retrieves complete
supporting files explicitly cited in requester messages. That reader action
requires no formal shared release, creates no acceptance or completion record,
and does not turn the earlier citation into a claim of earlier body delivery.
