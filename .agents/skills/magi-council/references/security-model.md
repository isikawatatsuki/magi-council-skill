# Security model

## Protected assets

- sealed persona votes
- vote manifest and hashes
- persona-private approved memory
- constitution and voting configuration
- hook and tally implementation

## Defenses included

1. Separate Custom Agents with isolated contexts.
2. Persona agents have `tools: []` in strict mode.
3. `subagentStart` injects only the selected persona's policy and memory.
4. `subagentStop` validates, normalizes, atomically writes, hashes, and redacts the vote.
5. `preToolUse` blocks direct model access to protected paths.
6. `postToolUse` redacts protected paths if a broad search accidentally returns them.
7. Tally is deterministic and refuses missing, duplicated, malformed, or hash-mismatched votes.
8. Memory promotion requires a human identifier and an explicit command.

## Limits

This is logical isolation inside one host account. It is not an operating-system sandbox.

If an agent can execute unrestricted arbitrary code as the same OS user, it can potentially bypass path-based Hooks by constructing paths dynamically, reading process memory, disabling Hooks, or replacing scripts. The strict persona agents therefore have no tools. The Orchestrator retains execution ability only to invoke reviewed scripts.

For hostile-agent or high-assurance deployments:

- run each persona in a separate process or container
- expose only `read_shared_context` and `submit_vote` RPC/MCP tools
- keep sealed storage in a service inaccessible to persona processes
- sign votes with service-held keys
- enforce append-only audit storage
- require human approval before external side effects

## Fail behavior

GitHub command `preToolUse` hooks fail closed on crashes and non-zero exits, but hook timeouts fail open. Keep guard scripts local, dependency-free, and fast. Do not replace them with network calls.
