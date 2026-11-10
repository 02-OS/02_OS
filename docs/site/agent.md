# Agent runtime

`02` is the local command. `02-agentd` is the user daemon. Both keep the index, the evidence memory, and the status file on this machine.

A skill document names a version and a list of tools that already exist. It cannot name an unknown tool or a path that leaves the skill directory. A capability token is stored only as a hash.

`02 dashboard` listens on `127.0.0.1` and shows agents, token counts, approvals, and evidence from the local status file.

There is no telemetry and no mandatory account.
