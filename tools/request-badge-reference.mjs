// Temporary view getters for Home, Friends and Pending badge captures. No dispatch or HTTP write.
// Start in the fixture guild; restore also works after navigating to Home.
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';

const [value] = process.argv.slice(2);
const restore = value === 'restore';
const count = Number(value);
if (!restore && (!Number.isSafeInteger(count) || count < 0 || count > 99999)) {
  throw new Error('usage: request-badge-reference.mjs <0..99999> | restore');
}
const page = await connect();
try {
  console.log(JSON.stringify(await page.evaluate(`(() => {
    ${PRELUDE}
    const key = '__gravecordRequestBadgeReference';
    const store = req(994500).A;
    const people = req(595623).Ay;
    if (${restore}) {
      const saved = window[key];
      if (!saved) return { restored: false };
      for (const [target, name, descriptor] of saved.descriptors) {
        if (descriptor) Object.defineProperty(target, name, descriptor);
        else delete target[name];
      }
      delete window[key];
      store.emitChange();
      people.emitChange();
      return { restored: true };
    }
    if (!window[key]) {
      if (!location.pathname.startsWith('/channels/1568692564637524109/')) {
        throw new Error('request badge reference must start in the fixture guild');
      }
      const originalState = people.getState;
      const saved = { count: 0, descriptors: [[store, 'getPendingCount'], [people, 'getState']]
        .map(([target, name]) => [target, name, Object.getOwnPropertyDescriptor(target, name)]) };
      window[key] = saved;
      Object.defineProperty(store, 'getPendingCount', { configurable: true, writable: true,
        value: () => saved.count });
      Object.defineProperty(people, 'getState', { configurable: true, writable: true,
        value: function() {
          const state = originalState.call(this);
          if (saved.rows !== state.rows || saved.rowsCount !== saved.count) {
            saved.rows = state.rows;
            saved.rowsCount = saved.count;
            saved.viewRows = Object.create(state.rows);
            saved.viewRows.getRelationshipCounts = () => ({ ...state.rows.getRelationshipCounts(), 3: saved.count });
          }
          return { ...state, rows: saved.viewRows };
        } });
    }
    window[key].count = ${restore ? 0 : count};
    store.emitChange();
    people.emitChange();
    return { viewOnly: true, count: window[key].count };
  })()`)));
} finally {
  page.close();
}
