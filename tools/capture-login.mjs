// node tools/capture-login.mjs <variant>...   (CDP_PORT=9231, an isolated instance with an empty profile)
//
// Login-screen goldens without any real credential. Network answers come from Fetch.fulfillRequest stubs of the recorded
// response shapes, and the remote auth socket is replaced by an in-page fake that does the key handshake with test data.
// Nothing here is ever sent to Discord: an auth request without a stub is failed, never continued.
import { spawnSync } from 'node:child_process';
import { connect, PORT } from './cdp.mjs';

if (PORT === 9230) throw new Error('refusing to touch the signed-in capture client on 9230');

const TEST_LOGIN = 'tester@example.invalid';
const TEST_SECRET = 'test-value-only';

const FAKE_SOCKET = (mode) => `(() => {
  const Real = window.WebSocket;
  const b64 = (buffer) => btoa(String.fromCharCode(...new Uint8Array(buffer)));
  const unb64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
  class Fake extends EventTarget {
    constructor(url) {
      super();
      this.url = url;
      this.readyState = 0;
      setTimeout(() => {
        this.readyState = 1;
        this.dispatchEvent(new Event('open'));
        this.emit({ op: 'hello', timeout_ms: 120000, heartbeat_interval: 41250 });
        if (${JSON.stringify(mode)} === 'closed') {
          setTimeout(() => this.dispatchEvent(Object.assign(new Event('close'), { code: 1006, reason: '' })), 50);
        }
      }, 20);
    }
    emit(object) { this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(object) })); }
    set onopen(f) { this.addEventListener('open', f); }
    set onmessage(f) { this.addEventListener('message', f); }
    set onclose(f) { this.addEventListener('close', f); }
    set onerror(f) { this.addEventListener('error', f); }
    close() { this.readyState = 3; }
    async send(data) {
      const message = JSON.parse(data);
      if (message.op === 'heartbeat') return this.emit({ op: 'heartbeat_ack' });
      if (message.op !== 'init' || ${JSON.stringify(mode)} === 'closed') return;
      const key = await crypto.subtle.importKey('spki', unb64(message.encoded_public_key), { name: 'RSA-OAEP', hash: 'SHA-256' }, true, ['encrypt']);
      const digest = await crypto.subtle.digest('SHA-256', unb64(message.encoded_public_key));
      const fingerprint = b64(digest).replace(/\\//g, '_').replace(/\\+/g, '-').replace(/={1,2}$/, '');
      this.emit({ op: 'pending_remote_init', fingerprint });
      if (${JSON.stringify(mode)} === 'ticket') {
        const payload = new TextEncoder().encode('123456789012345678:0:0:TestUser');
        const encrypted = await crypto.subtle.encrypt({ name: 'RSA-OAEP' }, key, payload);
        this.emit({ op: 'pending_ticket', encrypted_user_payload: b64(encrypted) });
      }
    }
  }
  window.WebSocket = function (url, ...rest) {
    return String(url).includes('remote-auth-gateway') ? new Fake(url) : new Real(url, ...rest);
  };
  window.WebSocket.prototype = Real.prototype;
  Object.assign(window.WebSocket, { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 });
})();`;

const LOGIN_ERROR = {
  code: 50035,
  errors: {
    login: { _errors: [{ code: 'INVALID_LOGIN', message: 'Login or password is invalid.' }] },
    password: { _errors: [{ code: 'INVALID_LOGIN', message: 'Login or password is invalid.' }] },
  },
  message: 'Invalid Form Body',
};
const MFA = { mfa: true, ticket: 'test-ticket', totp: true, sms: true, backup: true, webauthn: null, user_id: '1', login_instance_id: 'test' };

// Each scenario: the socket mode, the stubs by route (status + body, or hold), and the page steps before the shot.
const SCENARIOS = {
  'qr-loading': { socket: 'closed', steps: [] },
  'qr-ticket': { socket: 'ticket', steps: [] },
  'filled-error': {
    socket: 'ready',
    stubs: { '/auth/login': [400, LOGIN_ERROR] },
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['click', 'Log In'], ['wait', 1200]],
  },
  'logging-in': {
    socket: 'ready',
    stubs: { '/auth/login': 'hold' },
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['click', 'Log In'], ['wait', 600]],
  },
  'rate-limited': {
    socket: 'ready',
    stubs: { '/auth/login': [429, { message: 'You are being rate limited.', retry_after: 2.5, global: false }] },
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['click', 'Log In'], ['wait', 9000]],
  },
  'mfa-totp': {
    socket: 'ready',
    stubs: { '/auth/login': [200, MFA] },
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['click', 'Log In'], ['wait', 1500]],
  },
  'mfa-totp-error': {
    socket: 'ready',
    stubs: { '/auth/login': [200, MFA], '/auth/mfa/totp': [400, { code: 60008, message: 'Invalid two-factor code' }] },
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['click', 'Log In'], ['wait', 1500],
      ['typeActive', '123456'], ['click', 'Confirm'], ['wait', 1200]],
  },
  'mfa-select': {
    socket: 'ready',
    stubs: { '/auth/login': [200, MFA] },
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['click', 'Log In'], ['wait', 1500],
      ['clickLast', 'Verify with something else'], ['wait', 900]],
  },
  'mfa-sms': {
    socket: 'ready',
    stubs: { '/auth/login': [200, MFA], '/auth/mfa/sms/send': [200, { phone: '+** *** *** ** 34' }] },
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['click', 'Log In'], ['wait', 1500],
      ['clickLast', 'Verify with something else'], ['wait', 900], ['clickRow', 'sent to your phone'], ['wait', 1500]],
  },
  'mfa-backup': {
    socket: 'ready',
    stubs: { '/auth/login': [200, MFA] },
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['click', 'Log In'], ['wait', 1500],
      ['clickLast', 'Verify with something else'], ['wait', 900], ['clickRow', 'backup code'], ['wait', 1200]],
  },
  'mfa-backup-code': {
    socket: 'ready',
    stubs: { '/auth/login': [200, MFA] },
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['click', 'Log In'], ['wait', 1500],
      ['clickLast', 'Verify with something else'], ['wait', 900], ['clickRow', 'backup code'], ['wait', 12500], ['click', "I'm sure, use a backup code"], ['wait', 1200]],
  },
  'account-disabled': {
    socket: 'ready',
    stubs: { '/auth/login': [400, { code: 20013, message: 'This account is disabled.' }] },
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['click', 'Log In'], ['wait', 1500]],
  },
  'account-deletion': {
    socket: 'ready',
    stubs: { '/auth/login': [400, { code: 20011, message: 'This account is scheduled for deletion.' }] },
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['click', 'Log In'], ['wait', 1500]],
  },
  captcha: {
    socket: 'ready',
    stubs: {
      '/auth/login': [400, {
        captcha_key: ['captcha-required'], captcha_service: 'hcaptcha', captcha_sitekey: '00000000-0000-0000-0000-000000000000',
        captcha_session_id: 'test', captcha_rqdata: 'test', captcha_rqtoken: 'test', should_serve_invisible: false,
      }],
    },
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['click', 'Log In'], ['wait', 3500]],
  },
  'forgot-sent': {
    socket: 'ready',
    stubs: { '/auth/forgot': [200, {}] },
    steps: [['type', 'login', TEST_LOGIN], ['click', 'Forgot your password?'], ['wait', 1500]],
  },
  'forgot-empty': {
    socket: 'ready',
    stubs: { '/auth/forgot': [400, { code: 50035, errors: { login: { _errors: [{ code: 'BASE_TYPE_REQUIRED', message: 'This field is required' }] } }, message: 'Invalid Form Body' }] },
    steps: [['click', 'Forgot your password?'], ['wait', 1200]],
  },
  'password-shown': {
    socket: 'ready',
    steps: [['type', 'login', TEST_LOGIN], ['type', 'password', TEST_SECRET], ['clickLabel', 'View Password'], ['wait', 500]],
  },
};

const wanted = process.argv.slice(2);
if (wanted.length === 0) throw new Error(`usage: capture-login.mjs <variant>... (${Object.keys(SCENARIOS).join(' ')})`);

const page = await connect();
let current = null;
const held = [];
await page.send('Page.enable');
await page.send('Fetch.enable', { patterns: [{ urlPattern: '*discord.com/api/*auth/*' }] });
page.on('Fetch.requestPaused', async (event) => {
  const route = new URL(event.request.url).pathname.replace(/^\/api\/v\d+/, '');
  const stub = current?.stubs?.[route];
  if (stub === 'hold') {
    held.push(event.requestId);
    return;
  }
  if (stub) {
    const [status, body] = stub;
    await page.send('Fetch.fulfillRequest', {
      requestId: event.requestId,
      responseCode: status,
      responseHeaders: [{ name: 'content-type', value: 'application/json' }, { name: 'access-control-allow-origin', value: '*' }],
      body: Buffer.from(JSON.stringify(body)).toString('base64'),
    });
  } else if (event.request.method === 'GET') {
    await page.send('Fetch.continueRequest', { requestId: event.requestId });
  } else {
    await page.send('Fetch.failRequest', { requestId: event.requestId, errorReason: 'BlockedByClient' });
  }
});

const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));
const run = (expression) => page.evaluate(expression);

async function step([kind, a, b]) {
  if (kind === 'wait') return sleep(a);
  if (kind === 'type') {
    const selector = a === 'login' ? 'input[aria-label="Email or Phone Number"]' : 'input[name="password"], input[aria-label="Password"]';
    await run(`document.querySelector(${JSON.stringify(selector)}).focus()`);
    await page.send('Input.insertText', { text: b });
    return;
  }
  if (kind === 'typeActive') return page.send('Input.insertText', { text: a });
  const find = {
    click: `[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === ${JSON.stringify(a)})`,
    clickLast: `[...document.querySelectorAll('button')].filter((b) => b.textContent.includes(${JSON.stringify(a)})).pop()`,
    clickLabel: `document.querySelector('button[aria-label=${JSON.stringify(a)}]')`,
    clickRow: `[...document.querySelectorAll('div,span,button')].filter((e) => e.children.length === 0 && e.textContent.toLowerCase().includes(${JSON.stringify(a.toLowerCase())})).pop()`,
  }[kind];
  const ok = await run(`(() => { const el = ${find}; if (!el) return false; el.click(); return true; })()`);
  if (!ok) throw new Error(`step ${kind} ${a}: nothing to click`);
}

for (const name of wanted) {
  const scenario = SCENARIOS[name];
  if (!scenario) throw new Error(`unknown variant ${name}`);
  current = scenario;
  held.length = 0;
  const { identifier } = await page.send('Page.addScriptToEvaluateOnNewDocument', { source: FAKE_SOCKET(scenario.socket) });
  await page.send('Page.reload');
  await sleep(4500);
  for (const item of scenario.steps) await step(item);
  const shot = spawnSync('node', [new URL('./capture.mjs', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'), 'login', name], {
    env: process.env,
    encoding: 'utf8',
  });
  console.log((shot.stdout + shot.stderr).trim());
  for (const id of held) await page.send('Fetch.failRequest', { requestId: id, errorReason: 'BlockedByClient' }).catch(() => {});
  await page.send('Page.removeScriptToEvaluateOnNewDocument', { identifier });
}
await page.send('Fetch.disable');
page.close();
