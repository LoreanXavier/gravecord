// Read forum metadata and previews through the signed-in capture client; save content only under design/.
// node tools/forum-read.mjs <guild id> <forum id>
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';

const [guild, channel, action] = process.argv.slice(2);
if (![guild, channel].every(v => /^\d+$/.test(v ?? ''))) throw new Error('guild and forum ids required');
const page = await connect();
try {
  const data = await page.evaluate(`(async () => {
    ${PRELUDE}
    if (String(GLOBAL_ENV.BUILD_NUMBER) !== '667580') throw new Error('capture build changed');
    if (${action === 'open'}) req(976860).pX('/channels/${guild}/${channel}');
    const forum = req(734057).A.getChannel('${channel}');
    if (!forum || forum.guild_id !== '${guild}' || ![15,16].includes(forum.type)) throw new Error('not the requested forum');
    const threads = Object.values(req(970278).A.getThreadsForParent('${guild}', '${channel}')).map(t => req(734057).A.getChannel(t.id)).filter(Boolean);
    const previews = {};
    for (let at = 0; at < threads.length; at += 10) {
      const result = await http().post({url: '/channels/${channel}/post-data', body: {thread_ids: threads.slice(at, at+10).map(t => t.id)}});
      Object.assign(previews, result.body.threads);
    }
    const archived = await http().get({url: '/channels/${channel}/threads/search', query: {archived: true, sort_by:'last_message_time', sort_order:'desc', limit:25, tag_setting:'match_some', offset:0}});
    return JSON.parse(JSON.stringify({
      forum: {id:forum.id, name:forum.name, topic:forum.topic, type:forum.type,
        availableTags:forum.availableTags, defaultSortOrder:forum.defaultSortOrder,
        defaultForumLayout:forum.defaultForumLayout},
      threads: threads.map(t => ({id:t.id, name:t.name, parent_id:t.parent_id, type:t.type,
        owner_id:t.ownerId, message_count:t.messageCount, flags:t.flags, applied_tags:t.appliedTags,
        last_message_id:t.lastMessageId, thread_metadata:t.threadMetadata})),
      previews, archived: archived.body
    }, (_, value) => typeof value === 'bigint' ? value.toString() : value));
  })()`);
  const folder = join(import.meta.dirname, '..', 'design', 'forum');
  mkdirSync(folder, {recursive:true});
  writeFileSync(join(folder, `${channel}.json`), JSON.stringify(data));
  console.log(`Saved forum metadata: ${data.threads.length} active, ${data.archived.threads?.length ?? 0} archived, ${Object.keys(data.previews).length} previews.`);
} finally { page.close(); }
