// Writes the SVG geometry of the icons on the current screen to app/assets/icons/.
// Icons are found by their position on the capture screen, then normalised for gpui, which paints an SVG as a tinted mask.
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { connect } from './cdp.mjs';

// name: centre point of the icon on channel/text-darker-cozy.
const ICONS = {
  home: [36, 52], add_server: [36, 205], discover: [36, 253],
  chevron_down: [217, 55], invite: [351, 57], events: [98, 111], boosts: [98, 145], category_chevron: [182, 202],
  plus_small: [350, 202], text_channel: [98, 231], voice_channel: [98, 305], settings_small: [350, 231], invite_small: [330, 231],
  header_hash: [400, 57], threads: [880, 57], bell: [920, 57], pin: [960, 57], members: [1000, 57], search: [1259, 57],
  title_back: [24, 16], title_forward: [48, 16], inbox: [1103, 16], help: [1139, 16], minimize: [1192, 16], maximize: [1228, 16], close: [1264, 16],
  attach: [411, 683], gift: [1083, 683], gif: [1123, 683], sticker: [1163, 683], apps: [1243, 683],
  mic: [224, 683], audio_chevron: [249, 683], headphones: [281, 683], settings: [338, 683],
};

const page = await connect();
const found = JSON.parse(await page.evaluate(`(() => {
  const icons = ${JSON.stringify(ICONS)};
  const out = {};
  for (const [name, [x, y]] of Object.entries(icons)) {
    let element = document.elementFromPoint(x, y);
    let svg = null;
    if (element) svg = element.closest('svg') ?? element.querySelector('svg');
    if (!svg) { out[name] = null; continue; }
    const clone = svg.cloneNode(true);
    const r = svg.getBoundingClientRect();
    out[name] = { html: clone.outerHTML, width: r.width, height: r.height, viewBox: svg.getAttribute('viewBox') };
  }
  return JSON.stringify(out);
})()`));
page.close();

const dir = join(import.meta.dirname, '..', 'app', 'assets', 'icons');
mkdirSync(dir, { recursive: true });
const missing = [];
for (const [name, icon] of Object.entries(found)) {
  if (!icon) {
    missing.push(name);
    continue;
  }
  const svg = icon.html
    .replace(/\sclass="[^"]*"/g, '')
    .replace(/\saria-[a-z]+="[^"]*"/g, '')
    .replace(/\srole="[^"]*"/g, '')
    .replace(/currentColor/g, '#000')
    .replace(/var\(--[^)]*\)/g, '#000')
    .replace(/fill="transparent"/g, 'fill="none"');
  writeFileSync(join(dir, `${name}.svg`), svg);
}
console.log(`${Object.keys(found).length - missing.length} icons, missing: ${missing.join(' ') || 'none'}`);
