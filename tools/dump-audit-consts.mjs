// node tools/dump-audit-consts.mjs  Writes the enums the audit log formatter reads, from the clean client's modules,
// to design/snapshot/dumps/AUDIT_CONSTS.json, for tools/gen-audit-spec.py.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';

const page = await connect();
const out = await page.evaluate(`(async () => { ${PRELUDE}
  const plain = (value) => {
    if (value == null || typeof value !== 'object') return {};
    const copy = {};
    for (const key of Object.keys(value)) {
      const item = value[key];
      if (['string', 'number', 'bigint'].includes(typeof item)) copy[key] = String(item);
    }
    return copy;
  };
  const m = req(652215);
  const names = ['F_X', 'gGk', 'GaG', 'RWi', 'xBc', 'rbe', 'EkJ', 'PvD', 'orn', 'gh6', 'ogj', 'K3c', 'g0g', 'AO_'];
  const all = {};
  for (const name of names) all[name] = plain(m[name]);
  all.dD = plain(req(988794).dD);
  all.XG = plain(req(988794).XG);
  all.Ps = plain(req(988794).Ps);
  all.SD = plain(req(539916).SD);
  all.Q = plain(req(821418).Q);
  all.lx = plain(req(746080).lx);
  const intl = req(792996);
  all.locales = {};
  for (const locale of intl.getAvailableLocales()) all.locales[locale.value] = locale.name;
  return JSON.stringify(all);
})()`);
const path = join(import.meta.dirname, '..', 'design', 'snapshot', 'dumps', 'AUDIT_CONSTS.json');
writeFileSync(path, out);
const parsed = JSON.parse(out);
console.log(path, Object.entries(parsed).map(([key, value]) => `${key}:${Object.keys(value).length}`).join(' '));
process.exit(0);
