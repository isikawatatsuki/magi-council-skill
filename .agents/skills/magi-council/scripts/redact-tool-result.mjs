import { readStdin, parseJson, normalizeHookPayload } from './lib.mjs';

try {
  const raw = await readStdin();
  const input = parseJson(raw || '{}', 'hook input');
  const payload = normalizeHookPayload(input);
  const result = payload.toolResult?.textResultForLlm ?? payload.toolResult?.text_result_for_llm ?? '';
  const normalized = String(result).replaceAll('\\', '/');
  const sensitive = /\.magi\/runs\/[^\s"']+\/(?:sealed|manifest\.json)|\.magi\/memory\/personas/i;
  if (!sensitive.test(normalized)) { console.log('{}'); process.exit(0); }
  console.log(JSON.stringify({
    modifiedResult: { resultType: 'success', textResultForLlm: '[MAGI protected content redacted by policy Hook]' },
    additionalContext: 'Do not attempt to recover or infer protected MAGI vote or persona-memory content.'
  }));
} catch (error) {
  console.error(error.stack ?? error.message);
  process.exit(1);
}
