#!/bin/bash
# Crawls a fixed list of screens in inert mode and prints, per screen and mode, how many pointer elements there are and
# how each behaved. Not part of the default test run.
#
#   tools/crawl-scoreboard.sh [spec.tsv] [out dir] [modes] [only]
#
# spec.tsv: name<TAB>setup steps (| between steps)<TAB>steps that return to the screen (| between steps)
# modes:    click (default), right, hover, or several joined with commas
# only:     a regular expression; only screens whose name matches are crawled
# Needs a built `target/debug/gravecord.exe` (capture feature). A private copy of it is run on the hidden desktop, so the
# build is never locked, and it is stopped at the end. CRAWL_EXE names the copy (default gravecord-run-follow-crawl.exe).
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SPEC=${1:-$ROOT/tools/crawl-screens.tsv}
OUT=${2:-$ROOT/design/crawl}
MODES=${3:-click}
ONLY=${4:-.}
NAME=${CRAWL_EXE:-gravecord-run-follow-crawl.exe}
mkdir -p "$OUT"
OUTW=$(cd "$OUT" && pwd -W 2>/dev/null || pwd)
FOLLOW="$OUT/follow.txt"
: > "$FOLLOW"
cp "$ROOT/target/debug/gravecord.exe" "$ROOT/target/debug/$NAME" || exit 1
export GRAVECORD_INERT=1 GRAVECORD_SETTLE_MS=100 GRAVECORD_NO_SETTINGS_WRITE=1
cd "$ROOT" && target/debug/xtask.exe headless "target/debug/$NAME" --capture "$OUTW/final.png" 100 --follow "$OUTW/follow.txt" > /dev/null || exit 1
stop() { echo quit >> "$FOLLOW"; sleep 3; taskkill //F //IM "$NAME" > /dev/null 2>&1; }
trap stop EXIT
sleep 6
while IFS=$'\t' read -r name setup nav; do
  [ -z "$name" ] && continue
  [[ "$name" == \#* ]] && continue
  [[ "$name" =~ $ONLY ]] || continue
  for mode in ${MODES//,/ }; do
    dir="$OUT/${name}_$mode"
    rm -rf "$dir"; mkdir -p "$dir"
    dirw=$(cd "$dir" && pwd -W 2>/dev/null || pwd)
    echo "${setup//|/;}" >> "$FOLLOW"
    echo "wait 1200" >> "$FOLLOW"
    echo "crawl $mode $dirw --nav ${nav}" >> "$FOLLOW"
    last=-1; idle=0
    while true; do
      sleep 2
      grep -q "crawl summary\|crawl: refused" "$dir/progress.txt" 2>/dev/null && break
      size=$(stat -c %s "$dir/progress.txt" 2>/dev/null || echo 0)
      if [ "$size" = "$last" ]; then idle=$((idle+2)); else idle=0; last=$size; fi
      if [ $idle -ge 150 ]; then echo "crawl died: $name $mode (no progress for 150 s)" >&2; echo "crawl died" >> "$dir/progress.txt"; break; fi
    done
    [ -f "$dir/progress.txt" ] && grep -q "crawl died" "$dir/progress.txt" && { stop; trap - EXIT; echo "the app stopped; rerun from $name" >&2; break 2; }
  done
done < "$SPEC"
python -I "$ROOT/tools/crawl_score.py" "$OUT" "$ROOT/tools/crawl-known.tsv" "$ONLY" "$MODES"
