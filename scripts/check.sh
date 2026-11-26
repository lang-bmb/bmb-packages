#!/usr/bin/env bash
# Checks every package under packages/ (or the packages named as arguments)
# with gotgan, the package manager:
#   gotgan check   normal form and checks of the units and the test units;
#   gotgan verify  a full proof, with no escape hatch, no trusted definition,
#                  no unproved termination, and deps.std as the program reaches;
#   gotgan test    the test suites of package.json, run through their drivers.
# gotgan is $GOTGAN (default: `gotgan` on PATH) and the compiler $BMB (default:
# `bmb` on PATH; gotgan reads it too). Exits non-zero if any command fails for
# any package. The commands run one after another, never two proofs at once.
set -u

root=$(cd "$(dirname "$0")/.." && pwd)
export BMB=${BMB:-bmb}
gotgan=${GOTGAN:-gotgan}
if ! command -v "$BMB" > /dev/null 2>&1; then
    echo "check: compiler not found: $BMB (set BMB)" >&2
    exit 2
fi
if ! command -v "$gotgan" > /dev/null 2>&1; then
    echo "check: gotgan not found: $gotgan (set GOTGAN). To build it, in the gotgan" >&2
    echo "repository: \$BMB build src/*.bmb -o gotgan (see its README.md)" >&2
    exit 2
fi

if [ $# -gt 0 ]; then
    packages=("$@")
else
    packages=()
    for d in "$root"/packages/*/; do
        packages+=("$(basename "$d")")
    done
fi

summary_py=$(cat <<'PY'
import json, sys
name, command = sys.argv[1], sys.argv[2]
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        row = json.loads(line)
    except ValueError:
        print(line, file=sys.stderr)
        continue
    if "code" in row:
        print(line, file=sys.stderr)
    elif row.get("command") != command:
        continue
    elif "file" in row:
        print(f"{name}: FAIL {row['file']}:{row['line']}: {row['args']}: expected {row['expected']}, "
              f"got {row['got']} (exit {row['status']}) {row['stderr']}", file=sys.stderr)
    elif command == "check":
        print(f"{name}: check: {row['units']} unit(s), {row['test_units']} test unit(s), "
              f"fmt {json.dumps(row['fmt'])}, check {json.dumps(row['check'])}")
    elif command == "verify":
        std = f"and {row['std_defs']} of std::{', std::'.join(row['std'])}" if row['std'] else "no std unit"
        print(f"{name}: verify: {row['defs']} definitions proved ({std}), "
              f"{row['obligations']} obligations, rlimit {row['rlimit']} "
              f"(largest query {row['rlimit_max']}); unproved {row['unproved']}, escapes {row['escapes']}, "
              f"trusted {row['trusted']}, termination unproved {row['termination_unproved']}")
    elif command == "test":
        print(f"{name}: test {row.get('suite')}: built {json.dumps(row.get('built'))}, "
              f"{row.get('passed')} passed, {row.get('failed')} failed")
PY
)

# The summary of gotgan's JSON rows on standard input: one line per command,
# and the failing cases and the diagnostics in full on standard error.
summary() {
    python3 -c "$summary_py" "$1" "$2"
}

failed=0
for name in "${packages[@]}"; do
    dir="$root/packages/$name"
    ok=1
    for command in check verify test; do
        out=$("$gotgan" --package "$dir" "$command")
        code=$?
        printf '%s\n' "$out" | summary "$name" "$command"
        if [ $code -ne 0 ]; then
            echo "$name: gotgan $command failed (exit $code)" >&2
            ok=0
        fi
    done
    if [ $ok -eq 1 ]; then
        echo "$name: ok"
    else
        echo "$name: FAILED"
        failed=1
    fi
done
exit $failed
