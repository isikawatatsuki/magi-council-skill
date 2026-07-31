import crypto from 'node:crypto';
import path from 'node:path';
import fs from 'node:fs';
import { readStdin, parseJson, findRepoRoot, ensureDir, atomicWriteJson, hashRequest, PERSONAS, validateRequest } from './lib.mjs';

const args = process.argv.slice(2);
let input;
if (args.includes('--stdin')) {
  const raw = await readStdin();
  input = parseJson(raw, 'stdin request');
} else {
  const index = args.indexOf('--question');
  if (index < 0 || !args[index + 1]) throw new Error('Use --stdin with JSON or --question "...".');
  input = { question: args[index + 1], context: {} };
}
if (typeof input.question !== 'string' || !input.question.trim()) throw new Error('question is required.');
if (!input.context || typeof input.context !== 'object' || Array.isArray(input.context)) throw new Error('context must be an object.');

const now = new Date();
const stamp = now.toISOString().replace(/[-:TZ.]/g, '').slice(0, 14).toLowerCase();
const runId = `magi-${stamp}-${crypto.randomBytes(6).toString('hex')}`;
const root = findRepoRoot();
const config = JSON.parse(fs.readFileSync(path.join(root, '.magi', 'config.json'), 'utf8'));
const runDir = path.join(root, '.magi', 'runs', runId);
ensureDir(path.join(runDir, 'sealed'));
ensureDir(path.join(runDir, 'candidates'));
const request = validateRequest({
  schemaVersion: '1.0',
  runId,
  createdAt: now.toISOString(),
  status: 'collecting',
  executionMode: input.executionMode === 'inline' ? 'inline' : 'sealed-subagents',
  question: input.question.trim(),
  context: input.context,
  expectedPersonas: PERSONAS,
  voting: {
    method: 'majority',
    criticalRiskVeto: Boolean(config.voting?.criticalRiskVeto)
  }
});
atomicWriteJson(path.join(runDir, 'request.json'), request);
atomicWriteJson(path.join(runDir, 'manifest.json'), {
  schemaVersion: '1.0',
  runId,
  requestSha256: hashRequest(request),
  votes: {},
  finalized: false,
  createdAt: now.toISOString()
});
console.log(JSON.stringify({ runId, status: 'collecting', requestPath: `.magi/runs/${runId}/request.json` }));
