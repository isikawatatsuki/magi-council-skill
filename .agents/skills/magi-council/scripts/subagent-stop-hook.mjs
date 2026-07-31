import fs from 'node:fs';
import path from 'node:path';
import { readStdin, parseJson, normalizeHookPayload, AGENT_TO_PERSONA, findRepoRoot, extractJsonObject, validateVote, validateRequest, runDirFor, readJson, sha256, atomicWriteJson, withRunLock } from './lib.mjs';

try {
  const raw = await readStdin();
  const payload = normalizeHookPayload(parseJson(raw || '{}', 'hook input'));
  const persona = AGENT_TO_PERSONA.get(payload.agentName);
  if (!persona) { console.log('{}'); process.exit(0); }
  let vote;
  try {
    vote = validateVote(extractJsonObject(payload.response), persona);
  } catch (error) {
    console.log(JSON.stringify({
      decision: 'block',
      reason: `Your sealed vote was rejected: ${error.message} Return one corrected JSON object only. Do not add markdown or commentary.`
    }));
    process.exit(0);
  }
  const root = findRepoRoot(payload.cwd);
  const runDir = runDirFor(root, vote.runId);
  const requestFile = path.join(runDir, 'request.json');
  const manifestFile = path.join(runDir, 'manifest.json');
  if (!fs.existsSync(requestFile) || !fs.existsSync(manifestFile)) {
    console.log(JSON.stringify({ decision: 'block', reason: 'The supplied runId does not identify an active MAGI run. Return a vote using the exact runId supplied by the parent.' }));
    process.exit(0);
  }
  const request = validateRequest(readJson(requestFile));
  if (!['collecting','ready'].includes(request.status)) throw new Error(`Run is not accepting votes: ${request.status}`);
  const voteFile = path.join(runDir, 'sealed', `${persona}.json`);
  const voteHash = sha256(vote);
  await withRunLock(runDir, async () => {
    const manifest = readJson(manifestFile);
    if (fs.existsSync(voteFile)) {
      const existing = validateVote(readJson(voteFile), persona);
      if (sha256(existing) !== voteHash) throw new Error(`${persona} already sealed a different vote; overwriting is forbidden.`);
    } else {
      atomicWriteJson(voteFile, vote);
    }
    manifest.votes[persona] = {
      sha256: voteHash,
      sealedAt: new Date().toISOString(),
      agentId: payload.agentId ?? null
    };
    atomicWriteJson(manifestFile, manifest);
    const count = Object.keys(manifest.votes).filter((p) => ['melchior','balthasar','casper'].includes(p)).length;
    if (count === 3 && request.status === 'collecting') {
      request.status = 'ready';
      atomicWriteJson(requestFile, request);
    }
  });
  console.log(JSON.stringify({
    decision: 'allow',
    modifiedResponse: `${persona.toUpperCase()}: VOTE_SEALED run=${vote.runId} sha256=${voteHash.slice(0, 16)}`
  }));
} catch (error) {
  console.log(JSON.stringify({ decision: 'block', reason: `MAGI sealing failed: ${error.message}. Do not change the question or persona; return a valid vote JSON again.` }));
}
