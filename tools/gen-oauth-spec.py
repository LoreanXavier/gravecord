"""Generate app/src/ui/oauth_spec.rs from the pinned bundle: the OAuth2 consent modal's scope tables and strings.

- Module 647053: `k$` the account scopes in the order the consent modal lists them, `gX` the scopes it accepts, `A8`
  the one it drops, `O_` the made-up "will not do" lines, and the scope to string table `rY`.
- Module 179771: the scope enum (name to value).
- design/snapshot/dumps/STRINGS/en-US.json: the message of every hash the modal (module 610056) and the scope table read.
  A message is an ICU AST; it is written as `&[P]` parts (text, variable, bold, link).

python tools/gen-oauth-spec.py
"""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MODULES = ROOT / 'design' / 'snapshot' / 'modules'
STRINGS = ROOT / 'design' / 'snapshot' / 'dumps' / 'STRINGS' / 'en-US.json'
OUT = ROOT / 'app' / 'src' / 'ui' / 'oauth_spec.rs'

# Rust name, string hash.
MESSAGES = [
    ('CONNECT_TO', 'uT1CPa'), ('WANTS_ACCESS', 'jFbDnJ'), ('CONNECT_TO_DISCORD', 'X+Fdpo'),
    ('ALLOW_DEVELOPER', '1Hz+Sl'), ('ALLOW_TRUSTED', 'PZpY9c'), ('SEND_DMS', 'Ls2XRq'),
    ('NOT_ALLOWED', 'OX8EMU'), ('ALLOWED', '0lpCFG'), ('ERROR', 'j2d6Km'), ('CLOSE', 'cpT0Cq'),
    ('BACK', '13/7kX'), ('CANCEL', 'ETE/oC'), ('CONTINUE', '3PatSz'), ('AUTHORIZE', 'y+/PE9'),
    ('MISSED_FIELDS', 'BwwiSM'), ('KEEP_SCROLLING', 'N22i9F'), ('LOADING', 'ZTNur7'),
    ('ADD_TO_SERVER', '1DXFFd'), ('NEEDS_MANAGE_SERVER', 't9Jm9o'), ('SELECT_SERVER', 'oM4E1A'),
    ('ADD_WEBHOOK', '8qKd+J'), ('WEBHOOK_HELP', 'kQXMfN'), ('SELECT_CHANNEL', 'Re/64R'),
    ('GRANT_PERMISSIONS', 'sOaT2j'), ('CANNOT_GRANT', 'fsOkF4'),
    ('REDIRECT', '5k5OKD'), ('MANAGES', '8qui3M'), ('ACTIVE_SINCE', '+1bjc8'), ('USED_IN', 'UHGHSP'),
    ('CAN_READ_SEND', 'o0GMBD'), ('CAN_READ', 'Soy7jJ'), ('CANNOT_READ', 'TeL+Ct'),
    ('IP_LOCATION', '6wPmjo'), ('ADVERTISEMENTS', '/uOMKZ'),
    ('PRIVACY_BOTH_PARENT', 'yVfotv'), ('PRIVACY_BOTH', 'rxlyKL'), ('PRIVACY_POLICY_PARENT', 'pYVSah'),
    ('PRIVACY_POLICY', 'TBvmM2'), ('PRIVACY_TERMS_PARENT', 'nBLOp5'), ('PRIVACY_TERMS', 'q0T/Q1'),
    ('PRIVACY_NONE_PARENT', '8LemYv'), ('PRIVACY_NONE', '3Ywek3'),
    ('MEMBERS_VOICE', 'OSvmfH'), ('MEMBERS', 'o6M1aS'),
    ('SOCIAL_APP', 'GxjwZM'), ('SOCIAL_GAME', 'DfBeUq'),
    ('SOCIAL_FRIENDS_APP', 'z9peav'), ('SOCIAL_FRIENDS_GAME', 'WNKzo9'),
    ('SOCIAL_MESSAGE_APP', 'daY6xj'), ('SOCIAL_MESSAGE_GAME', 'j7peBh'),
    ('SOCIAL_ACTIVITY_APP', '/bdaNN'), ('SOCIAL_ACTIVITY_GAME', 'feD3+i'),
    ('SOCIAL_PROMO_APP', 'mSqazC'), ('SOCIAL_PROMO_GAME', 'YFFVM1'),
    ('SOCIAL_MANAGE_FRIENDS', 'Pl1dTW'), ('SOCIAL_FRIENDS_STATUS', 'mPRcyT'), ('SOCIAL_UPDATE_ACTIVITY', 'F7J4NE'),
    ('SOCIAL_GAME_INVITES', 'syJLx9'), ('SOCIAL_MEMBERSHIP', 'hc/+yg'), ('SOCIAL_MESSAGES', '2wxXX9'),
    ('IDENTITIES_WRITE', '1zioRF'),
    ('ERROR_GENERIC', 'mqn873'),
]


def parts(node):
    """An ICU AST as Rust `P` values."""
    out = []
    for item in node:
        if isinstance(item, str):
            out.append(f'P::Text({json.dumps(item, ensure_ascii=False)})')
        elif item[0] == 1:
            out.append(f'P::Var("{item[1]}")')
        elif item[0] == 2:
            out.append(f'P::Number("{item[1]}")')
        elif item[0] == 3:
            out.append(f'P::Date("{item[1]}")')
        elif item[0] == 8 and item[1] == '$b':
            out.append(f'P::Bold(&[{", ".join(parts(item[2]))}])')
        elif item[0] == 8 and item[1] == '$link':
            out.append(f'P::Link(&[{", ".join(parts(item[2]))}], "{item[3][0][1]}")')
        else:
            raise SystemExit(f'unhandled ICU node {item}')
    return out


def enum_values():
    source = (MODULES / '179771.js').read_text(encoding='utf8')
    # The enum is `(i={}).IDENTIFY="identify",i.IDENTIFY_PREMIUM=...`, ending at the `let a=` that follows it.
    return dict(re.findall(r'\.([A-Z0-9_]+)="([^"]+)"', source.split('let a=')[0]))


def message_key(match_a, match_b):
    return match_a or match_b


def main():
    strings = json.loads(STRINGS.read_text(encoding='utf8'))
    scopes = enum_values()
    source = (MODULES / '647053.js').read_text(encoding='utf8')
    ordered = re.search(r's=\[((?:i\.F\.[A-Z_0-9]+,?)+)\]', source).group(1)
    order = [scopes[name] for name in re.findall(r'i\.F\.([A-Z_0-9]+)', ordered)]
    extra_source = re.search(r'l=s\.concat\(\[([^\]]+)\]', source).group(1)
    extra = [scopes[name] for name in re.findall(r'i\.F\.([A-Z_0-9]+)', extra_source)]
    hidden = re.search(r'A8:\[([^\]]+)\]', source).group(1).replace('"', '').split(',')
    fake_source = re.search(r'a=\[(.*?)\],s=\[', source, re.S).group(1)
    fake = [strings[a or b][0] for a, b in re.findall(r'r\.t(?:\.([A-Za-z0-9]+)|\["([^"]+)"\])', fake_source)]
    simple = []
    pattern = r'case i\.F\.([A-Z_0-9]+):return\[r\.intl\.string\(r\.t(?:\.([A-Za-z0-9]+)|\["([^"]+)"\])\)\]'
    for name, key_a, key_b in re.findall(pattern, source):
        simple.append((scopes[name], strings[key_a or key_b][0]))
    lines = [
        '//! The OAuth2 consent modal\'s scope tables and strings, generated by `tools/gen-oauth-spec.py` from modules 647053,',
        '//! 179771 and 610056 and the en-US strings. Do not edit by hand.',
        '//!',
        '//! The picker, social-layer and redirect messages are generated with the rest and used as those parts are built.',
        '#![allow(dead_code)]',
        '',
        '/// A message part: an ICU message as the bundle stores it.',
        '#[derive(Debug, Clone, Copy, PartialEq)]',
        'pub(super) enum P {',
        '    Text(&\'static str),',
        '    Var(&\'static str),',
        '    Number(&\'static str),',
        '    Date(&\'static str),',
        '    Bold(&\'static [P]),',
        '    /// The words, and the name of the variable that holds the URL.',
        '    Link(&\'static [P], &\'static str),',
        '}',
        '',
        '/// `k$`: the scopes the modal lists, in its order.',
        'pub(super) const ORDER: &[&str] = &[',
    ]
    lines += [f'    "{scope}",' for scope in order]
    lines += ['];', '', '/// `gX`: `k$` and the two integration scopes; anything else is "Invalid scope".', 'pub(super) const ACCEPTED_EXTRA: &[&str] = &[']
    lines += [f'    "{scope}",' for scope in extra]
    lines += ['];', '', '/// `A8`: dropped before anything else.', 'pub(super) const HIDDEN: &[&str] = &[']
    lines += [f'    "{scope}",' for scope in hidden]
    lines += ['];', '', '/// `O_`: the made-up line that ends the list, one picked at random.', 'pub(super) const FAKE: &[&str] = &[']
    lines += [f'    {json.dumps(text, ensure_ascii=False)},' for text in fake]
    reading = re.search(r'SCOPES_CAN_READ_MESSAGES:new Set\(\[([^\]]+)\]', (MODULES / '179771.js').read_text(encoding='utf8')).group(1)
    lines += ['];', '', '/// Module 179771 `SCOPES_CAN_READ_MESSAGES`.', 'pub(super) const CAN_READ_MESSAGES: &[&str] = &[']
    lines += [f'    {name},' for name in reading.split(',')]
    lines += ['];', '', '/// `rY`: scopes that read as one fixed line.', 'pub(super) const SIMPLE: &[(&str, &str)] = &[']
    lines += [f'    ("{scope}", {json.dumps(text, ensure_ascii=False)}),' for scope, text in simple]
    lines += ['];', '']
    for name, key in MESSAGES:
        message = strings.get(key)
        if message is None:
            raise SystemExit(f'no string {key} for {name}')
        lines += [f'/// Message `{key}`.', f'pub(super) const {name}: &[P] = &[{", ".join(parts(message))}];', '']
    OUT.write_text('\n'.join(lines), encoding='utf8', newline='\n')
    print(f'{len(order)} scopes, {len(simple)} fixed lines, {len(fake)} fake lines, {len(MESSAGES)} messages -> {OUT}')


if __name__ == '__main__':
    main()
