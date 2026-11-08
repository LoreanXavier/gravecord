// node tools/insert.mjs "<text>"
// Inserts text into whatever field of the clean client has focus (click it first with click.mjs). Same guard as
// type.mjs: only inside the gravecord fixture guild and the designated test DM.
import { connect } from './cdp.mjs';

const [text] = process.argv.slice(2);
if (text == null) throw new Error('usage: insert.mjs "<text>"');
const page = await connect();
const path = await page.evaluate('location.pathname');
const allowed = ['/channels/1568692564637524109/', '/channels/@me/1568693333864480939'];
if (!allowed.some((prefix) => path.startsWith(prefix))) throw new Error(`refusing to type in ${path}`);
await page.send('Input.insertText', { text });
console.log('inserted in', path);
process.exit(0);
