"""Write the icons the OAuth2 consent modal's detail lines use that the capture does not show, from their icon modules.

python tools/gen-oauth-icons.py

LinkIcon 173936, HammerIcon 215026, RobotIcon 430392, GlobeEarthIcon 998445, EmbedIcon 11023 (module 610056 `em`).
The lock, clock and shield are in `design/dom/popouts/activity-consent-darker-cozy.json` (tools/extract-dom-icons.py).
"""
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MODULES = ROOT / 'design' / 'snapshot' / 'modules'
OUT = ROOT / 'app' / 'assets' / 'icons'
ICONS = {173936: 'oauth_entry_link', 215026: 'oauth_entry_hammer', 430392: 'oauth_entry_robot', 998445: 'oauth_entry_globe', 11023: 'oauth_entry_embed'}


def main():
    for module, name in ICONS.items():
        source = (MODULES / f'{module}.js').read_text(encoding='utf8')
        shapes = []
        for match in re.finditer(r'\(0,i\.jsx\)\("path",\{fill:[^,]+,((?:fillRule:"evenodd",)?)d:"([^"]+)"(,clipRule:"evenodd")?', source):
            rule = ' fill-rule="evenodd" clip-rule="evenodd"' if match.group(1) else ''
            shapes.append(f'<path fill="#000"{rule} d="{match.group(2)}"/>')
        assert shapes, module
        (OUT / f'{name}.svg').write_text(
            f'<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" fill="none" viewBox="0 0 24 24">{"".join(shapes)}</svg>\n',
            encoding='utf8', newline='\n')
        print(name, len(shapes))


main()
