import fs from 'node:fs';
import path from 'node:path';
import { findRepoRoot, runDirFor, readJson, PERSONAS } from './lib.mjs';

const runId = process.argv[2];
if (!runId) throw new Error('Usage: run-status.mjs <runId>');
const root = findRepoRoot();
const runDir = runDirFor(root, runId);
const request = readJson(path.join(runDir, 'request.json'));
const sealed = Object.fromEntries(PERSONAS.map((persona) => [persona, fs.existsSync(path.join(runDir, 'sealed', `${persona}.json`))]));
console.log(JSON.stringify({ runId, status: request.status, sealed, ready: PERSONAS.every((persona) => sealed[persona]) }));
