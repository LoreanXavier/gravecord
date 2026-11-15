// node tools/capture.mjs <screen> <variant> [hover x,y]
// Writes design/dom/<screen>/<variant>.json (full DOM, bounds, computed styles) and
// design/golden/windows/<screen>/<variant>@<dpr>.png from the clean client's current state.
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { connect } from './cdp.mjs';
import { assertPinnedBuild } from './pinned-client.mjs';

const [screen, variant, hover] = process.argv.slice(2);
if (!screen || !variant) throw new Error('usage: capture.mjs <screen> <variant>');

const SIDES = ['top', 'right', 'bottom', 'left'];
const CORNERS = ['top-left', 'top-right', 'bottom-left', 'bottom-right'];
const COMPUTED = [
  'display', 'position', 'top', 'right', 'bottom', 'left', 'width', 'height', 'min-width', 'min-height', 'max-width',
  'max-height', 'box-sizing', 'flex-direction', 'flex-wrap', 'flex-grow', 'flex-shrink', 'flex-basis', 'align-items',
  'align-self', 'justify-content', 'gap', 'grid-template-columns', 'grid-template-rows',
  ...SIDES.flatMap((s) => [`margin-${s}`, `padding-${s}`, `border-${s}-width`, `border-${s}-style`, `border-${s}-color`]),
  ...CORNERS.map((c) => `border-${c}-radius`),
  'font-family', 'font-size', 'font-weight', 'font-style', 'line-height', 'letter-spacing', 'text-align',
  'text-transform', 'text-decoration-line', 'text-overflow', 'white-space', 'word-break', 'color', 'background-color',
  'background-image', 'background-size', 'background-position', 'box-shadow', 'opacity', 'transform', 'transition',
  'animation-name', 'overflow-x', 'overflow-y', 'z-index', 'cursor', 'pointer-events', 'visibility', 'fill', 'stroke',
];

const root = join(import.meta.dirname, '..', 'design');
const page = await connect();
assertPinnedBuild(
  await page.evaluate('({build:window.GLOBAL_ENV?.BUILD_NUMBER,hash:window.GLOBAL_ENV?.VERSION_HASH})'),
  JSON.parse(readFileSync(join(root, 'snapshot', 'WEB_BUILD.json'), 'utf8')),
);
await page.send('DOMSnapshot.enable');
// A golden must not depend on where the pointer was left: park it on empty titlebar space.
const [x, y] = hover ? hover.split(',').map(Number) : [320, 16];
await page.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
await new Promise((ok) => setTimeout(ok, 500));
const { width, height, dpr, route, theme, density } = await page.evaluate(`({
  width: innerWidth,
  height: innerHeight,
  dpr: devicePixelRatio,
  route: location.pathname,
  theme: [...document.documentElement.classList].filter((c) => c.startsWith('theme-')).join(' '),
  density: [...document.documentElement.classList].find((c) => c.startsWith('density-')),
})`);
const snapshot = await page.send('DOMSnapshot.captureSnapshot', {
  computedStyles: COMPUTED,
  includeDOMRects: true,
  includePaintOrder: true,
});
const { data } = await page.send('Page.captureScreenshot', { format: 'png' });
page.close();

const domPath = join(root, 'dom', screen, `${variant}.json`);
const pngPath = join(root, 'golden', 'windows', screen, `${variant}@${dpr}.png`);
for (const path of [domPath, pngPath]) mkdirSync(dirname(path), { recursive: true });
writeFileSync(domPath, JSON.stringify({ viewport: { width, height, dpr }, route, theme, density, computed: COMPUTED, snapshot }));
writeFileSync(pngPath, Buffer.from(data, 'base64'));

const nodes = snapshot.documents.reduce((sum, doc) => sum + doc.nodes.nodeName.length, 0);
console.log(`${screen}/${variant}: ${width}x${height}@${dpr}, ${nodes} nodes, route ${route}, ${theme}, ${density}`);
