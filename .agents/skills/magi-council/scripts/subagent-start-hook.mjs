import { readStdin, parseJson, normalizeHookPayload, AGENT_TO_PERSONA, findRepoRoot, buildPersonaContext } from './lib.mjs';

try {
  const raw = await readStdin();
  const payload = normalizeHookPayload(parseJson(raw || '{}', 'hook input'));
  const persona = AGENT_TO_PERSONA.get(payload.agentName);
  if (!persona) { console.log('{}'); process.exit(0); }
  const additionalContext = buildPersonaContext(findRepoRoot(payload.cwd), persona);
  console.log(JSON.stringify({ additionalContext }));
} catch (error) {
  console.error(error.stack ?? error.message);
  process.exit(1);
}
