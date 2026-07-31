import { readStdin, findRepoRoot, extractJsonObject, validateVote, sealVote, PERSONAS } from './lib.mjs';

// Sealing entry point for hosts whose subagent-stop hook cannot rewrite the response
// that reaches the parent (Claude Code). The persona seals its own vote and returns
// only the receipt line, so the vote body never enters the parent context.
const args = process.argv.slice(2);
const personaIndex = args.indexOf('--persona');
const expected = personaIndex >= 0 ? args[personaIndex + 1] : null;
if (expected && !PERSONAS.includes(expected)) throw new Error(`--persona must be one of ${PERSONAS.join(', ')}.`);

const raw = await readStdin();
if (!raw) throw new Error('Usage: seal-vote.mjs [--persona <name>] --stdin < vote.json');
const vote = validateVote(extractJsonObject(raw), expected);
const { receipt, voteHash } = await sealVote(findRepoRoot(), vote.persona, vote, process.env.CLAUDE_AGENT_ID ?? null);
console.log(JSON.stringify({ sealed: true, persona: vote.persona, runId: vote.runId, sha256: voteHash, receipt }));
