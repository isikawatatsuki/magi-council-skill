import fs from 'node:fs';
import path from 'node:path';
import { readStdin, parseJson, findRepoRoot, runDirFor, readJson, validateRequest, validateVote, PERSONAS, sha256, hashRequest, atomicWriteJson, withRunLock } from './lib.mjs';

const runId = process.argv[2];
if (!runId) throw new Error('Usage: import-inline-votes.mjs <runId> < votes.json');
const input = parseJson(await readStdin(), 'inline votes');
if (!Array.isArray(input) || input.length !== 3) throw new Error('Input must be an array containing exactly three votes.');
const root = findRepoRoot();
const runDir = runDirFor(root, runId);
await withRunLock(runDir, async () => {
  const requestFile = path.join(runDir, 'request.json');
  const manifestFile = path.join(runDir, 'manifest.json');
  const request = validateRequest(readJson(requestFile));
  request.executionMode = 'inline';
  const manifest = readJson(manifestFile);
  manifest.requestSha256 = hashRequest(request);
  for (const persona of PERSONAS) {
    const vote = validateVote(input.find((item) => item.persona === persona), persona);
    if (vote.runId !== runId) throw new Error(`${persona} runId mismatch.`);
    atomicWriteJson(path.join(runDir, 'sealed', `${persona}.json`), vote);
    manifest.votes[persona] = { sha256: sha256(vote), sealedAt: new Date().toISOString(), agentId: null, warning: 'inline execution; independence not guaranteed' };
  }
  request.status = 'ready';
  atomicWriteJson(requestFile, request);
  atomicWriteJson(manifestFile, manifest);
});
console.log(JSON.stringify({ runId, imported: true, warning: 'Inline votes share one model context; persona independence is not guaranteed.' }));
