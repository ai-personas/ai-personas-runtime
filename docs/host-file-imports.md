# Retain host files as supporting assets

Personas can use `file_import` to keep an existing UTF-8 host file in the native
asset store without copying its content through a model response. This supports
retaining useful skill guides, scripts, code, notes and data for later selective
discovery. The persona chooses what to import and how to organize it.

This follows the design's [supporting-resource requirements UR-011–014](https://github.com/ai-personas/ai-personas-design/blob/main/USER-REQUIREMENTS.md#supporting-assets-and-historical-context):
keep assets separate from character, retain useful material, provide locations
and discovery cues, and compose relevant context. Imports use the same explicit
next-context selection and discovery as other assets (UR-018–021).

An ordinary input offers `CONTROL.file_import` with the current native retention
limit. For example, a persona can propose:

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

The runtime reads the regular file during adoption, retaining its exact UTF-8
bytes without newline normalization or replacement of invalid encoding. An
optional `expected_sha256` requires the observed bytes to match that digest.
An unreadable file, invalid encoding, excess size or digest mismatch rejects the
complete save plan without committing a partial asset. The retention bound is
the existing native per-file limit, at most 256 KiB; it does not restrict host
commands or installations. A concurrently changing file is not an atomic
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
Ordinary discovery can find its native path and content across later work.
Import does not select the asset implicitly or load a directory or tool catalogue.
The saved revision is independent of later changes to the live host file.

The native destination is not a host filesystem path. Retention and later model
context supply do not install the script or create an executable host copy.
Host execution needs an actual host file and its dependencies; importing the
file alone does not establish that this later execution is possible.

An imported asset can also be registered as a work artifact through the usual
selected `put_artifact` command. Its dependency names the exact import operation.
Registration, release and delivery remain separate choices; they are not needed
just to retain a useful resource.
