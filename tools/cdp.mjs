// Minimal CDP client for the clean capture client. Loopback only.
export const PORT = Number(process.env.CDP_PORT ?? 9230);

export async function connect(match = /discord(app)?\.com/) {
  const targets = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
  const target = targets.find((t) => t.type === 'page' && match.test(t.url));
  if (!target) throw new Error('no matching page target');
  const socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((ok, fail) => {
    socket.onopen = ok;
    socket.onerror = fail;
  });
  let next = 0;
  const pending = new Map();
  const listeners = new Map();
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    if (message.method) {
      listeners.get(message.method)?.(message.params);
      return;
    }
    const waiter = pending.get(message.id);
    if (!waiter) return;
    pending.delete(message.id);
    message.error ? waiter.fail(new Error(message.error.message)) : waiter.ok(message.result);
  };
  const send = (method, params = {}) =>
    new Promise((ok, fail) => {
      const id = ++next;
      pending.set(id, { ok, fail });
      socket.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expression) => {
    const { result, exceptionDetails } = await send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text);
    return result.value;
  };
  return { send, evaluate, on: (method, callback) => listeners.set(method, callback), close: () => socket.close() };
}
