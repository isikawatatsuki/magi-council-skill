import { readStdin, parseJson, normalizeHookPayload } from './lib.mjs';

// GitHub Copilot can replace a tool result, so protected content is removed before the
// model sees it. Claude Code's PostToolUse hook cannot rewrite a result — there the
// PreToolUse guard is the enforcing layer and this hook only adds a policy reminder.
function flatten(value) {
  if (typeof value === 'string') return value;
  try { return JSON.stringify(value); } catch { return String(value); }
}

try {
  const raw = await readStdin();
  const input = parseJson(raw || '{}', 'hook input');
  const payload = normalizeHookPayload(input);
  const result = typeof payload.toolResult === 'string'
    ? payload.toolResult
    : (payload.toolResult?.textResultForLlm ?? payload.toolResult?.text_result_for_llm ?? flatten(payload.toolResult ?? ''));
  const normalized = String(result).replaceAll('\\', '/');
  const sensitive = /\.magi\/runs\/[^\s"']+\/(?:sealed|manifest\.json)|\.magi\/memory\/personas/i;
  if (!sensitive.test(normalized)) { console.log('{}'); process.exit(0); }
  const warning = 'Do not attempt to recover or infer protected MAGI vote or persona-memory content.';
  console.log(JSON.stringify({
    modifiedResult: { resultType: 'success', textResultForLlm: '[MAGI protected content redacted by policy Hook]' },
    additionalContext: warning,
    hookSpecificOutput: { hookEventName: 'PostToolUse', additionalContext: warning }
  }));
} catch (error) {
  console.error(error.stack ?? error.message);
  process.exit(1);
}
