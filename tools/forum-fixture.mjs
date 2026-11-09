// Creates/removes only the temporary forum used by the forum-post capture checks.
// node tools/forum-fixture.mjs create|verify|remove
import { existsSync, mkdirSync, readFileSync, writeFileSync, unlinkSync } from 'node:fs';
import { join } from 'node:path';
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';
const guild = '1568692564637524109';
const name = 'forum-verification';
const record = join(import.meta.dirname, '..', 'design', 'forum', 'fixture.json');
const [action] = process.argv.slice(2);
if (!['create','verify','remove'].includes(action)) throw new Error('create, verify or remove required');
if (action === 'create' && existsSync(record)) throw new Error('fixture record already exists; inspect before reusing');
const channel = action !== 'create' ? JSON.parse(readFileSync(record,'utf8')).id : null;
if (channel !== null && !/^\d+$/.test(channel)) throw new Error('invalid saved channel id');
const page = await connect();
try {
  const result = await page.evaluate(`(async () => {
    ${PRELUDE}
    if (String(GLOBAL_ENV.BUILD_NUMBER) !== '667580') throw new Error('capture build changed');
    if (${action === 'create'}) {
      const reply = await http().post({url:'/guilds/${guild}/channels', body:{type:15,name:'${name}',permission_overwrites:[]}});
      return {id:reply.body.id,guild_id:reply.body.guild_id,name:reply.body.name};
    }
    const check = await http().get({url:'/channels/${channel}'});
    if(check.body.guild_id !== '${guild}' || check.body.name !== '${name}' || check.body.type !== 15) throw new Error('fixture identity mismatch');
    if (${action === 'verify'}) {
      const posts=Object.values(req(970278).A.getThreadsForParent('${guild}','${channel}'))
        .map(t => req(734057).A.getChannel(t.id)).filter(t => t?.name === 'Forum verification 2026-11-09');
      if(posts.length !== 1) throw new Error('posted title not found exactly once');
      const reply=await http().post({url:'/channels/${channel}/post-data',body:{thread_ids:[posts[0].id]}});
      if(reply.body.threads?.[posts[0].id]?.first_message?.content !== 'This post checks the native forum creation path.') throw new Error('first message does not match');
      req(976860).pX('/channels/${guild}/' + posts[0].id);
      return {verified:true, post:posts[0].id};
    }
    await http().del({url:'/channels/${channel}'});
    return {removed:true};
  })()`);
  if (action === 'create') {
    if (result.guild_id !== guild || result.name !== name || !/^\d+$/.test(result.id ?? '')) throw new Error('unexpected created forum');
    mkdirSync(join(import.meta.dirname, '..', 'design', 'forum'), {recursive:true});
    writeFileSync(record, JSON.stringify(result));
    console.log(`Created fixture forum ${result.id}`);
  } else if (action === 'verify') { console.log(`Verified one fixture post and its first message; opened post ${result.post}`); }
  else { unlinkSync(record); console.log('Removed temporary fixture forum'); }
} finally { page.close(); }
