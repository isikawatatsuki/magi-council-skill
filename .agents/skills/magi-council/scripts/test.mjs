import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { validateVote, sha256, findRepoRoot } from './lib.mjs';

function assert(condition, message) { if (!condition) throw new Error(message); }
function run(cwd, script, args = [], input = '') {
  const result = spawnSync(process.execPath, [script, ...args], { cwd, input, encoding: 'utf8' });
  if (result.status !== 0) throw new Error(`${path.basename(script)} failed: ${result.stderr || result.stdout}`);
  return result.stdout.trim();
}

const sampleRunId = 'magi-20260730121500-a1b2c3d4e5f6';
const baseVote = (runId, persona, decision) => ({
  schemaVersion: '1.0', runId, persona, decision, confidence: 80,
  summary: `${persona} summary`, reasons: [{ code: 'R1', statement: 'Reason', evidence: [] }],
  conditions: decision === 'approve' ? ['Add tests'] : [], risks: [], assumptions: [], memoryCandidates: []
});
for (const [persona, decision] of [['melchior','approve'],['balthasar','reject'],['casper','approve']]) validateVote(baseVote(sampleRunId, persona, decision), persona);
assert(sha256({b:2,a:1}) === sha256({a:1,b:2}), 'canonical hashing failed');

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const guard = path.join(scriptDir, 'guard-tool-use.mjs');
const denied = spawnSync(process.execPath, [guard], { input: JSON.stringify({ cwd: process.cwd(), toolName: 'view', toolArgs: { path: '.magi/runs/x/sealed/melchior.json' } }), encoding: 'utf8' });
assert(denied.status === 0, `guard exited ${denied.status}: ${denied.stderr}`);
assert(JSON.parse(denied.stdout).permissionDecision === 'deny', 'guard did not deny sealed vote access');
const allowed = spawnSync(process.execPath, [guard], { input: JSON.stringify({ cwd: process.cwd(), toolName: 'view', toolArgs: { path: 'src/index.ts' } }), encoding: 'utf8' });
assert(JSON.parse(allowed.stdout).permissionDecision === 'allow', 'guard denied ordinary source read');
let mismatchFailed = false;
try { validateVote({ ...baseVote(sampleRunId, 'melchior','approve'), persona: 'casper' }, 'melchior'); } catch { mismatchFailed = true; }
assert(mismatchFailed, 'persona mismatch was accepted');

const sourceRoot = findRepoRoot();
const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'magi-skill-test-'));
try {
  for (const name of ['.agents','.github','.magi','package.json']) fs.cpSync(path.join(sourceRoot, name), path.join(tempRoot, name), { recursive: true });
  const scripts = path.join(tempRoot, '.agents', 'skills', 'magi-council', 'scripts');
  const requestInput = JSON.stringify({ question: 'Release the authentication change?', context: { evidence: [{ path: 'src/auth.ts', note: 'Revocation missing' }] } });
  const created = JSON.parse(run(tempRoot, path.join(scripts, 'create-run.mjs'), ['--stdin'], requestInput));
  const runId = created.runId;
  const votes = [
    baseVote(runId, 'melchior', 'approve'),
    { ...baseVote(runId, 'balthasar', 'reject'), risks: [{ severity: 'high', statement: 'Revocation missing', mitigated: false, mitigation: 'Implement revocation' }] },
    { ...baseVote(runId, 'casper', 'approve'), memoryCandidates: [{ principle: 'Use staged rollout when rollback exists.', scopes: ['release'], applicableWhen: ['Deadline fixed'], notApplicableWhen: ['Critical risk'], rationale: 'Contains delivery risk.' }] }
  ];
  for (const vote of votes) {
    const hookInput = JSON.stringify({ sessionId: 'test', cwd: tempRoot, agentId: `agent-${vote.persona}`, agentName: `magi-${vote.persona}`, response: JSON.stringify(vote), stopReason: 'end_turn' });
    const receipt = JSON.parse(run(tempRoot, path.join(scripts, 'subagent-stop-hook.mjs'), [], hookInput));
    assert(receipt.decision === 'allow' && receipt.modifiedResponse.includes('VOTE_SEALED'), `${vote.persona} was not sealed`);
  }
  const decision = JSON.parse(run(tempRoot, path.join(scripts, 'tally-votes.mjs'), [runId]));
  assert(decision.decision === 'approved_with_conditions', `unexpected decision: ${decision.decision}`);
  assert(decision.voteCounts.approve === 2 && decision.voteCounts.reject === 1, 'vote counts are incorrect');
  const audit = JSON.parse(run(tempRoot, path.join(scripts, 'audit-run.mjs'), [runId]));
  assert(audit.valid === true, `audit failed: ${JSON.stringify(audit.errors)}`);
} finally {
  fs.rmSync(tempRoot, { recursive: true, force: true });
}

console.log(JSON.stringify({ ok: true, tests: ['vote-validation','canonical-hash','sealed-path-guard','ordinary-read','persona-binding','sealed-integration','deterministic-tally','integrity-audit'] }));
