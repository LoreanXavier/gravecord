// node tools/click-text.mjs <exact text> [within css selector] [wait ms]
// Finds the element under `within` whose text is exactly <text>, scrolls it into view and clicks its centre through CDP input.
import { connect } from './cdp.mjs';

const [text, within = 'body', wait = '1200'] = process.argv.slice(2);
if (!text) throw new Error('usage: click-text.mjs <text> [within] [wait ms]');
const page = await connect();
const box = await page.evaluate(`(() => {
  const roots = document.querySelectorAll(${JSON.stringify(within)});
  for (const root of roots) {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
    let best = null;
    for (let node = walker.currentNode; node; node = walker.nextNode()) {
      if (node.textContent.trim() === ${JSON.stringify(text)}) best = node;
    }
    if (best) {
      best.scrollIntoView({ block: 'nearest' });
      const r = best.getBoundingClientRect();
      return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
    }
  }
  return null;
})()`);
if (!box) throw new Error(`no element reads ${text}`);
const mouse = (type, extra = {}) => page.send('Input.dispatchMouseEvent', { type, x: box.x, y: box.y, ...extra });
await mouse('mouseMoved');
await mouse('mousePressed', { button: 'left', clickCount: 1 });
await mouse('mouseReleased', { button: 'left', clickCount: 1 });
await new Promise((ok) => setTimeout(ok, Number(wait)));
console.log(`clicked "${text}" at ${Math.round(box.x)},${Math.round(box.y)}`);
page.close();
