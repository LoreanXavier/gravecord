"""Summarises crawl output folders: `crawl_score.py <out dir> <known.tsv> [only regex] [modes]`.

A result line looks like `#12 dead bounds=(x,y wxh) id=... text="..." notes`. Per screen and mode it counts the classes and
`dead-real`: dead lines minus the ones `known.tsv` lists (`screen<TAB>regex on the line<TAB>reason`: reviewed, the official
does nothing there too, or the target feature is listed as missing).
"""
import os, re, sys

out = sys.argv[1]
known_path = sys.argv[2] if len(sys.argv) > 2 else ""
only = re.compile(sys.argv[3] if len(sys.argv) > 3 else ".")
modes = (sys.argv[4] if len(sys.argv) > 4 else "click,right,hover").split(",")
known = []
if known_path and os.path.exists(known_path):
    for line in open(known_path, encoding="utf8"):
        parts = line.rstrip("\n").split("\t")
        if len(parts) >= 2 and not parts[0].startswith("#"):
            known.append((parts[0], re.compile(parts[1])))
bad = {"click": ("dead",), "right": (), "hover": ("no-tooltip",)}
rows = []
for name in sorted(os.listdir(out)):
    path = os.path.join(out, name, "progress.txt")
    if not os.path.isfile(path):
        continue
    screen, _, mode = name.rpartition("_")
    if mode not in modes or not only.search(screen):
        continue
    counts, real, text = {}, 0, open(path, encoding="utf8", errors="replace").read()
    total = 0
    for line in text.splitlines():
        m = re.match(r"#(\d+) (\S+) ", line)
        if not m:
            continue
        cls = m.group(2)
        counts[cls] = counts.get(cls, 0) + 1
        total += 1
        if cls in bad[mode] and not any(s == screen and r.search(line) for s, r in known):
            real += 1
    died = "crawl summary" not in text
    flag = "SETUP FAILED" if "setup failed" in text else ("LOST SCREEN" if "lost the screen" in text else ("DIED" if died else ""))
    rows.append((screen, mode, total, counts, real, flag))
head = f"{'screen':24} {'mode':6} {'elements':>8} {'changed':>7} {'acted':>5} {'read':>4} {'dead':>4} {'dead-real':>9} {'selected':>8} {'decor':>5}"
print(head)
for screen, mode, total, c, real, flag in rows:
    dead = c.get("dead", 0) if mode != "hover" else c.get("no-tooltip", 0)
    changed = c.get("changed", 0) + c.get("tooltip", 0)
    print(f"{screen:24} {mode:6} {total:8} {changed:7} {c.get('acted', 0):5} {c.get('read', 0):4} {dead:4} {real:9} {c.get('selected', 0):8} {c.get('decor', 0):5}" + (("  " + flag) if flag else ""))
