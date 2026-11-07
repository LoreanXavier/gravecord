// node tools/extract-svg-live.mjs <name>=<css selector> [...]
// Copies the first <svg> under each selector from the live clean client into app/assets/icons/<name>.svg, keeping
// groups, transforms and clip paths (Lottie icons need them). Painted fills and strokes become black so gpui can tint
// the result as a mask; "none" and transparent paint stay, and shapes inside <clipPath> are left alone.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { connect } from './cdp.mjs';

const pairs = process.argv.slice(2).map((arg) => {
  const at = arg.indexOf('=');
  return [arg.slice(0, at), arg.slice(at + 1)];
});
if (pairs.length === 0) throw new Error('usage: extract-svg-live.mjs <name>=<selector> ...');
const page = await connect();
const found = JSON.parse(await page.evaluate(`(() => {
  const out = {};
  for (const [name, selector] of ${JSON.stringify(pairs)}) {
    const host = document.querySelector(selector);
    const svg = host && (host.tagName.toLowerCase() === 'svg' ? host : host.querySelector('svg'));
    if (!svg) { out[name] = null; continue; }
    const clone = svg.cloneNode(true);
    const painted = (value) => value && value !== 'none' && value !== 'transparent' && !value.startsWith('url(');
    for (const element of clone.querySelectorAll('*')) {
      if (element.closest('clipPath, mask')) continue;
      for (const attribute of ['fill', 'stroke']) {
        const value = element.getAttribute(attribute);
        if (painted(value)) element.setAttribute(attribute, '#000');
      }
      const style = element.getAttribute('style');
      if (style) element.setAttribute('style', style.replace(/(fill|stroke):\\s*[^;]+/g, (all, key) => all.includes('none') ? all : key + ':#000'));
      element.removeAttribute('class');
    }
    clone.removeAttribute('class');
    clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
    const box = svg.getBoundingClientRect();
    if (!clone.getAttribute('width')) clone.setAttribute('width', String(box.width));
    if (!clone.getAttribute('height')) clone.setAttribute('height', String(box.height));
    out[name] = clone.outerHTML;
  }
  return JSON.stringify(out);
})()`));
page.close();
const dir = join(import.meta.dirname, '..', 'app', 'assets', 'icons');
for (const [name, html] of Object.entries(found)) {
  if (!html) {
    console.log(`${name}: not found`);
    continue;
  }
  writeFileSync(join(dir, `${name}.svg`), html + '\n');
  console.log(`${name}: ${html.length} bytes`);
}
