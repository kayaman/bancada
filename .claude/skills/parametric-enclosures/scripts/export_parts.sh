#!/usr/bin/env bash
# Render each part of an enclosure to STL (or 3MF), run the interference check
# and write PNG previews. Exits non-zero on a failed render, a render WARNING
# (non-manifold output included), or interference between the parts.
# Usage: export_parts.sh enclosure.scad [part ...]
#   default parts: base lid
#   FORMAT=3mf   export 3MF instead of STL (OpenSCAD 2021.01 may lack it)
#   OUT=dir      output directory (default ./out)
#   BACKEND=manifold   pass --backend=manifold (development snapshots only)
set -euo pipefail

SCAD="${1:?usage: export_parts.sh file.scad [part ...]}"; shift || true
PARTS=("$@"); [ ${#PARTS[@]} -eq 0 ] && PARTS=(base lid)
FORMAT="${FORMAT:-stl}"; OUT="${OUT:-out}"
NAME="$(basename "${SCAD%.scad}")"

if ! command -v openscad >/dev/null 2>&1; then
  echo "openscad not found on PATH — install it or render in the GUI (F6)." >&2
  exit 127
fi
EXTRA=(); [ -n "${BACKEND:-}" ] && EXTRA+=("--backend=${BACKEND}")
scad() { openscad ${EXTRA[@]+"${EXTRA[@]}"} "$@"; }
mkdir -p "$OUT"

status=0
for p in "${PARTS[@]}"; do
  f="$OUT/${NAME}_${p}.${FORMAT}"; log="$OUT/${NAME}_${p}.log"
  echo "-- $p → $f"
  if ! scad -o "$f" -D "part=\"$p\"" "$SCAD" 2>"$log"; then
    echo "   FAILED (see $log)"; grep -E "ERROR|WARNING" "$log" | sed 's/^/   /' || true
    status=1; continue
  fi
  if grep -v '^ECHO' "$log" | grep -qE "WARNING|Simple:[[:space:]]+no"; then
    echo "   NOT CLEAN — the slicer would be guessing (see $log):"
    grep -v '^ECHO' "$log" | grep -E "WARNING|Simple:" | sed 's/^/   /'
    status=1
  fi
done

# design notes echoed by the model (sizes, BOM, NOTE/WARNING lines)
grep -h '^ECHO' "$OUT/${NAME}_${PARTS[0]}.log" 2>/dev/null | sed 's/^ECHO: "\(.*\)"$/   \1/' || true

# interference check: part="check" must be empty
log="$OUT/${NAME}_check.log"
if scad -o "$OUT/${NAME}_check.stl" -D 'part="check"' "$SCAD" 2>"$log" \
     || grep -qi "top level object is empty" "$log"; then
  if grep -qi "top level object is empty" "$log"; then
    echo "-- interference check: OK (empty)"
  else
    echo "-- interference check: parts overlap! render part=\"check\" to see where"; status=1
  fi
else
  echo "-- interference check could not run (see $log)"; status=1
fi

for view in assembly exploded section; do
  scad -o "$OUT/${NAME}_${view}.png" --imgsize=1200,900 --viewall --autocenter \
       --colorscheme=Tomorrow -D "part=\"$view\"" "$SCAD" 2>/dev/null \
    && echo "-- preview $OUT/${NAME}_${view}.png" || true
done
exit $status
