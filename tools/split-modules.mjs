// Splits every downloaded chunk into design/snapshot/modules/<id>.js and writes MODULE_INDEX.tsv.
// Chunks are parsed, never executed.
import { parse } from 'acorn';
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const snapshot = join(import.meta.dirname, '..', 'design', 'snapshot');
const assets = join(snapshot, 'assets');
const modules = join(snapshot, 'modules');
mkdirSync(modules, { recursive: true });

function isModuleTable(node) {
  return (
    node.type === 'ObjectExpression' &&
    node.properties.length > 0 &&
    node.properties.every(
      (p) =>
        p.type === 'Property' &&
        (p.key.type === 'Literal' || (p.key.type === 'Identifier' && !p.computed)) &&
        (p.value.type === 'FunctionExpression' || p.value.type === 'ArrowFunctionExpression'),
    )
  );
}

// Walks the tree without descending into a found table.
function findTables(root) {
  const tables = [];
  const stack = [root];
  while (stack.length) {
    const node = stack.pop();
    if (!node || typeof node.type !== 'string') continue;
    if (isModuleTable(node) && node.properties.some((p) => /^\d+$/.test(String(p.key.value ?? p.key.name)))) {
      tables.push(node);
      continue;
    }
    for (const value of Object.values(node)) {
      if (Array.isArray(value)) stack.push(...value);
      else if (value && typeof value.type === 'string') stack.push(value);
    }
  }
  return tables;
}

const NAME_PATTERNS = [
  /static displayName="([^"]+)"/g,
  /\.displayName="([^"]+)"/g,
  /displayName:"([^"]+)"/g,
];

const index = [];
const seen = new Map();
let failed = 0;
for (const file of readdirSync(assets).filter((name) => name.endsWith('.js'))) {
  const source = readFileSync(join(assets, file), 'utf8');
  let tree;
  try {
    tree = parse(source, { ecmaVersion: 'latest', sourceType: 'script' });
  } catch {
    try {
      tree = parse(source, { ecmaVersion: 'latest', sourceType: 'module' });
    } catch (error) {
      failed++;
      console.log(`cannot parse ${file}: ${error.message}`);
      continue;
    }
  }
  for (const table of findTables(tree)) {
    for (const property of table.properties) {
      const id = String(property.key.value ?? property.key.name);
      if (!/^[\w-]+$/.test(id) || seen.has(id)) continue;
      seen.set(id, file);
      const code = source.slice(property.start, property.end);
      writeFileSync(join(modules, `${id}.js`), code);
      const names = new Set();
      for (const pattern of NAME_PATTERNS) for (const [, name] of code.matchAll(pattern)) names.add(name);
      index.push([id, file, code.length, [...names].slice(0, 6).join(',')].join('\t'));
    }
  }
}
writeFileSync(join(snapshot, 'MODULE_INDEX.tsv'), ['id\tchunk\tbytes\tnames', ...index].join('\n') + '\n');
console.log(`${index.length} modules, ${failed} chunks not parsed`);
