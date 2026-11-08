#!/usr/bin/env python3
"""Generates app/src/ui/audit_spec.rs from the bundle: what the audit log says for each action and change.

Reads design/snapshot/pretty/898472.js (the formatter), design/snapshot/dumps/AUDIT_CONSTS.json (its enums, from
tools/dump-audit-consts.mjs) and design/snapshot/dumps/STRINGS/en-US.json (its messages). A message stays in the string
table's parsed form, as JSON, and is formatted at run time.
"""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = (ROOT / 'design/snapshot/pretty/898472.js').read_text(encoding='utf8')
CONSTS = json.loads((ROOT / 'design/snapshot/dumps/AUDIT_CONSTS.json').read_text(encoding='utf8'))
STRINGS = json.loads((ROOT / 'design/snapshot/dumps/STRINGS/en-US.json').read_text(encoding='utf8'))

ID = r'y\.t(?:\.([\w$]+)|\["([^"]+)"\])'
ENUMS = {'k.F_X': 'F_X', 'k.gGk': 'gGk', 'k.GaG': 'GaG', 'k.EkJ': 'EkJ', 'k.PvD': 'PvD', 'k.orn': 'orn', 'k.gh6': 'gh6',
         'j.dD': 'dD', 'j.XG': 'XG', 'j.Ps': 'Ps', 'b.SD': 'SD', 'u.Q': 'Q', 'F.lx': 'lx', 'k.xBc': 'xBc'}
used = set()
missing = set()


def section(start, end):
    a = SOURCE.index(start)
    return SOURCE[a:SOURCE.index(end, a)]


def message_id(match):
    identifier = match.group(1) or match.group(2)
    # A few ids are in a later bundle's table than the dump: the Rust side falls back as `L.intl.format` of nothing does.
    if identifier in STRINGS:
        used.add(identifier)
    else:
        missing.add(identifier)
    return identifier


def ids_in(text):
    return [message_id(match) for match in re.finditer(ID, text)]


def rust(text):
    hashes = '#'
    while f'"{hashes}' in text:
        hashes += '#'
    return f'r{hashes}"{text}"{hashes}'


def enum_value(name):
    """`k.PvD.NONE` to its value, as the API sends it."""
    owner, member = name.rsplit('.', 1)
    return CONSTS[ENUMS[owner]][member]


def balanced_entries(body):
    """Splits an object literal's body into its top level `[key]: value` entries."""
    entries = []
    depth = 0
    current = ''
    in_string = None
    for char in body:
        if in_string:
            current += char
            if char == in_string:
                in_string = None
            continue
        if char in '"\'`':
            in_string = char
        if char in '([{':
            depth += 1
        elif char in ')]}':
            depth -= 1
        if char == ',' and depth == 0:
            entries.append(current.strip())
            current = ''
        else:
            current += char
    if current.strip():
        entries.append(current.strip())
    return entries


def rule_of(value):
    """One entry's rule, in the helpers' terms (`ed`, `eA`, `eg`, `eI`, `eT`, `eN`)."""
    value = re.sub(r'\s+', ' ', value.strip())
    simple = re.fullmatch(r'\(\)\s*=>\s*' + ID, value)
    if simple:
        return f'Rule::Fixed("{message_id(simple)}")'
    for helper, name in (('ed', 'OldNull'), ('eA', 'NewNull'), ('eI', 'Truthy')):
        if value.startswith(helper + '('):
            a, b = ids_in(value)
            return f'Rule::{name}("{a}", "{b}")'
    if value.startswith('eg('):
        ids = ids_in(value)
        return f'Rule::Three("{ids[0]}", "{ids[1]}", "{ids[2]}")'
    if value.startswith('eT(') or value.startswith('eN('):
        inner = value[3:-1]
        close = inner.index('}')
        pairs = []
        for entry in balanced_entries(inner[1:close]):
            key, target = entry.split(':', 1)
            key = key.strip()
            if key.startswith('['):
                key = key[1:-1]
                literal = re.fullmatch(r'y\.intl\.string\(' + ID + r'\)', key)
                if literal:
                    text = STRINGS[literal.group(1) or literal.group(2)]
                    key = ''.join(part for part in text if isinstance(part, str))
                else:
                    key = enum_value(key)
            else:
                key = key.strip('"')
            pairs.append(f'({json.dumps(key)}, "{ids_in(target)[0]}")')
        table = '&[' + ', '.join(pairs) + ']'
        if value.startswith('eT('):
            return f'Rule::Map({table})'
        rest = ids_in(inner[close:])
        return f'Rule::MapOr({table}, "{rest[0]}")'
    if value.startswith('(e) => !0 === (e.newValue ?? e.oldValue)'):
        a, b = ids_in(value)
        return f'Rule::EitherTrue("{a}", "{b}")'
    if value.startswith('(e) => (!0 === e.newValue'):
        a, b = ids_in(value)
        return f'Rule::NewTrue("{a}", "{b}")'
    if value.startswith('(e) => (null == e.newValue.secondary_color'):
        a, b = ids_in(value)
        return f'Rule::Gradient("{a}", "{b}")'
    if value.startswith('(e) => {') and 'e.oldValue && !e.newValue' in value:
        a, b = ids_in(value)
        return f'Rule::Canceled("{a}", "{b}")'
    raise SystemExit(f'unknown rule: {value[:120]}')


def change_rules():
    body = section('function V(e) {', 'let H = {')
    reason = ids_in(section('function B() {', 'function V(e) {'))[0]
    rows = []
    for block in re.split(r'(?=\n\s+case k\.GaG\.)', body)[1:]:
        cases = re.findall(r'case k\.GaG\.(\w+):', block)
        if 'return {' not in block:
            # A case falling through to the next one's table.
            pending.extend(cases)
            continue
        names = pending + cases
        pending.clear()
        inner = block[block.index('return {') + len('return {'):]
        depth = 1
        end = 0
        for index, char in enumerate(inner):
            if char == '{':
                depth += 1
            elif char == '}':
                depth -= 1
                if depth == 0:
                    end = index
                    break
        for entry in balanced_entries(inner[:end]):
            if entry.startswith('...B()'):
                for name in names:
                    rows.append((name, 'reason', f'Rule::Fixed("{reason}")'))
                continue
            head = re.match(r'\[k\.gGk\.(\w+)\]\s*:\s*', entry)
            assert head, entry[:80]
            key = CONSTS['gGk'][head.group(1)]
            rule = rule_of(entry[head.end():])
            for name in names:
                rows.append((name, key, rule))
    return rows, reason


pending = []
rows, reason_id = change_rules()

ignored = []
for target, inner in re.findall(r'\[k\.GaG\.(\w+)\]: \{([^{}]*)\}', section('let H = {', 'function w(e, t)')):
    for key in re.findall(r'\[k\.gGk\.(\w+)\]', inner):
        ignored.append((target, CONSTS['gGk'][key]))

filters = []
for match in re.finditer(r'\{ value: k\.F_X\.(\w+), label: y\.intl\.string\(' + ID + r'\)', section('let Y = () => [', 'function K(e, t)')):
    identifier = match.group(2) or match.group(3)
    if identifier in STRINGS:
        used.add(identifier)
    else:
        missing.add(identifier)
    # `ALL` is the filter's own first item, null on the wire.
    if match.group(1) == 'ALL':
        continue
    filters.append((int(CONSTS['F_X'][match.group(1)]), identifier))

titles = []
title_body = section('function ee(e) {', 'function et(e) {')
waiting = []
for line in title_body.split('\n'):
    case = re.match(r'\s*case k\.F_X\.(\w+):\s*$', line)
    if case:
        waiting.append(case.group(1))
        continue
    done = re.match(r'\s*return ' + ID + r';\s*$', line)
    if done and waiting:
        identifier = message_id(done)
        for action in waiting:
            titles.append((int(CONSTS['F_X'][action]), identifier))
        waiting = []
    elif waiting:
        waiting = []

permissions = []
permission_body = section('function el(e, t) {', 'let er = [')
for match in re.finditer(r'case k\.xBc\.(\w+):\s*(if \(.*?\)\s*return y\.intl\.string\(' + ID + r'\);\s*)?return y\.intl\.string\(' + ID + r'\);', permission_body, re.S):
    name = match.group(1)
    channel = (match.group(3) or match.group(4))
    other = match.group(5) or match.group(6)
    for identifier in (other, channel):
        if identifier in STRINGS:
            used.add(identifier)
        elif identifier:
            missing.add(identifier)
    permissions.append((int(CONSTS['xBc'][name]), f'Some("{channel}")' if channel else 'None', other))

added_flags, removed_flags = [], []
for name, target in (('function et(e) {', removed_flags), ('function en(e) {', added_flags)):
    for flag, identifier_a, identifier_b in re.findall(r'case F\.lx\.(\w+):\s*return y\.intl\.string\(y\.t(?:\.([\w$]+)|\["([^"]+)"\])\);', section(name, 'function el(e, t) {' if name.endswith('en(e) {') else 'function en(e) {')):
        identifier = identifier_a or identifier_b
        if identifier in STRINGS:
            used.add(identifier)
        else:
            missing.add(identifier)
        target.append((int(CONSTS['lx'][flag]), identifier))

# Messages the Rust side names itself: titles that depend on the entry, and the page's own text.
EXTRA = ['QxEVcv', 'ZRFdsL', 'hxnY/q', 'rautds', 'SPWLyT', 'Q/LSXp', 'tzkaD7', 'lNuYhh', 'Ww5Tjy', 'RHhk+P', '30mdIx', 'n+olu7', 'OKp4+o', 'NPOy4G', 'T3KIjz', 'VvNgHX', '4NWSxa', 'eYP6UV', 'wrYNG2', 'Yt6NkU', 'VW2XVd', 'hEqea3', 'qXazAb', 'K4eCZw',
         'tTwGD8', 'lODaeu', 'xuGjhY', 'Oi/in9', '7ZIFm9', 'Br0y5w', '6uaMmO', '2cxQ7G', 'YQsjej', 'SD0PwJ', 'UZ+U3A', 'PyEa+J',
         'hCuAb1', 'JH+89C', 'HUrFDu', 'vynxnV', 'y91UXV', 'K/EdV8', 'MSYhgh', 'ZNQyiR', 'VbpLyU', '7jOoJE', 'jjKYpu', 'z3wbj8',
         'geSp4K', 'iXLF9W', 'xCjYxK', 'k2UNz+', 'MA1ltr', 'RdMMew', '4GQqs8', '8mQ6x0', 'tZw1EW', '/mTqt5', 'Wk4pAJ', 'an9Ry3']
for identifier in EXTRA:
    if identifier in STRINGS:
        used.add(identifier)
    else:
        missing.add(identifier)

lines = [
    '//! What the audit log says for each action and change. Generated by `tools/gen-audit-spec.py` from module 898472,',
    '//! its enums and the string table; do not edit.',
    '',
    '/// How a change picks its message (`ed`, `eA`, `eg`, `eI`, `eT`, `eN` and the few written out in module 898472).',
    '#[derive(Clone, Copy, Debug, PartialEq)]',
    'pub(super) enum Rule {',
    '    Fixed(&\'static str),',
    '    /// The first without an old value, the second with one.',
    '    OldNull(&\'static str, &\'static str),',
    '    /// The first without a new value, the second with one.',
    '    NewNull(&\'static str, &\'static str),',
    '    /// The first with a truthy new value, the second without.',
    '    Truthy(&\'static str, &\'static str),',
    '    /// Both values, only the new one, only the old one.',
    '    Three(&\'static str, &\'static str, &\'static str),',
    '    /// By the new value.',
    '    Map(&\'static [(&\'static str, &\'static str)]),',
    '    MapOr(&\'static [(&\'static str, &\'static str)], &\'static str),',
    '    /// True in the new value, or the old one when there is no new one.',
    '    EitherTrue(&\'static str, &\'static str),',
    '    NewTrue(&\'static str, &\'static str),',
    '    /// One colour or a gradient.',
    '    Gradient(&\'static str, &\'static str),',
    '    Canceled(&\'static str, &\'static str),',
    '}',
    '',
    f'pub(super) const REASON: &str = "{reason_id}";',
    '',
    '/// Target type, change key, rule.',
    'pub(super) const CHANGE_RULES: &[(&str, &str, Rule)] = &[',
]
for target, key, rule in rows:
    lines.append(f'    ("{target}", {json.dumps(key)}, {rule}),')
lines += ['];', '', '/// Changes `w` never shows, by target type (module 898472 `H`).', 'pub(super) const HIDDEN_CHANGES: &[(&str, &str)] = &[']
for target, key in ignored:
    lines.append(f'    ("{target}", {json.dumps(key)}),')
lines += ['];', '', '/// `Y`: the action filter\'s items after All.', 'pub(super) const ACTION_FILTERS: &[(u64, &str)] = &[']
for action, identifier in filters:
    lines.append(f'    ({action}, "{identifier}"),')
lines += ['];', '', '/// `ee`, for the actions with one title.', 'pub(super) const TITLES: &[(u64, &str)] = &[']
for action, identifier in titles:
    lines.append(f'    ({action}, "{identifier}"),')
lines += ['];', '', '/// `el`: bit, its name on a channel if different, its name.', 'pub(super) const PERMISSIONS: &[(u64, Option<&str>, &str)] = &[']
for bit, channel, other in permissions:
    lines.append(f'    ({bit}, {channel}, "{other}"),')
lines += ['];', '', 'pub(super) const ADDED_FLAGS: &[(u64, &str)] = &[']
for flag, identifier in added_flags:
    lines.append(f'    ({flag}, "{identifier}"),')
lines += ['];', '', 'pub(super) const REMOVED_FLAGS: &[(u64, &str)] = &[']
for flag, identifier in removed_flags:
    lines.append(f'    ({flag}, "{identifier}"),')
lines += ['];', '', '/// `getAvailableLocales`.', 'pub(super) const LOCALES: &[(&str, &str)] = &[']
for value, name in sorted(CONSTS['locales'].items()):
    lines.append(f'    ({json.dumps(value)}, {json.dumps(name, ensure_ascii=False)}),')
lines += ['];', '', '/// The messages above, as the string table parses them, sorted by id.', 'pub(super) const MESSAGES: &[(&str, &str)] = &[']
for identifier in sorted(used):
    lines.append(f'    ({json.dumps(identifier)}, {rust(json.dumps(STRINGS[identifier], ensure_ascii=False))}),')
lines += ['];', '']
(ROOT / 'app/src/ui/audit_spec.rs').write_text('\n'.join(lines), encoding='utf8')
print(f'{len(rows)} rules, {len(ignored)} hidden, {len(filters)} filters, {len(titles)} titles, {len(permissions)} permissions, {len(used)} messages; not in the dump: {sorted(missing)}')

# The actions by name, for what the formatter decides in code.
text = (ROOT / 'app/src/ui/audit_spec.rs').read_text(encoding='utf8')
extra = ['/// `F_X`.', 'pub(super) mod action {']
for name, value in CONSTS['F_X'].items():
    if value.isdigit():
        extra.append(f'    pub const {name}: u64 = {value};')
extra += ['}', '']
(ROOT / 'app/src/ui/audit_spec.rs').write_text(text + '\n'.join(extra), encoding='utf8')
