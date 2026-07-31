import { findRepoRoot, buildPersonaContext, PERSONAS } from './lib.mjs';

// Hosts without a subagent-start hook (Claude Code) cannot inject persona-private policy,
// so the persona loads its own. Only this reviewed script hands out persona memory, and
// only for the single persona named on the command line; direct reads of
// `.magi/memory/personas` stay blocked by the tool-use guard.
const persona = process.argv[2];
if (!PERSONAS.includes(persona)) throw new Error(`Usage: load-persona.mjs <${PERSONAS.join('|')}>`);
console.log(buildPersonaContext(findRepoRoot(), persona));
