"""Name the darker-theme tokens whose value matches a computed CSS color.

python tools/token-of.py "oklab(0.569336 -0.0479867 -0.183657 / 0.0784314)" "rgb(248, 126, 122)" ...
"""
import math
import re
import sys
from pathlib import Path

SOURCE = Path(__file__).resolve().parent.parent / 'crates' / 'dc-tokens' / 'src' / 'generated.rs'


def darker_tokens():
    text = SOURCE.read_text(encoding='utf8')
    block = text.split('pub const DARKER')[1].split('pub const')[0]
    return [(name, int(value, 16)) for name, value in re.findall(r'\("([^"]+)", 0x([0-9a-f]{8})\)', block)]


def srgb_from_linear(value):
    value = max(0.0, min(1.0, value))
    return 12.92 * value if value <= 0.0031308 else 1.055 * value ** (1 / 2.4) - 0.055


def parse(css):
    numbers = [float(n) for n in re.findall(r'-?[\d.]+(?:e-?\d+)?', css)]
    if css.startswith('oklab'):
        l, a, b = numbers[:3]
        alpha = numbers[3] if len(numbers) > 3 else 1.0
        l_ = l + 0.3963377774 * a + 0.2158037573 * b
        m_ = l - 0.1055613458 * a - 0.0638541728 * b
        s_ = l - 0.0894841775 * a - 1.2914855480 * b
        lin = (l_ ** 3, m_ ** 3, s_ ** 3)
        r = 4.0767416621 * lin[0] - 3.3077115913 * lin[1] + 0.2309699292 * lin[2]
        g = -1.2684380046 * lin[0] + 2.6097574011 * lin[1] - 0.3413193965 * lin[2]
        bl = -0.0041960863 * lin[0] - 0.7034186147 * lin[1] + 1.7076147010 * lin[2]
        rgb = [round(255 * srgb_from_linear(c)) for c in (r, g, bl)]
    else:
        rgb = [round(n) for n in numbers[:3]]
        alpha = numbers[3] if len(numbers) > 3 else 1.0
    return rgb + [round(alpha * 255)]


def main():
    tokens = darker_tokens()
    for css in sys.argv[1:]:
        want = parse(css)
        scored = []
        for name, value in tokens:
            have = [(value >> 24) & 255, (value >> 16) & 255, (value >> 8) & 255, value & 255]
            scored.append((math.dist(want, have), name, f'{value:08x}'))
        scored.sort()
        best = [f'{name} {value}' for distance, name, value in scored[:4] if distance <= scored[0][0] + 1.5]
        print(f'{css} -> #{"".join(f"{c:02x}" for c in want)}: {", ".join(best)}')


main()
