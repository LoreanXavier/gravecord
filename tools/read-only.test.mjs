import assert from 'node:assert/strict';
import test from 'node:test';
import { handleRequest, installReadOnly } from './read-only.mjs';

test('continues read methods and blocks writes or unknown methods without reading payloads', async () => {
  for (const method of ['GET', 'HEAD', 'OPTIONS', 'POST', 'PATCH', 'PUT', 'DELETE', 'CONNECT', undefined]) {
    const calls = [];
    const page = { send: async (...args) => calls.push(args) };
    const request = { method };
    for (const key of ['headers', 'postData', 'url']) {
      Object.defineProperty(request, key, { get() { throw new Error('Payload must not be inspected'); } });
    }
    const blocked = await handleRequest(page, { requestId: 'fixture-request', request });
    const read = ['GET', 'HEAD', 'OPTIONS'].includes(method);
    assert.equal(blocked, !read);
    assert.deepEqual(calls, [[read ? 'Fetch.continueRequest' : 'Fetch.failRequest', {
      requestId: 'fixture-request', ...(!read ? { errorReason: 'BlockedByClient' } : {}),
    }]]);
  }
});

test('a failed block never falls back to continuing the request', async () => {
  const calls = [];
  const page = { send: async (method) => { calls.push(method); throw new Error('Disconnected'); } };
  await assert.rejects(handleRequest(page, { requestId: 'fixture-request', request: { method: 'POST' } }));
  assert.deepEqual(calls, ['Fetch.failRequest']);
});

test('installs the handler before interception and surfaces failures', async () => {
  const calls = [];
  let handler;
  const page = {
    on: (name, callback) => { calls.push(name); handler = callback; },
    send: async (name, args) => { calls.push([name, args]); },
  };
  const { failure } = await installReadOnly(page);
  assert.deepEqual(calls, [
    'Fetch.requestPaused',
    ['Network.setBypassServiceWorker', { bypass: true }],
    ['Fetch.enable', { patterns: [{ urlPattern: '*', requestStage: 'Request' }] }],
  ]);
  page.send = async () => { throw new Error('Disconnected'); };
  handler({ requestId: 'fixture-request', request: { method: 'PATCH' } });
  await assert.rejects(failure, /Disconnected/);
});
