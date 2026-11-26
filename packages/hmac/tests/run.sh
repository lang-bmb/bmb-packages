#!/usr/bin/env bash
# Runs the tests of the hmac package: builds tests/driver.bmb together with
# src/hmac.bmb (the build proves every obligation first), then runs each
# case of tests/cases.txt and tests/reference.txt through the executable: the
# key, the message and the tag (hex in the data) go to its standard input, their
# lengths to its command line, and its standard output must be the expected
# value. Exits non-zero on any mismatch.
#
#     BMB=/path/to/bmb tests/run.sh
set -u

here=$(cd "$(dirname "$0")" && pwd)
pkg=$(dirname "$here")
bmb=${BMB:-bmb}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

if ! "$bmb" build "$pkg/src/hmac.bmb" "$here/driver.bmb" -o "$work/driver" > "$work/build.log" 2>&1; then
    grep -v '"status":"verified"' "$work/build.log" >&2
    echo "hmac: the test driver does not build" >&2
    exit 1
fi

# The bytes whose hex is $1, on standard output.
unhex() {
    printf '%b' "$(printf '%s' "$1" | sed 's/../\\x&/g')"
}

pass=0
fail=0
for file in cases.txt reference.txt; do
    n=0
    while IFS='|' read -r op key msg tag expected || [ -n "$op" ]; do
        n=$((n + 1))
        case "$op" in '' | '#'*) continue ;; esac
        { unhex "$key"; unhex "$msg"; unhex "$tag"; } > "$work/in"
        got=$("$work/driver" "$op" $((${#key} / 2)) $((${#msg} / 2)) < "$work/in" 2> "$work/err")
        if [ $? -eq 0 ] && [ "$got" = "$expected" ]; then
            pass=$((pass + 1))
        else
            fail=$((fail + 1))
            echo "FAIL tests/$file:$n: $op: expected $expected, got $got" >&2
            cat "$work/err" >&2
        fi
    done < "$here/$file"
done

echo "hmac: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
