// node tools/read-only.mjs — keep running while navigating the clean capture page.
// Blocks this page's HTTP writes, including automatic settings/onboarding calls. It does not
// intercept gateway WebSocket messages or other targets; fixture-only interaction rules still apply.
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { connect } from './cdp.mjs';

const READ_METHODS = new Set(['GET', 'HEAD', 'OPTIONS']);

export async function handleRequest(page, event) {
  const blocked = !READ_METHODS.has(event.request.method);
  await page.send(blocked ? 'Fetch.failRequest' : 'Fetch.continueRequest', {
    requestId: event.requestId,
    ...(blocked ? { errorReason: 'BlockedByClient' } : {}),
  });
  return blocked;
}

export async function installReadOnly(page, onBlocked = () => {}) {
  let reject;
  const failure = new Promise((_, fail) => { reject = fail; });
  // Install before enabling interception so the first paused request cannot escape the handler.
  page.on('Fetch.requestPaused', (event) => {
    handleRequest(page, event).then((blocked) => {
      if (blocked) onBlocked();
    }).catch(reject);
  });
  await page.send('Network.setBypassServiceWorker', { bypass: true });
  await page.send('Fetch.enable', { patterns: [{ urlPattern: '*', requestStage: 'Request' }] });
  return { failure };
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const page = await connect();
  let blocked = 0;
  try {
    const { failure } = await installReadOnly(page, () => {
      console.log(`Blocked HTTP write ${++blocked}`);
    });
    console.log('Read-only HTTP guard ready on the clean capture page');
    await Promise.race([
      failure,
      page.closed.then(() => { throw new Error('Capture connection closed'); }),
    ]);
  } catch {
    console.error('Read-only HTTP guard stopped; do not continue reference navigation');
    process.exitCode = 1;
  } finally {
    page.close();
  }
}
