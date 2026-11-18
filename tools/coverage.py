"""Data coverage: every field real answers carry is read by gravecord or has a written reason.

    python tools/coverage.py record <folder of answer .json files> <area>
        Inventories every key path of the answers (arrays as `[]`, map keys that are ids as `{id}`), with how many
        answers carry it, into coverage/<area>.tsv. Rows already there keep their status and reason; a path the code
        reads (its last key appears as a string literal or a field in app/ or crates/) is marked `read`, a new one
        that it does not read is `unknown`. Only paths and counts are written: no values, ids or names.
    python tools/coverage.py check
        Lists the `unknown` rows, and the `read` rows whose key no longer appears in the code. `cargo xtask audit`
        runs the same check (xtask/src/coverage.rs) and fails on either.

Statuses: `read` (drawn or used), `ignored` (with a reason: not drawn by the official client either, analytics only,
...), `unknown` (to decide). Record answers read-only (GETs, the official capture client with tools/read-only.mjs
running) and keep the raw answers outside the repository: they are personal data.
"""
import csv
import json
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FOLDER = os.path.join(REPO, "coverage")
ID = re.compile(r"^[0-9]{6,}$")


def paths(value, prefix, out):
    if isinstance(value, dict):
        for key, item in value.items():
            name = "{id}" if ID.match(key) else key
            path = f"{prefix}.{name}" if prefix else name
            out.add(path)
            paths(item, path, out)
    elif isinstance(value, list):
        for item in value:
            paths(item, f"{prefix}[]", out)


def leaf(path):
    last = path.replace("[]", "").split(".")[-1]
    return None if last == "{id}" else last


def code_keys():
    found = set()
    pattern = re.compile(r'"([A-Za-z_][A-Za-z0-9_]*)"|\bpub ([a-z_][a-z0-9_]*):')
    for base in ("app", "crates"):
        for folder, _, files in os.walk(os.path.join(REPO, base)):
            if "target" in folder.split(os.sep):
                continue
            for name in files:
                if name.endswith(".rs"):
                    with open(os.path.join(folder, name), encoding="utf8", errors="ignore") as handle:
                        for first, second in pattern.findall(handle.read()):
                            found.add(first or second)
    return found


def load(area):
    path = os.path.join(FOLDER, f"{area}.tsv")
    rows = {}
    if os.path.exists(path):
        with open(path, encoding="utf8") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                rows[row["path"]] = row
    return rows


def record(folder, area):
    counts = {}
    total = 0
    for name in sorted(os.listdir(folder)):
        if not name.endswith(".json"):
            continue
        with open(os.path.join(folder, name), encoding="utf8") as handle:
            answer = json.load(handle)
        total += 1
        found = set()
        paths(answer, "", found)
        for path in found:
            counts[path] = counts.get(path, 0) + 1
    known = load(area)
    keys = code_keys()
    os.makedirs(FOLDER, exist_ok=True)
    out = os.path.join(FOLDER, f"{area}.tsv")
    with open(out, "w", encoding="utf8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
        writer.writerow(("path", "answers", "status", "reason"))
        for path in sorted(set(counts) | set(known)):
            row = known.get(path)
            key = leaf(path)
            if row and row["status"] in ("ignored", "read"):
                status, reason = row["status"], row["reason"]
            elif key is None or key in keys:
                status, reason = "read", ""
            else:
                status, reason = "unknown", ""
            writer.writerow((path, f"{counts.get(path, 0)}/{total}", status, reason))
    print(f"{out}: {len(counts)} paths from {total} answers")


def check():
    keys = code_keys()
    failures = []
    for name in sorted(os.listdir(FOLDER)):
        if not name.endswith(".tsv"):
            continue
        for path, row in load(name[:-4]).items():
            key = leaf(path)
            if row["status"] == "unknown":
                failures.append(f"{name}: {path} is not read and has no reason")
            elif row["status"] == "read" and key is not None and key not in keys:
                failures.append(f"{name}: {path} was read, but `{key}` no longer appears in the code")
            elif row["status"] == "ignored" and not row["reason"].strip():
                failures.append(f"{name}: {path} is ignored without a reason")
    for line in failures:
        print(line)
    print(f"{len(failures)} coverage failures")
    return 1 if failures else 0


if __name__ == "__main__":
    if sys.argv[1:2] == ["record"] and len(sys.argv) == 4:
        record(sys.argv[2], sys.argv[3])
    elif sys.argv[1:2] == ["check"]:
        sys.exit(check())
    else:
        print(__doc__)
