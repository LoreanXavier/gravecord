// Temporary fixture-only GuildReadStateStore getters for badge geometry captures. No dispatch or HTTP write.
// node tools/badge-reference.mjs <count> [low] | restore
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';

const [value, importance] = process.argv.slice(2);
const restore = value === 'restore';
const count = Number(value);
if (!restore && (!Number.isSafeInteger(count) || count < 0 || count > 99999)) {
  throw new Error('usage: badge-reference.mjs <0..99999> [low] | restore');
}
const page = await connect();
try {
  const result = await page.evaluate(`(() => {
    ${PRELUDE}
    const key = '__gravecordBadgeReference';
    const guild = '1568692564637524109';
    const store = req(458294).default;
    if (${restore}) {
      const saved = window[key];
      if (!saved) return { restored: false };
      for (const [name, descriptor] of saved.descriptors) {
        if (descriptor) Object.defineProperty(store, name, descriptor);
        else delete store[name];
      }
      delete window[key];
      store.emitChange();
      return { restored: true };
    }
    if (!location.pathname.startsWith('/channels/' + guild + '/')) {
      throw new Error('badge reference requires the fixture guild');
    }
    if (!window[key]) {
      const names = ['getMentionCount', 'getIsMentionLowImportance', 'hasUnread'];
      const saved = { descriptors: names.map(name => [name, Object.getOwnPropertyDescriptor(store, name)]) };
      window[key] = saved;
      for (const name of names) {
        const original = store[name];
        Object.defineProperty(store, name, { configurable: true, writable: true,
          value: function(id, ...rest) {
            if (id !== guild) return original.call(this, id, ...rest);
            return name === 'getMentionCount' ? saved.count :
              name === 'getIsMentionLowImportance' ? saved.low : saved.count > 0;
          }
        });
      }
    }
    window[key].count = ${restore ? 0 : count};
    window[key].low = ${importance === 'low'};
    store.emitChange();
    return { fixture: true, count: window[key].count, low: window[key].low };
  })()`);
  console.log(JSON.stringify(result));
} finally {
  page.close();
}
