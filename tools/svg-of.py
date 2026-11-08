"""Write the first svg under a capture node as an icon file.

python tools/svg-of.py design/dom/<screen>/<variant>.json <aria-label or class substring> app/assets/icons/<name>.svg

Paths keep their geometry and fill rules; fills become #000 so the icon can be tinted.
"""
import json
import sys


def main():
    path, needle, out = sys.argv[1], sys.argv[2], sys.argv[3]
    capture = json.load(open(path, encoding='utf8'))
    strings = capture['snapshot']['strings']
    nodes = capture['snapshot']['documents'][0]['nodes']
    parents = nodes['parentIndex']
    names = nodes['nodeName']

    def attrs(node):
        raw = nodes['attributes'][node]
        return {strings[raw[k]]: strings[raw[k + 1]] for k in range(0, len(raw), 2)}

    children = {}
    for child, parent in enumerate(parents):
        children.setdefault(parent, []).append(child)

    def first_svg(node):
        if strings[names[node]] == 'svg':
            return node
        for child in children.get(node, []):
            found = first_svg(child)
            if found is not None:
                return found
        return None

    for node in range(len(parents)):
        found = attrs(node)
        if needle in found.get('aria-label', '') or needle in found.get('class', ''):
            svg = first_svg(node)
            if svg is None:
                continue
            view_box = attrs(svg).get('viewBox', '0 0 24 24')
            parts = []
            for child in children.get(svg, []):
                kind = strings[names[child]]
                shape = attrs(child)
                if kind == 'path' and 'd' in shape:
                    rule = ''
                    if shape.get('fill-rule'):
                        rule = f' fill-rule="{shape["fill-rule"]}" clip-rule="{shape.get("clip-rule", shape["fill-rule"])}"'
                    fill = 'none' if shape.get('fill') in ('none', 'transparent') else '#000'
                    parts.append(f'<path fill="{fill}"{rule} d="{shape["d"]}"/>')
                elif kind == 'circle' and shape.get('fill') not in ('transparent', 'none'):
                    parts.append(f'<circle fill="#000" cx="{shape["cx"]}" cy="{shape["cy"]}" r="{shape["r"]}"/>')
            width, height = view_box.split()[2:]
            svg_text = (
                f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" fill="none" '
                f'viewBox="{view_box}">' + ''.join(parts) + '</svg>\n'
            )
            open(out, 'w', encoding='utf8').write(svg_text)
            print(f'{out}: {len(parts)} shapes from {needle}')
            return
    sys.exit(f'nothing matches {needle}')


main()
