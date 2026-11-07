// node tools/dom-summary.mjs <dom json> [minimum width]  Flat readable view of a captured DOM: box, tag, class, key styles.
import { readFileSync } from 'node:fs';

const [file, minimum = '0'] = process.argv.slice(2);
const { snapshot, computed } = JSON.parse(readFileSync(file, 'utf8'));
const { strings, documents } = snapshot;
const doc = documents[0];
const text = (index) => (index >= 0 ? strings[index] : '');
const style = (node, name) => {
  const row = doc.layout.styles[node];
  return row ? text(row[computed.indexOf(name)]) : '';
};
const SKIP = new Set(['path', 'circle', 'rect', 'ellipse', 'polygon', 'mask', 'g', 'defs', 'foreignObject', 'image', 'use']);
const layoutOf = new Map(doc.layout.nodeIndex.map((node, row) => [node, row]));
const depth = new Array(doc.nodes.nodeName.length).fill(0);
doc.nodes.parentIndex.forEach((parent, node) => { if (parent >= 0) depth[node] = depth[parent] + 1; });
// Chromium reports computed colors in oklab; convert to the sRGB hex the UI needs.
function hex(value) {
  const match = /oklab\(([-\d.e]+) ([-\d.e]+) ([-\d.e]+)(?: \/ ([\d.]+))?\)/.exec(value);
  if (match) {
    const [L, a, b, alpha = 1] = match.slice(1).map(Number);
    const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
    const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
    const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
    const linear = [4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s, -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s, -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s];
    const channel = (c) => Math.round(255 * Math.min(1, Math.max(0, c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055))).toString(16).padStart(2, '0');
    const out = '#' + linear.map(channel).join('');
    return alpha < 1 ? out + Math.round(alpha * 255).toString(16).padStart(2, '0') : out;
  }
  return value;
}
const SHORT = { color: 'c', 'background-color': 'bg', 'font-size': 'fs', 'font-weight': 'fw', 'line-height': 'lh', 'font-family': 'ff', 'padding-left': 'pl', 'padding-top': 'pt', 'margin-left': 'ml', 'margin-top': 'mt', 'border-top-left-radius': 'r', 'flex-direction': 'dir' };
const INHERITED = new Set(['color', 'font-size', 'font-weight', 'line-height', 'font-family']);
const parentValue = (node, name) => {
  const parent = doc.nodes.parentIndex[node];
  const row = layoutOf.get(parent);
  return row === undefined ? '' : hex(text(doc.layout.styles[row][computed.indexOf(name)]));
};
const WANT = ['display', 'flex-direction', 'color', 'background-color', 'font-size', 'font-weight', 'line-height', 'font-family', 'padding-left', 'padding-top', 'margin-left', 'margin-top', 'gap', 'border-top-left-radius'];
for (const [node, row] of layoutOf) {
  const name = text(doc.nodes.nodeName[node]);
  const [x, y, w, h] = doc.layout.bounds[row];
  if (name === '#text') { const t = text(doc.layout.text[row]); if (t.trim()) console.log(' '.repeat(Math.min(depth[node], 40)) + `"${t.slice(0, 50)}" ${x},${y} ${w}x${h}`); continue; }
  if (w < Number(minimum) || w === 0 || h === 0 || SKIP.has(name) || x < 0) continue;
  const attributes = doc.nodes.attributes[node] ?? [];
  let cls = '';
  for (let i = 0; i < attributes.length; i += 2) if (text(attributes[i]) === 'class') cls = text(attributes[i + 1]);
  const values = doc.layout.styles[row];
  const pick = WANT.map((n) => {
    const v = hex(text(values[computed.indexOf(n)]));
    if (!v || /^(0px|normal|none|rgba\(0, 0, 0, 0\)|block|row|#[0-9a-f]{6}00)$/.test(v)) return '';
    if (INHERITED.has(n) && parentValue(node, n) === v) return '';
    return `${SHORT[n] ?? n}:${v.slice(0, 30)}`;
  }).filter(Boolean).join(' ');
  console.log(' '.repeat(Math.min(depth[node], 40)) + `${name} .${cls.split(' ')[0].slice(0, 26)} ${x},${y} ${w}x${h} ${pick}`);
}
