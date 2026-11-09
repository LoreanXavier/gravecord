// Insert only into a focused read-only forum search field. Never focuses or submits a message composer.
// node tools/forum-search.mjs <query>
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';
const [text] = process.argv.slice(2);
if (text === undefined) throw new Error('query required');
const page = await connect();
try {
  await page.evaluate(`(() => {
    ${PRELUDE}
    const channel=req(734057).A.getChannel(location.pathname.split('/').at(-1));
    const field=document.activeElement;
    if (![15,16].includes(channel?.type) || field?.tagName !== 'TEXTAREA' ||
        field.getAttribute('placeholder') !== 'Search' ||
        field.closest('[aria-label]')?.getAttribute('aria-label') !== 'Search') throw new Error('not a read-only forum search field');
    field.select();
    return true;
  })()`);
  await page.send('Input.insertText', { text });
  console.log('Updated forum search; no submission');
} finally { page.close(); }
