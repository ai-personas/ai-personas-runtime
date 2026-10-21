# Contributing

Prioritize working functionality against the user requirements and authoritative design. Build Rust changes with `cargo build --locked --bins`. Use the authorized live workflow to inspect actual persona decisions, supplied resources, tool execution and results. Keep the repository free of automated test cases. Live provider use requires explicit authorization.

Use the design's [user requirements](https://github.com/ai-personas/ai-personas-design/blob/main/USER-REQUIREMENTS.md) as the product acceptance criteria, with later user clarifications taking precedence. This is the first release: backward compatibility with development formats and interfaces is not required. Remove obsolete adapters when changing a path; do not add migrations or compatibility branches solely to keep earlier development builds working. Retained run records remain evidence of the build that produced them, not a requirement to support every old format.

Personas choose their approach, cooperation, tools and skills. Full host access and tool installation are enabled by default, without application sandboxes, command allowlists or per-tool approval. Characteristic fragments remain separate from tools, skills, data and history. Each persona interaction includes work, fragment and asset maintenance, and next-context choice. Inspect actual supplied context, executions and resulting work before claiming these requirements are met.

The runtime must not hardcode domain intent, professions, scenario behavior, semantic resource ranking, or tool choices on behalf of personas. Deployment policy belongs in operator configuration. Fail closed when required authority or evidence is unknown, and record the reason. Surface deviations from the authoritative design.

Keep all state roots, credentials, provider traces, private archives, and research audit records outside the repository. Never add OAuth token extraction or undocumented backend authentication. Distinguish generated candidates, accepted state, supplied context, observed effects, assessed quality, and delivered results.
