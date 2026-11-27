// Builds the structured diff (diff.json) and renders REPORT.md from it.
import {
  classify,
  diffCss,
  diffGroups,
  diffModules,
  diffRecords,
  diffSets,
  diffStrings,
  holidayDiff,
  isEmpty,
  mapToScreens,
} from './diff.mjs';

const LIMIT = 120;

export function buildDiff(oldFacts, newFacts, { oldStrings, newStrings, parity, now = Date.now() }) {
  const modules = diffModules(oldFacts.modules, newFacts.modules);
  const groups = diffGroups(oldFacts.groups, newFacts.groups);
  const css = diffCss(oldFacts, newFacts);
  const holidays = holidayDiff(oldFacts.holidays, newFacts.holidays);
  const soundTables = diffRecords(oldFacts.soundTables, newFacts.soundTables);
  return {
    now,
    modules,
    strings: diffStrings(oldStrings, newStrings),
    endpoints: diffRecords(oldFacts.endpoints, newFacts.endpoints, (x, y) => x === y),
    groups,
    settings: diffSets(oldFacts.settings, newFacts.settings),
    menus: diffRecords(oldFacts.menus, newFacts.menus),
    stores: diffSets(oldFacts.stores, newFacts.stores),
    actions: diffSets(oldFacts.actions, newFacts.actions),
    experiments: diffRecords(oldFacts.experiments, newFacts.experiments, (x, y) => x.kind === y.kind),
    protos: diffRecords(oldFacts.protos, newFacts.protos),
    embeds: diffSets(oldFacts.embeds, newFacts.embeds),
    sounds: diffSets(new Set(Object.values(oldFacts.sounds)), new Set(Object.values(newFacts.sounds))),
    images: diffSets(new Set(Object.values(oldFacts.images)), new Set(Object.values(newFacts.images))),
    css,
    screens: mapToScreens(css, modules.changed, newFacts, parity),
    holidays: { ...holidays, current: newFacts.holidays, soundTables },
  };
}

const count = (record) => Object.keys(record ?? {}).length;
const fence = (text) => '```\n' + text + '\n```';
const trunc = (text, n = 110) => (text.length > n ? text.slice(0, n - 1) + '…' : text);
const day = (ms) => new Date(ms).toISOString().slice(0, 10);

function lines(items, format, limit = LIMIT) {
  if (!items.length) return '_none_\n';
  const shown = items.slice(0, limit).map(format);
  const more = items.length > limit ? `\n- … ${items.length - limit} more in diff.json` : '';
  return shown.map((l) => `- ${l}`).join('\n') + more + '\n';
}

function section(title, total, body, action) {
  const head = `## ${title}${total === null ? '' : ` (${total})`}\n\n`;
  if (total === 0) return `${head}No change.\n\n`;
  return `${head}${body}${action ? `\n**gravecord:** ${action}\n` : ''}\n`;
}

export function renderReport(d, meta) {
  const out = [];
  const { web, desktop } = meta;
  out.push(`# Pin move report: ${meta.from.label} → ${meta.to.label}\n`);
  out.push(`Generated ${new Date(d.now).toISOString()} by \`node tools/pin-watch.mjs\`. ${meta.selfTest ? '**Self-test: the pinned snapshot diffed against itself, so every section must be empty.**' : ''}\n`);
  if (meta.note) out.push(`> ${meta.note}\n`);

  out.push('## Builds\n');
  out.push('| | pinned (design/snapshot) | candidate |\n|---|---|---|');
  out.push(`| desktop host | ${meta.from.desktop} | ${meta.to.desktop} |`);
  out.push(`| web client build | ${meta.from.web} | ${meta.to.web} |`);
  out.push(`| web version hash | ${meta.from.hash} | ${meta.to.hash} |`);
  out.push(`| web built at | ${meta.from.builtAt} | ${meta.to.builtAt} |`);
  out.push(`| channel | ${meta.from.channel} | ${meta.to.channel} |\n`);

  // Summary
  const holidayChange = d.holidays.added.length + d.holidays.removed.length + d.holidays.changed.length;
  const rows = [
    ['Modules added / removed / changed / moved', `${d.modules.added.length} / ${d.modules.removed.length} / ${d.modules.changed.length} / ${d.modules.moved.length}`],
    ['Strings (en-US) added / removed / changed', d.strings ? `${count(d.strings.added)} / ${count(d.strings.removed)} / ${count(d.strings.changed)}` : 'not extracted'],
    ['Endpoints added / removed / changed', `${count(d.endpoints.added)} / ${count(d.endpoints.removed)} / ${count(d.endpoints.changed)}`],
    ['Enum and constant groups changed / added / removed', `${d.groups.matched.filter((g) => g.added.length || g.removed.length || g.changed.length).length} / ${d.groups.added.length} / ${d.groups.removed.length}`],
    ['Settings ids added / removed', `${d.settings.added.length} / ${d.settings.removed.length}`],
    ['Stores added / removed', `${d.stores.added.length} / ${d.stores.removed.length}`],
    ['Action types added / removed', `${d.actions.added.length} / ${d.actions.removed.length}`],
    ['Experiments added / removed / changed', `${count(d.experiments.added)} / ${count(d.experiments.removed)} / ${count(d.experiments.changed)}`],
    ['CSS selectors added / removed / changed / re-hashed', `${count(d.css.rules.added)} / ${count(d.css.rules.removed)} / ${count(d.css.rules.changed)} / ${d.css.rehashed.length}`],
    ['Parity screens reached by changes', String(d.screens.length)],
    ['Seasonal events added / removed / changed', `${holidayChange ? `${d.holidays.added.length} / ${d.holidays.removed.length} / ${d.holidays.changed.length}` : '0 / 0 / 0'}`],
    ['Desktop files changed', desktop ? String(desktop.files.added.length + desktop.files.removed.length + desktop.files.changed.length) : 'not fetched'],
  ];
  out.push('## Summary\n');
  out.push('| Area | Count |\n|---|---|');
  for (const [k, v] of rows) out.push(`| ${k} | ${v} |`);
  out.push('');

  // Seasonal events first: they are the one place a date matters.
  out.push(renderHolidays(d));

  out.push(renderModules(d));
  out.push(renderStrings(d));
  out.push(renderEndpoints(d));
  out.push(renderOpcodes(d));
  out.push(renderSettings(d));
  out.push(renderStores(d));
  out.push(renderConstants(d));
  out.push(renderProtos(d));
  out.push(renderExperiments(d));
  out.push(renderUi(d));
  out.push(renderDesktop(desktop, meta));
  out.push(renderCoverage(meta));
  return out.join('\n');
}

function renderHolidays(d) {
  const h = d.holidays;
  let body = '';
  body += `Source: the module that carries \`startTimeMs\`/\`endTimeMs\` with a \`soundpack\` member (pinned build: 382633), and the soundpack sound table (pinned build: 458640). Both are found by content, not by id.\n\n`;
  body += '| Event (experiment) | Window (UTC) | Soundpack | Desktop only | Effects | Status | Module |\n|---|---|---|---|---|---|---|\n';
  for (const e of h.current) {
    const status = d.now < e.startTimeMs ? `upcoming, starts in ${Math.ceil((e.startTimeMs - d.now) / 864e5)} d` : d.now > e.endTimeMs ? 'ended' : 'ACTIVE';
    body += `| ${e.experiment ?? '(unnamed)'} | ${day(e.startTimeMs)} to ${day(e.endTimeMs)} | ${e.soundpack ?? '-'} | ${e.desktopOnly ? 'yes' : 'no'} | ${e.effects.join(', ') || '-'} | ${status} | ${e.module} |\n`;
  }
  if (!h.current.length) body += '| (none found) | | | | | | |\n';
  body += '\n';
  const tableChange = h.soundTables;
  const total = h.added.length + h.removed.length + h.changed.length + count(tableChange.added) + count(tableChange.removed) + count(tableChange.changed);
  if (h.added.length) body += `**New events**\n\n${lines(h.added, (e) => `${e.experiment ?? e.module}: ${day(e.startTimeMs)} to ${day(e.endTimeMs)}, soundpack \`${e.soundpack}\`${e.desktopOnly ? ', desktop only' : ''}`)}\n`;
  if (h.removed.length) body += `**Events gone**\n\n${lines(h.removed, (e) => `${e.experiment ?? e.module}: ${day(e.startTimeMs)} to ${day(e.endTimeMs)}`)}\n`;
  if (h.changed.length) body += `**Events changed**\n\n${lines(h.changed, ({ from, to }) => `${to.experiment ?? to.module}: window ${day(from.startTimeMs)}..${day(from.endTimeMs)} → ${day(to.startTimeMs)}..${day(to.endTimeMs)}; soundpack ${from.soundpack} → ${to.soundpack}`)}\n`;
  const packs = [...Object.keys(tableChange.added), ...Object.keys(tableChange.changed), ...Object.keys(tableChange.removed)];
  if (packs.length) {
    body += `**Soundpack tables changed:** ${packs.join(', ')}\n\n`;
    for (const [pack, sounds] of Object.entries(tableChange.added)) body += `- new pack \`${pack}\`: ${Object.entries(sounds).map(([k, v]) => `${k}=${v}`).join(', ')}\n`;
    for (const [pack, { from, to }] of Object.entries(tableChange.changed)) {
      const dd = diffRecords(from, to, (x, y) => x === y);
      body += `- \`${pack}\`: +${Object.keys(dd.added).join(', ') || '-'} -${Object.keys(dd.removed).join(', ') || '-'} ~${Object.keys(dd.changed).join(', ') || '-'}\n`;
    }
    body += '\n';
  }
  const known = [...new Set(Object.keys(d.holidays.current.length ? {} : {}))];
  void known;
  return section('Seasonal and holiday events', h.current.length || total ? null : 0, body, total ? 'for each new or moved event add the window, the experiment gate and the soundpack to the soundpack/holiday code, add the pack\'s sound files (SOUNDS.md rows, with the file hash), add the strings (loading tips, label), and a FEATURES.md row. An event whose window passes between pin moves is reached only by moving the pin.' : 'no change since the pin; nothing to do.');
}

function renderModules(d) {
  const m = d.modules;
  const total = m.added.length + m.removed.length + m.changed.length + m.moved.length;
  let body = `${m.total.old} modules pinned, ${m.total.new} now; ${m.unchanged} byte-identical after webpack-id normalisation.\n\n`;
  const who = (x) => `${x.names?.length ? x.names.join(',') : ''}${x.exports?.length ? ` exports {${x.exports.slice(0, 6).join(',')}}` : ''}`.trim() || '(no name or exports)';
  body += `**Added** (${m.added.length})\n\n${lines(m.added, (x) => `\`${x.id}\` ${x.len} B ${who(x)}`, 60)}\n`;
  body += `**Removed** (${m.removed.length})\n\n${lines(m.removed, (x) => `\`${x.id}\` ${x.len} B ${who(x)}`, 60)}\n`;
  body += `**Moved to another id** (${m.moved.length})\n\n${lines(m.moved, (x) => `\`${x.from}\` → \`${x.to}\`${x.identical ? ' identical' : ` changed (${x.delta >= 0 ? '+' : ''}${x.delta} B)`} ${who(x)}`, 60)}\n`;
  body += `**Changed** (${m.changed.length}, largest size change first)\n\n${lines(m.changed, (x) => `\`${x.id}\` ${x.delta >= 0 ? '+' : ''}${x.delta} B (${x.from} → ${x.to}) ${who(x)}`, 60)}\n`;
  return section('Modules', total, body, 'a changed module is only work when it feeds one of the sections below. Read the module diff for any listed module a screen or store depends on. Module ids are not stable; match by the names and exports shown.');
}

function renderStrings(d) {
  const s = d.strings;
  if (!s) return '## Strings\n\nNot extracted (no `dumps/STRINGS/en-US.json` in one of the trees).\n';
  const total = count(s.added) + count(s.removed) + count(s.changed);
  let body = `${s.total.old} en-US messages pinned, ${s.total.new} now. Keys are the bundle's own hashed message ids.\n\n`;
  body += `**Added** (${count(s.added)})\n\n${lines(Object.entries(s.added), ([k, v]) => `\`${k}\` ${trunc(JSON.stringify(v))}`)}\n`;
  body += `**Removed** (${count(s.removed)})\n\n${lines(Object.entries(s.removed), ([k, v]) => `\`${k}\` ${trunc(JSON.stringify(v))}`, 60)}\n`;
  body += `**Changed** (${count(s.changed)})\n\n${lines(Object.entries(s.changed), ([k, v]) => `\`${k}\` ${trunc(JSON.stringify(v.from), 70)} → ${trunc(JSON.stringify(v.to), 70)}`)}\n`;
  return section('Strings (STRINGS/en-US)', total, body, 'replace the catalogs under STRINGS/ (all locales, not only en-US) and rebuild the string table. Changed text moves parity: re-run the text-region parity for any screen showing a changed message. New strings are the best pointer to new UI: read them before reading code.');
}

function renderEndpoints(d) {
  const e = d.endpoints;
  const total = count(e.added) + count(e.removed) + count(e.changed);
  const body =
    `**Added** (${count(e.added)})\n\n${lines(Object.entries(e.added), ([k, v]) => `\`${k}\` ${v}`)}\n` +
    `**Removed** (${count(e.removed)})\n\n${lines(Object.entries(e.removed), ([k, v]) => `\`${k}\` ${v}`)}\n` +
    `**Changed** (${count(e.changed)})\n\n${lines(Object.entries(e.changed), ([k, { from, to }]) => `\`${k}\` \`${from}\` → \`${to}\``)}\n`;
  return section('Endpoints', total, body, 'add or retarget the route in dc-http (routes come from ENDPOINTS.md), add a replay test, and for a new route find its call sites first: the route table only says the path exists. HTTP methods are not in this table.');
}

function renderOpcodes(d) {
  const picks = { gateway: [], voice: [] };
  for (const g of d.groups.matched) {
    const kind = classify({ mem: g.all });
    if (kind && (g.added.length || g.removed.length || g.changed.length)) picks[kind].push(g);
  }
  for (const g of d.groups.added) {
    const kind = classify(g);
    if (kind) picks[kind].push({ added: g.mem, removed: [], changed: [], new: g.m, old: '(new)' });
  }
  const fmt = (g) => `module \`${g.old}\`→\`${g.new}\`: +${g.added.map(([n, v]) => `${n}=${v}`).join(', ') || '-'}  -${g.removed.map(([n, v]) => `${n}=${v}`).join(', ') || '-'}  ~${g.changed.map(([n, a, b]) => `${n} ${a}→${b}`).join(', ') || '-'}`;
  const total = picks.gateway.length + picks.voice.length;
  const body = `**Gateway opcode enums changed**\n\n${lines(picks.gateway, fmt)}\n**Voice opcode enums changed**\n\n${lines(picks.voice, fmt)}\n_Dispatch events are action types: see Stores and action types. Close codes, IDENTIFY shape, capabilities and heartbeat logic are not enums and are not compared here; read GATEWAY.md against module 868 (gateway socket) by hand if a gateway module is in the changed list._\n`;
  return section('Gateway and voice opcodes', total, body, 'update the opcode tables in crates/dc-proto/src/op.rs and the matching match arms in dc-gateway / dc-voice; a new opcode needs a replay test from a recorded frame (TRAFFIC/), and an unknown opcode is a decision, not a guess.');
}

function renderSettings(d) {
  const total = d.settings.added.length + d.settings.removed.length + count(d.menus.added) + count(d.menus.removed) + count(d.menus.changed);
  const body =
    `**Settings ids** (members of the user-settings id enum, found by its anchor members): added ${d.settings.added.length}, removed ${d.settings.removed.length}\n\n` +
    `Added:\n${lines(d.settings.added, (x) => `\`${x}\``)}\nRemoved:\n${lines(d.settings.removed, (x) => `\`${x}\``)}\n` +
    `**Menus (navId groups)**: added ${count(d.menus.added)}, removed ${count(d.menus.removed)}, changed ${count(d.menus.changed)}\n\n` +
    lines(Object.entries(d.menus.added), ([k, v]) => `new \`${k}\` (module ${v.module}) items: ${v.items.slice(0, 8).join(', ')}`) +
    lines(Object.entries(d.menus.removed), ([k]) => `removed \`${k}\``) +
    lines(Object.entries(d.menus.changed), ([k, { from, to }]) => `\`${k}\`: +${to.items.filter((i) => !from.items.includes(i)).join(', ') || '-'} -${from.items.filter((i) => !to.items.includes(i)).join(', ') || '-'}`);
  return section('Settings keys and menus', total, body, 'add a FEATURES.md row per new settings id or menu item (rows are never deleted: a removed one becomes `retired` with the build number), then the control itself in the matching settings page, with its parity screen. Setting defaults, types and gating are not in this static scan: read the module that renders it.');
}

function renderStores(d) {
  const total = d.stores.added.length + d.stores.removed.length + d.actions.added.length + d.actions.removed.length;
  const body =
    `Static scan of \`displayName="…Store"\` and action-type literals (the same patterns as tools/dump-static.mjs, applied to both trees, so the live dumps are not needed to compare). It over-reports action types (any \`type:"X"\` literal) and cannot see store methods; \`stores.live.json\` needs a running client.\n\n` +
    `**Stores added**\n${lines(d.stores.added, (x) => `\`${x}\``)}\n**Stores removed**\n${lines(d.stores.removed, (x) => `\`${x}\``)}\n` +
    `**Action types added**\n${lines(d.actions.added, (x) => `\`${x}\``)}\n**Action types removed**\n${lines(d.actions.removed, (x) => `\`${x}\``)}\n`;
  return section('Stores and action types', total, body, 'port each new store under its official name with its handlers (STORES.md / ACTION_TYPES.md name rules), and each new action type under its official name in dc-state. A new store\'s methods and handlers have to be read from its module (live dump needs the official client running).');
}

function renderConstants(d) {
  const g = d.groups;
  const skip = (x) => classify({ mem: x.all ?? x.mem });
  const changed = g.matched.filter((x) => (x.added.length || x.removed.length || x.changed.length) && !skip(x));
  const total = changed.length + g.added.filter((x) => !skip(x)).length + g.removed.length;
  const label = (x) => `${x.kind === 'n' ? 'enum' : x.kind === 's' ? 'string enum' : 'constants'} ${x.names.join('/')}…(${x.memberCount})`;
  let body = `${g.identical} groups identical. Groups are matched by member names, so a module id change does not show up as a change.\n\n`;
  body += `**Changed groups** (${changed.length})\n\n${lines(changed, (x) => `${label(x)} module \`${x.new}\`: +${x.added.map(([n, v]) => `${n}=${v}`).join(', ') || '-'}  -${x.removed.map(([n, v]) => `${n}=${v}`).join(', ') || '-'}  ~${x.changed.map(([n, a, b]) => `${n} ${a}→${b}`).join(', ') || '-'}`, 100)}\n`;
  body += `**New groups** (${g.added.filter((x) => !skip(x)).length})\n\n${lines(g.added.filter((x) => !skip(x)), (x) => `${x.k === 'n' ? 'enum' : x.k === 's' ? 'string enum' : 'constants'} module \`${x.m}\`: ${x.mem.slice(0, 8).map(([n, v]) => `${n}=${v}`).join(', ')}${x.mem.length > 8 ? '…' : ''}`, 60)}\n`;
  body += `**Removed groups** (${g.removed.length})\n\n${lines(g.removed, (x) => `module \`${x.m}\`: ${x.mem.slice(0, 6).map(([n, v]) => `${n}=${v}`).join(', ')}`, 40)}\n`;
  body += '_Timings, limits and ladders that are plain numbers inside code (not in an enum or a frozen table) are not found by this scan. CONSTANTS.md came from a live walk of module 652215 and is regenerated only with a running client._\n';
  return section('Constants and enums', total, body, 'for each changed constant, update dc-* constants from CONSTANTS.md and re-run the tests that pin it; a changed numeric limit or enum value changes behaviour even when no UI moved.');
}

function renderProtos(d) {
  const total = count(d.protos.added) + count(d.protos.removed) + count(d.protos.changed);
  const body =
    `**Added messages**\n${lines(Object.entries(d.protos.added), ([k, v]) => `\`${k}\` ${v.length} fields`)}\n**Removed**\n${lines(Object.keys(d.protos.removed), (k) => `\`${k}\``)}\n` +
    `**Changed**\n${lines(Object.entries(d.protos.changed), ([k, { from, to }]) => `\`${k}\`: +${to.filter((f) => !from.includes(f)).join(' ') || '-'} -${from.filter((f) => !to.includes(f)).join(' ') || '-'}`)}\n`;
  return section('Settings protos (PROTOS/)', total, body, 'regenerate the matching message in dc-proto from the field list (`no:name:kind`), keep unknown fields round-tripping, and add a round-trip test.');
}

function renderExperiments(d) {
  const total = count(d.experiments.added) + count(d.experiments.removed) + count(d.experiments.changed);
  const body =
    `Experiments defined in code (\`name:"YYYY-MM_…", kind:"user|guild"\`). The server-side assignment is not here; EXPERIMENTS.md is the READY payload of the pinned account.\n\n` +
    `**Added**\n${lines(Object.entries(d.experiments.added), ([k, v]) => `\`${k}\` (${v.kind}, module ${v.module})`)}\n**Removed**\n${lines(Object.keys(d.experiments.removed), (k) => `\`${k}\``)}\n` +
    `**Kind changed**\n${lines(Object.entries(d.experiments.changed), ([k, { from, to }]) => `\`${k}\` ${from.kind} → ${to.kind}`)}\n`;
  return section('Experiments', total, body, 'a new experiment is a gated feature: add its FEATURES.md row with the gate, and the bucket hash (murmur3 of the name) must resolve to the same assignment. Gated-off features stay `blocked-substitute` only if the pinned account is not in them.');
}

function renderUi(d) {
  const css = d.css;
  const total = count(css.rules.added) + count(css.rules.removed) + count(css.rules.changed) + css.rehashed.length + d.modules.changed.length;
  let body = '';
  body += `### Parity screens reached\n\nA screen is listed when a changed CSS selector or a changed class-table module names a class that \`tests/parity/<screen>.json\` wraps (exact class name, else the name without its hash suffix).\n\n`;
  // A shared element (the titlebar) reaches every screen; screens with the same hit list are shown once.
  const grouped = new Map();
  for (const s of d.screens) {
    const key = s.ids.map((i) => i.id).sort().join(',');
    (grouped.get(key) ?? grouped.set(key, { ids: s.ids, screens: [] }).get(key)).screens.push(s.screen);
  }
  body += lines([...grouped.values()], (g) => `**${g.screens.length > 6 ? `${g.screens.length} screens (${g.screens.slice(0, 4).join(', ')}, …)` : g.screens.join(', ')}**: ${g.ids.slice(0, 8).map((i) => `\`${i.id}\``).join(', ')}${g.ids.length > 8 ? ` +${g.ids.length - 8}` : ''}`, 80);
  body += `\n### CSS\n\n${css.selectors.old} selectors pinned, ${css.selectors.new} now.\n\n`;
  const changed = Object.entries(css.rules.changed);
  body += `**Changed rules** (${changed.length})\n\n${lines(changed, ([sel, { from, to }]) => `\`${trunc(sel, 60)}\`: ${trunc(from, 60)} → ${trunc(to, 60)}`, 60)}\n`;
  body += `**Added rules** (${count(css.rules.added)})\n\n${lines(Object.entries(css.rules.added), ([sel, b]) => `\`${trunc(sel, 60)}\` ${trunc(b, 60)}`, 40)}\n`;
  body += `**Removed rules** (${count(css.rules.removed)})\n\n${lines(Object.keys(css.rules.removed), (sel) => `\`${trunc(sel, 80)}\``, 40)}\n`;
  body += `**Re-hashed selectors, same name** (${css.rehashed.length}; ${css.rehashed.filter((r) => !r.sameBody).length} with a changed body)\n\n${lines(css.rehashed, (r) => `\`${trunc(r.from, 50)}\` → \`${trunc(r.to, 50)}\`${r.sameBody ? '' : ' (body changed)'}`, 40)}\n`;
  const tokens = css.tokens;
  body += `**Design tokens (custom properties per scope)**: added ${count(tokens.added)}, removed ${count(tokens.removed)}, changed ${count(tokens.changed)}\n\n${lines(Object.entries(tokens.changed), ([k, { from, to }]) => `\`${trunc(k, 80)}\` ${from} → ${to}`, 40)}\n`;
  body += `**Keyframes**: +${css.keyframes.added.join(', ') || '-'}  -${css.keyframes.removed.join(', ') || '-'}\n\n`;
  body += `### Changed components (modules with a display name or exports)\n\n${lines(d.modules.changed.filter((m) => m.names.length), (m) => `\`${m.id}\` ${m.names.join(',')} ${m.delta >= 0 ? '+' : ''}${m.delta} B`, 60)}\n`;
  return section('UI: components and styles', total, body, 'each listed screen needs `cargo xtask parity --screen <name>` re-run against fresh goldens captured from the new build, plus a recalibration of thresholds from official-vs-official on that build. Token changes go into ds::tokens as values, never as literals. Components are not mapped to screens beyond the class overlap above: a changed component with no listed screen needs a manual look.');
}

function renderDesktop(desktop, meta) {
  if (!desktop) return '## Desktop host and native modules\n\nNot fetched (`--skip-desktop`).\n';
  const f = desktop.files;
  const total = f.added.length + f.removed.length + f.changed.length + desktop.versions.length;
  let body = `Manifest: ${meta.manifestUrl}\n\n`;
  body += `**Module versions in the update manifest vs the pin's BUILD**\n\n${lines(desktop.versions, (v) => v, 40)}\n`;
  body += `**Files** (sha256 against desktop-manifest.tsv of the pin)\n\n- added ${f.added.length}, removed ${f.removed.length}, changed ${f.changed.length}, unchanged ${f.unchanged}\n\n`;
  body += `Changed:\n${lines(f.changed, (x) => `\`${x}\``, 60)}\nAdded:\n${lines(f.added, (x) => `\`${x}\``, 40)}\nRemoved:\n${lines(f.removed, (x) => `\`${x}\``, 40)}\n`;
  if (desktop.notes.length) body += desktop.notes.map((n) => `- ${n}`).join('\n') + '\n';
  return section('Desktop host and native modules', total, body, 'a new module version is a new native surface; a changed `discord_desktop_core` JS file is a changed main-process or preload behaviour, so read the diff and update dc-platform / dc-rpc / dc-detect. `DiscordNative.json` cannot be regenerated without a running client (see Coverage).');
}

function renderCoverage(meta) {
  return `## Coverage: what this run could and could not produce

Produced for the candidate by static reading only (no client was started, nothing was installed):
\`modules/\`, \`MODULE_INDEX.tsv\`, \`dumps/STRINGS/\` (tools/split-modules.mjs and tools/dump-strings.mjs, run unchanged against the candidate tree), and in memory the facts compared above (modules, enums and constants, endpoints, settings ids, menus, stores, action types, experiments, protos, sounds, images, embed types, CSS rules and tokens, seasonal events).

Not produced, because each needs a **running** client (CDP into the page, \`tools/cdp.mjs\` port 9230) and the only way to get one for the candidate is to install it, which this tool never does (the installed \`AppData\\Local\\Discord\` is only read):

- \`stores.live.json\`, \`dispatcher-nodes.json\`, \`actions.live.json\` (tools/dump-live.mjs): store methods, handler maps, the dispatcher graph. The static store/action lists above are the substitute.
- \`endpoints.live.json\`: HTTP-method-bearing route functions evaluated in the page. The static route table above holds the templates.
- \`discord-native.json\` (\`window.DiscordNative\`) and \`CONSTANTS.md\` (live walk of module 652215), \`KEYBINDS.md\`, \`EXPERIMENTS.md\` (the account's READY payload).
- \`design/tokens/<theme>.json\`, \`design/dom/**\`, \`design/golden/**\`: computed styles, DOM and screenshots per screen state. These are the Law 1 inputs; the CSS diff above only says which rules moved.
- \`TRAFFIC/\`: recordings of a signed-in session.

How to get them without touching the installed client: unpack the candidate's host distro (kept as \`desktop-raw/host.distro\`; brotli over tar, \`tar -xf\` after decompressing) into a throwaway directory and launch that copy on the hidden desktop with \`cargo xtask headless <exe>\` and a separate \`--user-data-dir\`, then run the live tools against it. That is a pin-move task, not something this watcher does by itself.
`;
}
