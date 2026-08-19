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
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
mkdir -p "$OUT"

: "${SWEEP_PER_FILE:=8}" "${SWEEP_MAX_FILES:=700}" "${SWEEP_SEED:=v1}"
: "${SWEEP_TIMEOUT:=15}" "${SWEEP_READY_TIMEOUT:=300}"

python3 "$HERE/selftest.py"

python3 "$HERE/sweep.py" positions --root "$ROOT" --out "$OUT/positions.jsonl" \
  --per-file "$SWEEP_PER_FILE" --max-files "$SWEEP_MAX_FILES" --seed "$SWEEP_SEED"

run_side() {  # side, binary
  python3 "$HERE/sweep.py" run --bin "$2" --root "$ROOT" \
    --positions "$OUT/positions.jsonl" --out "$OUT/answers-$1.jsonl" --side "$1" \
    --cache-dir "$OUT/cache-$1" --timeout "$SWEEP_TIMEOUT" \
    --ready-timeout "$SWEEP_READY_TIMEOUT" "$@:3" 2>&1 | sed "s/^/[$1] /"
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
