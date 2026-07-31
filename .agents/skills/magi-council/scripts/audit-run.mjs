import fs from 'node:fs';
import path from 'node:path';
import { findRepoRoot, runDirFor, readJson, validateRequest, validateVote, PERSONAS, sha256, hashRequest } from './lib.mjs';

const runId = process.argv[2];
if (!runId) throw new Error('Usage: audit-run.mjs <runId>');
const root = findRepoRoot();
const runDir = runDirFor(root, runId);
const errors = [];
const request = validateRequest(readJson(path.join(runDir, 'request.json')));
const manifest = readJson(path.join(runDir, 'manifest.json'));
if (manifest.requestSha256 !== hashRequest(request)) errors.push('request hash mismatch');
for (const persona of PERSONAS) {
  const file = path.join(runDir, 'sealed', `${persona}.json`);
  if (!fs.existsSync(file)) { errors.push(`missing ${persona} vote`); continue; }
  const vote = validateVote(readJson(file), persona);
  if (manifest.votes?.[persona]?.sha256 !== sha256(vote)) errors.push(`${persona} vote hash mismatch`);
}
const decisionFile = path.join(runDir, 'decision.json');
if (fs.existsSync(decisionFile)) {
  const decision = readJson(decisionFile);
  const expected = sha256({ ...decision, integrity: { ...decision.integrity, decisionSha256: '' } });
  if (decision.integrity?.decisionSha256 !== expected) errors.push('decision content hash mismatch');
  if (manifest.decisionSha256 !== expected) errors.push('manifest decision hash mismatch');
}
console.log(JSON.stringify({ runId, valid: errors.length === 0, errors }));
if (errors.length) process.exitCode = 1;
