"""Print part of a capture's DOM as an indented tree with bounds, text and chosen computed styles.

python tools/dom-tree.py design/dom/<screen>/<variant>.json <class substring> [depth] [style,style,...]
"""
import json
import sys

DEFAULT_STYLES = 'font-size,line-height,font-weight,color,background-color,padding-left,padding-top,margin-top,border-top-left-radius,gap'


def main():
    path, needle = sys.argv[1], sys.argv[2]
    depth = int(sys.argv[3]) if len(sys.argv) > 3 else 6
    wanted = (sys.argv[4] if len(sys.argv) > 4 else DEFAULT_STYLES).split(',')
    capture = json.load(open(path, encoding='utf8'))
    snapshot = capture['snapshot']
    strings = snapshot['strings']
    document = snapshot['documents'][0]
    nodes, layout, names = document['nodes'], document['layout'], capture['computed']
    attributes, parents = nodes['attributes'], nodes['parentIndex']
    layout_of = {node: index for index, node in enumerate(layout['nodeIndex'])}
    children = {}
    for child, parent in enumerate(parents):
        children.setdefault(parent, []).append(child)

    def attrs(node):
        raw = attributes[node]
        return {strings[raw[k]]: strings[raw[k + 1]] for k in range(0, len(raw), 2)}

    def styles(node):
        if node not in layout_of:
            return {}
        values = [strings[v] if v >= 0 else '' for v in layout['styles'][layout_of[node]]]
        return dict(zip(names, values))

    def show(node, level, parent_style):
        name = strings[nodes['nodeName'][node]]
        value = strings[nodes['nodeValue'][node]] if nodes['nodeValue'][node] >= 0 else ''
        bounds = layout['bounds'][layout_of[node]] if node in layout_of else None
        if name in ('#comment', 'mask', 'defs', 'linearGradient', 'stop', 'filter') or (name == '#text' and not value.strip()):
            return
        own = styles(node)
        changed = {k: own[k] for k in wanted if k in own and own[k] not in ('', 'normal', '0px', 'rgba(0, 0, 0, 0)', 'none') and parent_style.get(k) != own[k]}
        cls = attrs(node).get('class', '')[:48]
        label = attrs(node).get('aria-label', '')
        box = [round(v, 2) for v in bounds] if bounds else ''
        text = repr(value[:60]) if value else ''
        print(f"{'  ' * level}{name} {cls} {box} {text} {label} {changed if changed else ''}".rstrip())
        if level < depth:
            for child in children.get(node, []):
                show(child, level + 1, own or parent_style)

    for node in range(len(attributes)):
        if needle in attrs(node).get('class', ''):
            parent = parents[node]
            show(node, 0, styles(parent) if parent >= 0 else {})
            if "ALL" not in sys.argv: break


main()
