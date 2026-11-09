// Read the current guild's screening form and public preview. Never answers or submits the form.
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';

const page = await connect();
try {
  const data = await page.evaluate(`(async () => {
    ${PRELUDE}
    if (String(GLOBAL_ENV.BUILD_NUMBER) !== '667580') throw new Error('capture build changed');
    const guild = location.pathname.split('/')[2];
    if (!/^\\d+$/.test(guild)) throw new Error('not in a guild');
    const form = await http().get({url:'/guilds/'+guild+'/member-verification',query:{with_guild:false}});
    const preview = await http().get({url:'/guilds/'+guild+'/preview'});
    return {guild, form:form.body, preview:preview.body};
  })()`);
  const dir = join(import.meta.dirname, '..', 'design', 'screening');
  mkdirSync(dir, {recursive:true});
  writeFileSync(join(dir, `${data.guild}.json`), JSON.stringify(data, null, 2));
  console.log('Read screening form:', data.form.form_fields.map(f => ({type:f.field_type, required:f.required, entries:f.values?.length ?? 0})));
} finally { page.close(); }
