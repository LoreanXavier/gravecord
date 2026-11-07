// node tools/attach.mjs <file> [file...]  Puts files on the composer's hidden file input, as picking them in the dialog
// does. Nothing is sent; the files wait in the upload area until Enter.
import { resolve } from 'node:path';
import { connect } from './cdp.mjs';

const files = process.argv.slice(2).map((file) => resolve(file));
if (files.length === 0) throw new Error('usage: attach.mjs <file> [file...]');
const page = await connect();
await page.send('DOM.enable');
const { root } = await page.send('DOM.getDocument', { depth: 0 });
const { nodeId } = await page.send('DOM.querySelector', { nodeId: root.nodeId, selector: 'input[type="file"]' });
if (!nodeId) throw new Error('no file input on the page');
await page.send('DOM.setFileInputFiles', { nodeId, files });
await new Promise((ok) => setTimeout(ok, 1500));
console.log(`attached ${files.length} file(s)`);
page.close();
