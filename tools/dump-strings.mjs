// Extracts every message catalog into design/snapshot/dumps/STRINGS/. Chunks are parsed, never executed.
// A catalog is a module holding `JSON.parse('{...}')` with the known key hPBScv. Locales come from the bundle's own loader
// map, so nothing is inferred from the language of the text.
import { parse } from 'acorn';
import { mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const snapshot = join(import.meta.dirname, '..', 'design', 'snapshot');
const assets = join(snapshot, 'assets');
const modules = join(snapshot, 'modules');
const out = join(snapshot, 'dumps', 'STRINGS');
mkdirSync(out, { recursive: true });

function literals(node, found) {
  const stack = [node];
  while (stack.length) {
    const current = stack.pop();
    if (!current || typeof current.type !== 'string') continue;
    if (
      current.type === 'CallExpression' &&
      current.callee.type === 'MemberExpression' &&
      current.callee.object.name === 'JSON' &&
      current.arguments[0]?.type === 'Literal' &&
      typeof current.arguments[0].value === 'string'
    ) {
      found.push(current.arguments[0].value);
    }
    for (const value of Object.values(current)) {
      if (Array.isArray(value)) stack.push(...value);
      else if (value && typeof value.type === 'string') stack.push(value);
    }
  }
}

const index = readFileSync(join(snapshot, 'MODULE_INDEX.tsv'), 'utf8').split('\n').slice(1).map((line) => line.split('\t'));
const catalogs = new Map();
for (const file of readdirSync(assets).filter((name) => name.endsWith('.js'))) {
  const source = readFileSync(join(assets, file), 'utf8');
  if (!source.includes('hPBScv')) continue;
  const moduleIds = index.filter(([, chunk]) => chunk === file).map(([id]) => id);
  for (const id of moduleIds) {
    const code = readFileSync(join(modules, `${id}.js`), 'utf8');
    if (!code.includes('hPBScv')) continue;
    const found = [];
    literals(parse(`(function(){({${code}})})`, { ecmaVersion: 'latest' }), found);
    const body = found.find((text) => text.includes('"hPBScv"'));
    if (body) catalogs.set(id, { file, messages: JSON.parse(body) });
  }
}

// Module 802249 maps locales to catalogs: `"zh-CN":()=>n.e(836958).then(n.t.bind(n,184237,19))`.
const loader = readFileSync(join(modules, '802249.js'), 'utf8');
const locales = new Map();
for (const [, locale, id] of loader.matchAll(/"?([a-zA-Z]{2}(?:-[a-zA-Z0-9]{2,4})?)"?:\(\)=>n\.e\(\d+\)\.then\(n\.(?:t\.)?bind\(n,(\d+)/g)) {
  locales.set(id, locale);
}

const stale = readdirSync(out);
for (const name of stale) rmSync(join(out, name));
const summary = [];
for (const [id, { messages }] of catalogs) {
  const locale = locales.get(id) ?? `unknown-${id}`;
  writeFileSync(join(out, `${locale}.json`), JSON.stringify(messages));
  summary.push(`${locale}:${Object.keys(messages).length}`);
}
console.log(`${catalogs.size} catalogs`);
console.log(summary.sort().join(' '));
