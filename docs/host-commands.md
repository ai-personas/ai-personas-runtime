# Host commands and tool installation

Personas have full host command access by default in ordinary decisions, on both
new and existing runtimes. They can install packages, run installed tools, read
and write host files, use the network, and start subprocesses with the runtime's
host account and inherited environment. There is no application sandbox, command
allowlist, per-tool approval, or tool-installation grant.

`CONTROL.host_command_capability` supplies the current `host.command.v1`
definition on every ordinary input. No supporting guide needs to be discovered
or selected first. Personas choose which tools and skill guides to use, including
guides available as host files. WebAssembly and HTTPS callable definitions are
also supplied by default; historical grant declarations do not restrict access.

Commands can render plots, diagrams and document previews. To inspect a
result visually, choose [image_read](image-input.md) after observing the command
result. Command success and text output do not supply the rendered pixels.

## Invocation

Use an `effect` operation with the supplied capability ID:

```json
{
  "kind": "effect",
  "id": "inspect-tools",
  "capability_id": "host.command.v1",
  "inputs": {
    "command": "python3 --version",
    "result_context": "complete",
    "working_directory": null,
    "stdin": null,
    "timeout_seconds": null
  },
  "depends_on": []
}
```

The capability ID selects the unique interface retained for this invocation.
The runtime checks its complete binding, including the runner, input schema,
shell and default working directory, against the current implementation. Each
accepted job retains that exact interface. Do not put `binding_revision` in the
operation; it is internal runtime evidence, not an authored argument. A missing
or ambiguous admitted interface is rejected before any effect is staged.

For `effect` and `work_command`, omitting the operation's `depends_on` means
an empty prerequisite list. Put it beside `inputs`, not inside the command
inputs. Consuming a file saved in the same response still requires naming that
save explicitly. Omission does not infer a dependency or make a missing result
available.

`command` is a Bash script. The command inherits the host environment, including
`PATH`; it can invoke package managers and other installed software directly.
The interactive launcher preserves that environment when starting native helpers,
including package-manager, proxy and tool settings.
`working_directory` defaults to the native process directory displayed in the
interface. Relative paths resolve against that directory. `stdin` optionally
supplies UTF-8 input. A persona may choose `timeout_seconds`; omission or `null`
means no timeout. A chosen timeout stops that command's process group.
The interactive launcher waits for the native foreground session without adding
a wall-clock limit to installations or other host commands. Explicit user
interruption still applies.

`result_context` is required. Choose `complete` when all returned text belongs in
the next decision, or `reference` when execution metadata and exact readable
references are enough to decide what to inspect next. This is the persona's
context choice, not an output capture limit or an automatic size threshold.

Combine dependent shell steps in a single script. Multiple independent command
operations may be submitted in one decision and execute in their listed order.
Shell environment changes last for that command; persist reusable settings in
host files or source them in later commands. Native supporting assets remain
separate from host filesystem files.

Use [host-file imports](host-file-imports.md) to retain an existing UTF-8 guide,
script, note or data file as an exact native supporting asset for later selection
and discovery, without rewriting its contents in the model response.

`file_write.path` names a relative path inside the native fragment or asset
store, such as `notes/review.md`; it does not save a host file. Use
`host.command.v1` to write a file at an absolute host workspace path. Native
paths use nonempty slash-separated components and exclude leading or trailing
slashes, `.` and `..` components, backslashes, colons and control characters.
The decision schema describes this distinction, and validation reports an
invalid native path at the corresponding operation's `path` field.
Native paths must also be unique within the persona and scope; see
[native file conflicts](native-file-conflicts.md) for revisions and recovery.

## Use a retained asset as standard input

`stdin_asset` supplies an exact full UTF-8 supporting asset to the command
without copying its content through the model response. Use either a literal
`stdin` string or `stdin_asset`; supplying both is rejected.

For a handle offered in the current input, the command inputs can be:

```json
{
  "command": "cat > /workspace/retained-helper.py",
  "result_context": "complete",
  "stdin_asset": {"kind": "input_file", "handle": "COPY_A_CURRENT_INPUT_HANDLE"}
}
```

Alternatively use `{"kind":"reference","reference":EXACT_FILE_REFERENCE}`
with the asset's source, revision and `utf8` representation. The reference must
name the whole file, with no byte range. A same-response `file_write` or
`file_import` may be consumed through its `proposal-op:<id>` revision when
`depends_on` names that exact save ID. A current-input handle always names an
already retained revision. No current head or similarly named host file is
substituted for the chosen revision.

The shell receives the exact saved bytes on standard input. `cat > path` writes
a host copy; `python3 -` runs Python source directly, with arguments after `-`.
Choose a host file when the program expects `__file__`, nearby resources, or
separate stdin for its own data. Dependencies and environment still belong to
the actual host command. A command that never reads stdin may ignore it.

Native inputs are resolved while staging the complete decision, before any
command executes. The accepted intent retains the materialized bytes and their
source revision, byte count and SHA-256. Dispatch rechecks that exact source and
its bindings; it never rereads an original host path or rebuilds the bytes from
model text. The existing command reservation accounts for the materialized
input. Importing and immediately using a file does not skip its maintenance or
complete-decision checks.

The result's `inputs.stdin_asset` records the exact source, size and digest;
`inputs.stdin` is omitted for this case. The body is retained in the accepted
intent and native asset, not repeated in result context. This receipt proves
which bytes were supplied as stdin, not that the process consumed them, that the
script succeeded, or that a skill was applied. Inspect the actual command result.
Source qualifications remain attached to the result. Passing an asset to the
runner does not select it for model context or claim that its body was supplied
to the model; code and bulk data can stay outside the prompt.

## Execution and observations

The normal complete-decision adoption records the exact command before dispatch.
Each observed terminal result is retained before the next command executes.
Result observations and supporting assets are published after the decision's
execution pass, so publishing one result does not invalidate another command's
accepted prerequisites. This also applies to decisions mixing host commands
with the optional WebAssembly or HTTPS profiles.

New task-scoped Direct decisions also retain the command's relevant dispatch
dependencies. A change in another task does not cancel an accepted command
before it starts. Changes to its own task, character, lifecycle or saved
prerequisites still matter, and pause/resume does not revive an earlier command.
The final check and dispatch record are atomic. Older accepted commands retain
their original exact runtime-head prerequisite. See
[accepted command dispatch](concurrent-decisions.md#accepted-command-dispatch).

In native foreground runs and launcher sessions, a final host command runs on
a worker while other eligible personas can take a turn. Commands earlier in
that decision still finish in authored order before the final command starts.
The originating persona/work becomes eligible again after its result is
published, subject to its accepted continuation. Results are published between
adoptions, with each pending decision checked against its relevant current state.
Independent accepted personas can also have [primary calls in flight together](concurrent-decisions.md).
The standalone one-step API and explicit tool recovery remain synchronous.

If only running commands remain, the launcher waits for an actual completion
without spending another model admission or repeatedly scanning Store. Every
normal run exit drains its workers and records their outcomes before returning
the final view. Cancellation joins workers before releasing execution ownership;
an outcome that was not durably recorded still requires explicit recovery and
is never inferred or rerun. There is no added timeout.
There is no separate tool attempt allocation or additional model-call charge.
Input and retained-output bytes use the existing Store accounting. A shell
installation changes the actual host environment; it is not simulated.

## Results and inspection

The next decision receives a `host_command_result` with execution status, exit
code or terminating signal, optional timeout/failure, elapsed time and observed byte
counts. The persona's `result_context` chooses how retained command input, stdout
and stderr enter that context. The current runner
and shell artifact hashes, effective user ID, and a digest of the inherited
environment identify the observed execution without copying environment values.

`stdout` and `stderr` are capture objects with `bytes_observed`,
`capture_complete`, `log_path` and `segments`. New commands retain the complete
observed stream in one segment, without a byte ceiling. An empty stream has no
segments. Each segment preserves raw log byte offsets and SHA-256, decoded
`text`, and `lossy_utf8`. Invalid UTF-8 may be replaced in text; its raw offsets
and digest still identify the observed bytes. A null observed count means the
capture was unavailable. Allocation or file errors remain actual failures.

Host logs remain available at `log_path`. They are live files: background
processes may append after the captured length was observed. Retention describes
that observed range, not future writes. Earlier partial captures keep their
original gaps and completeness flags; removing a cap does not recover missing
historical bytes.

With `result_context: "complete"`, pending observations supply all retained
command, literal stdin and captured output text. Asset stdin stays an exact input
receipt. `context_projection.format` is `host-command-pending-complete-v2`.
Each supplied command, literal stdin and output segment also has its exact
decoded-string reference, byte count, hash and `*_supplied: true`. These fields
appear in `CONTROL.input_file_choices` as individually selectable excerpts;
their `context_state.exact_body_supplied` is true. The whole result document
remains a separate reference whose body is not supplied by this projection.
Historical observations without a choice keep the complete context choice.

With `result_context: "reference"`, pending observations supply execution metadata
and text references. The format is `host-command-pending-reference-v2`.
`inputs.command` and literal `inputs.stdin` become `command_reference` and
`stdin_reference`, with UTF-8 byte counts, SHA-256 and supplied flags. Every output
segment replaces `text` with `text_reference`, `text_utf8_bytes`, `text_sha256`
and `text_supplied: false`. Raw offsets and hashes, lossy decoding flags, capture
completeness, log paths and required qualifications remain available.

These references address decoded JSON strings in the immutable result document:
for example, `json_string:/result/stdout/segments/0/text`. Ordinary reads and
next-context selections can supply any range or the complete string. Range
offsets apply to decoded UTF-8 text, which can differ from raw log offsets when
decoding was lossy. The whole result reference supplies the complete document
when read or selected. The original observation and result asset retain all
captured text in both modes; there is no text quota or beginning/end truncation.

Both projections include the original observation payload digest and exact
result reference. Reference metadata does not supply a body or select it
implicitly. Capture completeness does not establish that text reached a later
model input. Reading results never repeats the command.

A pending result is consumed after the decision receiving it is accepted.
`context_projection.context_lifetime` makes this explicit. “Already supplied”
means present in that request; it does not carry the output into the next one.
When a later image review, action or reply will need a current output excerpt,
select its handle in `next_assets`. The persona can retain just stderr, a useful
stdout segment or a chosen range instead of the whole command record. For
example, an existing field containing output hashes can remain selected while
the next request receives a newly captured image. No rewrite or new native file
is required merely to keep that exact field available.

`preserve` retains existing optional selections; `clear` removes them. Neither
receiving a result nor preserving an empty selection selects its fields.
The persona chooses useful lifetime, replacement, removal or a supporting note.
This does not create an automatic failure memory, a tool blacklist or a required
maintenance edit. A later exact read can recover omitted output without running
the original command again.

Native development inspection lists `resource_use.pending_host_result_text`
for actual complete-v2 fields. It checks each supplied string against the exact
retained JSON-string projection and reports hashes, sizes and producing
invocations. Older admitted formats retain their original evidence; inspection
does not invent field references or later selection for them.

Results are also available as exact supporting assets under `tools/results/`.
These are runtime asset paths, not files beneath the host working directory.
Read or select the exact result reference through the runtime file interface.
The result body can identify the original command's actual host output paths;
use those exact paths when revising an output. The `stdout.log_path` and `stderr.log_path`
paths refer to host files directly.

`CONTROL.host_command_state.retained_results` supplies provenance only for exact
result revisions already referenced in the current input, including results
selected from other work. It does not enumerate all past commands. There is no
entry cap: `retained_results_limit` is null and `retained_results_truncated` is
false. Unresolved jobs for the current actor/work remain operational state,
independent of historical retrieval. An absent result entry does not establish
that no older result exists. Use [native file discovery](input-file-references.md#discover-resources-before-selecting-their-bodies)
or retained-history queries when another result is needed.
The ordinary input-file handles can select or cite supplied references. Selecting an old result
does not run its command again. `development PERSONA --invocation CALL` includes
`host_command_execution` with dispatch and terminal receipts.

To settle an interrupted native execution record:

```sh
ai-personas --state-root /absolute/private/state --operator local-operator recover-host-commands
```

Recovery never repeats a dispatched command. If the host lost its terminal
observation, the result remains uncertain. Native records distinguish that case
from a successful process exit or an installation failure.
For commands still awaiting first dispatch, recovery preserves their order in
the accepted decision and checks the existing prerequisites. Recovery also
completes its execution pass before publishing result observations.

After saving host-command records, use helpers that understand this profile.
Older helpers do not recognize the new job and result-origin fields.
