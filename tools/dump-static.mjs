// Static scan of every downloaded chunk, for what the live dump cannot reach: stores and action types in lazy chunks
// that have not executed, and the design tokens in every stylesheet. Merges with the live dumps.
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dirname, '..', 'design');
const assets = join(root, 'snapshot', 'assets');
const dumps = join(root, 'snapshot', 'dumps');
const tokensDir = join(root, 'tokens');
mkdirSync(tokensDir, { recursive: true });

const names = readdirSync(assets);
const scripts = names.filter((name) => name.endsWith('.js'));
const styles = names.filter((name) => name.endsWith('.css'));

const actionTypes = new Set();
const storeNames = new Set();
for (const name of scripts) {
  const source = readFileSync(join(assets, name), 'utf8');
  for (const [, type] of source.matchAll(/\btype:"([A-Z][A-Z0-9_]{2,})"/g)) actionTypes.add(type);
  for (const [, handler] of source.matchAll(/[{,]([A-Z][A-Z0-9_]{2,}):(?:[a-zA-Z_$][\w$]*|\(|function)/g)) actionTypes.add(handler);
  for (const [, store] of source.matchAll(/displayName="?'?([A-Za-z0-9_-]+Store)"?'?/g)) storeNames.add(store);
}

const live = (file) => JSON.parse(readFileSync(join(dumps, file), 'utf8'));
const liveActions = new Set(live('actions.live.json'));
const liveNodes = Object.keys(live('dispatcher-nodes.json'));
const staticOnlyStores = [...storeNames].filter((name) => !liveNodes.includes(name)).sort();
const staticOnlyActions = [...actionTypes].filter((name) => !liveActions.has(name)).sort();
writeFileSync(join(dumps, 'stores.static-only.json'), JSON.stringify(staticOnlyStores, null, 1));
writeFileSync(join(dumps, 'actions.static-candidates.json'), JSON.stringify(staticOnlyActions, null, 1));

// Custom properties grouped by the selector that sets them.
const bySelector = new Map();
const keyframes = new Set();
const fontFaces = [];
for (const name of styles) {
  const css = readFileSync(join(assets, name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
  for (const [, frame] of css.matchAll(/@keyframes\s+([\w-]+)/g)) keyframes.add(frame);
  for (const [block] of css.matchAll(/@font-face\s*\{[^}]*\}/g)) fontFaces.push(block);
  for (const [, selector, body] of css.matchAll(/([^{}@;]+)\{([^{}]*--[^{}]*)\}/g)) {
    const key = selector.trim();
    const values = bySelector.get(key) ?? {};
    for (const [, prop, value] of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+)/g)) values[prop] = value.trim();
    bySelector.set(key, values);
  }
}
const ranked = [...bySelector.entries()].sort((a, b) => Object.keys(b[1]).length - Object.keys(a[1]).length);
writeFileSync(join(tokensDir, 'by-selector.json'), JSON.stringify(Object.fromEntries(ranked), null, 1));
writeFileSync(join(tokensDir, 'keyframes.json'), JSON.stringify([...keyframes].sort(), null, 1));
writeFileSync(join(tokensDir, 'font-faces.json'), JSON.stringify([...new Set(fontFaces)], null, 1));

console.log(`scripts ${scripts.length}, styles ${styles.length}`);
console.log(`stores: ${liveNodes.length} live nodes, ${staticOnlyStores.length} more in static code`);
console.log(`actions: ${liveActions.size} live, ${staticOnlyActions.length} static candidates (unverified)`);
console.log(`selectors with tokens ${bySelector.size}, keyframes ${keyframes.size}, font faces ${new Set(fontFaces).size}`);
for (const [selector, values] of ranked.slice(0, 10)) {
  console.log(`  ${Object.keys(values).length} ${selector.slice(0, 90)}`);
}
