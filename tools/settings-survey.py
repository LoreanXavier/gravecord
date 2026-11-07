"""List the CSS-module classes inside the settings content of each captured page, to see which components the pages use.

python tools/settings-survey.py [min pages]
"""
import collections
import glob
import json
import os
import re
import sys


def classes_in_content(path):
    capture = json.load(open(path, encoding='utf8'))
    strings = capture['snapshot']['strings']
    nodes = capture['snapshot']['documents'][0]['nodes']
    attributes, parents = nodes['attributes'], nodes['parentIndex']

    def attrs(node):
        raw = attributes[node]
        return {strings[raw[k]]: strings[raw[k + 1]] for k in range(0, len(raw), 2)}

    children = collections.defaultdict(list)
    for child, parent in enumerate(parents):
        children[parent].append(child)
    root = next(i for i in range(len(attributes)) if attrs(i).get('class', '').startswith('content_3Rvb29'))
    stack = [root]
    while stack:
        node = stack.pop()
        stack.extend(children[node])
        for cls in attrs(node).get('class', '').split():
            if re.match(r'[A-Za-z][A-Za-z0-9]*_[A-Za-z0-9-]{6}$', cls):
                yield cls


def main():
    minimum = int(sys.argv[1]) if len(sys.argv) > 1 else 2
    counts = collections.Counter()
    pages = collections.defaultdict(set)
    for path in sorted(glob.glob(os.path.join('design', 'dom', 'settings', '*-darker-cozy.json'))):
        name = os.path.basename(path).replace('-darker-cozy.json', '')
        if name.startswith('account-'):
            continue
        for cls in classes_in_content(path):
            counts[cls] += 1
            pages[cls].add(name)
    for cls, count in sorted(counts.items(), key=lambda item: (-len(pages[item[0]]), item[0])):
        if len(pages[cls]) >= minimum:
            print(f'{len(pages[cls]):2} {count:4} {cls}')


main()
