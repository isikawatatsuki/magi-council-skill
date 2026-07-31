import { readStdin, parseJson, normalizeHookPayload, AGENT_TO_PERSONA, findRepoRoot, extractJsonObject, validateVote, sealVote, SealRejected } from './lib.mjs';

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
  let sealed;
  try {
    sealed = await sealVote(root, persona, vote, payload.agentId);
  } catch (error) {
    if (error instanceof SealRejected) {
      console.log(JSON.stringify({ decision: 'block', reason: error.message }));
      process.exit(0);
    }
    throw error;
  }
  console.log(JSON.stringify({ decision: 'allow', modifiedResponse: sealed.receipt }));
} catch (error) {
  console.log(JSON.stringify({ decision: 'block', reason: `MAGI sealing failed: ${error.message}. Do not change the question or persona; return a valid vote JSON again.` }));
}
