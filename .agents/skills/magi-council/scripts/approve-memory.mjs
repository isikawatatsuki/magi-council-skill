import fs from 'node:fs';
import path from 'node:path';
import { findRepoRoot, runDirFor, readJson, atomicWriteJson, sha256 } from './lib.mjs';

const [runId, candidateId, ...rest] = process.argv.slice(2);
const approvedByIndex = rest.indexOf('--approved-by');
const approvedBy = approvedByIndex >= 0 ? rest[approvedByIndex + 1] : null;
if (!runId || !candidateId || !approvedBy) throw new Error('Usage: approve-memory.mjs <runId> <candidateId> --approved-by "name"');
const root = findRepoRoot();
const runDir = runDirFor(root, runId);
const decision = readJson(path.join(runDir, 'decision.json'));
const candidate = decision.memoryCandidates.find((item) => item.id === candidateId);
if (!candidate) throw new Error('Memory candidate not found in finalized decision.');
const memoryFile = path.join(root, '.magi', 'memory', 'personas', `${candidate.persona}.json`);
const memory = readJson(memoryFile);
const entry = {
  id: `memory-${sha256({ candidate, approvedBy }).slice(0, 12)}`,
  status: 'approved',
  enabled: true,
  priority: 50,
  approvedBy,
  approvedAt: new Date().toISOString(),
  sourceRunId: runId,
  principle: candidate.principle,
  scopes: candidate.scopes,
  applicableWhen: candidate.applicableWhen,
  notApplicableWhen: candidate.notApplicableWhen,
  rationale: candidate.rationale
};
if (!memory.entries.some((item) => item.id === entry.id)) memory.entries.push(entry);
atomicWriteJson(memoryFile, memory);
const indexFile = path.join(root, '.magi', 'memory', 'approved', 'index.json');
const index = fs.existsSync(indexFile) ? readJson(indexFile) : { schemaVersion: '1.0', entries: [] };
if (!index.entries.some((item) => item.id === entry.id)) index.entries.push({ id: entry.id, persona: candidate.persona, sourceRunId: runId, approvedAt: entry.approvedAt });
atomicWriteJson(indexFile, index);
console.log(JSON.stringify({ approved: true, persona: candidate.persona, entry }));
