// node tools/eval.mjs "<expression>"
// Evaluates one expression in the clean capture client's page and prints the JSON result. Loopback CDP only; never
// use it to read tokens or storage.
import { connect } from './cdp.mjs';

const [expression] = process.argv.slice(2);
if (!expression) throw new Error('usage: eval.mjs "<expression>"');
const page = await connect();
const value = await page.evaluate(expression);
console.log(JSON.stringify(value, null, 1));
process.exit(0);
