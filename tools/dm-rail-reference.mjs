// Fixture DM view getters for rail captures. No global dispatch or HTTP write; read counters stay unchanged.
import { connect } from './cdp.mjs';
import { PRELUDE } from './webpack.mjs';

const [value, layout] = process.argv.slice(2);
const restore = value === 'restore';
const count = Number(value);
if (!restore && (!Number.isSafeInteger(count) || count < 0 || count > 99999 || (layout && layout !== 'group'))) {
  throw new Error('usage: dm-rail-reference.mjs <0..99999> [group] | restore');
}
const page = await connect();
try {
  console.log(JSON.stringify(await page.evaluate(`(() => {
    ${PRELUDE}
    const key = '__gravecordDmRailReference';
    const dm = '1568693333864480939';
    const reads = req(218461).Ay;
    const privateReads = req(131677).A;
    const channels = req(734057).A;
    if (${restore}) {
      const saved = window[key];
      if (!saved) return { restored: false };
      for (const [target, name, descriptor] of saved.descriptors) {
        if (descriptor) Object.defineProperty(target, name, descriptor);
        else delete target[name];
      }
      // Navigation can recompute the derived unread list while the count getter is overridden.
      // 131677's isolated CONNECTION_OPEN handler only rebuilds that list from the real getters.
      saved.rebuild();
      delete window[key];
      reads.emitChange();
      privateReads.emitChange();
      return { restored: true, unreadCount: privateReads.getUnreadPrivateChannelIds().length };
    }
    if (!window[key]) {
      if (!location.pathname.startsWith('/channels/1568692564637524109/')) {
        throw new Error('DM rail reference must start in the fixture guild');
      }
      if (channels.getChannel(dm)?.type !== 1) throw new Error('fixture DM unavailable');
      const originalCount = reads.getMentionCount;
      const originalChannels = channels.getMutablePrivateChannels;
      const node = privateReads._dispatcher?._actionHandlers?._nodes?.get(privateReads._dispatchToken);
      const rebuild = node?.actionHandler?.CONNECTION_OPEN;
      if (typeof rebuild !== 'function') throw new Error('private unread rebuild unavailable');
      const saved = { count: 0, group: false, rebuild,
        descriptors: [[reads, 'getMentionCount'], [privateReads, 'getUnreadPrivateChannelIds'], [channels, 'getMutablePrivateChannels']]
        .map(([target, name]) => [target, name, Object.getOwnPropertyDescriptor(target, name)]) };
      window[key] = saved;
      Object.defineProperty(reads, 'getMentionCount', { configurable: true, writable: true,
        value: function(id, ...rest) { return id === dm ? saved.count : originalCount.call(this, id, ...rest); } });
      Object.defineProperty(privateReads, 'getUnreadPrivateChannelIds', { configurable: true, writable: true,
        value: () => saved.count > 0 ? [dm] : [] });
      Object.defineProperty(channels, 'getMutablePrivateChannels', { configurable: true, writable: true,
        value: function() {
          const real = originalChannels.call(this);
          if (!saved.group) return real;
          const group = Object.create(real[dm]);
          const me = req(287809).default.getCurrentUser().id;
          Object.defineProperties(group, Object.fromEntries(Object.entries({
            type: 3, name: 'fixture group', icon: null, applicationId: null,
            recipients: [real[dm].recipients[0], me]
          }).map(([key, value]) => [key, { value, configurable: true }])));
          return { ...real, [dm]: group };
        } });
    }
    window[key].count = ${restore ? 0 : count};
    window[key].group = ${layout === 'group'};
    reads.emitChange();
    privateReads.emitChange();
    return { fixture: true, viewOnly: true, count: window[key].count, group: window[key].group };
  })()`)));
} finally {
  page.close();
}
