// Local-only SlowmodeStore preview; sends no request and changes no channel settings.
// node tools/slowmode-preview.mjs <milliseconds>; use 0 to clear.
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';
const millis = Number(process.argv[2]);
if (!Number.isSafeInteger(millis) || millis < 0 || millis > 21600000) throw new Error('invalid cooldown');
const page = await connect();
try {
  const result = await page.evaluate(`(() => {
    ${PRELUDE}
    if (String(GLOBAL_ENV.BUILD_NUMBER) !== '667580') throw new Error('capture build changed');
    const channelId = location.pathname.split('/').at(-1);
    const channel = req(734057).A.getChannel(channelId);
    if (!channel || channel.rateLimitPerUser <= 0) throw new Error('open a slowmode channel first');
    req(73153).h.dispatch({type:'SLOWMODE_SET_COOLDOWN',channelId,slowmodeType:0,cooldownMs:${millis}});
    return {localCooldownMs: req(101392).A.getSlowmodeCooldownGuess(channelId,0)};
  })()`);
  console.log(result);
} finally { page.close(); }
