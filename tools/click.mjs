// node tools/click.mjs <css selector | x,y> [wait ms]  Clicks through CDP input, so hover and focus behave as for a real user.
import { connect } from './cdp.mjs';

const [selector, wait = '1200'] = process.argv.slice(2);
if (!selector) throw new Error('usage: click.mjs <css selector> [wait ms]');
const page = await connect();
const point = /^\d+,\d+$/.test(selector) ? selector.split(',').map(Number) : null;
const box = point ? { x: point[0], y: point[1] } : await page.evaluate(`(() => {
  const element = document.querySelector(${JSON.stringify(selector)});
  if (!element) return null;
  element.scrollIntoView({ block: 'nearest' });
  const r = element.getBoundingClientRect();
  return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
})()`);
if (!box) throw new Error(`nothing matches ${selector}`);
const mouse = (type, extra = {}) => page.send('Input.dispatchMouseEvent', { type, x: box.x, y: box.y, ...extra });
await mouse('mouseMoved');
await mouse('mousePressed', { button: 'left', clickCount: 1 });
await mouse('mouseReleased', { button: 'left', clickCount: 1 });
await new Promise((ok) => setTimeout(ok, Number(wait)));
console.log(`clicked ${selector} at ${Math.round(box.x)},${Math.round(box.y)}; now ${await page.evaluate('location.pathname')}`);
page.close();
