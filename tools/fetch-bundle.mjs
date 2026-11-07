// Downloads the web client the clean client is running: index, every loaded script and stylesheet, then every lazy
// chunk named by the webpack runtime. Writes design/snapshot/assets/, assets-manifest.tsv and WEB_BUILD.json.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { connect } from './cdp.mjs';

const snapshot = join(import.meta.dirname, '..', 'design', 'snapshot');
const assets = join(snapshot, 'assets');
mkdirSync(assets, { recursive: true });

const page = await connect();
const loaded = await page.evaluate(`(() => ({
  origin: location.origin,
  env: Object.fromEntries(['BUILD_NUMBER', 'VERSION_HASH', 'BUILT_AT', 'RELEASE_CHANNEL', 'API_VERSION', 'PUBLIC_PATH']
    .map(k => [k, GLOBAL_ENV[k]])),
  urls: [...document.scripts].map(s => s.src).filter(Boolean)
    .concat([...document.querySelectorAll('link[rel=stylesheet]')].map(l => l.href)),
}))()`);
page.close();

const failed = [];
async function download(url) {
  const name = basename(new URL(url).pathname);
  const path = join(assets, name);
  if (existsSync(path)) return;
  for (let attempt = 0; attempt < 3; attempt++) {
    const response = await fetch(url);
    if (response.ok) {
      writeFileSync(path, Buffer.from(await response.arrayBuffer()));
      return;
    }
    if (response.status === 404) break;
    await new Promise((ok) => setTimeout(ok, 1000 * (attempt + 1)));
  }
  failed.push(url);
}

async function downloadAll(urls) {
  const queue = [...new Set(urls)];
  await Promise.all(
    Array.from({ length: 8 }, async () => {
      while (queue.length) await download(queue.shift());
    }),
  );
}

await downloadAll(loaded.urls);
const indexResponse = await fetch(`${loaded.origin}/app`);
writeFileSync(join(snapshot, 'index.html'), await indexResponse.text());

// `p.k` (styles) and `p.u` (scripts) name lazy files two ways: `if(<id>===e)return""+e+".<hash>.js"` chains for the first
// few hundred (id in the filename), then one `""+({<id>:"<stem>",...})[e]+".js"` table for the rest (stem only).
const runtimeFile = readdirSync(assets).find((name) => /^web\.[0-9a-f]+\.js$/.test(name));
if (!runtimeFile) throw new Error('no web.*.js runtime in assets');
const runtime = readFileSync(join(assets, runtimeFile), 'utf8');
const stylesAt = runtime.indexOf('p.k=e=>{');
const scriptsAt = runtime.indexOf('p.u=e=>{');
const wrapperAt = runtime.indexOf('p.u=function(e)');
if (stylesAt < 0 || scriptsAt < stylesAt || wrapperAt < scriptsAt) throw new Error('runtime layout changed');
const lazy = new Set();
for (const [segment, extension] of [
  [runtime.slice(stylesAt, scriptsAt), 'css'],
  [runtime.slice(scriptsAt, wrapperAt), 'js'],
]) {
  for (const [, id, prefixed, name] of segment.matchAll(/if\((\d+|"[^"]+")===e\)return(""\+e\+)?"([^"]+\.(?:js|css))"/g)) {
    lazy.add(prefixed ? `${id}${name}` : name);
  }
  const table = segment.slice(segment.lastIndexOf('({'), segment.lastIndexOf('})[e]'));
  for (const [, stem] of table.matchAll(/[,{](?:\d+|"[^"]+"):"([^"]+)"/g)) lazy.add(`${stem}.${extension}`);
}
console.log(`lazy files named by the runtime: ${lazy.size}`);
writeFileSync(join(snapshot, 'lazy-files.txt'), [...lazy].join(String.fromCharCode(10)));
await downloadAll([...lazy].map((name) => `${loaded.origin}/assets/${name}`));

const lines = ['sha256\tsize\tname'];
for (const name of readdirSync(assets).sort()) {
  const bytes = readFileSync(join(assets, name));
  lines.push(`${createHash('sha256').update(bytes).digest('hex')}\t${bytes.length}\t${name}`);
}
writeFileSync(join(snapshot, 'assets-manifest.tsv'), lines.join('\n') + '\n');
writeFileSync(join(snapshot, 'WEB_BUILD.json'), JSON.stringify({ ...loaded.env, origin: loaded.origin }, null, 2));
writeFileSync(join(snapshot, 'assets-failed.txt'), failed.join('\n'));
console.log(`${lines.length - 1} files, ${failed.length} failed`);
