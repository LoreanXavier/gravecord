// node tools/key.mjs <Key> [code] [windowsVirtualKeyCode]   e.g. Escape, or ArrowUp ArrowUp 38
// Presses and releases one key in the clean capture client through CDP input.
import { connect } from './cdp.mjs';

const KNOWN = { Escape: ['Escape', 27], Enter: ['Enter', 13], Tab: ['Tab', 9], ArrowUp: ['ArrowUp', 38], ArrowDown: ['ArrowDown', 40] };
const [key, code, vk] = process.argv.slice(2);
if (!key) throw new Error('usage: key.mjs <Key> [code] [vk]');
const [knownCode, knownVk] = KNOWN[key] ?? [code ?? key, Number(vk ?? 0)];
const page = await connect();
for (const type of ['keyDown', 'keyUp']) {
  await page.send('Input.dispatchKeyEvent', { type, key, code: code ?? knownCode, windowsVirtualKeyCode: knownVk });
}
console.log('pressed', key);
process.exit(0);
