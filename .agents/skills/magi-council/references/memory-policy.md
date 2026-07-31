# Persona memory policy

## What belongs in memory

Store only reusable, scoped decision principles explicitly approved by a human, for example:

- prefer compatibility over elegance in legacy modules
- require a rollback plan for irreversible data migrations
- permit temporary technical debt only when a tracked remediation exists

Do not store:

- raw conversations
- secrets or personal data
- model-generated personality guesses
- temporary project facts
- another persona's vote
- final vote counts as a persuasion signal

## Candidate lifecycle

```text
vote suggestion -> decision memoryCandidates -> human review -> approved memory
```

A candidate must contain:

- a narrow principle
- scopes
- applicable conditions
- non-applicable conditions
- rationale
- source run and persona

## Precedence

1. Constitution
2. Explicit project policy
3. Persona foundation
4. Approved scoped memory
5. Current shared context
6. General model knowledge

Higher levels override lower levels. Old memory must not override a newer constitution.

## Maintenance

Approved entries have priority, enabled status, timestamps, source run, and approver. Disable or supersede stale entries instead of silently editing their history.
