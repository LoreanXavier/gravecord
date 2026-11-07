// node tools/pretty.mjs <module id>...  Writes readable copies to design/snapshot/pretty/ and prints their paths.
import { format } from 'prettier';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const snapshot = join(import.meta.dirname, '..', 'design', 'snapshot');
mkdirSync(join(snapshot, 'pretty'), { recursive: true });
for (const id of process.argv.slice(2)) {
  const source = readFileSync(join(snapshot, 'modules', `${id}.js`), 'utf8');
  const out = join(snapshot, 'pretty', `${id}.js`);
  writeFileSync(out, await format(`({${source}})`, { parser: 'babel', printWidth: 110 }));
  console.log(out);
}
