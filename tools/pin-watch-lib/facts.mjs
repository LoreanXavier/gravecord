// Static facts from an unbundled tree (modules/, assets/, MODULE_INDEX.tsv). Everything is read from source text, so the
// same extractor runs on the pinned snapshot and on a new tree and the two results compare like for like. Nothing is
// executed. The patterns mirror tools/dump-static.mjs, dump-more.mjs and split-modules.mjs.
import { parse } from 'acorn';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

/** Bump when the extraction changes, so cached facts of the pin are rebuilt. */
export const FACTS_VERSION = 3;

const sha = (text) => createHash('sha1').update(text).digest('hex').slice(0, 16);

const PROTO = /super\("(discord_protos\.[\w.]+)",\[([\s\S]*?)\]\)/g;
const FIELD = /no:(\d+),name:"(\w+)",kind:"(\w+)"(?:,(?:T:(\d+)|repeat:\d+|opt:!0|oneof:"\w+"))*/g;
const NAV = /navId:"([\w-]+)"/g;
const ITEM_ID = /[,{]id:"([a-z][a-z0-9-]{2,})"/g;
const ASSET = /^(\d+)\(e(?:,t,n)?\)\{(?:"use strict";)?e\.exports=(?:n\.p\+)?"(?:\/assets\/)?([0-9a-f]+\.(mp3|ogg|png|svg|webp|gif|mp4|webm|lottie|json))"\}/;
const EMBED = /\.(GIFV|ARTICLE|LINK|RICH|VIDEO|IMAGE|TWEET|AUTO_MODERATION_MESSAGE|AUTO_MODERATION_NOTIFICATION|POLL_RESULT|APPLICATION_NEWS|POST_PREVIEW|SAFETY_POLICY_NOTICE|SAFETY_SYSTEM_NOTIFICATION|VOICE_CHANNEL|GAMING_PROFILE)="([a-z_]+)"/g;
const NUM_ENUM = /(?:\b(\w+)|\((\w+)=\{\}\))\[(\w+)\.([A-Z][A-Za-z0-9_]*)=(-?\d+)\]="\4"/g;
const STR_ENUM = /\(\((\w+)=\{\}\)\.([A-Za-z_][\w]*)="((?:[^"\\]|\\.)*)"((?:,\1\.[A-Za-z_]\w*="(?:[^"\\]|\\.)*")*)/g;
const STR_ENUM_MEMBER = /\.([A-Za-z_]\w*)="((?:[^"\\]|\\.)*)"/g;
const FROZEN = /Object\.freeze\(\{((?:[A-Z][A-Z0-9_]*:(?:-?\d+(?:\.\d+)?(?:e\d+)?|"[^"]*"|!0|!1),?){2,})\}\)/g;
const FROZEN_MEMBER = /([A-Z][A-Z0-9_]*):(-?\d+(?:\.\d+)?(?:e\d+)?|"[^"]*"|!0|!1)/g;
const EXPERIMENT = /\bname:"(\d{4}-\d{2}[_-][\w-]+)",kind:"(\w+)"/g;
const STORE = /displayName="?'?([A-Za-z0-9_-]+Store)"?'?/g;
const ACTION_TYPE = /\btype:"([A-Z][A-Z0-9_]{2,})"/g;
const ACTION_KEY = /[{,]([A-Z][A-Z0-9_]{2,}):(?:[a-zA-Z_$][\w$]*|\(|function)/g;
const CLASS_NAME = /"([a-zA-Z][a-zA-Z0-9]*_[A-Za-z0-9_-]{6})"/g;

/** The settings-id enum is reached through a minified export letter that changes per build, so it is learned from these anchors. */
const ANCHOR_REF = /\.([A-Za-z_$]{1,3})\.(?:ACCOUNT_DELETE_SETTING|ACCESSIBILITY_PANEL)(?![A-Z_0-9])/g;

function enumGroups(code, moduleId) {
  const groups = [];
  let last = null;
  let end = -1;
  for (const m of code.matchAll(NUM_ENUM)) {
    const variable = m[1] ?? m[2];
    if (variable !== m[3]) continue;
    const pair = [m[4], Number(m[5])];
    if (last && last.variable === variable && m.index - end <= 1) last.mem.push(pair);
    else groups.push((last = { variable, m: moduleId, k: 'n', mem: [pair] }));
    end = m.index + m[0].length;
  }
  for (const m of code.matchAll(STR_ENUM)) {
    const mem = [[m[2], m[3]]];
    for (const [, name, value] of m[4].matchAll(STR_ENUM_MEMBER)) mem.push([name, value]);
    groups.push({ m: moduleId, k: 's', mem });
  }
  for (const m of code.matchAll(FROZEN)) {
    const mem = [...m[1].matchAll(FROZEN_MEMBER)].map(([, name, value]) => [name, value.startsWith('"') ? value.slice(1, -1) : Number(value.replace('!0', '1').replace('!1', '0'))]);
    groups.push({ m: moduleId, k: 'f', mem });
  }
  return groups.filter((g) => g.mem.length >= 2).map(({ variable, ...g }) => g);
}

/** `endpoints`: the route table, rendered as name -> template with `:argN` for every template argument. */
function endpointTable(code) {
  let tree;
  try {
    tree = parse(`(function(){({${code}})})`, { ecmaVersion: 'latest' });
  } catch {
    return {};
  }
  let table = null;
  const stack = [tree];
  while (stack.length && !table) {
    const node = stack.pop();
    if (!node || typeof node.type !== 'string') continue;
    if (node.type === 'ObjectExpression') {
      const keys = new Set(node.properties.map((p) => p.key?.name ?? p.key?.value));
      if (keys.has('USER_CHANNELS') && keys.has('MESSAGES') && keys.has('GUILD_CHANNELS')) {
        table = node;
        break;
      }
    }
    for (const value of Object.values(node)) {
      if (Array.isArray(value)) stack.push(...value);
      else if (value && typeof value.type === 'string') stack.push(value);
    }
  }
  const out = {};
  if (!table) return out;
  const source = `(function(){({${code}})})`;
  const templateOf = (value) => {
    if (value.type === 'TemplateLiteral') return value;
    if (!/Function/.test(value.type)) return null;
    if (value.body.type === 'TemplateLiteral') return value.body;
    if (value.body.type === 'BlockStatement') {
      const ret = value.body.body.filter((s) => s.type === 'ReturnStatement').at(-1);
      if (ret?.argument?.type === 'TemplateLiteral') return ret.argument;
    }
    return null;
  };
  for (const property of table.properties) {
    const name = property.key?.name ?? property.key?.value;
    const value = property.value;
    if (!name) continue;
    if (value.type === 'Literal' && typeof value.value === 'string') {
      out[name] = value.value;
      continue;
    }
    if (/Function/.test(value.type) && value.body.type === 'Literal' && typeof value.body.value === 'string') {
      out[name] = value.body.value;
      continue;
    }
    const template = templateOf(value);
    if (!template) {
      out[name] = `<${value.type}> ${source.slice(value.start, value.end).slice(0, 160)}`;
      continue;
    }
    // Arguments are numbered by parameter order, then by first use for locals that default a missing argument.
    const slots = (value.params ?? []).map((p) => (p.type === 'AssignmentPattern' ? p.left.name : p.name));
    const slot = (id) => `:arg${slots.includes(id) ? slots.indexOf(id) : slots.push(id) - 1}`;
    let text = '';
    template.quasis.forEach((q, i) => {
      text += q.value.cooked;
      let expr = template.expressions[i];
      if (!expr) return;
      if (expr.type === 'LogicalExpression' && expr.left.type === 'Identifier') expr = expr.left;
      text += expr.type === 'Identifier' ? slot(expr.name) : `{${source.slice(expr.start, expr.end)}}`;
    });
    out[name] = text;
  }
  return out;
}

/** CSS rules by selector. Hashed class suffixes are kept: they are what tests/parity/*.json names. */
function cssRules(dir) {
  const rules = new Map();
  const customProps = new Map();
  const keyframes = new Set();
  for (const name of readdirSync(dir).filter((n) => n.endsWith('.css'))) {
    const css = readFileSync(join(dir, name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
    for (const [, frame] of css.matchAll(/@keyframes\s+([\w-]+)/g)) keyframes.add(frame);
    for (const [, selector, body] of css.matchAll(/([^{}@;]+)\{([^{}]*)\}/g)) {
      const key = selector.trim().replace(/\s+/g, ' ');
      if (!key || key.length > 600) continue;
      // Keyframe steps are not selectors; they repeat in every @keyframes block and would merge across files.
      if (/^(from|to|\d+(\.\d+)?%)(\s*,\s*(from|to|\d+(\.\d+)?%))*$/.test(key)) continue;
      rules.set(key, (rules.get(key) ?? '') + body.trim());
      for (const [, prop, value] of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+)/g)) {
        const scope = `${key} ${prop}`;
        customProps.set(scope, value.trim());
      }
    }
  }
  return { rules, customProps, keyframes };
}

export function extractFacts(tree, log = () => {}) {
  const modulesDir = join(tree, 'modules');
  const index = new Map();
  const indexPath = join(tree, 'MODULE_INDEX.tsv');
  if (existsSync(indexPath)) {
    for (const line of readFileSync(indexPath, 'utf8').split('\n').slice(1)) {
      const [id, chunk, bytes, names] = line.split('\t');
      if (id) index.set(id, { chunk, bytes: Number(bytes), names: names ? names.split(',') : [] });
    }
  }
  const facts = {
    modules: {},
    stores: new Set(),
    actions: new Set(),
    groups: [],
    protos: {},
    menus: {},
    sounds: {},
    images: {},
    embeds: new Set(),
    experiments: {},
    soundTables: {},
    holidays: [],
    classesByModule: {},
    endpoints: {},
  };
  const files = readdirSync(modulesDir);
  const sources = new Map();
  const anchorCodes = [];
  for (const file of files) {
    const id = file.slice(0, -3);
    const code = readFileSync(join(modulesDir, file), 'utf8');
    const normalized = code.replace(/\bn\(\d+\)/g, 'n(#)').replace(/\bn\.e\(\d+\)/g, 'n.e(#)').replace(/^[\w-]+\(/, '(');
    const exportsMatch = /n\.d\(t,\{([^}]*)\}/.exec(code);
    const exportNames = exportsMatch ? [...exportsMatch[1].matchAll(/(?:^|,)(\w+):/g)].map((m) => m[1]) : [];
    const meta = index.get(id);
    facts.modules[id] = { h: sha(normalized), len: code.length, chunk: meta?.chunk ?? '', names: meta?.names ?? [], exports: exportNames };

    for (const [, store] of code.matchAll(STORE)) facts.stores.add(store);
    for (const [, type] of code.matchAll(ACTION_TYPE)) facts.actions.add(type);
    for (const [, key] of code.matchAll(ACTION_KEY)) facts.actions.add(key);
    for (const [, name, body] of code.matchAll(PROTO)) {
      facts.protos[name] = [...body.matchAll(FIELD)].map(([, no, field, kind, scalar]) => `${no}:${field}:${kind}${scalar ? `:T${scalar}` : ''}`);
    }
    const navs = [...code.matchAll(NAV)].map((m) => m[1]);
    if (navs.length) {
      const items = [...new Set([...code.matchAll(ITEM_ID)].map((m) => m[1]))];
      for (const nav of navs) facts.menus[nav] = { module: id, items };
    }
    const asset = ASSET.exec(code);
    if (asset) (asset[3] === 'mp3' || asset[3] === 'ogg' ? facts.sounds : facts.images)[id] = asset[2];
    for (const [, name, value] of code.matchAll(EMBED)) facts.embeds.add(`${name}=${value}`);
    for (const [, name, kind] of code.matchAll(EXPERIMENT)) facts.experiments[name] = { kind, module: id };
    facts.groups.push(...enumGroups(code, id));

    // Class names a module carries, kept only for style modules (mostly class-name tables).
    const classes = [...new Set([...code.matchAll(CLASS_NAME)].map((m) => m[1]))];
    if (classes.length >= 3 && code.length < 60000) facts.classesByModule[id] = classes;

    if (code.includes('ACCOUNT_DELETE_SETTING') || code.includes('ACCESSIBILITY_PANEL')) anchorCodes.push(code);
    if (code.includes('startTimeMs:') && code.includes('endTimeMs:')) sources.set(id, code);
    if (/\[\w+\.\w+\.[A-Z_]+\]:\{/.test(code) && code.includes('message1')) sources.set(`sound:${id}`, code);
    if (id === '652215' || (code.includes('USER_CHANNELS:') && code.includes('GUILD_CHANNELS:') && !Object.keys(facts.endpoints).length)) {
      Object.assign(facts.endpoints, endpointTable(code));
    }
  }

  // Seasonal events: each entry carries a window, an experiment, and a soundpack enum member.
  const resolveEnum = (code, alias, member) => {
    const ref = new RegExp(`\\b${alias.replace('$', '\\$')}=n\\((\\d+)\\)`).exec(code);
    if (!ref) return undefined;
    const target = sources.get(ref[1]) ?? readFileSync(join(modulesDir, `${ref[1]}.js`), 'utf8');
    return new RegExp(`\\.${member}="([^"]*)"`).exec(target)?.[1];
  };
  for (const [key, code] of sources) {
    if (key.startsWith('sound:')) {
      for (const [, alias, , member, body] of code.matchAll(/\[(\w+)\.(\w+)\.([A-Z_]+)\]:\{([^{}]*)\}/g)) {
        facts.soundTables[member] = Object.fromEntries([...body.matchAll(/(\w+):"([^"]*)"/g)].map((m) => [m[1], m[2]]));
      }
      continue;
    }
    for (const m of code.matchAll(/startTimeMs:([^,]+),endTimeMs:([^,]+)/g)) {
      const start = Number(m[1]);
      const end = Number(m[2]);
      if (!Number.isFinite(start) || !Number.isFinite(end)) continue;
      const window = code.slice(Math.max(0, m.index - 600), m.index + 1200);
      const soundpack = /soundpack:(\w+)\.(\w+)\.([A-Z_]+)/.exec(window);
      facts.holidays.push({
        module: key,
        experiment: /name:"(\d{4}-\d{2}[_-][\w-]*)"/.exec(code)?.[1] ?? null,
        startTimeMs: start,
        endTimeMs: end,
        start: new Date(start).toISOString(),
        end: new Date(end).toISOString(),
        soundpack: soundpack ? (resolveEnum(code, soundpack[1], soundpack[3]) ?? soundpack[3]) : null,
        soundpackMember: soundpack?.[3] ?? null,
        desktopOnly: /isDesktopOnly:!0/.test(window),
        effects: [...code.matchAll(/\b([A-Z][A-Z_]+)=\d\]="\1"/g)].map((e) => e[1]),
      });
    }
  }

  const assetsDir = join(tree, 'assets');
  const css = existsSync(assetsDir) ? cssRules(assetsDir) : { rules: new Map(), customProps: new Map(), keyframes: new Set() };
  facts.css = Object.fromEntries(css.rules);
  facts.customProps = Object.fromEntries(css.customProps);
  facts.keyframes = [...css.keyframes].sort();

  // Settings ids are referenced as `<letter>.NAME` where the letter is the build's minified export. The letter is learned
  // from the anchor ids, then every reference through that letter is collected (the pinned build: `X`).
  const letters = new Set(); 
  for (const code of anchorCodes) for (const [, letter] of code.matchAll(ANCHOR_REF)) letters.add(letter);
  const settings = new Set();
  if (letters.size) {
    const alternatives = [...letters].map((l) => l.replace(/\$/g, '\\$&')).join('|');
    const refs = new RegExp('\\.(?:' + alternatives + ')\\.([A-Z][A-Z_0-9]{3,})', 'g');
    for (const file of files) for (const [, name] of readFileSync(join(modulesDir, file), 'utf8').matchAll(refs)) settings.add(name);
  }
  facts.settings = settings;
  log(`facts: ${files.length} modules, ${facts.groups.length} enum-like groups, ${Object.keys(facts.endpoints).length} routes, ${facts.holidays.length} timed events`);
  return facts;
}

/** JSON-safe form for the cache. */
export function serialize(facts) {
  return JSON.stringify(facts, (_k, v) => (v instanceof Set ? { __set: [...v] } : v));
}
export function deserialize(text) {
  return JSON.parse(text, (_k, v) => (v && v.__set ? new Set(v.__set) : v));
}
