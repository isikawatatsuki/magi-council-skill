import fs from 'node:fs';
import path from 'node:path';
import { findRepoRoot, runDirFor, readJson, validateRequest, validateVote, PERSONAS, sha256, hashRequest, atomicWriteJson, atomicWriteText, withRunLock, formatDecisionMarkdown } from './lib.mjs';

const runId = process.argv[2];
if (!runId) throw new Error('Usage: tally-votes.mjs <runId>');
const root = findRepoRoot();
const runDir = runDirFor(root, runId);
const requestFile = path.join(runDir, 'request.json');
const manifestFile = path.join(runDir, 'manifest.json');

await withRunLock(runDir, async () => {
  const request = validateRequest(readJson(requestFile));
  if (request.status === 'finalized' && fs.existsSync(path.join(runDir, 'decision.json'))) {
    console.log(JSON.stringify(readJson(path.join(runDir, 'decision.json'))));
    return;
  }
  const manifest = readJson(manifestFile);
  if (manifest.requestSha256 !== hashRequest(request)) throw new Error('Request hash mismatch. Run is invalid.');
  const votes = {};
  for (const persona of PERSONAS) {
    const file = path.join(runDir, 'sealed', `${persona}.json`);
    if (!fs.existsSync(file)) throw new Error(`Missing sealed vote: ${persona}`);
    const vote = validateVote(readJson(file), persona);
    const hash = sha256(vote);
    if (manifest.votes?.[persona]?.sha256 !== hash) throw new Error(`Vote hash mismatch: ${persona}`);
    votes[persona] = vote;
  }
  const voteCounts = { approve: 0, reject: 0, abstain: 0 };
  for (const vote of Object.values(votes)) voteCounts[vote.decision] += 1;
  const unmitigatedCritical = Object.values(votes).flatMap((vote) => vote.risks.filter((r) => r.severity === 'critical' && !r.mitigated).map((r) => ({ persona: vote.persona, ...r })));
  const vetoApplied = request.voting.criticalRiskVeto && unmitigatedCritical.length > 0;
  let result;
  if (vetoApplied) result = 'rejected_by_veto';
  else if (voteCounts.approve >= 2) result = Object.values(votes).some((v) => v.decision === 'approve' && v.conditions.length) ? 'approved_with_conditions' : 'approved';
  else if (voteCounts.reject >= 2) result = 'rejected';
  else result = 'undecided';
  const winningDecision = result.startsWith('approved') ? 'approve' : result.startsWith('rejected') ? 'reject' : null;
  const confidences = Object.values(votes).map((v) => v.confidence).sort((a,b) => a-b);
  const memoryCandidates = [];
  for (const vote of Object.values(votes)) {
    vote.memoryCandidates.forEach((candidate, index) => memoryCandidates.push({
      id: `${vote.persona}-${index + 1}-${sha256(candidate).slice(0, 8)}`,
      persona: vote.persona,
      sourceRunId: runId,
      status: 'candidate',
      ...candidate
    }));
  }
  const decision = {
    schemaVersion: '1.0',
    runId,
    finalizedAt: new Date().toISOString(),
    executionMode: request.executionMode,
    decision: result,
    voteCounts,
    confidence: { min: confidences[0], median: confidences[1], max: confidences[2] },
    veto: { enabled: request.voting.criticalRiskVeto, applied: vetoApplied, criticalRisks: unmitigatedCritical },
    conditions: [...new Set(Object.values(votes).filter((v) => v.decision === 'approve').flatMap((v) => v.conditions))],
    highRisks: Object.values(votes).flatMap((v) => v.risks.filter((r) => ['high','critical'].includes(r.severity)).map((r) => ({ persona: v.persona, ...r }))),
    dissent: winningDecision ? Object.values(votes).filter((v) => v.decision !== winningDecision).map((v) => ({ persona: v.persona, decision: v.decision, summary: v.summary })) : Object.values(votes).map((v) => ({ persona: v.persona, decision: v.decision, summary: v.summary })),
    assumptions: [...new Set(Object.values(votes).flatMap((v) => v.assumptions))],
    personaSummaries: Object.fromEntries(PERSONAS.map((p) => [p, { decision: votes[p].decision, confidence: votes[p].confidence, summary: votes[p].summary, reasons: votes[p].reasons, conditions: votes[p].conditions, risks: votes[p].risks }])),
    memoryCandidates,
    integrity: {
      requestSha256: manifest.requestSha256,
      voteSha256: Object.fromEntries(PERSONAS.map((p) => [p, manifest.votes[p].sha256])),
      decisionSha256: ''
    }
  };
  decision.integrity.decisionSha256 = sha256({ ...decision, integrity: { ...decision.integrity, decisionSha256: '' } });
  atomicWriteJson(path.join(runDir, 'decision.json'), decision);
  atomicWriteText(path.join(runDir, 'decision.md'), formatDecisionMarkdown(decision));
  request.status = 'finalized';
  atomicWriteJson(requestFile, request);
  manifest.finalized = true;
  manifest.finalizedAt = decision.finalizedAt;
  manifest.decisionSha256 = decision.integrity.decisionSha256;
  atomicWriteJson(manifestFile, manifest);
  console.log(JSON.stringify(decision));
});
