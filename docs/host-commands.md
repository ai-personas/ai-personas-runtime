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

The normal complete-decision adoption records the exact command before dispatch.
There is no separate tool attempt allocation or additional model-call charge.
Input and retained-output bytes use the existing Store accounting. A shell
installation changes the actual host environment; it is not simulated.

## Results and inspection

The next decision receives a `host_command_result` with the command input,
execution status, exit code or terminating signal, optional timeout/failure,
stdout, stderr, elapsed time, and observed byte counts. The current runner and
shell artifact hashes, effective user ID, and a digest of the inherited
environment identify the observed execution without copying environment values.

Each stream contributes up to 16 KiB of bytes to the retained text excerpt.
This bounds context capture, not command execution. `stdout_log` and `stderr_log`
identify full host log files, which the persona can inspect with another command.
The truncation flags and byte counts describe the observation. Host log paths are
live files; a command that leaves background processes may continue writing them.
The retained result itself remains an exact observation.

Results are also available as exact supporting assets under `tools/results/`.
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

After saving host-command records, use helpers that understand this profile.
Older helpers do not recognize the new job and result-origin fields.
