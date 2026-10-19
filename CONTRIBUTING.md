# Contributing

Prioritize working functionality against the user requirements and authoritative design. Build Rust changes with `cargo build --locked --bins`. Use the authorized live workflow to inspect actual persona decisions, supplied resources, tool execution and results. Keep the repository free of automated test cases. Live provider use requires explicit authorization.

The runtime must not hardcode domain intent, professions, scenario behavior, semantic resource ranking, or tool choices on behalf of personas. Deployment policy belongs in operator configuration. Fail closed when required authority or evidence is unknown, and record the reason. Surface deviations from the authoritative design.

Keep all state roots, credentials, provider traces, private archives, and research audit records outside the repository. Never add OAuth token extraction or undocumented backend authentication. Distinguish generated candidates, accepted state, supplied context, observed effects, assessed quality, and delivered results.
