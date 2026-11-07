"""Generate app/src/ui/settings/pages.rs from the captured settings pages, and the icons their rows lead with.

python tools/gen-settings-pages.py

Every page in design/dom/settings/<page>-darker-cozy.json becomes a const tree of `Node`s, keyed by the official
`data-nav-anchor-key`. Values (switch states, radio choices, slider positions) are the capture's; the app binds them to
the user's settings by key. Containers whose component is not modelled yet become `Custom` with their measured height,
so everything below them keeps its place.
"""
import collections
import json
import os
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOM = ROOT / 'design' / 'dom' / 'settings'
OUT = ROOT / 'app' / 'src' / 'ui' / 'settings' / 'pages.rs'
ICONS = ROOT / 'app' / 'assets' / 'icons'

PAGES = [
    ('DataPrivacy', 'data-privacy'),
    ('MessagingPermissions', 'messaging-permissions'),
    ('Notifications', 'notifications'),
    ('Nitro', 'nitro'),
    ('ServerBoost', 'server-boost'),
    ('Subscriptions', 'subscriptions'),
    ('GiftInventory', 'gift-inventory'),
    ('Billing', 'billing'),
    ('VoiceVideo', 'voice-video'),
    ('Appearance', 'appearance'),
    ('Accessibility', 'accessibility'),
    ('System', 'system'),
    ('LanguageTime', 'language-time'),
    ('RegisteredGames', 'registered-games'),
    ('ActivityPrivacy', 'activity-privacy'),
    ('GameOverlay', 'game-overlay'),
    ('ConnectedApps', 'connected-apps'),
    ('Developer', 'developer'),
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
        self.parent = self.nodes['parentIndex']
        checked = self.nodes.get('inputChecked') or {}
        self.checked = set(checked.get('index', []))
        self.cache = {}

    def attrs(self, node):
        if node not in self.cache:
            raw = self.nodes['attributes'][node]
            self.cache[node] = {self.strings[raw[k]]: self.strings[raw[k + 1]] for k in range(0, len(raw), 2)}
        return self.cache[node]

    def cls(self, node):
        return self.attrs(node).get('class', '')

    def name(self, node):
        return self.strings[self.nodes['nodeName'][node]]

    def walk(self, node, stop=None):
        yield node
        for child in self.children[node]:
            if stop is not None and stop(child):
                continue
            yield from self.walk(child, stop)

    def first(self, node, needle, stop=None):
        return next((inner for inner in self.walk(node, stop) if needle in self.cls(inner)), None)

    def all(self, node, needle, stop=None):
        return [inner for inner in self.walk(node, stop) if needle in self.cls(inner)]

    def is_hidden(self, node):
        return 'hiddenVisually' in self.cls(node)

    def runs(self, node, skip=None):
        """Text of `node` as (text, is_link) runs, without visually hidden spans or `skip` subtrees."""
        out = []

        def visit(inner, link):
            if self.is_hidden(inner) or (skip and skip(inner)):
                return
            link = link or 'anchor_R6ReNl' in self.cls(inner)
            value = self.nodes['nodeValue'][inner]
            if self.name(inner) == '#text' and value >= 0:
                text = self.strings[value]
                if out and out[-1][1] == link:
                    out[-1] = (out[-1][0] + text, link)
                else:
                    out.append((text, link))
            for child in self.children[inner]:
                visit(child, link)

        visit(node, False)
        while out and not out[0][0].strip():
            out.pop(0)
        while out and not out[-1][0].strip():
            out.pop()
        if out:
            out[0] = (out[0][0].lstrip(), out[0][1])
            out[-1] = (out[-1][0].rstrip(), out[-1][1])
        return out

    def text(self, node, skip=None):
        return ''.join(text for text, _ in self.runs(node, skip))


def rust_str(text):
    return json.dumps(text, ensure_ascii=False)


def rust_runs(runs):
    return '&[' + ', '.join(f'({rust_str(text)}, {"true" if link else "false"})' for text, link in runs) + ']'


def write_icon(capture, svg, name):
    attrs = capture.attrs(svg)
    view_box = attrs.get('viewBox', '0 0 24 24')
    shapes = []
    for inner in capture.walk(svg):
        if capture.name(inner) not in ('path', 'circle', 'rect'):
            continue
        shape = capture.attrs(inner)
        fill = shape.get('fill', attrs.get('fill', 'currentColor'))
        if fill == 'transparent':
            continue
        if fill != 'none':
            fill = '#000'
        kept = {k: v for k, v in shape.items() if k in ('d', 'cx', 'cy', 'r', 'x', 'y', 'width', 'height', 'rx', 'fill-rule', 'clip-rule')}
        body = ' '.join(f'{k}="{v}"' for k, v in kept.items())
        shapes.append(f'<{capture.name(inner)} fill="{fill}" {body}/>')
    if not shapes:
        return None
    width, height = view_box.split()[2:4]
    (ICONS / f'{name}.svg').write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" fill="none" viewBox="{view_box}">{"".join(shapes)}</svg>\n',
        encoding='utf8',
    )
    return name


def slug(key):
    return re.sub(r'[^a-z0-9]+', '_', (key or 'unnamed').lower()).strip('_')


class Generator:
    def __init__(self, capture):
        self.capture = capture
        self.unknown = []

    def nested(self, node):
        return 'container_Gl6U0S' in self.capture.cls(node)

    def label_and_badge(self, label):
        c = self.capture
        badge = c.first(label, 'badge') is not None
        text = c.text(label, skip=lambda inner: 'badge' in c.cls(inner))
        return text, badge

    def leaf(self, node, key, indent):
        c = self.capture
        stop = self.nested
        pad = ' ' * indent
        box = c.bounds.get(node, [0, 0, 0, 0])
        label_node = c.first(node, 'label_VhFsFg', stop) or c.first(node, 'baseControlItemTitle_FiiQ6I', stop)
        label, badge = self.label_and_badge(label_node) if label_node is not None else ('', False)
        description_node = c.first(node, 'description_VhFsFg', stop)
        if description_node is None:
            content = c.first(node, 'baseControlItemContent_FiiQ6I', stop)
            if content is not None:
                parts = [child for child in c.children[content] if 'baseControlItemTitle' not in c.cls(child)]
                description_node = parts[0] if parts else None
        description = c.runs(description_node) if description_node is not None else []
        common = f'key: {rust_str(key)}, label: {rust_str(label)}, badge: {str(badge).lower()}, description: {rust_runs(description)}'
        if c.first(node, 'container_0v8Ae1', stop) is not None and label_node is None:
            notice = c.first(node, 'copyContainer_0v8Ae1', stop) or c.first(node, 'container_0v8Ae1', stop)
            return f'{pad}Node::Notice {{ key: {rust_str(key)}, text: {rust_runs(c.runs(notice))} }},'
        switch = c.first(node, 'container_9AraPt', stop)
        if switch is not None:
            checked = any(c.name(inner) == 'INPUT' and inner in c.checked for inner in c.walk(switch))
            return f'{pad}Node::Switch {{ {common}, on: {str(checked).lower()} }},'
        slider = c.first(node, 'slider_OnqacX', stop)
        if slider is not None:
            track = c.first(slider, 'track_OnqacX')
            track_box = c.bounds.get(track, box)
            marks = []
            default = 'None'
            for index, mark in enumerate(c.all(slider, 'mark_OnqacX')):
                if 'markValue' in c.cls(mark) or 'markDash' in c.cls(mark):
                    continue
                mark_box = c.bounds.get(mark)
                fraction = (mark_box[0] + 12 - track_box[0]) / track_box[2] if mark_box else 0
                value = c.first(mark, 'markValue_OnqacX')
                marks.append((c.text(value) if value is not None else '', round(fraction, 5)))
                if 'defaultValue' in c.cls(mark):
                    default = f'Some({len(marks) - 1})'
            grabber = c.first(slider, 'grabber_OnqacX')
            style = c.attrs(grabber).get('style', '') if grabber is not None else ''
            found = re.search(r'inset-inline-start:\s*([\d.]+)%', style)
            value = round(float(found.group(1)) / 100, 5) if found else 0.0
            marks_rust = '&[' + ', '.join(f'({rust_str(text)}, {fraction:.5})' for text, fraction in marks) + ']'
            return f'{pad}Node::Slider {{ {common}, marks: {marks_rust}, default_mark: {default}, value: {value:.5} }},'
        group = c.first(node, 'group_oJuRbG', stop)
        if group is not None:
            options = []
            selected = 0
            for index, option in enumerate(c.all(group, 'radioGroupOption_oJuRbG')):
                described = c.first(option, 'description_oJuRbG')
                title = c.text(option, skip=lambda inner: 'description_oJuRbG' in c.cls(inner))
                runs = c.runs(described) if described is not None else []
                options.append(f'RadioOption {{ label: {rust_str(title)}, description: {rust_runs(runs)} }}')
                if c.attrs(option).get('data-selected') == 'true':
                    selected = index
            return f'{pad}Node::Radio {{ {common}, options: &[{", ".join(options)}], selected: {selected} }},'
        select = c.first(node, 'selectField_vWC4bk', stop)
        if select is not None:
            value_node = c.first(select, 'option_o2hjBM') or c.first(select, 'selectFieldContent_vWC4bk')
            value = c.text(value_node) if value_node is not None else ''
            field = c.first(node, 'container_VhFsFg', stop)
            vertical = field is not None and c.attrs(field).get('data-layout') == 'vertical'
            width = round(c.bounds.get(select, [0, 0, 0, 0])[2], 1)
            return f'{pad}Node::Select {{ {common}, value: {rust_str(value)}, vertical: {str(vertical).lower()}, width: {width:.1f} }},'
        item = c.first(node, 'baseControlItem_FiiQ6I', stop)
        if item is not None:
            leading = c.first(item, 'baseControlItemLeadingElement_FiiQ6I')
            icon = 'None'
            if leading is not None:
                svg = next((inner for inner in c.walk(leading) if c.name(inner).lower() == 'svg'), None)
                if svg is not None and write_icon(c, svg, f'settings_lead_{slug(key)}'):
                    icon = f'Some("settings_lead_{slug(key)}")'
            trailing = c.first(item, 'baseControlItemTrailingElement_FiiQ6I')
            value = ''
            buttons = []
            if trailing is not None:
                button_nodes = c.all(trailing, 'button_F7qvhY')
                for button in button_nodes:
                    kind = 'Secondary'
                    if 'critical-primary' in c.cls(button):
                        kind = 'CriticalPrimary'
                    elif 'critical-secondary' in c.cls(button):
                        kind = 'CriticalSecondary'
                    elif 'primary' in c.cls(button):
                        kind = 'Primary'
                    buttons.append(f'({rust_str(c.text(button))}, ButtonKind::{kind})')
                value = c.text(trailing, skip=lambda inner: 'button_F7qvhY' in c.cls(inner))
            clickable = 'clickable_FiiQ6I' in c.cls(item)
            return (
                f'{pad}Node::Control {{ {common}, icon: {icon}, value: {rust_str(value)}, '
                f'buttons: &[{", ".join(buttons)}], clickable: {str(clickable).lower()} }},'
            )
        self.unknown.append(key)
        height = round(box[3] - 16, 1)
        return f'{pad}Node::Custom {{ key: {rust_str(key)}, height: {height:.1f} }},'

    def container(self, node, indent):
        c = self.capture
        key = c.attrs(node).get('data-nav-anchor-key') or ''
        nested = [child for child in c.walk(node, self.nested) if child != node and self.nested(child)]
        direct = []
        for child in c.walk(node, lambda inner: inner != node and self.nested(inner)):
            pass
        # Direct nested containers: those whose nearest container ancestor is `node`.
        for inner in c.walk(node):
            if inner == node or not self.nested(inner):
                continue
            parent = c.parent[inner]
            while parent >= 0 and not self.nested(parent):
                parent = c.parent[parent]
            if parent == node:
                direct.append(inner)
        if not direct:
            return [self.leaf(node, key, indent)]
        pad = ' ' * indent
        heading = next(
            (inner for inner in c.walk(node, self.nested) if re.search(r'heading-(xl|lg)', c.cls(inner))),
            None,
        )
        title = rust_str(c.text(heading)) if heading is not None else None
        note = []
        if heading is not None:
            stack = c.parent[heading]
            while stack >= 0 and 'stack_TWWX0S' not in c.cls(stack):
                stack = c.parent[stack]
            header = c.parent[stack] if stack >= 0 else -1
            if header >= 0:
                for sibling in c.children[header]:
                    if sibling != stack and c.text(sibling):
                        note = c.runs(sibling)
        group = next(
            (inner for inner in c.walk(node, self.nested) if 'heading-sm/semibold' in c.cls(inner)),
            None,
        )
        group = rust_str(c.text(group)) if group is not None and heading is None else None
        legend = next((inner for inner in c.walk(node, self.nested) if 'title_GX4h9d' in c.cls(inner)), None)
        legend = rust_str(c.text(legend)) if legend is not None and heading is None else None
        lines = [
            f'{pad}Node::Category {{ key: {rust_str(key)}, title: {"Some(" + title + ")" if title else "None"}, '
            f'group: {"Some(" + group + ")" if group else "None"}, legend: {"Some(" + legend + ")" if legend else "None"}, '
            f'description: {rust_runs(note)}, children: &['
        ]
        for child in direct:
            lines.extend(self.container(child, indent + 4))
        lines.append(f'{pad}] }},')
        return lines

    def page(self):
        c = self.capture
        categories = next(i for i in range(len(c.nodes['attributes'])) if c.cls(i).startswith('categories_nNq2Vb'))
        lines = []
        for child in c.children[categories]:
            cls = c.cls(child)
            if 'container_Gl6U0S' in cls:
                lines.extend(self.container(child, 4))
            elif 'divider' in cls:
                lines.append('    Node::Divider,')
            elif 'decorationWrapper' in cls:
                box = c.bounds.get(child, [0, 0, 0, 0])
                # `decorationWrapper sticky` keeps 48px under it.
                lines.append(f'    Node::Custom {{ key: "decoration", height: {round(box[3] + 48, 1):.1f} }},')
        return lines


def main():
    out = [
        '//! Generated by `tools/gen-settings-pages.py` from `design/dom/settings/*-darker-cozy.json`. Do not edit by hand.',
        '//! Keys are the official `data-nav-anchor-key`s; values are the capture\'s and are bound to user settings by key.',
        '',
        'use super::Page;',
        'use super::model::{ButtonKind, Node, RadioOption};',
        '',
        'pub(in crate::ui) fn nodes(page: Page) -> &\'static [Node] {',
        '    match page {',
    ]
    for variant, _ in PAGES:
        out.append(f'        Page::{variant} => {variant.upper()},')
    out += ['        _ => &[],', '    }', '}', '']
    report = []
    for variant, file in PAGES:
        path = DOM / f'{file}-darker-cozy.json'
        generator = Generator(Capture(path))
        body = generator.page()
        out.append(f'const {variant.upper()}: &[Node] = &[')
        out.extend(body)
        out.append('];')
        out.append('')
        report.append(f'{file}: {len(generator.unknown)} custom ({", ".join(generator.unknown[:8])})')
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text('\n'.join(out), encoding='utf8')
    print('\n'.join(report))


main()
