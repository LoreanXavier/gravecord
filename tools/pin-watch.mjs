// pin-watch: notices a new stable Discord desktop build and turns it into a reviewable pin-move report.
//
//   node tools/pin-watch.mjs                 check; when newer than design/snapshot/BUILD, fetch + dump + diff + report
//   node tools/pin-watch.mjs --check         only ask the endpoints and compare (exit 0 up to date, 10 newer)
//   node tools/pin-watch.mjs --force         run the whole pipeline even when the pin is current (pipeline test)
//   node tools/pin-watch.mjs --self          no network: diff design/snapshot against itself (must report nothing)
//   node tools/pin-watch.mjs --host-version 1.0.9266   desktop-only check of an older host still on the CDN
//   Options: --skip-desktop  web client only      --no-reuse  download every web asset instead of linking unchanged
//            --all-modules   fetch every native module, not only new/changed versions
//            --verify-reuse N  re-download N reused assets and compare sha256 (default 12)
//            --out DIR       tree directory (default design/next/<desktop>-<web>)
//
// Where the answers come from (read from the shipped updater, nothing guessed):
//   desktop host + native modules  https://updates.discord.com/distributions/app/manifests/latest?channel=stable&platform=win&arch=x64
//       The base URL is NEW_UPDATE_ENDPOINT in the bootstrap (design/snapshot/desktop/bootstrap/bundle.js); the path is the
//       string in updater.node. The manifest lists each package URL (stable.dl2.discordapp.net/distro/...) and its sha256.
//       A .distro is brotli over a tar: delta_manifest.json + files/. Verified against the pin: Discord.exe sha256 matches BUILD.
//   web client                      https://discord.com/app  (GLOBAL_ENV.BUILD_NUMBER), assets under /assets/. The UI is this
//       bundle, not the host; it moves several times a week while the host moves rarely, so both are compared.
//
// What it writes (all under design/next/, which design/ already gitignores; design/snapshot is only ever read):
//   <tree>/{index.html,assets/,WEB_BUILD.json,modules/,MODULE_INDEX.tsv,dumps/STRINGS/,desktop/,desktop-manifest.tsv,
//           manifest.json,REPORT.md,diff.json,timings.json}
// Runs unchanged: tools/split-modules.mjs and tools/dump-strings.mjs, executed on the candidate tree by substituting their
// snapshot path (the substitution is asserted, so a changed tool fails loudly instead of writing into design/snapshot).
// Downloads go only to updates.discord.com, *.dl2.discordapp.net, discord.com and its CDN hosts. Nothing is installed or
// launched; the installed AppData\Local\Discord is not touched. Exit: 0 current, 10 newer build processed, 1 error.
import { createHash } from 'node:crypto';
import {
  copyFileSync, createReadStream, createWriteStream, existsSync, linkSync, mkdirSync, readFileSync, readdirSync, renameSync,
  rmSync, statSync, writeFileSync,
} from 'node:fs';
import { spawn, spawnSync } from 'node:child_process';
import { basename, dirname, join, relative } from 'node:path';
import { pipeline } from 'node:stream/promises';
import { createBrotliDecompress } from 'node:zlib';
import { FACTS_VERSION, deserialize, extractFacts, serialize } from './pin-watch-lib/facts.mjs';
import { loadParity, loadStrings, stringLocales } from './pin-watch-lib/diff.mjs';
import { buildDiff, renderReport } from './pin-watch-lib/report.mjs';

const TOOLS = import.meta.dirname;
const ROOT = join(TOOLS, '..');
const SNAP = join(ROOT, 'design', 'snapshot');
const NEXT = join(ROOT, 'design', 'next');
const MANIFEST_URL = 'https://updates.discord.com/distributions/app/manifests/latest?channel=stable&platform=win&arch=x64';
const APP_URL = 'https://discord.com/app';
const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36';
const HOSTS = [/^updates\.discord\.com$/, /(^|\.)dl2\.discordapp\.net$/, /^(www\.)?discord\.com$/, /^cdn\.discordapp\.com$/, /^discordapp\.com$/];

const argv = process.argv.slice(2);
const flag = (name) => argv.includes(`--${name}`);
const option = (name, fallback) => (argv.includes(`--${name}`) ? argv[argv.indexOf(`--${name}`) + 1] : fallback);
const timings = {};
const t0 = Date.now();
const log = (...parts) => console.log(`[${((Date.now() - t0) / 1000).toFixed(0).padStart(4)}s]`, ...parts);
async function timed(name, fn) {
  const start = Date.now();
  try {
    return await fn();
  } finally {
    timings[name] = Math.round((Date.now() - start) / 100) / 10;
  }
}

// ---------- network ----------
async function guarded(url, init = {}) {
  const host = new URL(url).hostname;
  if (!HOSTS.some((re) => re.test(host))) throw new Error(`refusing ${host}: not a Discord update or CDN host`);
  let lastError;
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      const response = await fetch(url, { ...init, headers: { 'user-agent': UA, ...(init.headers ?? {}) }, redirect: 'follow' });
      if (response.ok || response.status === 404) return response;
      lastError = new Error(`${url}: HTTP ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await new Promise((ok) => setTimeout(ok, 1000 * (attempt + 1)));
  }
  throw lastError;
}
const getText = async (url) => {
  const response = await guarded(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return response.text();
};

// ---------- the pin ----------
function readPin() {
  const build = Object.fromEntries(readFileSync(join(SNAP, 'BUILD'), 'utf8').split('\n').filter(Boolean).map((l) => [l.slice(0, l.indexOf('=')), l.slice(l.indexOf('=') + 1)]));
  const web = JSON.parse(readFileSync(join(SNAP, 'WEB_BUILD.json'), 'utf8'));
  const modules = Object.fromEntries(Object.entries(build).filter(([k]) => k.startsWith('module.')).map(([k, v]) => [k.slice(7), Number(v)]));
  return { build, web, modules };
}
const versionParts = (text) => String(text).split('.').map(Number);
const compareVersions = (a, b) => {
  const [x, y] = [versionParts(a), versionParts(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) < (y[i] ?? 0) ? -1 : 1;
  return 0;
};

async function checkLatest(pin) {
  const manifest = JSON.parse(await getText(MANIFEST_URL));
  const desktop = manifest.full.host_version.join('.');
  const html = await getText(APP_URL);
  const env = (key) => new RegExp(`"${key}":"([^"]*)"`).exec(html)?.[1];
  const web = { build: env('BUILD_NUMBER'), hash: env('VERSION_HASH'), builtAt: env('BUILT_AT'), channel: env('RELEASE_CHANNEL') };
  if (!web.build) throw new Error('discord.com/app: GLOBAL_ENV.BUILD_NUMBER not found; the page layout changed');
  const moduleNotes = [];
  for (const [name, entry] of Object.entries(manifest.modules ?? {})) {
    const version = entry.full.module_version;
    if (!(name in pin.modules)) moduleNotes.push(`\`${name}\` v${version}: in the update manifest, not in the pinned BUILD (new or optional module)`);
    else if (pin.modules[name] !== version) moduleNotes.push(`\`${name}\`: pinned v${pin.modules[name]}, now v${version}`);
  }
  for (const name of Object.keys(pin.modules)) if (!(name in (manifest.modules ?? {}))) moduleNotes.push(`\`${name}\`: pinned, no longer in the update manifest`);
  const desktopNewer = compareVersions(desktop, pin.build.desktop_version) > 0;
  const webNewer = Number(web.build) > Number(pin.web.BUILD_NUMBER);
  return { manifest, desktop, web, html, moduleNotes, desktopNewer, webNewer, newer: desktopNewer || webNewer, requiredUpdate: manifest.required_update };
}

// ---------- web client ----------
const sha256File = (path) => createHash('sha256').update(readFileSync(path)).digest('hex');
function place(from, to) {
  try {
    linkSync(from, to);
  } catch {
    copyFileSync(from, to);
  }
}

// Lazy chunk names from the webpack runtime (the parse of tools/fetch-bundle.mjs, plus ids the minifier writes as 592e3).
function lazyFiles(runtime) {
  const stylesAt = runtime.indexOf('p.k=e=>{');
  const scriptsAt = runtime.indexOf('p.u=e=>{');
  const wrapperAt = runtime.indexOf('p.u=function(e)');
  if (stylesAt < 0 || scriptsAt < stylesAt || wrapperAt < scriptsAt) throw new Error('web runtime layout changed: p.k/p.u markers moved; tools/fetch-bundle.mjs needs the same update');
  const lazy = new Set();
  for (const [segment, extension] of [[runtime.slice(stylesAt, scriptsAt), 'css'], [runtime.slice(scriptsAt, wrapperAt), 'js']]) {
    for (const [, id, prefixed, name] of segment.matchAll(/if\((\d+(?:e\d+)?|"[^"]+")===e\)return(""\+e\+)?"([^"]+\.(?:js|css))"/g)) lazy.add(prefixed ? `${/^[0-9]/.test(id) ? Number(id) : id}${name}` : name);
    const table = segment.slice(segment.lastIndexOf('({'), segment.lastIndexOf('})[e]'));
    for (const [, stem] of table.matchAll(/[,{](?:\d+(?:e\d+)?|"[^"]+"):"([^"]+)"/g)) lazy.add(`${stem}.${extension}`);
  }
  return lazy;
}

async function fetchWeb(tree, latest) {
  const assets = join(tree, 'assets');
  mkdirSync(assets, { recursive: true });
  writeFileSync(join(tree, 'index.html'), latest.html);
  const reuse = !flag('no-reuse');
  const stats = { downloaded: 0, linked: 0, failed: [], bytes: 0 };
  const reused = [];
  async function download(name) {
    const target = join(assets, name);
    if (existsSync(target)) return;
    const known = join(SNAP, 'assets', name);
    if (reuse && existsSync(known)) {
      place(known, target);
      stats.linked++;
      reused.push(name);
      return;
    }
    const response = await guarded(`https://discord.com/assets/${name}`);
    if (!response.ok) {
      stats.failed.push(name);
      return;
    }
    const bytes = Buffer.from(await response.arrayBuffer());
    writeFileSync(target, bytes);
    stats.downloaded++;
    stats.bytes += bytes.length;
  }
  async function all(names) {
    const queue = [...new Set(names)];
    let done = 0;
    await Promise.all(Array.from({ length: 8 }, async () => {
      while (queue.length) {
        await download(queue.shift());
        if (++done % 1000 === 0) log(`  web assets: ${done} handled`);
      }
    }));
  }
  const fromIndex = [...latest.html.matchAll(/(?:src|href)="\/assets\/([^"?#]+)"/g)].map((m) => m[1]);
  await all(fromIndex);
  const runtimeName = readdirSync(assets).find((n) => /^web\.[0-9a-f]+\.js$/.test(n));
  if (!runtimeName) throw new Error('no web.<hash>.js runtime among the index assets');
  const lazy = lazyFiles(readFileSync(join(assets, runtimeName), 'utf8'));
  log(`web: index names ${new Set(fromIndex).size} files, runtime names ${lazy.size} lazy files`);
  writeFileSync(join(tree, 'lazy-files.txt'), [...lazy].join('\n'));
  await all([...lazy]);

  // Integrity of the reuse shortcut: names are content hashes, so re-fetch a sample and compare.
  const sample = Number(option('verify-reuse', 12));
  let mismatches = 0;
  const picks = reused.filter((_, i) => i % Math.max(1, Math.floor(reused.length / Math.max(sample, 1))) === 0).slice(0, sample);
  for (const name of picks) {
    const response = await guarded(`https://discord.com/assets/${name}`);
    if (!response.ok) continue;
    const live = createHash('sha256').update(Buffer.from(await response.arrayBuffer())).digest('hex');
    if (live !== sha256File(join(assets, name))) mismatches++;
  }
  if (picks.length) log(`web: reuse check, ${picks.length} linked assets re-downloaded, ${mismatches} differ`);

  const rows = ['sha256\tsize\tname'];
  for (const name of readdirSync(assets).sort()) {
    const bytes = readFileSync(join(assets, name));
    rows.push(`${createHash('sha256').update(bytes).digest('hex')}\t${bytes.length}\t${name}`);
  }
  writeFileSync(join(tree, 'assets-manifest.tsv'), rows.join('\n') + '\n');
  writeFileSync(join(tree, 'assets-failed.txt'), stats.failed.join('\n'));
  writeFileSync(join(tree, 'WEB_BUILD.json'), JSON.stringify({ BUILD_NUMBER: latest.web.build, VERSION_HASH: latest.web.hash, BUILT_AT: latest.web.builtAt, RELEASE_CHANNEL: latest.web.channel, origin: 'https://discord.com' }, null, 2));
  log(`web: ${rows.length - 1} files; downloaded ${stats.downloaded} (${(stats.bytes / 1e6).toFixed(1)} MB), linked ${stats.linked} from the pin, failed ${stats.failed.length}, reuse mismatches ${mismatches}`);
  return { ...stats, files: rows.length - 1, reuseChecked: picks.length, mismatches };
}

// ---------- existing tools, run on the candidate tree ----------
function runTool(script, tree, edits = []) {
  const pattern = "join(import.meta.dirname, '..', 'design', 'snapshot')";
  let source = readFileSync(join(TOOLS, script), 'utf8');
  if (!source.includes(pattern)) throw new Error(`${script}: snapshot path expression not found; pin-watch must be updated for this tool`);
  source = source.replace(pattern, JSON.stringify(tree.replace(/\\/g, '/')));
  for (const [from, to] of edits) source = source.replace(from, to);
  const result = spawnSync(process.execPath, ['--max-old-space-size=6144', '--input-type=module', '-'], { cwd: TOOLS, input: source, encoding: 'utf8', maxBuffer: 1 << 28 });
  if (result.status !== 0) throw new Error(`${script} failed (exit ${result.status}): ${(result.stderr || result.stdout).slice(-600)}`);
  return result.stdout.trim().split('\n').slice(-3).join(' | ');
}

function findLocaleLoader(tree) {
  // The locale -> catalog loader is the module whose `"xx-YY":()=>n.e(chunk).then(n.t.bind(n,id,..))` entries point at the
  // most catalog modules (modules holding the message key hPBScv). Several modules look alike; only one points at catalogs.
  const dir = join(tree, 'modules');
  const entry = /"?[a-zA-Z]{2}(?:-[a-zA-Z0-9]{2,4})?"?:\(\)=>n\.e\(\d+\)\.then\(n\.(?:t\.)?bind\(n,(\d+)/g;
  const catalogs = new Set();
  const candidates = [];
  for (const file of readdirSync(dir)) {
    const code = readFileSync(join(dir, file), 'utf8');
    if (code.includes('"hPBScv"')) catalogs.add(file.slice(0, -3));
    if (code.length < 200000 && code.includes('n.t.bind(n,')) candidates.push([file.slice(0, -3), [...code.matchAll(entry)].map((m) => m[1])]);
  }
  let best = null;
  for (const [id, targets] of candidates) {
    const hits = targets.filter((t) => catalogs.has(t)).length;
    if (hits >= 10 && (!best || hits > best.hits)) best = { id, hits };
  }
  return best?.id ?? null;
}

// ---------- asar (same format as xtask/src/asar.rs) ----------
function extractAsar(archive, out) {
  const bytes = readFileSync(archive);
  const headerSize = bytes.readUInt32LE(4);
  const jsonLength = bytes.readUInt32LE(12);
  const header = JSON.parse(bytes.subarray(16, 16 + jsonLength).toString('utf8'));
  const base = 8 + headerSize;
  const unpacked = `${archive}.unpacked`;
  let files = 0;
  const missing = [];
  (function walk(entries, dir, rel) {
    mkdirSync(dir, { recursive: true });
    for (const [name, entry] of Object.entries(entries)) {
      if (!name || name === '.' || name === '..' || /[\\/:]/.test(name)) throw new Error(`unsafe asar entry ${name}`);
      if (entry.files) walk(entry.files, join(dir, name), `${rel}/${name}`);
      else if (entry.link) continue;
      else if (entry.unpacked) {
        const source = join(unpacked, rel, name);
        if (existsSync(source)) {
          copyFileSync(source, join(dir, name));
          files++;
        } else missing.push(`${rel}/${name}`);
      } else {
        const start = base + Number(entry.offset);
        writeFileSync(join(dir, name), bytes.subarray(start, start + entry.size));
        files++;
      }
    }
  })(header.files, out, '');
  return { files, missing };
}

// ---------- desktop host and native modules ----------
async function download(url, target, expectedSha) {
  if (existsSync(target) && (!expectedSha || sha256File(target) === expectedSha)) return false;
  const response = await guarded(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  await pipeline(response.body, createWriteStream(target));
  const actual = sha256File(target);
  if (expectedSha && actual !== expectedSha) throw new Error(`${url}: sha256 ${actual} does not match the manifest's ${expectedSha}`);
  return true;
}

// Second integrity layer: every .distro carries delta_manifest.json with the sha256 of each file it holds.
function verifyDistro(dir) {
  const manifest = JSON.parse(readFileSync(join(dir, 'delta_manifest.json'), 'utf8'));
  let checked = 0;
  const bad = [];
  for (const [rel, entry] of Object.entries(manifest.files ?? {})) {
    const want = entry?.New?.Sha256;
    const path = join(dir, 'files', rel);
    if (!want || !existsSync(path)) continue;
    checked++;
    if (sha256File(path) !== want) bad.push(rel);
  }
  return { checked, bad };
}

// .distro = brotli(tar). The system tar reads it from stdin with the destination as its working directory.
async function unpackDistro(file, dest) {
  mkdirSync(dest, { recursive: true });
  const tar = spawn('tar', ['-xf', '-'], { cwd: dest, stdio: ['pipe', 'inherit', 'inherit'] });
  const exited = new Promise((ok, fail) => {
    tar.on('close', (code) => (code === 0 ? ok() : fail(new Error(`tar exited ${code}`))));
    tar.on('error', fail);
  });
  await Promise.all([pipeline(createReadStream(file), createBrotliDecompress(), tar.stdin), exited]);
}

function filesUnder(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) filesUnder(path, out);
    else out.push(path);
  }
  return out;
}

async function fetchDesktop(tree, latest, pin, { hostVersion = null } = {}) {
  const raw = join(tree, 'desktop-raw');
  const desktop = join(tree, 'desktop');
  mkdirSync(raw, { recursive: true });
  rmSync(desktop, { recursive: true, force: true });
  mkdirSync(desktop, { recursive: true });
  const notes = [];
  const fetched = [];
  const skipped = [];

  const hostFile = join(raw, 'host.distro');
  // An older host is the same URL scheme with its version in the path; the manifest's sha256 only covers the current one.
  const hostUrl = hostVersion ? latest.manifest.full.url.replace(`/${latest.desktop}/`, `/${hostVersion}/`) : latest.manifest.full.url;
  const fresh = await download(hostUrl, hostFile, hostVersion ? null : latest.manifest.full.package_sha256);
  const hostDir = join(raw, 'host');
  rmSync(hostDir, { recursive: true, force: true });
  await unpackDistro(hostFile, hostDir);
  const hostCheck = verifyDistro(hostDir);
  log(`desktop: host ${hostVersion ?? latest.desktop} ${(statSync(hostFile).size / 1e6).toFixed(0)} MB ${fresh ? 'downloaded' : 'cached'}, ${hostVersion ? 'package sha256 not in the manifest' : 'package sha256 ok'}, ${hostCheck.checked} files match delta_manifest.json${hostCheck.bad.length ? `, ${hostCheck.bad.length} DO NOT` : ''}`);
  if (hostCheck.bad.length) notes.push(`WARNING: ${hostCheck.bad.length} host files differ from delta_manifest.json: ${hostCheck.bad.slice(0, 5).join(', ')}`);
  const resources = join(hostDir, 'files', 'resources');
  const bootstrap = extractAsar(join(resources, 'app.asar'), join(desktop, 'bootstrap'));
  if (bootstrap.missing.length) notes.push(`bootstrap: ${bootstrap.missing.length} unpacked files not found`);
  const info = JSON.parse(readFileSync(join(resources, 'build_info.json'), 'utf8'));
  const exeSha = sha256File(join(hostDir, 'files', 'Discord.exe'));
  notes.push(`host build_info: version ${info.version}, ${info.releaseChannel}, ${info.sentryDist}, sentryRelease \`${info.sentryRelease}\``);
  notes.push(`Discord.exe sha256 ${exeSha === pin.build.desktop_exe_sha256 ? 'equals the pin' : `differs from the pin (${pin.build.desktop_exe_sha256.slice(0, 12)}… → ${exeSha.slice(0, 12)}…)`}`);
  if (info.sentryRelease !== pin.build.sentry_release) notes.push(`sentry release changed: \`${pin.build.sentry_release}\` → \`${info.sentryRelease}\``);
  rmSync(hostDir, { recursive: true, force: true }); // the .distro stays in desktop-raw/ for a later throwaway unpack
  const modulesDir = join(desktop, 'modules');
  for (const [name, entry] of hostVersion ? [] : Object.entries(latest.manifest.modules ?? {})) {
    const version = entry.full.module_version;
    if (!flag('all-modules') && pin.modules[name] === version) {
      skipped.push(name);
      continue;
    }
    const file = join(raw, `${name}-${version}.distro`);
    await download(entry.full.url, file, entry.full.package_sha256);
    const staging = join(raw, `${name}-${version}`);
    rmSync(staging, { recursive: true, force: true });
    await unpackDistro(file, staging);
    const check = verifyDistro(staging);
    if (check.bad.length) notes.push(`WARNING: ${name}: ${check.bad.length} files differ from delta_manifest.json`);
    const dest = join(modulesDir, `${name}-${version}`, name);
    mkdirSync(dirname(dest), { recursive: true });
    renameSync(join(staging, 'files'), dest);
    for (const archive of filesUnder(dest).filter((p) => p.endsWith('.asar'))) extractAsar(archive, archive.replace(/\.asar$/, '.extracted'));
    fetched.push(`${name}-${version}`);
  }
  log(`desktop: modules fetched ${fetched.length} (${fetched.join(', ') || 'none'}); same version as the pin, not fetched: ${skipped.length}`);

  const rows = ['sha256\tsize\tpath'];
  const files = new Map();
  for (const path of filesUnder(desktop).sort()) {
    const sha = sha256File(path);
    const rel = relative(desktop, path).replace(/\\/g, '/');
    files.set(rel, sha);
    rows.push(`${sha}\t${statSync(path).size}\t${rel}`);
  }
  writeFileSync(join(tree, 'desktop-manifest.tsv'), rows.join('\n') + '\n');
  return { files, fetched, skipped, notes };
}

function readManifestTsv(path) {
  const map = new Map();
  for (const line of readFileSync(path, 'utf8').split('\n').slice(1)) {
    const [sha, , rel] = line.split('\t');
    if (rel) map.set(rel, sha);
  }
  return map;
}

function diffDesktop(oldFiles, built, versionNotes) {
  // Only trees that were fetched can be compared: bootstrap always, a module only when its package was downloaded.
  const scoped = (rel) => rel.startsWith('bootstrap/') || built.fetched.some((m) => rel.startsWith(`modules/${m}/`));
  const added = [];
  const removed = [];
  const changed = [];
  let unchanged = 0;
  for (const [rel, sha] of built.files) {
    if (!oldFiles.has(rel)) added.push(rel);
    else if (oldFiles.get(rel) !== sha) changed.push(rel);
    else unchanged++;
  }
  for (const rel of oldFiles.keys()) if (scoped(rel) && !built.files.has(rel)) removed.push(rel);
  const notes = [...built.notes];
  if (built.skipped.length) notes.push(`not fetched, module version equals the pin: ${built.skipped.join(', ')}`);
  return { files: { added, removed, changed, unchanged }, versions: versionNotes, notes };
}

// ---------- facts cache ----------
function pinFacts(pin) {
  const cacheDir = join(NEXT, '_cache');
  mkdirSync(cacheDir, { recursive: true });
  const file = join(cacheDir, `snapshot-${pin.web.BUILD_NUMBER}-${pin.build.captured_unix}-v${FACTS_VERSION}.json`);
  if (existsSync(file)) {
    log(`pin facts: cache ${basename(file)}`);
    return deserialize(readFileSync(file, 'utf8'));
  }
  const facts = extractFacts(SNAP, log);
  writeFileSync(file, serialize(facts));
  return facts;
}

// ---------- main ----------
async function main() {
  mkdirSync(NEXT, { recursive: true });
  const pin = readPin();
  const from = { desktop: pin.build.desktop_version, web: pin.web.BUILD_NUMBER, hash: pin.web.VERSION_HASH, builtAt: new Date(Number(pin.web.BUILT_AT)).toISOString(), channel: pin.build.release_channel };
  from.label = `desktop ${from.desktop} / web ${from.web}`;

  if (flag('self')) {
    const tree = join(NEXT, '_self');
    mkdirSync(tree, { recursive: true });
    const facts = await timed('facts(pin)', () => pinFacts(pin));
    const strings = loadStrings(SNAP);
    const diff = buildDiff(facts, facts, { oldStrings: strings, newStrings: strings, parity: loadParity(ROOT) });
    const desktopFiles = readManifestTsv(join(SNAP, 'desktop-manifest.tsv'));
    const desktop = diffDesktop(desktopFiles, { files: desktopFiles, fetched: [], skipped: [], notes: [] }, []);
    writeFileSync(join(tree, 'REPORT.md'), renderReport(diff, { from, to: from, selfTest: true, web: null, desktop, manifestUrl: MANIFEST_URL, note: 'Self-test, no network.' }));
    writeFileSync(join(tree, 'diff.json'), JSON.stringify(diff, null, 1));
    log(`self-test report: ${join(tree, 'REPORT.md')}`);
    return 0;
  }

  const latest = await timed('check', () => checkLatest(pin));
  const to = { desktop: latest.desktop, web: latest.web.build, hash: latest.web.hash, builtAt: new Date(Number(latest.web.builtAt)).toISOString(), channel: latest.web.channel };
  to.label = `desktop ${to.desktop} / web ${to.web}`;
  log(`pinned:  desktop ${from.desktop}, web build ${from.web} (${from.hash.slice(0, 10)}), ${from.channel}`);
  log(`current: desktop ${to.desktop}, web build ${to.web} (${(to.hash ?? '').slice(0, 10)}), ${to.channel}${latest.requiredUpdate ? ', required_update' : ''}`);
  for (const note of latest.moduleNotes) log(`  module: ${note.replace(/`/g, '')}`);
  log(latest.newer ? `NEWER: ${[latest.desktopNewer && 'desktop host', latest.webNewer && 'web client'].filter(Boolean).join(' and ')}` : 'up to date: the pin is the current stable build');
  const status = { checked: new Date().toISOString(), pinned: from, current: to, newer: latest.newer, desktopNewer: latest.desktopNewer, webNewer: latest.webNewer, moduleNotes: latest.moduleNotes };
  writeFileSync(join(NEXT, 'LAST_CHECK.json'), JSON.stringify(status, null, 2));
  if (flag('check')) return latest.newer ? 10 : 0;
  if (option('host-version')) {
    // Second check against an older host build: desktop side only (the web client keeps no old builds online).
    const hostVersion = option('host-version');
    const tree = join(NEXT, `${hostVersion}-hostonly`);
    mkdirSync(tree, { recursive: true });
    const built = await timed('desktop fetch', () => fetchDesktop(tree, latest, pin, { hostVersion }));
    const desktop = diffDesktop(readManifestTsv(join(SNAP, 'desktop-manifest.tsv')), built, []);
    const facts = await timed('facts(pin)', () => pinFacts(pin));
    const strings = loadStrings(SNAP);
    const diff = buildDiff(facts, facts, { oldStrings: strings, newStrings: strings, parity: loadParity(ROOT) });
    const label = `desktop ${hostVersion} (host only) / web ${from.web} (not fetched, shown unchanged)`;
    const note = `Host-only check against an older build: only the desktop sections are meaningful; every web section is the pin diffed with itself. Run time ${Math.round((Date.now() - t0) / 100) / 10} s.`;
    writeFileSync(join(tree, 'REPORT.md'), renderReport(diff, { from, to: { ...from, desktop: hostVersion, label }, desktop, manifestUrl: MANIFEST_URL, note }));
    log(`report: ${join(tree, 'REPORT.md')}`);
    return 0;
  }
  if (!latest.newer && !flag('force')) {
    log('nothing to fetch (use --force to run the pipeline anyway, --self to test the diff offline)');
    return 0;
  }

  const tree = option('out') ? join(ROOT, option('out')) : join(NEXT, `${to.desktop}-${to.web}`);
  if (tree === SNAP || tree.startsWith(SNAP)) throw new Error('refusing to write into design/snapshot');
  mkdirSync(tree, { recursive: true });
  log(`tree: ${tree}`);
  writeFileSync(join(tree, 'manifest.json'), JSON.stringify(latest.manifest, null, 1));

  const web = await timed('web fetch', () => fetchWeb(tree, latest));
  let split = '';
  await timed('unbundle', async () => {
    split = runTool('split-modules.mjs', tree);
    log(`unbundle: ${split}`);
  });
  const notes = [];
  await timed('strings', async () => {
    const loader = findLocaleLoader(tree);
    const edits = loader && loader !== '802249' ? [['802249.js', `${loader}.js`]] : [];
    if (!loader) notes.push('dump-strings: no locale loader module found in the candidate; STRINGS not extracted');
    else {
      try {
        log(`strings: ${runTool('dump-strings.mjs', tree, edits).slice(0, 160)}`);
      } catch (error) {
        notes.push(`dump-strings failed: ${error.message.slice(0, 300)}`);
        log(notes.at(-1));
      }
    }
  });

  let desktop = null;
  if (!flag('skip-desktop')) {
    desktop = await timed('desktop fetch', async () => {
      const built = await fetchDesktop(tree, latest, pin);
      return diffDesktop(readManifestTsv(join(SNAP, 'desktop-manifest.tsv')), built, latest.moduleNotes);
    });
  }

  const [oldFacts, newFacts] = [await timed('facts(pin)', () => pinFacts(pin)), await timed('facts(new)', () => extractFacts(tree, log))];
  const oldStrings = loadStrings(SNAP);
  const newStrings = loadStrings(tree);
  const diff = await timed('diff', () => buildDiff(oldFacts, newFacts, { oldStrings, newStrings, parity: loadParity(ROOT) }));
  const locales = { old: stringLocales(SNAP), new: stringLocales(tree) };
  const total = Math.round((Date.now() - t0) / 100) / 10;
  timings.total = total;
  const note = [
    notes.join(' '),
    latest.newer ? '' : '--force: the pin is current, so anything listed as changed comes from how the trees were produced, not from an update. Known cause: modules and CSS "added" are lazy chunks the CDP session that made the pin never loaded (it recorded only files the page requested); this tool reads the whole chunk map from the web runtime, including ids the minifier writes as `592e3`. api.js (hCaptcha, a third-party host) is in the pin but is not fetched here.',
    web.mismatches ? `WARNING: ${web.mismatches} reused assets differ from the live ones; rerun with --no-reuse.` : '',
    `Run time ${total} s (${Object.entries(timings).map(([k, v]) => `${k} ${v}s`).join(', ')}). Web assets: ${web.downloaded} downloaded, ${web.linked} linked from the pin.`,
  ].filter(Boolean).join(' ');
  writeFileSync(join(tree, 'REPORT.md'), renderReport(diff, { from, to, web, desktop, manifestUrl: MANIFEST_URL, note, locales }));
  writeFileSync(join(tree, 'diff.json'), JSON.stringify({ status, diff, desktop, locales }, null, 1));
  writeFileSync(join(tree, 'timings.json'), JSON.stringify(timings, null, 1));
  log(`report: ${join(tree, 'REPORT.md')}`);
  return latest.newer ? 10 : 0;
}

main().then((code) => process.exit(code), (error) => {
  console.error(`pin-watch: ${error.stack ?? error}`);
  process.exit(1);
});
