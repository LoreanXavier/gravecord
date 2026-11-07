// Page-side helpers injected over CDP. Reads only; never touches tokens or storage.
export const PRELUDE = `
const __reqs = [];
window.webpackChunkdiscord_app.push([[Symbol('gc')], {}, r => { __reqs.push(r); }]);
window.webpackChunkdiscord_app.pop();
const __size = r => Object.keys(r.c ?? {}).length;
const req = [...__reqs].sort((a, b) => __size(b) - __size(a))[0];
// Stores are keyed by their dispatcher node name. getName() is unusable: some stores override it to return a localized
// message object and others return a minified class name.
// The i18n message proxy answers every property name, so duck typing must first rule out anything that answers nonsense.
const isProxyLike = v => { try { return v.__gc_no_such_property__ !== undefined; } catch { return true; } };
// The raw HTTP client. 499785 wraps it and hangs forever without trackedActionData, so call this one. Build-specific id.
const http = () => req(620632).Bo;
const findStores = () => {
  const found = new Set();
  const seen = new Set();
  const visit = v => {
    if (!v || (typeof v !== 'object' && typeof v !== 'function') || seen.has(v)) return;
    seen.add(v);
    try {
      if (!isProxyLike(v) && typeof v.addChangeListener === 'function' && typeof v._dispatchToken === 'string') found.add(v);
    } catch {}
  };
  for (const m of Object.values(req.c)) {
    const e = m?.exports;
    if (!e) continue;
    visit(e);
    if (typeof e === 'object') for (const k of Object.keys(e)) { try { visit(e[k]); } catch {} }
  }
  const stores = new Map();
  for (const store of found) {
    const node = store._dispatcher?._actionHandlers?._nodes?.get(store._dispatchToken);
    stores.set(node?.name ?? String(store._dispatchToken), store);
  }
  return stores;
};
`;
