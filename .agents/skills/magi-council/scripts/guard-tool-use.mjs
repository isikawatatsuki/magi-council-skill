import { readStdin, parseJson, normalizeHookPayload } from './lib.mjs';

function flatten(value) {
  if (typeof value === 'string') return value;
  try { return JSON.stringify(value); } catch { return String(value); }
}

// Emitted on deny only. Claude Code reads `hookSpecificOutput`; a `permissionDecision`
// of `allow` there would bypass its permission system, so the allow path stays inert and
// carries the top-level field that GitHub Copilot expects.
function deny(reason) {
  console.log(JSON.stringify({
    permissionDecision: 'deny',
    permissionDecisionReason: reason,
    hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: 'deny', permissionDecisionReason: reason }
  }));
  process.exit(0);
}

try {
  const raw = await readStdin();
  const payload = normalizeHookPayload(parseJson(raw || '{}', 'hook input'));
  const text = flatten(payload.toolArgs).replaceAll('\\', '/').toLowerCase();
  const tool = String(payload.toolName ?? '').toLowerCase();
  const protectedRead = [
    /\.magi\/runs\/[^/]+\/sealed(?:\/|\b)/,
    /\.magi\/runs\/[^/]+\/manifest\.json/,
    /\.magi\/memory\/personas(?:\/|\b)/
  ];
  const protectedMutation = [
    /\.github\/hooks(?:\/|\b)/,
    /\.github\/agents\/magi-/,
    /\.claude\/settings(?:\.local)?\.json/,
    /\.claude\/agents\/magi-/,
    /\.claude\/skills\/magi-council(?:\/|\b)/,
    /\.agents\/skills\/magi-council\/scripts(?:\/|\b)/,
    /\.magi\/constitution(?:\/|\b)/,
    /\.magi\/config\.json/,
    /\.magi\/memory(?:\/|\b)/,
    /\.magi\/runs\/[^/]+\/(?:sealed|manifest\.json|decision\.json|decision\.md)/
  ];
  const isReadLike = ['view','grep','glob','read'].includes(tool);
  const isMutation = ['create','edit','write','apply_patch','str_replace_editor'].includes(tool);
  if (isReadLike && protectedRead.some((pattern) => pattern.test(text))) {
    deny('MAGI sealed votes, manifests, and persona-private memory are not model-readable.');
  }
  if (isMutation && protectedMutation.some((pattern) => pattern.test(text))) {
    deny('Protected MAGI state may be changed only by reviewed MAGI scripts and explicit human memory approval.');
  }
  if (['bash','powershell','execute'].includes(tool)) {
    const directSecretAccess = protectedRead.some((pattern) => pattern.test(text));
    const sourceMutation = protectedMutation.some((pattern) => pattern.test(text)) && /(rm|del|remove|write|set-content|out-file|sed\s+-i|perl\s+-i|node\s+-e|python\s+-c)/.test(text);
    if (directSecretAccess || sourceMutation) {
      deny('Direct shell access to protected MAGI state or policy implementation is denied.');
    }
  }
  console.log(JSON.stringify({ permissionDecision: 'allow' }));
} catch (error) {
  console.error(error.stack ?? error.message);
  process.exit(1);
}
