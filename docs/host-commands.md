# Host commands and tool installation

Personas have full host command access by default in ordinary decisions, on both
new and existing runtimes. They can install packages, run installed tools, read
and write host files, use the network, and start subprocesses with the runtime's
host account and inherited environment. There is no application sandbox, command
allowlist, per-tool approval, or tool-installation grant.

`CONTROL.host_command_capability` supplies the current `host.command.v1`
definition on every ordinary input. No supporting guide needs to be discovered
or selected first. Personas choose which tools and skill guides to use, including
guides available as host files. The separate WebAssembly and HTTPS profiles keep
their existing grant contracts; those grants do not restrict host commands.

## Invocation

Use an `effect` operation with the supplied capability ID and binding revision:

```json
{
  "kind": "effect",
  "id": "inspect-tools",
  "capability_id": "host.command.v1",
  "binding_revision": "COPY_THE_CURRENT_BINDING",
  "inputs": {
    "command": "python3 --version",
    "working_directory": null,
    "stdin": null,
    "timeout_seconds": null
  },
  "depends_on": []
}
```

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
a wall-clock limit to installations or other host commands. Model-call limits
and explicit user interruption still apply.

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
code or terminating signal, optional timeout/failure, elapsed time, observed byte
counts and complete retained command input, stdout and stderr. The current runner
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

Pending observations supply the complete retained command, stdin and captured
output. There is no shared text quota or beginning/end truncation.
`context_projection.format` is `host-command-pending-complete-v1`. Its retained
payload digest and result reference preserve attribution. The reference names
a separate exact result document; metadata alone does not supply that document
or select it implicitly. Reading results never repeats the command. The work's
result-reference catalogue also has no fixed entry cap.

Other context and file-corpus limits remain pending removal under
[the unrestricted-execution requirement](execution-resources.md). Successful
capture does not itself prove that a later model input was supplied or accepted.

Results are also available as exact supporting assets under `tools/results/`.
These are runtime asset paths, not files beneath the host working directory.
Read or select the exact result reference through the runtime file interface.
The result body can identify the original command's actual host output paths;
use those exact paths when revising an output. The `stdout.log_path` and `stderr.log_path`
paths refer to host files directly.

`CONTROL.host_command_state.retained_results` lists up to eight of the most
recently observed permitted result references. `retained_results_truncated`
reports when older permitted references were omitted from this catalogue; those
results remain stored and discoverable under `tools/results/`, and existing
selections are unchanged. Unresolved jobs are listed independently of this limit.
The result catalogue is optional and is supplied only when it fits alongside
required context and the persona's selected files. It may be absent, or have
`retained_results_included: false` when only unresolved jobs are shown. That does
not establish that no older results exist. Current result observations, selected
file bodies and required qualifications are unaffected by catalogue omission.
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
