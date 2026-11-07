"""Write the SVG icons inside matching elements of a capture to app/assets/icons/, normalised for gpui (a tinted mask).

python tools/extract-dom-icons.py design/dom/<screen>/<variant>.json <class substring> <prefix>

Each matching element gives one icon, named <prefix>_<slug of its text>. Paths painted with currentColor become black;
explicit "none" fills stay, so outlines and cut-outs keep their shape.
"""
import json
import re
import sys
from pathlib import Path


def main():
    path, needle, prefix = sys.argv[1:4]
    capture = json.load(open(path, encoding='utf8'))
    strings = capture['snapshot']['strings']
    nodes = capture['snapshot']['documents'][0]['nodes']
    attributes, parents = nodes['attributes'], nodes['parentIndex']
    children = {}
    for child, parent in enumerate(parents):
        children.setdefault(parent, []).append(child)

    def attrs(node):
        raw = attributes[node]
        return {strings[raw[k]]: strings[raw[k + 1]] for k in range(0, len(raw), 2)}

    def name(node):
        return strings[nodes['nodeName'][node]]

    def walk(node):
        yield node
        for child in children.get(node, []):
            yield from walk(child)

    def text(node):
        parts = []
        for inner in walk(node):
            value = nodes['nodeValue'][inner]
            if name(inner) == '#text' and value >= 0 and strings[value].strip():
                parts.append(strings[value].strip())
        return ' '.join(parts)

    out = Path(__file__).resolve().parent.parent / 'app' / 'assets' / 'icons'
    written = []
    for node in range(len(attributes)):
        if needle not in attrs(node).get('class', ''):
            continue
        svg = next((inner for inner in walk(node) if name(inner).lower() == 'svg'), None)
        if svg is None:
            continue
        svg_attrs = attrs(svg)
        view_box = svg_attrs.get('viewBox', '0 0 24 24')
        shapes = []
        for inner in walk(svg):
            if name(inner) not in ('path', 'circle', 'rect'):
                continue
            shape = attrs(inner)
            fill = shape.get('fill', svg_attrs.get('fill', 'currentColor'))
            if fill in ('transparent',):
                continue
            if fill != 'none':
                fill = '#000'
            kept = {k: v for k, v in shape.items() if k in ('d', 'cx', 'cy', 'r', 'x', 'y', 'width', 'height', 'rx', 'fill-rule', 'clip-rule')}
            stroke = shape.get('stroke')
            extra = f' stroke="#000" stroke-width="{shape.get("stroke-width", "1")}"' if stroke and stroke != 'none' else ''
            body = ' '.join(f'{k}="{v}"' for k, v in kept.items())
            shapes.append(f'<{name(inner)} fill="{fill}"{extra} {body}/>')
        slug = re.sub(r'[^a-z0-9]+', '_', text(node).lower()).strip('_') or f'icon{node}'
        file = out / f'{prefix}_{slug}.svg'
        width, height = view_box.split()[2:4]
        file.write_text(f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" fill="none" viewBox="{view_box}">{"".join(shapes)}</svg>\n', encoding='utf8')
        written.append(file.name)
    print(f'{len(written)} icons: {", ".join(written)}')


main()
