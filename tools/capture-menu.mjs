// node tools/capture-menu.mjs <name> <x> <y>
// Right-clicks in the clean client, dumps the menu that opens and takes its golden, then closes it with Escape.
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { connect } from './cdp.mjs';

const [name, x, y] = process.argv.slice(2);
if (!name || !x || !y) throw new Error('usage: capture-menu.mjs <name> <x> <y>');
const root = join(import.meta.dirname, '..', 'design');
const page = await connect();
const mouse = (type, extra = {}) => page.send('Input.dispatchMouseEvent', { type, x: Number(x), y: Number(y), ...extra });
await mouse('mouseMoved');
await new Promise((ok) => setTimeout(ok, 300));
await mouse('mousePressed', { button: 'right', buttons: 2, clickCount: 1 });
await mouse('mouseReleased', { button: 'right', buttons: 0, clickCount: 1 });
await new Promise((ok) => setTimeout(ok, 1200));

const info = JSON.parse(await page.evaluate(`(() => {
  const menu = document.querySelector('[role=menu]');
  if (!menu) return JSON.stringify({ missing: true, at: document.elementFromPoint(${Number(x)}, ${Number(y)})?.className?.slice?.(0, 60) });
  const style = getComputedStyle(menu);
  const box = menu.getBoundingClientRect();
  const parts = [...menu.querySelectorAll('[role=menuitem],[role=menuitemcheckbox],[role=menuitemradio],[role=separator],[class*="separator"]')];
  return JSON.stringify({
    id: menu.id,
    rect: [box.x, box.y, box.width, box.height],
    background: style.backgroundColor,
    border: style.borderTopWidth + ' ' + style.borderTopColor,
    radius: style.borderTopLeftRadius,
    padding: style.padding,
    shadow: style.boxShadow,
    items: parts.map((part) => {
      const r = part.getBoundingClientRect();
      const s = getComputedStyle(part);
      return { role: part.getAttribute('role') ?? 'separator', id: part.id, label: (part.innerText ?? '').split('\\n')[0], rect: [r.x, r.y, r.width, r.height], color: s.color, size: s.fontSize, weight: s.fontWeight, radius: s.borderRadius, padding: s.padding };
    }),
  });
})()`));
const { data } = await page.send('Page.captureScreenshot', { format: 'png' });
for (const type of ['keyDown', 'keyUp']) {
  await page.send('Input.dispatchKeyEvent', { type, key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
}
await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: 320, y: 16 });
page.close();

const dumps = join(root, 'snapshot', 'dumps', 'menus');
const golden = join(root, 'golden', 'windows', 'menus');
mkdirSync(dumps, { recursive: true });
mkdirSync(golden, { recursive: true });
writeFileSync(join(dumps, `${name}.json`), JSON.stringify(info, null, 1));
writeFileSync(join(golden, `${name}-darker-cozy@1.png`), Buffer.from(data, 'base64'));
if (info.missing) {
  console.log('no menu opened', info.at);
} else {
  console.log(JSON.stringify({ id: info.id, rect: info.rect, background: info.background, border: info.border, radius: info.radius, padding: info.padding }));
  console.log(info.shadow);
  for (const item of info.items) {
    console.log(item.role.padEnd(10), (item.id ?? '').padEnd(34), item.label.padEnd(24), item.rect.join(','), item.color.slice(0, 30), item.size, item.weight, item.radius, item.padding);
  }
}
