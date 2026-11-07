// node tools/scroll.mjs <css selector> <scrollTop>  Scrolls an element in the clean client, for captures of lower content.
import { connect } from './cdp.mjs';

const [selector, top] = process.argv.slice(2);
if (!selector || top === undefined) throw new Error('usage: scroll.mjs <css selector> <scrollTop>');
const page = await connect();
const result = await page.evaluate(`(() => {
  const element = document.querySelector(${JSON.stringify(selector)});
  if (!element) return null;
  element.scrollTop = ${Number(top)};
  return element.scrollTop;
})()`);
await new Promise((ok) => setTimeout(ok, 600));
console.log(result === null ? `nothing matches ${selector}` : `scrolled ${selector} to ${result}`);
page.close();
