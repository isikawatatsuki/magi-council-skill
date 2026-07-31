import fs from 'node:fs';
import path from 'node:path';
import { findRepoRoot, ensureDir, atomicWriteJson, atomicWriteText } from './lib.mjs';

const root = findRepoRoot();
const skill = path.join(root, '.agents', 'skills', 'magi-council');
const copies = [
  [path.join(skill, 'templates', 'config.json'), path.join(root, '.magi', 'config.json')],
  [path.join(skill, 'templates', 'constitution.md'), path.join(root, '.magi', 'constitution', 'principles.md')]
];
for (const [source, target] of copies) {
  if (!fs.existsSync(target)) {
    ensureDir(path.dirname(target));
    fs.copyFileSync(source, target);
    console.log(`created ${path.relative(root, target)}`);
  } else console.log(`kept ${path.relative(root, target)}`);
}
for (const persona of ['melchior','balthasar','casper']) {
  const target = path.join(root, '.magi', 'memory', 'personas', `${persona}.json`);
  if (!fs.existsSync(target)) atomicWriteJson(target, { schemaVersion: '1.0', persona, entries: [] });
}
const approved = path.join(root, '.magi', 'memory', 'approved', 'index.json');
if (!fs.existsSync(approved)) atomicWriteJson(approved, { schemaVersion: '1.0', entries: [] });
for (const dir of ['runs','tmp','locks']) ensureDir(path.join(root, '.magi', dir));
console.log('MAGI project state initialized without overwriting existing policy.');
