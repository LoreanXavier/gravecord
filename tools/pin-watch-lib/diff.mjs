// Compares two fact sets (see facts.mjs). Pure functions: no file or network access except `loadStrings`/`loadParity`.
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const sha = (text) => createHash('sha1').update(text).digest('hex').slice(0, 12);

export function diffRecords(a, b, same = (x, y) => JSON.stringify(x) === JSON.stringify(y)) {
  const out = { added: {}, removed: {}, changed: {} };
  for (const key of Object.keys(b)) {
    if (!(key in a)) out.added[key] = b[key];
    else if (!same(a[key], b[key])) out.changed[key] = { from: a[key], to: b[key] };
  }
  for (const key of Object.keys(a)) if (!(key in b)) out.removed[key] = a[key];
  return out;
}

export function diffSets(a, b) {
  return { added: [...b].filter((x) => !a.has(x)).sort(), removed: [...a].filter((x) => !b.has(x)).sort() };
}

const size = (record) => Object.keys(record).length;
export const isEmpty = (d) => !size(d.added ?? {}) && !size(d.removed ?? {}) && !size(d.changed ?? {});

/** Modules by id and by what they carry, because ids can move. */
export function diffModules(a, b) {
  const added = [];
  const removed = [];
  const changed = [];
  let unchanged = 0;
  for (const [id, m] of Object.entries(b)) {
    const old = a[id];
    if (!old) added.push({ id, ...m });
    else if (old.h === m.h) unchanged++;
    else changed.push({ id, names: m.names, exports: m.exports, chunk: m.chunk, from: old.len, to: m.len, delta: m.len - old.len });
  }
  for (const [id, m] of Object.entries(a)) if (!(id in b)) removed.push({ id, ...m });

  // A module that left one id and appeared at another with identical code, or identical exports and names.
  const moved = [];
  const addedByHash = new Map(added.map((m) => [m.h, m]));
  const signature = (m) => (m.exports.length || m.names.length ? JSON.stringify([m.names, m.exports]) : null);
  const addedBySig = new Map(added.map((m) => [signature(m), m]).filter(([s]) => s));
  const takenAdded = new Set();
  const takenRemoved = new Set();
  for (const r of removed) {
    const same = addedByHash.get(r.h);
    if (same && !takenAdded.has(same.id)) {
      moved.push({ from: r.id, to: same.id, names: r.names, exports: r.exports, identical: true });
      takenAdded.add(same.id);
      takenRemoved.add(r.id);
      continue;
    }
    const sig = signature(r);
    const near = sig && addedBySig.get(sig);
    if (near && !takenAdded.has(near.id)) {
      moved.push({ from: r.id, to: near.id, names: r.names, exports: r.exports, identical: false, delta: near.len - r.len });
      takenAdded.add(near.id);
      takenRemoved.add(r.id);
    }
  }
  return {
    total: { old: size(a), new: size(b) },
    unchanged,
    added: added.filter((m) => !takenAdded.has(m.id)),
    removed: removed.filter((m) => !takenRemoved.has(m.id)),
    changed: changed.sort((x, y) => Math.abs(y.delta) - Math.abs(x.delta)),
    moved,
  };
}

/** Enum-like groups (TS enums, string enums, frozen constant tables). Matched by member names when the module id moves. */
export function diffGroups(oldGroups, newGroups) {
  const key = (g) => sha(`${g.k}|${JSON.stringify(g.mem)}`);
  const pool = new Map();
  for (const g of oldGroups) {
    const k = key(g);
    (pool.get(k) ?? pool.set(k, []).get(k)).push(g);
  }
  const unmatchedNew = [];
  for (const g of newGroups) {
    const bucket = pool.get(key(g));
    if (bucket?.length) bucket.pop();
    else unmatchedNew.push(g);
  }
  const unmatchedOld = [...pool.values()].flat();

  const byName = new Map();
  unmatchedOld.forEach((g, i) => {
    for (const [name] of g.mem) (byName.get(name) ?? byName.set(name, []).get(name)).push(i);
  });
  const used = new Set();
  const matched = [];
  const added = [];
  for (const g of unmatchedNew) {
    const votes = new Map();
    for (const [name] of g.mem) for (const i of byName.get(name) ?? []) if (!used.has(i)) votes.set(i, (votes.get(i) ?? 0) + 1);
    let best = null;
    for (const [i, count] of votes) {
      const score = count / Math.max(g.mem.length, unmatchedOld[i].mem.length);
      if (score >= 0.6 && (!best || score > best.score || (score === best.score && unmatchedOld[i].m === g.m))) best = { i, score };
    }
    if (!best) {
      added.push(g);
      continue;
    }
    used.add(best.i);
    const old = unmatchedOld[best.i];
    const oldMap = new Map(old.mem);
    const newMap = new Map(g.mem);
    matched.push({
      old: old.m,
      new: g.m,
      kind: g.k,
      names: g.mem.slice(0, 3).map(([n]) => n),
      memberCount: g.mem.length,
      all: g.mem,
      added: g.mem.filter(([n]) => !oldMap.has(n)),
      removed: old.mem.filter(([n]) => !newMap.has(n)),
      changed: g.mem.filter(([n, v]) => oldMap.has(n) && oldMap.get(n) !== v).map(([n, v]) => [n, oldMap.get(n), v]),
    });
  }
  const removed = unmatchedOld.filter((_, i) => !used.has(i));
  return { matched, added, removed, identical: newGroups.length - unmatchedNew.length };
}

export function classify(group) {
  const names = new Set(group.mem.map(([n]) => n));
  if (names.has('HELLO') && names.has('IDENTIFY') && names.has('HEARTBEAT')) {
    return names.has('SELECT_PROTOCOL') || names.has('SPEAKING') || names.has('READY') && names.has('SESSION_DESCRIPTION') ? 'voice' : 'gateway';
  }
  if (names.has('SELECT_PROTOCOL') || names.has('SESSION_DESCRIPTION')) return 'voice';
  return null;
}

export function plainMessage(value) {
  if (typeof value === 'string') return value;
  if (Array.isArray(value)) return value.map(plainMessage).join('');
  if (value && typeof value === 'object') return '{…}';
  return String(value ?? '');
}

export function loadStrings(tree) {
  const path = join(tree, 'dumps', 'STRINGS', 'en-US.json');
  if (!existsSync(path)) return null;
  return JSON.parse(readFileSync(path, 'utf8'));
}

export function stringLocales(tree) {
  const dir = join(tree, 'dumps', 'STRINGS');
  if (!existsSync(dir)) return {};
  const out = {};
  for (const name of readdirSync(dir).filter((n) => n.endsWith('.json'))) {
    out[name.slice(0, -5)] = Object.keys(JSON.parse(readFileSync(join(dir, name), 'utf8'))).length;
  }
  return out;
}

export function diffStrings(a, b) {
  if (!a || !b) return null;
  const same = (x, y) => JSON.stringify(x) === JSON.stringify(y);
  const d = diffRecords(a, b, same);
  return {
    total: { old: size(a), new: size(b) },
    added: Object.fromEntries(Object.entries(d.added).map(([k, v]) => [k, plainMessage(v)])),
    removed: Object.fromEntries(Object.entries(d.removed).map(([k, v]) => [k, plainMessage(v)])),
    changed: Object.fromEntries(Object.entries(d.changed).map(([k, { from, to }]) => [k, { from: plainMessage(from), to: plainMessage(to) }])),
  };
}

/** class name -> [{screen, id}] from tests/parity/*.json, the files that say which official class each parity id wraps. */
export function loadParity(root) {
  const dir = join(root, 'tests', 'parity');
  const byClass = new Map();
  if (!existsSync(dir)) return byClass;
  for (const file of readdirSync(dir).filter((n) => n.endsWith('.json'))) {
    let json;
    try {
      json = JSON.parse(readFileSync(join(dir, file), 'utf8'));
    } catch {
      continue;
    }
    const screen = file.slice(0, -5);
    for (const [id, entry] of Object.entries(json)) {
      if (!entry || typeof entry.class !== 'string') continue;
      for (const cls of entry.class.split(/\s+/)) (byClass.get(cls) ?? byClass.set(cls, []).get(cls)).push({ screen, id });
    }
  }
  return byClass;
}

const classesIn = (selector) => [...selector.matchAll(/\.([a-zA-Z][a-zA-Z0-9]*_[A-Za-z0-9_-]{6})(?![\w-])/g)].map((m) => m[1]);
const stripHash = (cls) => cls.replace(/_[A-Za-z0-9_-]{6}$/, '');

export function diffCss(a, b) {
  const rules = diffRecords(a.css, b.css, (x, y) => x === y);
  // A selector that disappears and reappears under another hash suffix is a re-hash, not a style change.
  const baseOf = (selector) => selector.replace(/\.([a-zA-Z][a-zA-Z0-9]*)_[A-Za-z0-9_-]{6}(?![\w-])/g, '.$1');
  const removedByBase = new Map(Object.keys(rules.removed).map((s) => [baseOf(s), s]));
  const rehashed = [];
  for (const selector of Object.keys(rules.added)) {
    const old = removedByBase.get(baseOf(selector));
    if (old && old !== selector) {
      rehashed.push({ from: old, to: selector, sameBody: rules.removed[old] === rules.added[selector] });
      delete rules.removed[old];
      delete rules.added[selector];
      removedByBase.delete(baseOf(selector));
    }
  }
  return {
    selectors: { old: Object.keys(a.css).length, new: Object.keys(b.css).length },
    rules,
    rehashed,
    tokens: diffRecords(a.customProps, b.customProps, (x, y) => x === y),
    keyframes: diffSets(new Set(a.keyframes), new Set(b.keyframes)),
  };
}

/** Which parity screens a CSS or class-table change reaches. */
export function mapToScreens(cssDiff, changedModules, newFacts, parity) {
  const screens = new Map();
  const touch = (cls, how) => {
    for (const hit of parity.get(cls) ?? parity.get(stripHash(cls)) ?? []) {
      const entry = screens.get(hit.screen) ?? screens.set(hit.screen, { screen: hit.screen, ids: new Map() }).get(hit.screen);
      (entry.ids.get(hit.id) ?? entry.ids.set(hit.id, new Set()).get(hit.id)).add(`${how}:${cls}`);
    }
  };
  for (const [kind, set] of [['added', cssDiff.rules.added], ['removed', cssDiff.rules.removed], ['changed', cssDiff.rules.changed]]) {
    for (const selector of Object.keys(set)) for (const cls of classesIn(selector)) touch(cls, `css ${kind}`);
  }
  for (const r of cssDiff.rehashed) for (const cls of classesIn(r.to)) touch(cls, 'css rehashed');
  for (const m of changedModules) for (const cls of newFacts.classesByModule[m.id] ?? []) touch(cls, `module ${m.id}`);
  return [...screens.values()].map((s) => ({ screen: s.screen, ids: [...s.ids].map(([id, why]) => ({ id, why: [...why].slice(0, 6) })) })).sort((x, y) => y.ids.length - x.ids.length);
}

export function holidayDiff(a, b) {
  const key = (h) => h.experiment ?? h.module;
  const oldBy = new Map(a.map((h) => [key(h), h]));
  const newBy = new Map(b.map((h) => [key(h), h]));
  const added = b.filter((h) => !oldBy.has(key(h)));
  const removed = a.filter((h) => !newBy.has(key(h)));
  const changed = b.filter((h) => oldBy.has(key(h)) && JSON.stringify({ ...oldBy.get(key(h)), module: 0 }) !== JSON.stringify({ ...h, module: 0 })).map((h) => ({ from: oldBy.get(key(h)), to: h }));
  return { added, removed, changed };
}
