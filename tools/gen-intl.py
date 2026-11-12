"""Generate the dc-intl data from the pinned build's message catalogs.

python tools/gen-intl.py

The bundle has one `MessageLoader` per catalog (`createLoader({locale: () => import(module)}, "en-US")` of module
95498); 802249 is the main one (what tools/dump-strings.mjs dumps to STRINGS/), the others hold the messages of single
features (the 94 loaders module 449275 `Av` loads on a locale change). Keys are hashes of message names, unique across
catalogs.

Writes
- crates/dc-intl/data/en-US.json.zst: the en-US messages of every loader (each en-US catalog module's
  `JSON.parse('...')`, read from design/snapshot/modules/), compact JSON, module 802249's first, zstd level 19.
- crates/dc-intl/src/reverse.rs: every en-US message that is literal text only (no argument, plural, select or tag),
  as (text, key) pairs sorted by the text's UTF-8 bytes, keys of one text in catalog order.
- crates/dc-intl/src/chunks.rs: for every locale and loader, the catalog module and the asset file it ships in
  (MODULE_INDEX.tsv), which the app fetches from the pinned asset host as the official client does.
"""
import json
import re
from pathlib import Path

import zstandard

ROOT = Path(__file__).resolve().parent.parent
SNAPSHOT = ROOT / 'design' / 'snapshot'
MODULES = SNAPSHOT / 'modules'
CRATE = ROOT / 'crates' / 'dc-intl'
MAIN = '802249'

LOADER = re.compile(r'createLoader:\w+\}=\w+\(95498\),\w+=\w+\(\{(.*?)\},"en-US"\)')
ENTRY = re.compile(r'"?([a-zA-Z]{2}(?:-[a-zA-Z0-9]{2,4})?)"?:\(\)=>n\.e\(\d+\)\.then\(n\.(?:t\.)?bind\(n,(\d+)')


def js_string(source, start):
    """The value of the single-quoted JavaScript literal whose body starts at `start`."""
    units = []
    i = start
    while True:
        ch = source[i]
        if ch == "'":
            break
        if ch == '\\':
            nxt = source[i + 1]
            if nxt == 'x':
                units.append(int(source[i + 2:i + 4], 16))
                i += 4
                continue
            if nxt == 'u':
                if source[i + 2] == '{':
                    end = source.index('}', i)
                    units.extend(memoryview(chr(int(source[i + 3:end], 16)).encode('utf-16-le')).cast('H'))
                    i = end + 1
                    continue
                units.append(int(source[i + 2:i + 6], 16))
                i += 6
                continue
            simple = {'n': 10, 'r': 13, 't': 9, 'b': 8, 'f': 12, 'v': 11, '0': 0}
            if nxt in simple:
                units.append(simple[nxt])
            elif nxt != '\n':
                units.extend(memoryview(nxt.encode('utf-16-le')).cast('H'))
            i += 2
            continue
        units.extend(memoryview(ch.encode('utf-16-le')).cast('H'))
        i += 1
    return b''.join(u.to_bytes(2, 'little') for u in units).decode('utf-16-le', errors='surrogatepass')


def module_catalog(module):
    source = (MODULES / f'{module}.js').read_text(encoding='utf8')
    at = source.index("JSON.parse('") + len("JSON.parse('")
    return json.loads(js_string(source, at))


def plain(message):
    if isinstance(message, str):
        return message
    if all(isinstance(part, str) for part in message):
        return ''.join(message)
    return None


def rust(text):
    out = ['"']
    for ch in text:
        if ch == '\\':
            out.append('\\\\')
        elif ch == '"':
            out.append('\\"')
        elif ch == '\n':
            out.append('\\n')
        elif ch == '\t':
            out.append('\\t')
        elif ord(ch) < 0x20 or ord(ch) == 0x7f:
            out.append('\\u{%x}' % ord(ch))
        else:
            out.append(ch)
    out.append('"')
    return ''.join(out)


index = {}
for line in (SNAPSHOT / 'MODULE_INDEX.tsv').read_text(encoding='utf8').split('\n')[1:]:
    parts = line.split('\t')
    if len(parts) >= 2:
        index[parts[0]] = parts[1]

loaders = []
for path in sorted(MODULES.glob('*.js'), key=lambda path: (path.stem != MAIN, int(path.stem) if path.stem.isdigit() else 0)):
    source = path.read_text(encoding='utf8')
    if '95498' not in source:
        continue
    for match in LOADER.finditer(source):
        loaders.append((path.stem, ENTRY.findall(match.group(1))))

catalog = {}
collisions = 0
for owner, entries in loaders:
    for locale, module in entries:
        if locale != 'en-US':
            continue
        for key, message in module_catalog(module).items():
            if key in catalog and catalog[key] != message:
                collisions += 1
            catalog.setdefault(key, message)

(CRATE / 'data').mkdir(parents=True, exist_ok=True)
body = json.dumps(catalog, ensure_ascii=False, separators=(',', ':')).encode('utf8')
(CRATE / 'data' / 'en-US.json.zst').write_bytes(zstandard.ZstdCompressor(level=19).compress(body))

pairs = []
for order, (key, message) in enumerate(catalog.items()):
    text = plain(message)
    if text is not None and text != '':
        pairs.append((text.encode('utf8'), order, text, key))
pairs.sort()
lines = [
    '// Generated by tools/gen-intl.py from the en-US catalog modules of design/snapshot/modules. Do not edit.',
    '',
    '/// Every en-US message that is literal text only, as (text, key), sorted by the text\'s bytes; keys of one text',
    '/// keep their catalog order.',
    'pub(crate) static REVERSE: &[(&str, &str)] = &[',
]
for _, _, text, key in pairs:
    lines.append(f'    ({rust(text)}, {rust(key)}),')
lines.append('];')
lines.append('')
(CRATE / 'src' / 'reverse.rs').write_text('\n'.join(lines), encoding='utf8', newline='\n')

chunks = []
seen = set()
for owner, entries in loaders:
    for locale, module in entries:
        if locale == 'en-US' or (locale, module) in seen or module not in index:
            continue
        seen.add((locale, module))
        chunks.append((locale, module, index[module]))
chunks.sort(key=lambda row: (row[0], row[1] != dict((l, m) for l, m in loaders[0][1]).get(row[0])))
lines = [
    '// Generated by tools/gen-intl.py from the loaders in design/snapshot/modules and MODULE_INDEX.tsv. Do not edit.',
    '',
    '/// The catalogs of each locale, one per loader (module 802249\'s first): (locale, catalog module, the asset file',
    '/// it ships in).',
    'pub static CHUNKS: &[(&str, &str, &str)] = &[',
]
for locale, module, file in chunks:
    lines.append(f'    ({rust(locale)}, {rust(module)}, {rust(file)}),')
lines.append('];')
lines.append('')
(CRATE / 'src' / 'chunks.rs').write_text('\n'.join(lines), encoding='utf8', newline='\n')
print(f'{len(loaders)} loaders, {len(catalog)} en-US messages ({collisions} key collisions), {len(pairs)} literal, '
      f'{len(chunks)} locale catalogs, {len(body)} bytes')
