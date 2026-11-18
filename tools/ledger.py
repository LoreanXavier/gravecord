"""The parity ledger: every UI module of the pinned client, the en-US text it shows, and where gravecord ports it.

    python tools/ledger.py            writes LEDGER.tsv and prints the summary
    python tools/ledger.py --missing  also prints the unported modules with the most text, biggest first

A UI module is a module of design/snapshot/modules that renders elements (`.jsx`) and shows at least one message
(`t["xxxxxx"]` / `t.xxxxxx`): a dialog, a card, a menu, a settings section. It counts as ported when a source file of
gravecord names its id (the code cites the module it ports: "module 560296", "(965086 `ep`)"). A module that is ported
without citing its id shows as unported until the citation is added, so the ledger also keeps the code honest about
its sources.

The comparisons so far looked at screens a test account could reach; a whole dialog nobody opened was never compared.
This lists everything the official client can draw, so nothing is missed by not being looked at. Columns:
module, chunk, bytes, messages (count), status (ported / unported), ported_in (files), sample (first texts).
"""
import collections
import csv
import json
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MODULES = os.path.join(REPO, "design", "snapshot", "modules")
INDEX = os.path.join(REPO, "design", "snapshot", "MODULE_INDEX.tsv")
STRINGS = os.path.join(REPO, "design", "snapshot", "dumps", "STRINGS", "en-US.json")
OUT = os.path.join(REPO, "LEDGER.tsv")
KEY = re.compile(r'\.t\["([A-Za-z0-9+/=]{6})"\]|\.t\.([A-Za-z0-9]{6})\b')
# Lines that name a module as missing do not cite it (xtask/src/coverage.rs GAP_MARKERS).
GAP_MARKERS = ("GAP:", "does not show", "not ported", "not in the app", "isn't in the app")
CITE = re.compile(r"(?<![0-9A-Za-z])([0-9]{4,6})(?![0-9A-Za-z])")


def messages():
    with open(STRINGS, encoding="utf8") as handle:
        data = json.load(handle)
    if isinstance(data, dict) and "messages" in data:
        data = data["messages"]
    return {key: flatten(value) for key, value in data.items()}


def flatten(value):
    """A message's text with its placeholders as `{name}`: the AST of the strings dump flattened."""
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        if len(value) >= 2 and isinstance(value[0], int) and isinstance(value[1], str) and value[0] in (1, 2, 6):
            return "{" + value[1] + "}"
        if len(value) >= 3 and value[0] == 8 and isinstance(value[2], list):
            return flatten(value[2])
        return "".join(flatten(item) for item in value if not isinstance(item, int))
    if isinstance(value, dict):
        return flatten(value.get("other", ""))
    return ""


def chunks():
    found = {}
    if os.path.exists(INDEX):
        with open(INDEX, encoding="utf8") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                found[row["id"]] = (row["chunk"], row["bytes"])
    return found


def citations():
    cited = collections.defaultdict(set)
    for base in ("app", "crates", "xtask"):
        for folder, _, files in os.walk(os.path.join(REPO, base)):
            if "target" in folder.split(os.sep):
                continue
            for name in files:
                if not name.endswith(".rs"):
                    continue
                path = os.path.join(folder, name)
                with open(path, encoding="utf8", errors="ignore") as handle:
                    text = chr(10).join(
                        line for line in handle.read().splitlines() if not any(marker in line for marker in GAP_MARKERS)
                    )
                relative = os.path.relpath(path, REPO).replace(os.sep, "/")
                for module in set(CITE.findall(text)):
                    cited[module].add(relative)
    return cited


def main():
    texts = messages()
    index = chunks()
    cited = citations()
    triage = {}
    path = os.path.join(REPO, "coverage", "ledger-triage.tsv")
    if os.path.exists(path):
        with open(path, encoding="utf8") as handle:
            for row in csv.DictReader(handle, delimiter="	"):
                triage[row["module"]] = (row["class"], row["where"])
    rows = []
    for name in sorted(os.listdir(MODULES)):
        if not name.endswith(".js"):
            continue
        module = name[:-3]
        with open(os.path.join(MODULES, name), encoding="utf8", errors="ignore") as handle:
            source = handle.read()
        if ".jsx" not in source:
            continue
        keys = []
        for first, second in KEY.findall(source):
            key = first or second
            if key not in keys:
                keys.append(key)
        if not keys:
            continue
        files = sorted(cited.get(module, ()))
        chunk, size = index.get(module, ("", str(len(source))))
        status = "ported" if files else triage.get(module, ("untriaged", ""))[0]
        # A partial or missing triage row wins over a citation that covers only a piece of the module.
        if triage.get(module, ("", ""))[0] in ("partial", "missing"):
            status = triage[module][0]
        if not files and status == "ported":
            files = [triage[module][1]]
        sample = " | ".join(texts.get(key, key).replace("\t", " ").replace("\n", " ")[:60] for key in keys[:4])
        rows.append((module, chunk, size, len(keys), status, ",".join(files[:4]), sample))
    order = {"untriaged": 0, "missing": 1, "partial": 2, "ported": 3, "not-applicable": 4}
    rows.sort(key=lambda row: (order.get(row[4], 0), -row[3]))
    with open(OUT, "w", encoding="utf8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
        writer.writerow(("module", "chunk", "bytes", "messages", "status", "ported_in", "sample"))
        writer.writerows(rows)
    counts = collections.Counter(row[4] for row in rows)
    print("by status: " + ", ".join(f"{name} {count}" for name, count in sorted(counts.items())))
    ported = sum(1 for row in rows if row[4] == "ported")
    shown = sum(row[3] for row in rows)
    shown_ported = sum(row[3] for row in rows if row[4] == "ported")
    print(f"UI modules: {len(rows)}, ported (cited or triaged ported): {ported} ({100 * ported / max(1, len(rows)):.1f}%)")
    print(f"messages they show: {shown}, in ported modules: {shown_ported} ({100 * shown_ported / max(1, shown):.1f}%)")
    print(f"written: {os.path.relpath(OUT, REPO)}")
    if "--missing" in sys.argv:
        for row in rows:
            if row[4] in ("missing", "partial", "untriaged"):
                print(f"{row[0]}\t{row[3]}\t{row[6]}")


if __name__ == "__main__":
    main()
