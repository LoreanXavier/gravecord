"""Compare each settings row's box in a parity render with the official container of the same key.

python tools/settings-rows.py <page>    (after `cargo xtask parity --screen settings --variant <page>-darker-cozy ...`)
Prints rows whose top or height differ by more than a pixel, in page order.
"""
import json
import sys

page = sys.argv[1]
ours = json.load(open(f'design/parity/settings-{page}-darker-cozy-latest.layout.json', encoding='utf8'))
capture = json.load(open(f'design/dom/settings/{page}-darker-cozy.json', encoding='utf8'))
strings = capture['snapshot']['strings']
document = capture['snapshot']['documents'][0]
nodes, layout = document['nodes'], document['layout']
bounds = {node: layout['bounds'][index] for index, node in enumerate(layout['nodeIndex'])}
official = []
for node, raw in enumerate(nodes['attributes']):
    attrs = {strings[raw[k]]: strings[raw[k + 1]] for k in range(0, len(raw), 2)}
    key = attrs.get('data-nav-anchor-key')
    if key and 'container_Gl6U0S' in attrs.get('class', '') and node in bounds:
        x, y, w, h = bounds[node]
        official.append((y + 8, key, h - 16))
official.sort()
shown = 0
for top, key, height in official:
    mine = ours.get(key)
    if not mine:
        print(f'{key:55} missing in ours (official top {top:.1f} h {height:.1f})')
        shown += 1
        continue
    dy, dh = mine[1] - top, mine[3] - height
    if abs(dy) > 1 or abs(dh) > 1:
        print(f'{key:55} top {top:7.1f} ours {mine[1]:7.1f} ({dy:+.1f})  h {height:6.1f} ours {mine[3]:6.1f} ({dh:+.1f})')
        shown += 1
    if shown > 25:
        break
