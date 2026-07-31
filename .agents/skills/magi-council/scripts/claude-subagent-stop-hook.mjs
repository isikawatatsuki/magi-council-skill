import fs from 'node:fs';
import path from 'node:path';
import { readStdin, parseJson, normalizeHookPayload, readLastAssistantMessage, findRepoRoot, runDirFor, readJson, extractJsonObject, validateVote, sealVote, PERSONAS } from './lib.mjs';

// Claude Code SubagentStop hook.
//
// Claude Code cannot rewrite the message a subagent hands back to its parent, so this
// hook enforces sealing from the other direction: a persona that leaks its vote body is
// blocked and told to return only the receipt line, and a receipt is accepted only when a
// matching sealed vote actually exists on disk.
function allow() { console.log('{}'); process.exit(0); }
function block(reason) { console.log(JSON.stringify({ decision: 'block', reason })); process.exit(0); }

try {
  const raw = await readStdin();
  const input = parseJson(raw || '{}', 'hook input');
  const payload = normalizeHookPayload(input);
  if (input.stop_hook_active) allow();

  const message = payload.response || readLastAssistantMessage(payload.transcriptPath);
  if (!message) allow();

  const receipt = message.match(/VOTE_SEALED\s+run=(magi-[a-z0-9-]+)\s+sha256=([0-9a-f]{16})/i);
  const looksLikeVote = /"persona"\s*:\s*"(melchior|balthasar|casper)"/.test(message);
  if (!receipt && !looksLikeVote) allow();

  const root = findRepoRoot(payload.cwd);

  if (looksLikeVote) {
    let vote;
    try {
      vote = validateVote(extractJsonObject(message));
    } catch (error) {
      block(`Your vote was rejected: ${error.message} Pipe one corrected vote JSON to seal-vote.mjs and return only the receipt line.`);
    }
    try {
      await sealVote(root, vote.persona, vote, payload.agentId);
    } catch (error) {
      block(`MAGI sealing failed: ${error.message} Fix the vote, pipe it to seal-vote.mjs, and return only the receipt line.`);
    }
    block('Your vote body must never reach the parent agent. It has been sealed for you. Reply with the single receipt line printed by seal-vote.mjs and nothing else.');
  }

  const [, runId, shortHash] = receipt;
  const manifestFile = path.join(runDirFor(root, runId), 'manifest.json');
  if (!fs.existsSync(manifestFile)) block(`No MAGI run named ${runId} exists. Seal your vote with seal-vote.mjs before finishing.`);
  const manifest = readJson(manifestFile);
  const matched = PERSONAS.some((persona) => manifest.votes?.[persona]?.sha256?.startsWith(shortHash.toLowerCase()));
  if (!matched) block(`No sealed vote in run ${runId} matches that receipt. Pipe your vote JSON to seal-vote.mjs and return the receipt it prints.`);
  allow();
} catch (error) {
  block(`MAGI stop hook failed: ${error.message}. Seal your vote with seal-vote.mjs and return only the receipt line.`);
}
