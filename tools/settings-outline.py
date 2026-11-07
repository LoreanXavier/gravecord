"""Outline a captured settings page: every anchored setting with its kind, texts and state.

python tools/settings-outline.py design/dom/settings/<page>-darker-cozy.json [--json]

Kinds come from the component classes inside each `container_Gl6U0S` (keyed by `data-nav-anchor-key`).
"""
import collections
import json
import sys

KIND_CLASSES = [
    ('switch', 'container_9AraPt'),
    ('slider', 'slider_OnqacX'),
    ('radio', 'radioGroupOption_oJuRbG'),
    ('select', 'selectField_vWC4bk'),
    ('notice', 'container_0v8Ae1'),
    ('navigator', 'navigatorTrailing_yLRxYp'),
    ('control', 'baseControlItem_FiiQ6I'),
    ('setting', 'baseSettingWrapper_4Z2WJy'),
]


class Capture:
    def __init__(self, path):
        data = json.load(open(path, encoding='utf8'))
        self.strings = data['snapshot']['strings']
        document = data['snapshot']['documents'][0]
        self.nodes = document['nodes']
        layout = document['layout']
        self.bounds = {node: layout['bounds'][index] for index, node in enumerate(layout['nodeIndex'])}
        self.children = collections.defaultdict(list)
        for child, parent in enumerate(self.nodes['parentIndex']):
            self.children[parent].append(child)
        self.attr_cache = {}

    def attrs(self, node):
        if node not in self.attr_cache:
            raw = self.nodes['attributes'][node]
            self.attr_cache[node] = {self.strings[raw[k]]: self.strings[raw[k + 1]] for k in range(0, len(raw), 2)}
        return self.attr_cache[node]

    def cls(self, node):
        return self.attrs(node).get('class', '')

    def name(self, node):
        return self.strings[self.nodes['nodeName'][node]]

    def walk(self, node, stop=None):
        yield node
        for child in self.children[node]:
            if stop and stop(child):
                continue
            yield from self.walk(child, stop)

    def text(self, node):
        parts = []
        for inner in self.walk(node):
            value = self.nodes['nodeValue'][inner]
            if self.name(inner) == '#text' and value >= 0:
                parts.append(self.strings[value])
        return ''.join(parts).strip()

    def first(self, node, needle, stop=None):
        return next((inner for inner in self.walk(node, stop) if needle in self.cls(inner)), None)

    def all(self, node, needle, stop=None):
        return [inner for inner in self.walk(node, stop) if needle in self.cls(inner)]


def describe(capture, node):
    nested = lambda child: 'container_Gl6U0S' in capture.cls(child)
    kinds = [kind for kind, needle in KIND_CLASSES if capture.first(node, needle, nested) is not None]
    entry = {'key': capture.attrs(node).get('data-nav-anchor-key'), 'kinds': kinds}
    bounds = capture.bounds.get(node)
    if bounds:
        entry['box'] = [round(v, 1) for v in bounds]
    heading = next((inner for inner in capture.walk(node, nested) if 'heading-xl' in capture.cls(inner) or 'heading-lg' in capture.cls(inner)), None)
    if heading is not None:
        entry['heading'] = capture.text(heading)
    label = capture.first(node, 'label_VhFsFg', nested) or capture.first(node, 'baseControlItemTitle_FiiQ6I', nested)
    if label is not None:
        entry['label'] = capture.text(label)
    description = capture.first(node, 'description_VhFsFg', nested)
    if description is not None:
        entry['description'] = capture.text(description)
    if 'switch' in kinds:
        switch = capture.first(node, 'container_9AraPt', nested)
        entry['on'] = 'checked' in capture.cls(switch) or any('checked' in capture.cls(i) for i in capture.walk(switch))
        entry['switch_class'] = capture.cls(switch)
    if 'radio' in kinds:
        entry['options'] = []
        for option in capture.all(node, 'radioGroupOption_oJuRbG', nested):
            entry['options'].append({'text': capture.text(option), 'class': capture.cls(option)[:80], 'checked': capture.attrs(option).get('aria-checked')})
    if 'select' in kinds:
        entry['selects'] = [capture.text(field) for field in capture.all(node, 'selectField_vWC4bk', nested)]
    if 'slider' in kinds:
        entry['marks'] = [capture.text(mark) for mark in capture.all(node, 'markValue_OnqacX', nested)]
    buttons = [capture.text(button) for button in capture.all(node, 'button_F7qvhY', nested)]
    if buttons:
        entry['buttons'] = buttons
    return entry


def main():
    capture = Capture(sys.argv[1])
    as_json = '--json' in sys.argv
    panel = next(i for i in range(len(capture.nodes['attributes'])) if capture.cls(i).startswith('panel_nNq2Vb'))
    out = []

    def visit(node, depth):
        for child in capture.children[node]:
            if 'container_Gl6U0S' in capture.cls(child):
                entry = describe(capture, child)
                entry['depth'] = depth
                out.append(entry)
                visit(child, depth + 1)
            elif 'divider_nNq2Vb' in capture.cls(child):
                out.append({'divider': True, 'depth': depth})
            else:
                visit(child, depth)

    visit(panel, 0)
    if as_json:
        print(json.dumps(out, ensure_ascii=False, indent=1))
        return
    for entry in out:
        pad = '  ' * entry['depth']
        if entry.get('divider'):
            print(f'{pad}---')
            continue
        bits = [entry.get('key') or '?', '/'.join(entry['kinds'])]
        for field in ('heading', 'label', 'description', 'on', 'options', 'selects', 'marks', 'buttons'):
            if field in entry:
                value = entry[field]
                if field == 'options':
                    value = [(o['text'][:30], o['checked']) for o in value]
                bits.append(f'{field}={str(value)[:110]}')
        print(pad + ' | '.join(bits))


main()
