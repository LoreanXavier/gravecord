// Behavior dumps from the running clean client into design/snapshot/dumps/. Read-only: store and native members are
// listed by name and type, nothing is called, so tokens and account data never enter a dump.
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';

const dumps = join(import.meta.dirname, '..', 'design', 'snapshot', 'dumps');
mkdirSync(dumps, { recursive: true });

const page = await connect();
// Serialized in the page: CDP's returnByValue silently drops entries from objects this large.
const data = JSON.parse(await page.evaluate(`(() => { ${PRELUDE}
  const stores = findStores();

  const dispatcher = [...stores.values()].find(s => s._dispatcher)?._dispatcher;
  const graph = dispatcher?._actionHandlers;
  const handlersByStore = {};
  const nodeDump = {};
  for (const [token, node] of graph?._nodes ?? []) {
    handlersByStore[node.name] = Object.keys(node.actionHandler ?? {}).sort();
    nodeDump[node.name] = { token, band: node.band, dependencies: [...(node.dependencies ?? [])] };
  }

  const storeDump = {};
  for (const [name, store] of stores) {
    const methods = new Set();
    for (let p = Object.getPrototypeOf(store); p && p !== Object.prototype; p = Object.getPrototypeOf(p)) {
      if (Object.prototype.hasOwnProperty.call(p, 'addChangeListener')) break;
      for (const key of Object.getOwnPropertyNames(p)) if (key !== 'constructor') methods.add(key);
    }
    storeDump[name] = { methods: [...methods].sort(), actions: handlersByStore[name] ?? [] };
  }

  // The route table: the largest export whose keys include the routes every build has.
  let endpoints = null;
  for (const m of Object.values(req.c)) {
    const e = m?.exports;
    if (!e || typeof e !== 'object') continue;
    for (const key of Object.keys(e)) {
      let v;
      try { v = e[key]; } catch { continue; }
      if (v && typeof v === 'object' && 'USER_CHANNELS' in v && 'MESSAGES' in v && 'GUILD_CHANNELS' in v) {
        if (!endpoints || Object.keys(v).length > Object.keys(endpoints).length) endpoints = v;
      }
    }
  }
  const routes = {};
  for (const [name, value] of Object.entries(endpoints ?? {})) {
    if (typeof value === 'string') { routes[name] = { template: value }; continue; }
    if (typeof value !== 'function') continue;
    const args = Array.from({ length: 8 }, (_, i) => ':arg' + i);
    let template = null;
    try { template = decodeURIComponent(String(value(...args))); } catch {}
    routes[name] = { template, source: value.toString() };
  }

  const describe = (value, depth, seen) => {
    if (value === null) return 'null';
    if (typeof value === 'function') return 'function/' + value.length;
    if (typeof value !== 'object') return typeof value;
    if (seen.has(value)) return 'circular';
    if (depth > 6) return 'object';
    seen.add(value);
    const out = {};
    for (const key of Object.keys(value)) {
      const descriptor = Object.getOwnPropertyDescriptor(value, key);
      out[key] = descriptor && 'value' in descriptor ? describe(descriptor.value, depth + 1, seen) : 'accessor';
    }
    return out;
  };

  return JSON.stringify({
    stores: storeDump,
    subscribed: Object.keys(dispatcher?._subscriptions ?? {}).sort(),
    tokenActions: Object.keys(graph?._tokensByActionType ?? {}).sort(),
    nodes: nodeDump,
    routes,
    native: describe(window.DiscordNative, 0, new Set()),
    moduleCounts: { factories: Object.keys(req.m).length, executed: Object.keys(req.c).length },
  });
})()`));
page.close();

const count = (value) => Object.keys(value ?? {}).length;
writeFileSync(join(dumps, 'stores.live.json'), JSON.stringify(data.stores, null, 1));
writeFileSync(join(dumps, 'endpoints.live.json'), JSON.stringify(data.routes, null, 1));
writeFileSync(join(dumps, 'discord-native.json'), JSON.stringify(data.native, null, 1));

const actions = new Set([...data.subscribed, ...data.tokenActions]);
for (const store of Object.values(data.stores)) for (const action of store.actions) actions.add(action);
writeFileSync(join(dumps, 'actions.live.json'), JSON.stringify([...actions].sort(), null, 1));

const withHandlers = Object.values(data.stores).filter((store) => store.actions.length).length;
console.log(`stores ${count(data.stores)} (${withHandlers} with handler maps)`);
console.log(`actions ${actions.size}`);
console.log(`routes ${count(data.routes)}`);
console.log(`native top level: ${Object.keys(data.native ?? {}).join(' ')}`);
console.log(`modules ${JSON.stringify(data.moduleCounts)}`);
const nodeNames = Object.keys(data.nodes);
const unresolved = nodeNames.filter((name) => !(name in data.stores));
writeFileSync(join(dumps, 'dispatcher-nodes.json'), JSON.stringify(data.nodes, null, 1));
console.log(`dispatcher nodes ${nodeNames.length}, without a found store object: ${unresolved.length}`);
console.log(unresolved.slice(0, 25).join(' '));
