#!/usr/bin/env bash
# Checks every package under packages/ (or the packages named as arguments):
#   1. no escape hatch in its sources (`assume`, `#[external_body]`);
#   2. every source file, and every test program with them, is in normal form;
#   3. `bmb verify` proves every definition, with no trusted fact and no
#      unproved termination;
#   4. its tests pass (tests/run.sh).
# The compiler is $BMB (default: `bmb` on PATH). Exits non-zero if any check
# fails for any package.
set -u

root=$(cd "$(dirname "$0")/.." && pwd)
export BMB=${BMB:-bmb}
if ! command -v "$BMB" > /dev/null 2>&1; then
    echo "check: compiler not found: $BMB (set BMB)" >&2
    exit 2
fi

# Files in the order of their unit names: `bmb fmt --check` with several files
# compares them in that order.
by_unit() {
    for f in "$@"; do
        printf '%s\t%s\n' "$(sed -n 's/^mod \([a-z_0-9]*\) {$/\1/p' "$f" | head -1)" "$f"
    done | sort | cut -f2
}

if [ $# -gt 0 ]; then
    packages=("$@")
else
    packages=()
    for d in "$root"/packages/*/; do
        packages+=("$(basename "$d")")
    done
fi

failed=0
for name in "${packages[@]}"; do
    dir="$root/packages/$name"
    ok=1
    mapfile -t srcs < <(by_unit "$dir"/src/*.bmb)
    if [ ! -f "$dir/package.json" ] || [ ${#srcs[@]} -eq 0 ]; then
        echo "$name: no package.json or no src/*.bmb" >&2
        failed=1
        continue
    fi

    if grep -n -E '\bassume\s*\(|external_body' "${srcs[@]}" >&2; then
        echo "$name: escape hatch in the sources" >&2
        ok=0
    fi

    if ! "$BMB" fmt --check "${srcs[@]}"; then
        echo "$name: sources not in normal form (bmb fmt)" >&2
        ok=0
    fi
    for t in "$dir"/tests/*.bmb; do
        [ -e "$t" ] || continue
        mapfile -t all < <(by_unit "${srcs[@]}" "$t")
        if ! "$BMB" fmt --check "${all[@]}"; then
            echo "$name: $(basename "$t") not in normal form (bmb fmt)" >&2
            ok=0
        fi
    done

    log=$(mktemp)
    "$BMB" verify "${srcs[@]}" > "$log"
    code=$?
    if ! python3 - "$log" "$code" "$name" <<'PY'
import json, sys
log, code, name = sys.argv[1], int(sys.argv[2]), sys.argv[3]
rows = [json.loads(l) for l in open(log) if l.strip()]
defs = [r for r in rows if "def" in r and "status" in r]
bad = [r for r in defs if r["status"] != "verified" or "trust" in r or "termination" in r]
for r in rows:
    if "code" in r:
        print(json.dumps(r), file=sys.stderr)
for r in bad:
    print(json.dumps(r), file=sys.stderr)
mine = [r for r in defs if not r["def"].startswith("std::")]
print(f"{name}: verify: {len(mine)} definitions proved ({len(defs)} with the std units used), "
      f"{sum(r['obligations'] for r in mine)} obligations")
sys.exit(0 if code == 0 and defs and not bad else 1)
PY
    then
        echo "$name: not proved without escape hatches" >&2
        ok=0
    fi
    rm -f "$log"

    if [ -x "$dir/tests/run.sh" ]; then
        if ! "$dir/tests/run.sh"; then
            echo "$name: tests failed" >&2
            ok=0
        fi
    else
        echo "$name: no tests/run.sh" >&2
        ok=0
    fi

    if [ $ok -eq 1 ]; then
        echo "$name: ok"
    else
        echo "$name: FAILED"
        failed=1
    fi
done
exit $failed
