#!/usr/bin/env bash
# One differential sweep, end to end.
#
#   bench/sweep/run.sh <base-binary> <head-binary> <corpus-root> <out-dir> [extra args...]
#
# The two sides run CONCURRENTLY. They contend for CPU, so nothing here is a
# timing measurement — this compares ANSWERS, and `bench/lsp_bench.py` is
# what measures latency. Run them serially (SWEEP_SERIAL=1) if the box is
# small enough that contention could cause timeouts rather than just slowness.
set -euo pipefail

BASE_BIN=${1:?base binary}; HEAD_BIN=${2:?head binary}
ROOT=${3:?corpus root};     OUT=${4:?output dir}; shift 4
# Captured HERE, at top level. Inside a function `$@` is that function's own
# arguments, so reaching for the caller's extras from within `run_side` gets
# the wrong list — and `"$@:3"` is not slice syntax at all (that is `${@:3}`),
# so it expanded to the function's two args plus a literal `:3` and both
# sides died on `unrecognized arguments`. The wrapper was completely
# non-functional and the Python selftest could not see it, which is why
# `selftest-shell.sh` now invokes this script for real.
EXTRA=("$@")
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
mkdir -p "$OUT"

: "${SWEEP_PER_FILE:=8}" "${SWEEP_MAX_FILES:=700}" "${SWEEP_SEED:=v1}"
: "${SWEEP_TIMEOUT:=15}" "${SWEEP_READY_TIMEOUT:=300}"

python3 "$HERE/selftest.py"

python3 "$HERE/sweep.py" positions --root "$ROOT" --out "$OUT/positions.jsonl" \
  --per-file "$SWEEP_PER_FILE" --max-files "$SWEEP_MAX_FILES" --seed "$SWEEP_SEED"

# Extra args come from the caller AFTER the four positionals; `shift 4` above
# left them in "$@", so capture them before any function shadows it. `"$@:3"`
# is not slicing syntax — it expands to the function's own args plus a literal
# ":3", which made every invocation fail.
EXTRA=("$@")

run_side() {  # side, binary
  local side=$1 bin=$2
  # `${EXTRA+...}` rather than a bare `"${EXTRA[@]}"`: under `set -u` an empty
  # array is an unbound-variable error on bash before 4.4, and macOS still
  # ships 3.2 as /bin/bash. Untested here (this box is 5.2), free to keep.
  python3 "$HERE/sweep.py" run --bin "$bin" --root "$ROOT" \
    --positions "$OUT/positions.jsonl" --out "$OUT/answers-$side.jsonl" --side "$side" \
    --cache-dir "$OUT/cache-$side" --timeout "$SWEEP_TIMEOUT" \
    --ready-timeout "$SWEEP_READY_TIMEOUT" ${EXTRA+"${EXTRA[@]}"} 2>&1 | sed "s/^/[$side] /"
}

if [ "${SWEEP_SERIAL:-0}" = 1 ]; then
  run_side base "$BASE_BIN"
  run_side head "$HEAD_BIN"
else
  run_side base "$BASE_BIN" & b=$!
  run_side head "$HEAD_BIN" & h=$!
  wait $b; wait $h
fi

python3 "$HERE/sweep.py" diff --base "$OUT/answers-base.jsonl" \
  --head "$OUT/answers-head.jsonl" --out "$OUT/report.md"
echo "report: $OUT/report.md"
