// node tools/type.mjs "<text>" [--enter]
// Focuses the clean client's message composer, inserts the text as typed input and optionally presses Enter. Only for
// the gravecord fixture guild and the designated test DM (the working rules "Never").
import { connect } from './cdp.mjs';

const args = process.argv.slice(2);
const text = args.find((arg) => arg !== '--enter');
if (text == null) throw new Error('usage: type.mjs "<text>" [--enter]');
const page = await connect();
const path = await page.evaluate('location.pathname');
const allowed = ['/channels/1568692564637524109/', '/channels/@me/1568693333864480939'];
if (!allowed.some((prefix) => path.startsWith(prefix))) throw new Error(`refusing to type in ${path}`);
await page.evaluate(`document.querySelector('[role="textbox"][data-slate-editor="true"]')?.focus(), 1`);
await page.send('Input.insertText', { text });
if (args.includes('--enter')) {
  for (const type of ['keyDown', 'keyUp']) {
    await page.send('Input.dispatchKeyEvent', { type, key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, text: type === 'keyDown' ? '\r' : undefined });
  }
}
console.log('typed in', path);
process.exit(0);
