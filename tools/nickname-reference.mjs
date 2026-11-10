// Fixture-only relationship nickname view. No dispatcher action or account write.
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';

const [mode] = process.argv.slice(2);
if (!['preview', 'restore'].includes(mode)) throw new Error('usage: nickname-reference.mjs preview | restore');
const page = await connect();
try {
  console.log(JSON.stringify(await page.evaluate(`(() => {
    ${PRELUDE}
    const key = '__gravecordNicknameReference';
    const store = req(994500).A;
    if (${mode === 'restore'}) {
      const saved = window[key];
      if (!saved) return { restored: false };
      if (saved.descriptor) Object.defineProperty(store, 'getNickname', saved.descriptor);
      else delete store.getNickname;
      delete window[key];
      store.emitChange();
      return { restored: true, nickname: store.getNickname(saved.user) ?? null };
    }
    if (!window[key]) {
      if (location.pathname !== '/channels/@me/1568693333864480939') throw new Error('fixture DM required');
      const channel = req(734057).A.getChannel('1568693333864480939');
      const user = channel?.recipients?.[0];
      if (!user || channel.type !== 1 || !store.isFriend(user)) throw new Error('fixture friend unavailable');
      const original = store.getNickname;
      window[key] = { user, descriptor: Object.getOwnPropertyDescriptor(store, 'getNickname') };
      Object.defineProperty(store, 'getNickname', { configurable: true, writable: true,
        value: function(id) { return id === user ? 'fixture nickname' : original.call(this, id); } });
    }
    store.emitChange();
    return { viewOnly: true, nickname: 'fixture nickname' };
  })()`)));
} finally {
  page.close();
}
