import { readStdin, parseJson, normalizeHookPayload } from './lib.mjs';

function flatten(value) {
  if (typeof value === 'string') return value;
  try { return JSON.stringify(value); } catch { return String(value); }
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
    /\.agents\/skills\/magi-council\/scripts(?:\/|\b)/,
    /\.magi\/constitution(?:\/|\b)/,
    /\.magi\/config\.json/,
    /\.magi\/memory(?:\/|\b)/,
    /\.magi\/runs\/[^/]+\/(?:sealed|manifest\.json|decision\.json|decision\.md)/
  ];
  const isReadLike = ['view','grep','glob','read'].includes(tool);
  const isMutation = ['create','edit','write','apply_patch','str_replace_editor'].includes(tool);
  if (isReadLike && protectedRead.some((pattern) => pattern.test(text))) {
    console.log(JSON.stringify({ permissionDecision: 'deny', permissionDecisionReason: 'MAGI sealed votes, manifests, and persona-private memory are not model-readable.' }));
    process.exit(0);
  }
  if (isMutation && protectedMutation.some((pattern) => pattern.test(text))) {
    console.log(JSON.stringify({ permissionDecision: 'deny', permissionDecisionReason: 'Protected MAGI state may be changed only by reviewed MAGI scripts and explicit human memory approval.' }));
    process.exit(0);
  }
  if (['bash','powershell','execute'].includes(tool)) {
    const directSecretAccess = protectedRead.some((pattern) => pattern.test(text));
    const sourceMutation = protectedMutation.some((pattern) => pattern.test(text)) && /(rm|del|remove|write|set-content|out-file|sed\s+-i|perl\s+-i|node\s+-e|python\s+-c)/.test(text);
    if (directSecretAccess || sourceMutation) {
      console.log(JSON.stringify({ permissionDecision: 'deny', permissionDecisionReason: 'Direct shell access to protected MAGI state or policy implementation is denied.' }));
      process.exit(0);
    }
  }
  console.log(JSON.stringify({ permissionDecision: 'allow' }));
} catch (error) {
  console.error(error.stack ?? error.message);
  process.exit(1);
}
