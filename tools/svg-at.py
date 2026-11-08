"""Write the svg drawn at a point of a capture as an icon file (when no label or class singles it out).

python tools/svg-at.py design/dom/<screen>/<variant>.json <x> <y> app/assets/icons/<name>.svg
"""
import json
import sys


def main():
    path, x, y, out = sys.argv[1], float(sys.argv[2]), float(sys.argv[3]), sys.argv[4]
    capture = json.load(open(path, encoding='utf8'))
    strings = capture['snapshot']['strings']
    document = capture['snapshot']['documents'][0]
    nodes, layout = document['nodes'], document['layout']
    at_layout = {node: index for index, node in enumerate(layout['nodeIndex'])}
    children = {}
    for child, parent in enumerate(nodes['parentIndex']):
        children.setdefault(parent, []).append(child)

    def attrs(node):
        raw = nodes['attributes'][node]
        return {strings[raw[k]]: strings[raw[k + 1]] for k in range(0, len(raw), 2)}

    for node in range(len(nodes['parentIndex'])):
        if strings[nodes['nodeName'][node]] != 'svg' or node not in at_layout:
            continue
        left, top = layout['bounds'][at_layout[node]][:2]
        if abs(left - x) > 1 or abs(top - y) > 1:
            continue
        parts = []
        descendants, stack = [], list(children.get(node, []))
        while stack:
            child = stack.pop(0)
            descendants.append(child)
            stack.extend(children.get(child, []))
        for child in descendants:
            shape = attrs(child)
            if strings[nodes['nodeName'][child]] == 'path' and 'd' in shape:
                rule = ''
                if shape.get('fill-rule'):
                    rule = f' fill-rule="{shape["fill-rule"]}" clip-rule="{shape.get("clip-rule", shape["fill-rule"])}"'
                parts.append(f'<path fill="#000"{rule} d="{shape["d"]}"/>')
        view_box = attrs(node).get('viewBox', '0 0 24 24')
        width, height = view_box.split()[2:]
        open(out, 'w', encoding='utf8').write(
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" fill="none" viewBox="{view_box}">'
            + ''.join(parts) + '</svg>\n')
        print(f'{out}: {len(parts)} shapes at {x},{y}')
        return
    sys.exit(f'no svg at {x},{y}')


main()
