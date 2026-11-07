// One pass over every module, plus live reads, for the dumps that do not need UI traversal.
// Output goes to design/snapshot/dumps/. Items found by pattern are marked so they are not mistaken for verified lists.
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';

const snapshot = join(import.meta.dirname, '..', 'design', 'snapshot');
const dumps = join(snapshot, 'dumps');
const modulesDir = join(snapshot, 'modules');
mkdirSync(join(dumps, 'PROTOS'), { recursive: true });
const save = (name, value) => writeFileSync(join(dumps, name), typeof value === 'string' ? value : JSON.stringify(value, null, 1));

const protos = {};
const menus = {};
const settings = new Set();
const sounds = {};
const images = {};
const embedTypes = new Set();
const enums = {};

const PROTO = /super\("(discord_protos\.[\w.]+)",\[([\s\S]*?)\]\)/g;
const FIELD = /no:(\d+),name:"(\w+)",kind:"(\w+)"(?:,(?:T:(\d+)|repeat:\d+|opt:!0|oneof:"\w+"))*/g;
const NAV = /navId:"([\w-]+)"/g;
const ITEM_ID = /[,{]id:"([a-z][a-z0-9-]{2,})"/g;
const SETTING = /\.X\.([A-Z][A-Z_0-9]{3,})/g;
const ASSET = /^(\d+)\(e(?:,t,n)?\)\{(?:"use strict";)?e\.exports=(?:n\.p\+)?"(?:\/assets\/)?([0-9a-f]+\.(mp3|ogg|png|svg|webp|gif|mp4|webm|lottie|json))"\}/;
const EMBED = /\.(GIFV|ARTICLE|LINK|RICH|VIDEO|IMAGE|TWEET|AUTO_MODERATION_MESSAGE|AUTO_MODERATION_NOTIFICATION|POLL_RESULT|APPLICATION_NEWS|POST_PREVIEW|SAFETY_POLICY_NOTICE|SAFETY_SYSTEM_NOTIFICATION|VOICE_CHANNEL|GAMING_PROFILE)="([a-z_]+)"/g;
const ENUM_MEMBER = /\[(?:[a-z]\.)([A-Z][A-Z_0-9]+)=(\d+)\]="\1"/g;

const files = readdirSync(modulesDir);
for (const file of files) {
  const id = file.slice(0, -3);
  const code = readFileSync(join(modulesDir, file), 'utf8');

  for (const [, name, body] of code.matchAll(PROTO)) {
    protos[name] = [...body.matchAll(FIELD)].map(([, no, field, kind, scalar]) => ({ no: Number(no), name: field, kind, scalar: scalar ? Number(scalar) : undefined }));
  }
  const navs = [...code.matchAll(NAV)].map((match) => match[1]);
  if (navs.length) {
    const items = [...new Set([...code.matchAll(ITEM_ID)].map((match) => match[1]))];
    for (const nav of navs) menus[nav] = { module: id, itemsInModule: items };
  }
  for (const [, name] of code.matchAll(SETTING)) settings.add(name);
  const asset = ASSET.exec(code);
  if (asset) (asset[3] === 'mp3' || asset[3] === 'ogg' ? sounds : images)[id] = asset[2];
  for (const [, name, value] of code.matchAll(EMBED)) embedTypes.add(`${name}=${value}`);
  if (id === '921747' || code.includes('SELECT_PROTOCOL_ACK')) {
    enums[id] = [...code.matchAll(ENUM_MEMBER)].map(([, name, value]) => [name, Number(value)]);
  }
}

const page = await connect();
const live = JSON.parse(await page.evaluate(`(() => { ${PRELUDE}
  const stores = findStores();
  const constants = req(652215);
  const flat = {};
  const seen = new Set();
  const walk = (value, path, depth) => {
    if (typeof value === 'number' || typeof value === 'string' || typeof value === 'boolean') { flat[path] = value; return; }
    if (!value || typeof value !== 'object' || seen.has(value) || depth > 3) return;
    seen.add(value);
    let keys;
    try { keys = Object.keys(value); } catch { return; }
    if (keys.length > 4000) return;
    for (const key of keys) {
      let child;
      try { child = value[key]; } catch { continue; }
      walk(child, path ? path + '.' + key : key, depth + 1);
    }
  };
  walk(constants, '', 0);
  // PUSH_TO_TALK also appears in the RPC and input-mode enums; only keybind actions have the overlay lock.
  const keybindEnum = Object.keys(flat).find((k) => k.endsWith('.TOGGLE_OVERLAY_INPUT_LOCK')).replace(/\\.[A-Z_]+$/, '');
  const keybinds = Object.entries(flat).filter(([k]) => k.startsWith(keybindEnum + '.')).map(([k, v]) => [k.slice(keybindEnum.length + 1), v]);
  const experiments = stores.get('ExperimentStore');
  return JSON.stringify({ flat, keybinds, experimentMethods: experiments ? Object.getOwnPropertyNames(Object.getPrototypeOf(experiments)) : [] });
})()`));
page.close();

const trafficFile = join(snapshot, 'TRAFFIC', 'identify.json');
const ready = JSON.parse(readFileSync(trafficFile, 'utf8')).frames.find((frame) => frame.event === 'READY').d;

save('CONSTANTS.md', live.flat);
save('KEYBINDS.md', { actions: live.keybinds });
save('EXPERIMENTS.md', { experiments: ready.experiments, guild_experiments: ready.guild_experiments, apex_experiments: ready.apex_experiments });
save('CONTEXT_MENUS.md', menus);
save('SETTINGS.md', { source: 'pattern: every .X.<NAME> settings id in the bundle', ids: [...settings].sort() });
save('SOUNDS.md', sounds);
save('IMAGES.md', images);
save('EMBEDS.md', { source: 'pattern: embed type members', types: [...embedTypes].sort() });
save('VOICE.md', {
  status: 'partial: the voice gateway and its opcodes live in the native discord_voice module, not in the bundle',
  source: 'pattern: numeric enums in modules that use the voice protocol',
  enums,
});
for (const [name, fields] of Object.entries(protos)) save(join('PROTOS', `${name}.json`), fields);

save(
  'GATEWAY.md',
  `# Gateway, build 667580

Source: module 868 (\`GatewaySocket\`), \`TRAFFIC/identify.json\`.

- URL: \`wss://gateway.discord.gg/?encoding=etf&v=9&compress=zstd-stream\`, then READY's \`resume_gateway_url\`.
- Handshake: Origin \`https://discordapp.com\`, the pinned User-Agent.
- Encoding out: ETF, strings and keys as binaries, null as the atom \`nil\`, arrays as lists.
- Encoding in: binaries as UTF-8, \`STRING_EXT\` as bytes, nil as \`[]\`, integers over 4 bytes as decimal strings.
- Compression: one zstd stream per socket, one flushed block per message.
- Heartbeat: op 40 \`{seq, qos: {active, ver: 31, reasons}}\`. First beat after a random part of the interval.
- Ack timeout: a beat with no ack since the last one reconnects.
- HELLO timeout: 30 s. Resume window: 3 minutes since the last ack.
- Backoff: min 1 s, max 60 s, \`current += 2 * current * random\`. More than 4 fails resets the session.
- Close 4004: authentication failed, no reconnect.
- capabilities: 1734653, or 1767421 with channel obfuscation. The pinned session sent 1767421.
- IDENTIFY: token, capabilities, properties, presence, compress, client_state, qos_token.
- qos_token: base64 \`QosToken{client_provided{is_active}, derived}\`, \`derived\` from READY's \`qos_token\`.

Opcodes are in \`crates/dc-proto/src/op.rs\`. Events are the dispatch names in \`actions.live.json\`.
`,
);

console.log(`modules ${files.length}`);
console.log(`protos ${Object.keys(protos).length}, fields ${Object.values(protos).flat().length}`);
console.log(`menus ${Object.keys(menus).length}, settings ids ${settings.size}`);
console.log(`sounds ${Object.keys(sounds).length}, images ${Object.keys(images).length}, embed types ${embedTypes.size}`);
console.log(`constants ${Object.keys(live.flat).length}, keybind actions ${live.keybinds.length}`);
console.log(`voice enum modules ${Object.keys(enums).length}: ${Object.entries(enums).map(([k, v]) => `${k}:${v.length}`).join(' ')}`);
console.log(`experiments ${ready.experiments?.length}, guild ${ready.guild_experiments?.length}`);
