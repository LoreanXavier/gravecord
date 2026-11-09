// Read visible presence fields from the isolated reference client; never read credentials or activity secrets.
import { mkdirSync, writeFileSync } from 'node:fs';
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';

const [username] = process.argv.slice(2);
if (!username) throw new Error('usage: presence-probe.mjs <visible username>');
const page = await connect();
const result = await page.evaluate(`(() => {
  ${PRELUDE}
  const stores = findStores();
  const user = Object.values(stores.get('UserStore').getUsers()).find(u => u.username === ${JSON.stringify(username)});
  if (!user) return null;
  const guild = location.pathname.split('/')[2];
  const fields = ['type', 'name', 'application_id', 'details', 'state', 'assets', 'timestamps', 'party', 'buttons', 'sync_id', 'status_display_type'];
  const activities = stores.get('PresenceStore').getActivities(user.id, guild);
  return {activities: activities.map(a => Object.fromEntries(fields.filter(f => a[f] !== undefined).map(f => [f,a[f]]))),
    applications: activities.filter(a=>a.application_id).map(a=> {
      const app = stores.get('ApplicationStore')?.getApplication(a.application_id);
      if (!app) return null;
      return Object.fromEntries(['id','name','icon','thirdPartySkus','primarySkuId'].filter(f=>app[f]!==undefined).map(f=>[f,app[f]]));
    }).filter(Boolean)};
})()`);
page.close();
mkdirSync(new URL('../design/presence/', import.meta.url), {recursive:true});
writeFileSync(new URL('../design/presence/visible-profile.json', import.meta.url), JSON.stringify(result,null,2));
console.log(`read ${result?.activities?.length ?? 0} visible activities`);
