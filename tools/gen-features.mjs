// Adds one FEATURES.md row per dump item that has none. Existing rows, ids and statuses are never changed or removed.
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dirname, '..');
const dumps = join(root, 'design', 'snapshot', 'dumps');
const read = (name) => JSON.parse(readFileSync(join(dumps, name), 'utf8'));

function nativeLeaves(value, path, out) {
  if (typeof value === 'string') {
    if (value.startsWith('function')) out.push(path);
    return out;
  }
  for (const [key, child] of Object.entries(value ?? {})) nativeLeaves(child, path ? `${path}.${key}` : key, out);
  return out;
}

const items = [];
// Sources are compared as trimmed table cells; one official store name ends in a space.
const add = (area, feature, source, notes = '') => items.push({ area, feature: feature.trim(), source: source.trim(), notes });

const nodes = read('dispatcher-nodes.json');
const live = read('stores.live.json');
for (const name of Object.keys(nodes).sort()) {
  const notes = [
    name.length < 4 ? 'minified name, resolve from static code' : '',
    name in live ? '' : 'no store object reached, methods not dumped',
  ].filter(Boolean).join('; ');
  add('store', `Port ${name}`, `STORES:${name}`, notes);
}
for (const name of read('stores.static-only.json')) add('store', `Port ${name}`, `STORES:${name}`, 'lazy chunk, static only');
for (const type of read('actions.live.json')) add('action', `Handle ${type}`, `ACTION_TYPES:${type}`);
for (const [name, route] of Object.entries(read('endpoints.live.json')).sort()) {
  add('route', `Route ${name}`, `ENDPOINTS:${name}`, (route.template ?? '').replaceAll('|', '/'));
}
for (const path of nativeLeaves(read('discord-native.json'), '', []).sort()) add('native', `Native ${path}`, `DISCORD_NATIVE:${path}`);

for (const [action] of read('KEYBINDS.md').actions) add('keybind', `Keybind action ${action}`, `KEYBINDS:${action}`);
for (const [menu, { itemsInModule }] of Object.entries(read('CONTEXT_MENUS.md')).sort()) {
  add('menu', `Context menu ${menu}`, `CONTEXT_MENUS:${menu}`, `${itemsInModule.length} item ids in its module`);
}
for (const id of read('SETTINGS.md').ids) add('setting', `Setting ${id}`, `SETTINGS:${id}`);
for (const [id, name] of Object.entries(read('SOUNDS.md')).sort()) add('sound', `Sound ${name}`, `SOUNDS:${id}`);
for (const type of read('EMBEDS.md').types) add('embed', `Embed type ${type}`, `EMBEDS:${type}`);
for (const file of readdirSync(join(dumps, 'PROTOS')).sort()) {
  const name = file.replace(/\.json$/, '');
  add('proto', `Proto ${name}`, `PROTOS:${name}`);
}

const file = join(root, 'FEATURES.md');
const text = readFileSync(file, 'utf8');
const rows = text.split(/\r?\n/).filter((line) => /^\| F-\d{4,} \|/.test(line));
const known = new Set(rows.map((row) => row.split('|')[4].trim()));
let next = rows.reduce((high, row) => Math.max(high, Number(row.slice(4, row.indexOf(' ', 4)))), 0) + 1;

const added = [];
for (const { area, feature, source, notes } of items) {
  if (known.has(source)) continue;
  known.add(source);
  added.push(`| F-${String(next++).padStart(4, '0')} | ${area} | ${feature} | ${source} | todo | | ${notes} |`);
}
writeFileSync(file, text.replace(/\s*$/, '\n') + added.map((row) => row + '\n').join(''));
console.log(`${items.length} dump items, ${added.length} rows added, ${rows.length + added.length} rows total`);
