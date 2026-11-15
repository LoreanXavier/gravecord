import assert from 'node:assert/strict';
import test from 'node:test';
import { assertPinnedBuild, requestPlan } from './pinned-client.mjs';

const plan = (url, type = 'Script', method = 'GET') => requestPlan({ url, method }, type);

test('golden captures require both the pinned build number and version hash', () => {
  const expected = { BUILD_NUMBER: '667580', VERSION_HASH: 'pinned-hash' };
  assert.doesNotThrow(() => assertPinnedBuild({ build: 667580, hash: 'pinned-hash' }, expected));
  for (const actual of [
    { build: '673869', hash: 'new-hash' },
    { build: '667580', hash: 'different-hash' },
    {},
  ]) assert.throws(() => assertPinnedBuild(actual, expected), /do not overwrite goldens/);
});

test('rejects writes on every host before considering local resources', () => {
  for (const method of ['POST', 'PATCH', 'PUT', 'DELETE']) {
    assert.equal(plan('https://discord.com/assets/web.js', 'Script', method).action, 'block');
    assert.equal(plan('https://example.com/api', 'Fetch', method).action, 'block');
  }
});

test('pins application documents and executable assets while retaining read-only API access', () => {
  assert.deepEqual(plan('https://discord.com/channels/@me', 'Document'),
    { action: 'local', file: 'index.html', required: true });
  assert.deepEqual(plan('https://discordapp.com/assets/web.123.js?cache=1'),
    { action: 'local', file: 'assets/web.123.js', required: true });
  assert.equal(plan('https://discord.com/assets/styles.123.css').required, true);
  assert.equal(plan('https://discord.com/assets/codec.wasm').required, true);
  assert.equal(plan('https://discord.com/assets/avatar.webp', 'Image').required, false);
  assert.equal(plan('https://discord.com/api/v9/channels/1', 'Fetch').action, 'continue');
});

test('cannot resolve traversal or another origin into the snapshot', () => {
  for (const url of [
    'https://discord.com/assets/../../secrets.toml',
    'https://discord.com/assets/%2e%2e%2fsecrets.toml',
    'https://discord.com/assets/a%5cb.js',
    'https://discord.com/assets/sub/path.js',
    'https://example.com/assets/web.js',
  ]) assert.equal(plan(url).action, 'continue');
});
