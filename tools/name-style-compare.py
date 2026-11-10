"""Compares display name styles with the official style editor's preview, one side-by-side picture per capture.

python tools/name-style-compare.py list                     the nineteen captures and the harness step for each
python tools/name-style-compare.py run [name ...]           renders our sheet for each (headless, via cargo xtask
                                                             parity) and writes the pictures
python tools/name-style-compare.py compare [name ...]       only redoes the pictures from the latest frames

A capture is `font-<name>` or `effect-<name>` (golden under design/golden/windows/settings, its DOM dump under
design/dom/settings). The official preview shows "onugeccen" in a profile card (20px), a message header (16px, font
only) and a member row (16px). Our `name-sheet` harness step draws the same three at the same positions; the regions
are cropped by the bounds in the DOM dump. Output: design/parity/name-styles/<name>.png with, per place, the
official crop, ours and the difference, and `<name>.txt` with the numbers. Nothing here talks to the network.
"""
import json
import os
import subprocess
import sys

from PIL import Image, ImageChops, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
GOLDEN = os.path.join(ROOT, 'design', 'golden', 'windows', 'settings')
DOM = os.path.join(ROOT, 'design', 'dom', 'settings')
FRAMES = os.path.join(ROOT, 'design', 'parity')
OUT = os.path.join(FRAMES, 'name-styles')

# DisplayNameFont ids (module 559949) of the twelve captured fonts.
FONTS = {'tempo': 12, 'sakura': 3, 'jellybean': 4, 'modern': 6, 'medieval': 7, '8bit': 8, 'vampyre': 10,
         'monkeybars': 13, 'mainframe': 14, 'headbang': 15, 'journal': 16, 'ggsans': 11}
# DisplayNameEffect ids (module 508425).
EFFECTS = {'solid': 1, 'gradient': 2, 'neon': 3, 'toon': 4, 'pop': 5, 'prism': 7, 'gummy': 8}
# The colours each capture had selected. The font pictures and the Toon and Neon effects use the purple swatch; the
# Solid capture had no colour (the default option); the other colours are the first preset row of module 750656.
COLORS = {
    'solid': '-',
    'gradient': '2aaea6,ffc490',
    'neon': '691ded',
    'toon': '9671fc',
    'pop': '0fcf86',
    'gummy': 'e9a9d5,a9ddf0,b9a7ee,b9eacb',
    'prism': 'fe9242,35ed7e,ffe047,ff4cd2,5865f2',
}
# Every animation of the captures stood at this moment (the Prism's background position and the Gummy letters'
# transforms in the dumps both give 1.545 s after the preview mounted).
CAPTURE_MS = 1545
MARGIN = 6
MARGIN_Y = 3
SCALE = 4
PLACES = ['card', 'message', 'row']
NL = chr(10)


def names():
    return ['font-' + n for n in FONTS] + ['effect-' + n for n in EFFECTS]


def load_dom(name):
    path = os.path.join(DOM, f'name-style-{name}-darker-cozy.json')
    capture = json.load(open(path, encoding='utf8'))
    snap = capture['snapshot']
    doc = snap['documents'][0]
    layout = doc['layout']
    layout_of = {node: i for i, node in enumerate(layout['nodeIndex'])}
    return snap['strings'], doc['nodes'], layout, layout_of


def places(name):
    """The name regions (x, y, w, h) of the editor preview from the DOM dump, top to bottom: card, message, row."""
    strings, nodes, layout, layout_of = load_dom(name)
    parents = nodes['parentIndex']

    def attrs(i):
        raw = nodes['attributes'][i]
        return {strings[raw[k]]: strings[raw[k + 1]] for k in range(0, len(raw), 2)}

    found = []
    for i in range(len(parents)):
        value = strings[nodes['nodeValue'][i]] if nodes['nodeValue'][i] >= 0 else ''
        if value.strip() != 'onugeccen' or parents[i] not in layout_of:
            continue
        j = parents[i]
        chosen = None
        for _ in range(4):
            cls = attrs(j).get('class', '')
            if 'innerContainer_or4Hj-' in cls or cls.startswith('username_2i8Bfm'):
                chosen = j
                break
            j = parents[j]
        if chosen is None or chosen not in layout_of:
            continue
        x, y, w, h = layout['bounds'][layout_of[chosen]]
        if x < 690 or any(abs(y - f[1]) < 1 for f in found):
            continue
        found.append((x, y, w, h))
    found.sort(key=lambda f: f[1])
    return found


def all_places(name):
    found = places(name)
    if len(found) == 3:
        return found
    # The Gummy letters sit in their own spans: use the preview's fixed positions with room around them.
    return [(701.5, 311.0, 100.0, 29.0), (779.5, 386.5, 86.0, 22.0), (736.5, 456.5, 82.0, 24.0)]


def box_for(place):
    x, y, w, h = place
    return (int(x - MARGIN), int(y - MARGIN_Y), int(x + w + MARGIN + 1), int(y + h + MARGIN_Y + 1))


def spec(name):
    kind, which = name.split('-', 1)
    if kind == 'font':
        return FONTS[which], EFFECTS['toon'], COLORS['toon']
    return FONTS['tempo'], EFFECTS[which], COLORS[which]


def hexpixel(image, x, y):
    return '%02x%02x%02x' % image.getpixel((int(x), int(y)))[:3]


def step(name):
    font, effect, colors = spec(name)
    golden = Image.open(os.path.join(GOLDEN, f'name-style-{name}-darker-cozy@1.png')).convert('RGB')
    # Plain panel colours sampled beside each name: left of the card name's line, right of the other two.
    backgrounds = [hexpixel(golden, 850, 326), hexpixel(golden, 960, 396), hexpixel(golden, 900, 468)]
    moment = 3000 if name.startswith('font-') else CAPTURE_MS
    # Chromium puts a half-pixel position on the next whole pixel; the sheet's default tops are 314, 387 and 459.
    tops = (314, 387, 459)
    found = places(name)
    dy = [int(place[1] + 0.5) - top for place, top in zip(found, tops)] if name.startswith('font-') and len(found) == 3 else [0, 0, 0]
    return f'name-sheet {font} {effect} {colors} at {moment} bg {" ".join(backgrounds)} dy {" ".join(map(str, dy))}'


def frame_path(name):
    return os.path.join(FRAMES, f'settings-ns-{name}-darker-cozy-latest.png')


def render(name):
    env = dict(os.environ, GRAVECORD_SETTLE_MS='100', GRAVECORD_NO_SETTINGS_WRITE='1', CARGO_BUILD_JOBS='4')
    script = f'{step(name)}; wait 2500'
    subprocess.run(['cargo', 'xtask', 'parity', '--screen', 'settings', '--variant', f'ns-{name}-darker-cozy',
                    '--script', script], cwd=ROOT, env=env, stdout=subprocess.DEVNULL, check=False)


def ink(image):
    """The picture minus its panel: the colour in the crop's top left corner (plain panel beside the name) is taken as
    the panel and subtracted from every pixel."""
    pixels = image.load()
    out = Image.new('RGB', image.size)
    target = out.load()
    for y in range(image.height):
        # The panel of the profile card is a vertical gradient: each row subtracts its own left edge.
        base = pixels[1, y]
        for x in range(image.width):
            target[x, y] = tuple(int(max(0, min(255, 128 + pixels[x, y][c] - base[c]))) for c in range(3))
    return out


def mass(image):
    """The ink's total strength and its centre (x, y) in pixels: what the picture adds to its panel."""
    pixels = image.load()
    total = cx = cy = 0.0
    for y in range(image.height):
        for x in range(image.width):
            weight = sum(abs(pixels[x, y][c] - 128) for c in range(3))
            total += weight
            cx += weight * x
            cy += weight * y
    return total, (cx / total if total else 0.0), (cy / total if total else 0.0)


def compare(name):
    golden = Image.open(os.path.join(GOLDEN, f'name-style-{name}-darker-cozy@1.png')).convert('RGB')
    ours = Image.open(frame_path(name)).convert('RGB')
    rows, report, offsets = [], [], []
    for index, place in enumerate(all_places(name)):
        box = box_for(place)
        a, b = golden.crop(box), ours.crop(box)
        # The panels differ (ours is one colour); only what the names add to their panel is compared.
        ia, ib = ink(a), ink(b)
        diff = ImageChops.difference(ia, ib)
        data = list(diff.get_flattened_data()) if hasattr(diff, 'get_flattened_data') else list(diff.getdata())
        mean = sum(sum(p) for p in data) / (3 * len(data))
        over = sum(1 for p in data if max(p) > 24) / len(data) * 100
        total = sum(sum(abs(v - 128) for v in p) for p in (ia.getdata() if not hasattr(ia, 'get_flattened_data') else ia.get_flattened_data()))
        relative = sum(sum(p) for p in data) / max(1, total) * 100
        (ma, ax, ay), (mb, bx, by) = mass(ia), mass(ib)
        report.append((PLACES[index], round(mean, 2), round(over, 1), round(relative)))
        offsets.append(f'{PLACES[index]}: ours/official ink {mb / ma:.2f}, centre dx {bx - ax:+.2f} dy {by - ay:+.2f} px')
        size = (a.width * SCALE, a.height * SCALE)
        amplified = diff.point(lambda v: min(255, v * 3))
        row = Image.new('RGB', (size[0] * 3 + 20, size[1] + 14), (0, 0, 0))
        for k, tile in enumerate((a, b, amplified)):
            row.paste(tile.resize(size, Image.NEAREST), (k * (size[0] + 10), 14))
        ImageDraw.Draw(row).text((2, 1), f'{PLACES[index]}: official | ours | difference x3 (panel removed)', fill=(255, 255, 255))
        rows.append(row)
    width = max(r.width for r in rows)
    sheet = Image.new('RGB', (width, sum(r.height + 6 for r in rows)), (0, 0, 0))
    y = 0
    for row in rows:
        sheet.paste(row, (0, y))
        y += row.height + 6
    os.makedirs(OUT, exist_ok=True)
    sheet.save(os.path.join(OUT, f'{name}.png'))
    text = NL.join(f'{where}: mean ink difference {mean} of 255, pixels over 24: {over}%, relative to the ink: {rel}%'
                     for where, mean, over, rel in report) + NL + NL.join(offsets)
    open(os.path.join(OUT, f'{name}.txt'), 'w').write(text + NL)
    print(name, report)


def main():
    command = sys.argv[1] if len(sys.argv) > 1 else 'list'
    chosen = sys.argv[2:] or names()
    for name in chosen:
        if command == 'list':
            print(name, '->', step(name), '|', all_places(name))
        elif command == 'run':
            render(name)
            compare(name)
        elif command == 'compare':
            compare(name)


main()
