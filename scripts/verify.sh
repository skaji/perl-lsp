#!/usr/bin/env bash
# Run the verification bar and keep every byte of output.
#
# WHY THIS EXISTS. Each gate below already reports correctly. What kept going
# wrong was the READING of them, always in one of three ways:
#
#   A. Truncated capture. `... | tail -6` catches the timing block and cuts the
#      verdict; `grep -E 'PASS|FAIL' | tail -4` catches the banner and not the
#      counts. Both look like a result. Fix: never pipe a gate through head/tail
#      — the full log is written to disk and the summary is extracted from the
#      file afterwards, so a bad extraction costs a re-read, not a re-run.
#
#   B. Grep as the verdict. A suite's EXIT CODE is the verdict; grep is for the
#      human summary only. Nothing here decides pass/fail from matched text.
#
#   C. An unasserted PREMISE — the one that actually bit hardest. A gate can run
#      correctly, exit 0, and have tested half of what you think:
#        - gold EXCLUDES lang-skip from its exit code ON PURPOSE, because a
#          perl-only build legitimately cannot serve cpp rows. So a build that
#          forgot `--features cpp` skips 255 rows, reports them as skips, and
#          EXITS 0. That is not gold's bug; gold cannot know what you meant to
#          build. Only the caller knows, so the caller asserts it.
#        - e2e/run-cpp.sh existed for months wired to no workflow. "e2e-cpp 0"
#          was zero RUNS, not zero failures. A gate that did not run must never
#          read as a gate that passed, so a missing prerequisite is a HARD ERROR
#          here, never a skip.
#
# Usage:
#   scripts/verify.sh                 # every gate
#   scripts/verify.sh unit gold       # only these
#   scripts/verify.sh --list          # gate names
#   KEEP=20 scripts/verify.sh         # keep N run dirs (default 10)
#
# Gates: unit unit-cpp gold e2e e2e-cpp
# Logs:  /tmp/perl-lsp-verify/<stamp>/<gate>.log, with `latest` symlinked.
set -uo pipefail   # deliberately NOT -e: a failing gate must not abort the rest

cd "$(dirname "$0")/.."
ROOT="$PWD"
STAMP="$(date +%Y%m%d-%H%M%S)"
LOGDIR="/tmp/perl-lsp-verify/$STAMP"
mkdir -p "$LOGDIR"
ln -sfn "$LOGDIR" /tmp/perl-lsp-verify/latest

RED=$'\033[31m'; GRN=$'\033[32m'; YEL=$'\033[33m'; DIM=$'\033[2m'; OFF=$'\033[0m'
declare -a RESULTS=()
FAILED=0

note()  { printf '%s\n' "$*"; }
record() {  # gate verdict detail
  RESULTS+=("$1|$2|$3")
  [ "$2" = PASS ] || FAILED=1
}

# Run a command, tee the FULL output to a log, return its real exit code.
# `set -o pipefail` above makes the tee not swallow it.
run_logged() {
  local gate="$1"; shift
  local log="$LOGDIR/$gate.log"
  printf '%s$ %s%s\n' "$DIM" "$*" "$OFF"
  { "$@"; } >"$log" 2>&1
  local rc=$?
  printf '%s  → %s (rc=%d)%s\n' "$DIM" "$log" "$rc" "$OFF"
  return $rc
}

# ---- premise checks -------------------------------------------------------
# These answer "did the gate test what I meant?", which no gate can answer for
# itself. Each one corresponds to a real miss on this project.

require_bin() {  # a missing prerequisite is a hard error, never a silent skip
  command -v "$1" >/dev/null 2>&1 && return 0
  record "$2" ERROR "prerequisite '$1' not found — gate did NOT run"
  note "  ${RED}✗ $2: '$1' missing. A gate that did not run is not a gate that passed.${OFF}"
  [ -n "${3:-}" ] && note "    $3"
  return 1
}

# ---- gates ----------------------------------------------------------------

gate_unit() {
  run_logged unit cargo test
  local rc=$?
  local n; n="$(grep -oE '^test result: ok\. [0-9]+ passed' "$LOGDIR/unit.log" | awk '{for(i=1;i<=NF;i++) if($i ~ /^[0-9]+$/) t+=$i} END{print t+0}')"
  echo "$n" > "$LOGDIR/.unit-count"
  [ $rc -eq 0 ] && record unit PASS "$n passed" || record unit FAIL "rc=$rc — see unit.log"
}

gate_unit_cpp() {
  run_logged unit-cpp cargo test --features cpp
  local rc=$?
  local n; n="$(grep -oE '^test result: ok\. [0-9]+ passed' "$LOGDIR/unit-cpp.log" | awk '{for(i=1;i<=NF;i++) if($i ~ /^[0-9]+$/) t+=$i} END{print t+0}')"
  if [ $rc -ne 0 ]; then record unit-cpp FAIL "rc=$rc — see unit-cpp.log"; return; fi
  # PREMISE: the cpp build must run strictly MORE tests than the perl-only one.
  # Equal counts mean the feature did not actually take, which exits 0.
  local base=0; [ -f "$LOGDIR/.unit-count" ] && base="$(cat "$LOGDIR/.unit-count")"
  if [ "$base" -gt 0 ] && [ "$n" -le "$base" ]; then
    record unit-cpp FAIL "$n tests vs perl-only $base — cpp feature did not add tests"
  else
    record unit-cpp PASS "$n passed$([ "$base" -gt 0 ] && echo " (+$((n-base)) over perl-only)")"
  fi
}

gate_gold() {
  run_logged gold-build cargo build --release --features cpp
  [ $? -ne 0 ] && { record gold FAIL "release --features cpp build failed — see gold-build.log"; return; }
  run_logged gold perl gold-corpus/run.pl
  local rc=$?
  local log="$LOGDIR/gold.log"
  local get; get() { grep -oE "^  $1 +[0-9]+" "$log" | grep -oE '[0-9]+$' | tail -1; }
  local pass fail langskip skip crash xpass
  pass="$(get PASS)"; fail="$(get FAIL)"; langskip="$(get lang-skip)"
  skip="$(get skip)"; crash="$(get CRASH)"; xpass="$(get XPASS)"
  local detail="PASS=${pass:-?} FAIL=${fail:-?} XPASS=${xpass:-?} CRASH=${crash:-?} skip=${skip:-?} lang-skip=${langskip:-?}"
  if [ $rc -ne 0 ]; then record gold FAIL "$detail (rc=$rc)"; return; fi
  # PREMISE: gold's exit code deliberately ignores lang-skip, because a
  # perl-only build skipping cpp rows is a legitimate configuration. We built
  # WITH cpp, so any lang-skip means the flag did not take and a quarter of the
  # corpus silently did not run.
  if [ "${langskip:-0}" != "0" ]; then
    record gold FAIL "$detail — built --features cpp but lang-skip>0: those rows did NOT run"
  else
    record gold PASS "$detail"
  fi
}

gate_e2e() {
  require_bin nvim e2e "e2e/run.sh needs nvim 0.10+; Ubuntu ships 0.9.5. See CLAUDE.md for the tarball." || return
  run_logged e2e ./e2e/run.sh
  local rc=$?
  local d; d="$(grep -oE '[0-9]+ passed[^0-9]*[0-9]+ failed' "$LOGDIR/e2e.log" | tail -1)"
  [ $rc -eq 0 ] && record e2e PASS "${d:-rc=0}" || record e2e FAIL "${d:-rc=$rc} — see e2e.log"
}

gate_e2e_cpp() {
  require_bin nvim e2e-cpp "e2e/run-cpp.sh needs nvim 0.10+." || return
  run_logged e2e-cpp ./e2e/run-cpp.sh
  local rc=$?
  local d; d="$(grep -oE '[0-9]+ passed[^0-9]*[0-9]+ failed' "$LOGDIR/e2e-cpp.log" | tail -1)"
  # NOTE: run-cpp.sh is `set -euo pipefail` with no aggregation, so the FIRST
  # failing suite aborts and later ones never run. rc is still correct; the log
  # just names one failure rather than all of them.
  [ $rc -eq 0 ] && record e2e-cpp PASS "${d:-rc=0}" || record e2e-cpp FAIL "${d:-rc=$rc} — see e2e-cpp.log"
}

ALL=(unit unit-cpp gold e2e e2e-cpp)
[ "${1:-}" = "--list" ] && { printf '%s\n' "${ALL[@]}"; exit 0; }
SELECTED=("$@"); [ ${#SELECTED[@]} -eq 0 ] && SELECTED=("${ALL[@]}")

note "logs: $LOGDIR  (also /tmp/perl-lsp-verify/latest)"
note ""
for g in "${SELECTED[@]}"; do
  case "$g" in
    unit)     gate_unit ;;
    unit-cpp) gate_unit_cpp ;;
    gold)     gate_gold ;;
    e2e)      gate_e2e ;;
    e2e-cpp)  gate_e2e_cpp ;;
    *) note "unknown gate: $g (see --list)"; FAILED=1 ;;
  esac
done

note ""
note "════ verification bar ════"
for r in "${RESULTS[@]}"; do
  IFS='|' read -r g v d <<< "$r"
  case "$v" in
    PASS)  printf '  %s✓ %-9s%s %s\n' "$GRN" "$g" "$OFF" "$d" ;;
    FAIL)  printf '  %s✗ %-9s%s %s\n' "$RED" "$g" "$OFF" "$d" ;;
    ERROR) printf '  %s! %-9s%s %s\n' "$YEL" "$g" "$OFF" "$d" ;;
  esac
done
note ""
note "full output retained: $LOGDIR"
[ $FAILED -eq 0 ] && note "${GRN}bar green${OFF}" || note "${RED}bar RED — read the logs above, do not re-run to see what happened${OFF}"

# prune old runs, keep the most recent N
ls -1dt /tmp/perl-lsp-verify/*/ 2>/dev/null | tail -n +$(( ${KEEP:-10} + 1 )) | xargs -r rm -rf
exit $FAILED
