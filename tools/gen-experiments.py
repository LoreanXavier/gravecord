"""Writes app/src/plugins/experiments_table.rs: the experiments the pinned client registers, for the Experiments plugin's
page. Read from design/snapshot/modules: the legacy ones (`createExperiment`, module 782460: kind, id, label, treatments)
and the apex ones (module 945810 `mj`: name, kind, variations). The page lists them as the official one does (modules
32523 and 386976): a legacy experiment's buckets are Not Eligible, Control Bucket and "Treatment N: label"; an apex
one's are "Variant N" of its variations.

Usage: python tools/gen-experiments.py  (from the repository root)
"""
import glob
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MODULES = os.path.join(REPO, 'design', 'snapshot', 'modules')
OUT = os.path.join(REPO, 'app', 'src', 'plugins', 'experiments_table.rs')

STRING = r'"((?:[^"\\]|\\.)*)"'


def unescape(text):
    """A JavaScript string literal's body."""
    out, i = [], 0
    while i < len(text):
        c = text[i]
        if c != '\\':
            out.append(c)
            i += 1
            continue
        n = text[i + 1]
        if n == 'x':
            out.append(chr(int(text[i + 2:i + 4], 16)))
            i += 4
        elif n == 'u':
            out.append(chr(int(text[i + 2:i + 6], 16)))
            i += 6
        else:
            out.append({'n': '\n', 't': '\t'}.get(n, n))
            i += 2
    return ''.join(out)


def matching(text, start, open_char, close_char):
    """The index after the bracket that closes the one at `start`, skipping strings."""
    depth, i = 0, start
    while i < len(text):
        c = text[i]
        if c == '"':
            i += 1
            while text[i] != '"':
                i += 2 if text[i] == '\\' else 1
        elif c == open_char:
            depth += 1
        elif c == close_char:
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return -1


def rust_string(text):
    return '"' + text.replace('\\', '\\\\').replace('"', '\\"') + '"'


legacy, apex = {}, {}
for path in sorted(glob.glob(os.path.join(MODULES, '*.js'))):
    source = open(path, encoding='utf-8', errors='replace').read()
    # `createExperiment({kind, id, label, ..., treatments})`, its keys in any order.
    for found in re.finditer(r'\{(?=(?:id|kind):")', source):
        start = found.start()
        end = matching(source, start, '{', '}')
        body = source[start:end]
        at = body.find('treatments:[')
        if at < 0:
            continue
        head = body[:at]
        kind = re.search(r'[{,]kind:"(user|guild)"', head)
        name = re.search(r'[{,]id:' + STRING, head)
        label = re.search(r'[{,]label:' + STRING, head)
        if not (kind and name and label) or not re.match(r'\d{4}-\d{2}', unescape(name.group(1))):
            continue
        treatments_end = matching(body, at + len('treatments:'), '[', ']')
        treatments = body[at:treatments_end]
        # Module 782460's map: Not Eligible and Control first, each treatment by its id (an id 0 takes Control's
        # place).
        buckets = {-1: 'Not Eligible', 0: 'Control Bucket'}
        for treatment in re.finditer(r'\{id:(\d+),label:' + STRING, treatments):
            number = int(treatment.group(1))
            buckets[number] = f'Treatment {number}: {unescape(treatment.group(2))}'
        legacy[unescape(name.group(1))] = (kind.group(1), unescape(label.group(1)), list(buckets.items()))
    for found in re.finditer(r'\{name:' + STRING + r',kind:"(user|guild)"', source):
        name, kind = unescape(found.group(1)), found.group(2)
        start = found.start()
        end = matching(source, start, '{', '}')
        body = source[start:end]
        at = body.find('variations:{')
        if at < 0:
            continue
        variations_start = at + len('variations:')
        variations_end = matching(body, variations_start, '{', '}')
        variations = body[variations_start:variations_end]
        # The object's own keys: a number and a colon right after its `{` or a `,`, outside any nested bracket.
        keys, depth, i = [], 0, 0
        while i < len(variations):
            c = variations[i]
            if c == '"':
                i += 1
                while variations[i] != '"':
                    i += 2 if variations[i] == '\\' else 1
            elif c in '{([':
                depth += 1
            elif c in '})]':
                depth -= 1
            elif depth == 1 and variations[i - 1] in '{,':
                key = re.match(r'(-?\d+):', variations[i:])
                if key:
                    keys.append(int(key.group(1)))
            i += 1
        apex[name] = (kind, sorted(set(keys)))

if len(legacy) < 20 or len(apex) < 200:
    sys.exit(f'too few experiments found: {len(legacy)} legacy, {len(apex)} apex')

lines = [
    '//! Written by `tools/gen-experiments.py` from the pinned client\'s modules: every experiment it registers, for',
    '//! the Experiments plugin\'s page. Do not edit.',
    '',
    'use super::experiments::{Definition, Kind, System};',
    '',
    'pub(crate) const DEFINITIONS: &[Definition] = &[',
]
for name, (kind, title, buckets) in sorted(legacy.items()):
    rows = ', '.join(f'({number}, {rust_string(label)})' for number, label in buckets)
    lines.append(
        f'    Definition {{ name: {rust_string(name)}, title: {rust_string(title)}, '
        f'kind: Kind::{kind.capitalize()}, system: System::Legacy, buckets: &[{rows}] }},'
    )
for name, (kind, keys) in sorted(apex.items()):
    if name in legacy:
        continue
    rows = ', '.join(f'({key}, "Variant {key}")' for key in keys)
    lines.append(
        f'    Definition {{ name: {rust_string(name)}, title: {rust_string(name)}, '
        f'kind: Kind::{kind.capitalize()}, system: System::Apex, buckets: &[{rows}] }},'
    )
lines.append('];')
with open(OUT, 'w', encoding='utf-8', newline='\n') as out:
    out.write('\n'.join(lines) + '\n')
print(f'{OUT}: {len(legacy)} legacy, {len(apex)} apex')
