# Security policy

## Reporting

Please report vulnerabilities privately to the repository maintainers. Do not publish sealed-vote bypasses before a fix is available.

## Supported security boundary

This project provides logical isolation, deterministic tallying, path guards, redaction, and tamper evidence. It does not claim OS-level isolation when an agent has unrestricted code execution under the same user account.

For high-assurance use, place each persona and sealed storage in separate processes or containers and expose narrow RPC/MCP capabilities.

The canonical trust boundaries, attacker model, SHA-256 limitations, execution-mode guarantees, and deployment guidance are documented in [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md).
