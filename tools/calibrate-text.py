"""Sweeps gpui's text gamma and contrast against the channel golden.

Each setting renders gravecord offscreen on the hidden desktop. Scores cover text regions only: the mean absolute luminance
error, and the share of pixels past the parity tolerance. Run from the repository root after a capture build.
"""

import itertools
import os
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parent.parent
GOLDEN = ROOT / "design/golden/windows/channel/text-darker-cozy@1.png"
FRAME = ROOT / "design/parity/calibration.png"
EXE = ROOT / "target/debug/gravecord.exe"
TEXT_REGION = (447, 90, 1100, 640)
TOLERANCE = 24


def render(gamma, contrast):
    FRAME.unlink(missing_ok=True)
    env = dict(os.environ, GRAVECORD_TEXT_GAMMA=str(gamma), GRAVECORD_TEXT_CONTRAST=str(contrast))
    subprocess.run(
        ["cargo", "xtask", "headless", str(EXE.relative_to(ROOT)), "--capture", str(FRAME), "800"],
        cwd=ROOT,
        env=env,
        check=True,
        capture_output=True,
    )
    for _ in range(100):
        if FRAME.exists():
            time.sleep(0.3)
            return
        time.sleep(0.2)
    raise SystemExit("no frame")


def score():
    golden = Image.open(GOLDEN).convert("L").crop(TEXT_REGION)
    ours = Image.open(FRAME).convert("L").crop(TEXT_REGION)
    histogram = ImageChops.difference(golden, ours).histogram()
    total = sum(histogram)
    mean = sum(value * count for value, count in enumerate(histogram)) / total
    past = sum(histogram[TOLERANCE + 1 :]) / total
    return mean, 100 * past


def main():
    gammas = [float(value) for value in sys.argv[1].split(",")]
    contrasts = [float(value) for value in sys.argv[2].split(",")]
    results = []
    for gamma, contrast in itertools.product(gammas, contrasts):
        render(gamma, contrast)
        mean, past = score()
        results.append((mean, past, gamma, contrast))
        print(f"gamma {gamma:<4} contrast {contrast:<4} mean {mean:.3f} past {past:.3f}%", flush=True)
    best = min(results)
    print(f"best: gamma {best[2]} contrast {best[3]} mean {best[0]:.3f} past {best[1]:.3f}%")


if __name__ == "__main__":
    main()
