# MAGI Council Threat Model

This document is the canonical description of MAGI Council's security boundary. `SECURITY.md` explains vulnerability reporting; the skill's `security-model.md` is a short operational reference. This document defines what the system trusts, prevents, detects, and does not guarantee.

## Security objectives and protected assets

MAGI Council aims to preserve:

- isolation of each persona's context and private approved memory;
- confidentiality of sealed vote bodies from the parent model until the protocol permits disclosure;
- integrity of the request, votes, adversarial-review artifacts, decision, and recorded hashes;
- deterministic application of documented tally, veto, and review-trigger rules;
- preservation of dissent, conditions, risks, and evidence references in the final artifacts.

The protected on-disk assets include `.magi/runs/*/sealed`, both sealed round directories, `manifest.json`, persona memory, the anonymous THOMAS mapping and input, challenges, and `review-analysis.json`. Constitution, configuration, Hooks, agent definitions, and the `magi` executable are trusted policy or enforcement inputs rather than secrets.

## Threat actors in scope

- Untrusted repository content or prompt injection that asks a persona to change role, reveal policy, access another vote, or alter its output format.
- A mistaken or adversarial persona model that emits malformed output, a second vote, unsupported evidence references, or content intended for another phase.
- Ordinary parent-agent tool calls that accidentally read, search, modify, or return protected paths.
- Accidental file modification, missing artifacts, or out-of-order protocol operations after a run begins.

These actors are constrained only while the trusted Host invokes the configured Hooks and the reviewed `magi` binary.

## Trusted components and boundaries

| Boundary | Trusted responsibility | Data crossing the boundary | Consequence if compromised |
| --- | --- | --- | --- |
| Human administrator | Installs the reviewed binary and skill, controls configuration, approves memory, protects the OS account | Policy, configuration, approved memory, release artifacts | Can replace every control and rewrite both data and hashes |
| Host runtime | Creates genuinely separate subagent contexts, invokes every configured Hook with accurate tool/agent metadata, enforces timeouts and blocking responses | Prompts, Hook payloads, receipts, tool results | A malicious or incompatible Host can expose votes or skip enforcement |
| Hooks | Inject phase-scoped context, validate and seal outputs, guard protected paths, redact accidental results | Agent output, tool paths/arguments/results | Disabled or modified Hooks remove confidentiality and path controls |
| `magi` CLI | Validates schemas and state transitions, writes atomically, tallies deterministically, hashes and audits artifacts | Request, votes, challenges, manifest, decision | A replaced binary can accept invalid state or fabricate results |
| File system / OS account | Enforces file permissions and path semantics against other accounts | Protected state and trusted implementation files | Same-user arbitrary code, administrator access, or storage rollback bypasses the logical boundary |
| Persona model | Follows its fixed role and returns one schema-conforming vote | Shared input, private policy, vote or receipt | Output can be wrong or malicious; the model is not trusted to tally or enforce policy |

SHA-256 values are unkeyed change-detection records. They reveal ordinary missing or modified artifacts when a trusted manifest, binary, and audit path remain intact. They are not digital signatures, do not prove who created an artifact, and do not stop an administrator or same-user attacker from replacing an artifact and its recorded hash together.

## What the implementation prevents or detects

When the trusted boundary holds, the implementation:

- gives each persona only shared decision input plus that persona's policy and approved memory;
- rejects unknown personas, malformed votes, unexpected fields, unresolved evidence references, duplicate submissions, and out-of-order phases;
- seals votes before returning a receipt and avoids returning the vote body to the parent workflow;
- blocks direct tool requests containing recognized protected paths and redacts recognized protected paths from tool results;
- uses atomic writes, a run lock, immutable request hashing, per-vote hashes, protected adversarial-artifact hashes, decision hashing, and deterministic audit recomputation;
- computes vote counts, critical vetoes, evidence-aware review triggers, and final state in Rust rather than asking a model to summarize the outcome;
- detects ordinary changes to recorded artifacts and fails an audit when required files or hashes disagree.

The relevant implementation and regression checks are in `src/hooks.rs`, `src/core.rs`, `src/lifecycle.rs`, `src/adversarial.rs`, and `tests/cli.rs`. GitHub Hook wiring is in `.github/hooks/magi-council.json`; the Claude template is `.claude/settings.json.authoring-off` and must be deliberately enabled by the operator.

## What the implementation does not fully prevent

MAGI Council is not an OS sandbox or a cryptographic voting service. It does not fully prevent:

- an administrator, the same OS user, or unrestricted arbitrary code from reading memory or files, replacing Hooks or the binary, changing configuration, editing an artifact and all corresponding hashes, or rolling storage back;
- a malicious Host from omitting a Hook, misreporting an agent/tool identity, returning a protected body outside Hook coverage, or running nominal subagents in one shared context;
- access through aliases the Host never presents to the guard, including some symlink/junction paths, file identifiers, alternate streams, encoded paths, dynamically constructed Shell paths, environment or command expansion, subprocesses, or direct system calls;
- every Bash, PowerShell, CMD, or application-specific interpretation of a string. The guard performs bounded normalization and pattern checks; it is not a complete Shell parser;
- leakage that contains protected content without a recognizable protected path. Post-tool redaction is a safety net, not data-loss prevention;
- incorrect, biased, fabricated, or correlated model judgments. Schema validity, majority agreement, and high self-reported confidence do not establish truth or safety;
- network, model-provider, extension, terminal, debugger, backup, telemetry, or plugin access that bypasses the configured Host tool/Hooks path.

Timeout behavior is Host-defined. The supplied GitHub configuration requests fail-closed handling for nonzero guard results, but the current security reference records that a Host timeout may fail open. Operators must verify the actual Host version and policy rather than infer behavior from the template alone.

## Execution modes

### `sealed-subagents`

This mode describes a protocol result only when the Host actually provides separate Custom Agents/Subagents, invokes the start/stop and tool Hooks, keeps vote bodies out of the parent context, and runs the reviewed CLI. It isolates contexts and vote visibility at the application layer. It does not imply different providers, different foundation models, separate processes, or OS isolation. Using the same or similar model for all personas leaves correlated-error risk.

### `inline`

Inline execution has no secret ballot or independent subagent-context guarantee. The same model context may create or observe every vote. The CLI can still validate JSON, apply deterministic rules, persist dissent, and audit recorded files, but it cannot retroactively create independence or confidentiality. An operator must select and disclose inline execution explicitly; it is not security-equivalent to sealed execution.

Both labels depend on Host behavior. If capability metadata is unavailable, it must be reported as unknown rather than guessed. Runtime capability diagnosis and fail-closed mode admission are tracked separately from this descriptive threat model.

## Reproducibility and decision guarantees

The reproducible parts are the recorded input, schema and protocol version, deterministic tally/review rules, persisted outputs, and audit procedure. Given identical valid votes and policy artifacts, the trusted CLI produces the same logical result. Language-model generation is not guaranteed to be byte-for-byte repeatable: provider versions, sampling, hidden prompts, context ordering, and tool results can change a vote.

An `approved` result means that the configured process did not retain blocking evidence under its rules. It is not proof that the proposal is correct or safe. A majority can share the same blind spot. Confidence values are self-reported and uncalibrated; `80` is not an 80% correctness probability.

## Recommended high-assurance deployment

For decisions requiring stronger guarantees:

1. Pin and independently verify the `magi` binary, skill, Hook configuration, schemas, and model/provider configuration.
2. Run each persona in a separate process, container, VM, or OS identity with no general-purpose Shell and no access to sealed storage.
3. Expose narrow RPC/MCP operations such as `read_shared_context` and `submit_vote` instead of filesystem access.
4. Store sealed artifacts in an append-only service outside persona and parent-agent credentials.
5. Sign requests, votes, manifests, and decisions with keys held by that service; retain an external transparency or audit log.
6. Restrict and monitor administrator access, backups, plugins, telemetry, terminals, and provider-side data handling.
7. Require human approval before external side effects and independently verify critical evidence.

These controls are deployment responsibilities and are not implemented by the repository template.

## Review checklist

Before relying on a run, confirm:

- the intended execution mode matches observed Host capabilities;
- all required Hooks were enabled and their failure/timeout policy was verified;
- the binary and policy files came from reviewed revisions;
- the run audit succeeds and protected artifacts were not exposed through another channel;
- users understand that context separation, majority, hashes, and confidence have the limited meanings described above.
