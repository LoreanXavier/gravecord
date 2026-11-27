// node tools/no-local-rpc.mjs
// The clean capture client is a second Discord on this machine, so it also opens a local RPC pipe (discord-ipc-N).
// A program that sets Rich Presence connects to the first pipe that answers, and when the main Discord install
// restarts that can be this one: its activity then shows on the test account and stays there. This watcher closes
// every RPC socket a local program opens on the capture client, the way the client itself drops a socket, so the
// activity is cleared and the program reconnects to the main Discord. Loopback CDP only.
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';

const CLOSE = `(async () => { ${PRELUDE}
  let server = null;
  for (const m of Object.values(req.c)) {
    const e = m?.exports; if (!e || typeof e !== 'object') continue;
    if ('sockets' in e && 'commands' in e && 'subscriptions' in e) server = e;
    for (const k of Object.keys(e).slice(0, 12)) { let c; try { c = e[k]; } catch { continue; }
      if (c && typeof c === 'object' && 'sockets' in c && 'commands' in c && 'subscriptions' in c) server = c; }
  }
  if (!server) return 'no rpc server';
  const list = [...(server.sockets.values ? server.sockets.values() : Object.values(server.sockets))];
  const names = [];
  for (const s of list) { names.push(s.application?.name ?? 'unknown'); try { s.close(1000, 'the capture client does not serve local programs'); } catch {} }
  return names.length ? 'closed ' + names.join(', ') : '';
})()`;

console.log('Local RPC watcher ready on the clean capture page');
for (;;) {
  try {
    const page = await connect();
    const result = await page.evaluate(CLOSE);
    if (result) console.log(new Date().toISOString().slice(11, 19), result);
    page.close();
  } catch (error) {
    console.log('watcher: ' + String(error).slice(0, 120));
  }
  await new Promise((ok) => setTimeout(ok, 15000));
}
