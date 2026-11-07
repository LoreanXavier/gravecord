// node tools/record.mjs <name> <seconds> [--reidentify] [--script <file>]
// Records REST (CDP Network) and decoded gateway frames (hook on the socket) into design/snapshot/TRAFFIC/<name>.json.
// Authorization, cookies and every token-like field are removed before anything leaves the page or is written.
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';

const args = process.argv.slice(2);
const [name, seconds] = args;
if (!name || !seconds) throw new Error('usage: record.mjs <name> <seconds> [--reidentify] [--script <file>]');
const reidentify = args.includes('--reidentify');
const scriptIndex = args.indexOf('--script');
const script = scriptIndex >= 0 ? readFileSync(args[scriptIndex + 1], 'utf8') : null;

const SECRET_HEADERS = /^(authorization|cookie|set-cookie|x-fingerprint)$/i;
const cleanHeaders = (headers = {}) =>
  Object.fromEntries(Object.entries(headers).map(([k, v]) => [k, SECRET_HEADERS.test(k) ? '<redacted>' : v]));

const out = join(import.meta.dirname, '..', 'design', 'snapshot', 'TRAFFIC');
mkdirSync(out, { recursive: true });

const page = await connect();
const requests = new Map();
await page.send('Network.enable', { maxPostDataSize: 1 << 20 });
const pendingBodies = [];
page.on('Network.requestWillBeSent', ({ requestId, request, type }) => {
  if (!/\/api\/v\d+\//.test(request.url)) return;
  requests.set(requestId, {
    method: request.method,
    url: request.url,
    type,
    requestHeaders: cleanHeaders(request.headers),
    postData: request.postData ?? null,
  });
});
page.on('Network.requestWillBeSentExtraInfo', ({ requestId, headers }) => {
  const entry = requests.get(requestId);
  if (entry) entry.requestHeaders = { ...entry.requestHeaders, ...cleanHeaders(headers) };
});
page.on('Network.responseReceived', ({ requestId, response }) => {
  const entry = requests.get(requestId);
  if (!entry) return;
  entry.status = response.status;
  entry.responseHeaders = cleanHeaders(response.headers);
});
page.on('Network.loadingFinished', ({ requestId }) => {
  const entry = requests.get(requestId);
  if (!entry) return;
  pendingBodies.push(
    page
      .send('Network.getResponseBody', { requestId })
      .then(({ body, base64Encoded }) => {
        if (!base64Encoded) entry.responseBody = body;
      })
      .catch(() => {}),
  );
});

await page.evaluate(`(() => { ${PRELUDE}
  const SECRET = /^(token|auth_token|analytics_token|session_token|password|mfa_ticket|ticket)$/i;
  const redact = v => {
    if (Array.isArray(v)) return v.map(redact);
    if (v && typeof v === 'object') {
      const o = {};
      for (const k of Object.keys(v)) o[k] = SECRET.test(k) ? '<redacted>' : redact(v[k]);
      return o;
    }
    return v;
  };
  const socket = findStores().get('GatewayConnectionStore').getSocket();
  if (window.__gc) { window.__gc.frames = []; return; }
  window.__gc = { frames: [], socket };
  const push = f => window.__gc.frames.push({ t: Date.now(), ...f });
  const send = socket.send;
  socket.send = function (op, d, ...rest) { push({ dir: 'out', op, d: redact(d) }); return send.call(this, op, d, ...rest); };
  const receive = socket.dispatcher.receiveDispatch;
  socket.dispatcher.receiveDispatch = function (d, t, n) { push({ dir: 'in', event: t, compression: n, d: redact(d) }); return receive.call(this, d, t, n); };
})()`);

if (reidentify) {
  await page.evaluate(`(() => { const s = window.__gc.socket; s.sessionId = null; s._handleReconnect(); })()`);
}
if (script) await page.evaluate(`(async () => { ${PRELUDE}\n${script}\n})()`);

await new Promise((ok) => setTimeout(ok, Number(seconds) * 1000));
await Promise.all(pendingBodies);
const frames = JSON.parse(await page.evaluate(`JSON.stringify(window.__gc.frames)`));
await page.evaluate(`window.__gc.frames = []`);
page.close();

const file = join(out, `${name}.json`);
writeFileSync(file, JSON.stringify({ rest: [...requests.values()], frames }, null, 1));

const text = readFileSync(file, 'utf8');
const leak = /(mfa\.[\w-]{20,}|[\w-]{24,}\.[\w-]{6}\.[\w-]{25,})/.exec(text);
if (leak) {
  writeFileSync(file, '');
  throw new Error('token-shaped string found, recording discarded');
}
const events = {};
for (const frame of frames) {
  const key = frame.dir === 'in' ? frame.event : `op${frame.op}`;
  events[key] = (events[key] ?? 0) + 1;
}
console.log(`${name}: ${requests.size} REST, ${frames.length} frames`);
console.log(JSON.stringify(events).slice(0, 600));
