// node tools/pinned-client.mjs [--reload]
// Keep this running for reference captures. Serve the saved web build without replacing the snapshot;
// reject HTTP writes, as read-only.mjs does. WebSocket interaction restrictions still apply.
import { existsSync, readFileSync } from 'node:fs';
import { extname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { connect } from './cdp.mjs';

const READ_METHODS = new Set(['GET', 'HEAD', 'OPTIONS']);
const HOSTS = new Set(['discord.com', 'discordapp.com']);
const MIME = {
  '.html': 'text/html; charset=utf-8', '.js': 'application/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.json': 'application/json', '.svg': 'image/svg+xml',
  '.png': 'image/png', '.webp': 'image/webp', '.jpg': 'image/jpeg', '.gif': 'image/gif',
  '.woff': 'font/woff', '.woff2': 'font/woff2', '.wasm': 'application/wasm',
};

export function assertPinnedBuild(actual, expected) {
  if (String(actual.build) !== String(expected.BUILD_NUMBER) || actual.hash !== expected.VERSION_HASH) {
    throw new Error(`Reference web build mismatch: expected ${expected.BUILD_NUMBER}/${expected.VERSION_HASH}, got ${actual.build}/${actual.hash}; do not overwrite goldens`);
  }
}

export function requestPlan(request, resourceType) {
  if (!READ_METHODS.has(request.method)) return { action: 'block' };
  const url = new URL(request.url);
  if (!HOSTS.has(url.hostname)) return { action: 'continue' };
  if (request.method !== 'GET') return { action: 'continue' };
  if (resourceType === 'Document'
      && /^\/(?:app\/?|channels(?:\/.*)?|login\/?|)$/.test(url.pathname)) {
    return { action: 'local', file: 'index.html', required: true };
  }
  const asset = /^\/assets\/([A-Za-z0-9_.-]+)$/.exec(url.pathname);
  if (asset) {
    return {
      action: 'local', file: `assets/${asset[1]}`,
      required: /\.(?:js|css|wasm)$/.test(asset[1]),
    };
  }
  return { action: 'continue' };
}

async function run() {
  const snapshot = join(import.meta.dirname, '..', 'design', 'snapshot');
  const expected = JSON.parse(readFileSync(join(snapshot, 'WEB_BUILD.json'), 'utf8'));
  const page = await connect();
  let reject;
  const failure = new Promise((_, fail) => { reject = fail; });
  let served = 0;
  let blocked = 0;
  const missing = new Set();
  page.on('Fetch.requestPaused', (event) => {
    (async () => {
      const plan = requestPlan(event.request, event.resourceType);
      const file = plan.file && join(snapshot, plan.file);
      if (plan.action === 'block' || (plan.required && !existsSync(file))) {
        await page.send('Fetch.failRequest', { requestId: event.requestId, errorReason: 'BlockedByClient' });
        blocked += 1;
        if (plan.required && !missing.has(plan.file)) {
          missing.add(plan.file);
          console.error(`Missing pinned asset (blocked): ${plan.file}`);
        }
      } else if (file && existsSync(file)) {
        await page.send('Fetch.fulfillRequest', {
          requestId: event.requestId, responseCode: 200,
          responseHeaders: [
            { name: 'Content-Type', value: MIME[extname(file)] ?? 'application/octet-stream' },
            { name: 'Cache-Control', value: 'no-store' },
            { name: 'Access-Control-Allow-Origin', value: '*' },
          ],
          body: readFileSync(file).toString('base64'),
        });
        served += 1;
      } else {
        await page.send('Fetch.continueRequest', { requestId: event.requestId });
      }
    })().catch(reject);
  });
  try {
    await page.send('Network.setBypassServiceWorker', { bypass: true });
    await page.send('Network.setCacheDisabled', { cacheDisabled: true });
    await page.send('Fetch.enable', { patterns: [{ urlPattern: '*', requestStage: 'Request' }] });
    console.log(`Pinned reference guard ready: build ${expected.BUILD_NUMBER}; HTTP writes blocked`);
    if (process.argv.includes('--reload')) await page.send('Page.reload', { ignoreCache: true });
    await Promise.race([failure, page.closed.then(() => { throw new Error('Capture connection closed'); })]);
  } finally {
    console.log(`Pinned guard stopped: served ${served} local resources, blocked ${blocked} requests, ${missing.size} missing assets; stop reference navigation`);
    page.close();
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  run().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
