#!/usr/bin/env bash
# Measure perl-lsp across the real-project corpora, one JSONL line per fact.
#
#   bench/measure.sh                     # every corpus, 3 reps, cold+warm
#   bench/measure.sh --reps 1 FHEM       # one corpus, one rep (marked as such)
#   bench/measure.sh --out runs/         # where the JSONL lands
#
# Output is DELIBERATELY tall and raw: {kind,name,value,unit} per row, no
# aggregation, no derived ratios. Reports slice it in DuckDB. A mean computed
# here is a decision the collector has no business making, and a stored ratio
# is how an attempts-vs-completions mixup became a finding once already.
#
# Every repetition is its own row with its own `rep`. There is deliberately no
# way to emit "the number" — a single sample is a sample, and the schema says
# so, because a one-run baseline once produced a phantom +400ms regression
# that survived a day.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$HERE/target/release/perl-lsp"
BULK="${PERL_CORPORA:-$HOME/perl-corpora}/bulk"
DEPS="${PERL_CORPORA:-$HOME/perl-corpora}/deps"
REPS=3
OUT="$HERE/bench/runs"
ONLY=()

while [ $# -gt 0 ]; do
  case "$1" in
    --reps) REPS="$2"; shift 2;;
    --out)  OUT="$2"; shift 2;;
    -h|--help) sed -n '2,16p' "$0"; exit 0;;
    *) ONLY+=("$1"); shift;;
  esac
done

[ -x "$BIN" ] || { echo "no release binary at $BIN — cargo build --release --features cpp" >&2; exit 1; }
[ -d "$BULK" ] || { echo "no corpora at $BULK — run corpus/bootstrap.sh" >&2; exit 1; }
command -v jq >/dev/null || { echo "needs jq" >&2; exit 1; }

mkdir -p "$OUT"
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
JSONL="$OUT/$RUN_ID.jsonl"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

# --- provenance, once per run -------------------------------------------
# A measurement without its build features is a trap: a non-cpp build
# lang-skips half of gold, and the same class of silence poisons timings.
SHA="$(git -C "$HERE" rev-parse --short=8 HEAD)"
DIRTY=false; git -C "$HERE" diff --quiet || DIRTY=true
FEATURES="$("$BIN" --languages 2>/dev/null | sed 's/.*languages: //')"
jq -cn \
  --arg run_id "$RUN_ID" --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg sha "$SHA" --argjson dirty "$DIRTY" --arg features "$FEATURES" \
  --arg host "$(hostname)" --arg kernel "$(uname -r)" \
  --argjson nproc "$(nproc)" \
  --argjson mem_kb "$(awk '/MemTotal/{print $2}' /proc/meminfo)" \
  --argjson load "$(awk '{print $1}' /proc/loadavg)" \
  --argjson reps "$REPS" \
  '{t:"run",run_id:$run_id,ts:$ts,sha:$sha,dirty:$dirty,features:$features,
    host:$host,kernel:$kernel,nproc:$nproc,mem_kb:$mem_kb,loadavg_at_start:$load,
    reps_planned:$reps}' >> "$JSONL"

emit() { # corpus rep phase kind name value unit
  jq -cn --arg run_id "$RUN_ID" --arg c "$1" --argjson r "$2" --arg p "$3" \
        --arg k "$4" --arg n "$5" --argjson v "$6" --arg u "$7" \
    '{t:"m",run_id:$run_id,corpus:$c,rep:$r,phase:$p,kind:$k,name:$n,value:$v,unit:$u}' \
    >> "$JSONL"
}

# Load average is recorded per measurement, not just per run: a corpus that
# happened to run while the box was busy is not comparable to one that did
# not, and averaging across them silently is the trap.
measure_one() { # corpus root rep phase cachedir
  local corpus="$1" root="$2" rep="$3" phase="$4" cache="$5"
  local g="$SCRATCH/g.json" t="$SCRATCH/t.json"
  rm -f "$g" "$t"

  local pl=""
  [ -d "$DEPS/$corpus" ] && pl="$DEPS/$corpus/lib/perl5"

  emit "$corpus" "$rep" "$phase" "env" "loadavg" "$(awk '{print $1}' /proc/loadavg)" "load"

  # /usr/bin/time gives peak RSS the process cannot under-report about itself.
  local tf="$SCRATCH/time.txt"
  PERL5LIB="$pl" XDG_CACHE_HOME="$cache" \
  PERL_LSP_TIMINGS=1 PERL_LSP_GHOST_STATS=1 \
  PERL_LSP_GHOST_JSON="$g" PERL_LSP_TIMINGS_JSON="$t" \
    /usr/bin/time -f '%e %M %P' -o "$tf" \
    "$BIN" --check "$root" >/dev/null 2>&1

  if [ -s "$tf" ]; then
    read -r wall maxrss cpupct < <(tail -1 "$tf")
    emit "$corpus" "$rep" "$phase" "timing" "check.wall" "$(awk -v w="$wall" 'BEGIN{print w*1000}')" "ms"
    emit "$corpus" "$rep" "$phase" "rss"    "peak"       "$(awk -v m="$maxrss" 'BEGIN{print m/1024}')" "MB"
    emit "$corpus" "$rep" "$phase" "cpu"    "utilization" "${cpupct%\%}" "pct"
  fi

  # Every counter, every module. No top-N, no rounding — the distribution is
  # the point, and one pathological file inside a healthy total is exactly
  # what a mean hides.
  [ -s "$g" ] && jq -c --arg run_id "$RUN_ID" --arg c "$corpus" --argjson r "$rep" --arg p "$phase" '
      (.counters   | to_entries[] | {kind:"counter",   name:.key, value:.value,     unit:"n"}),
      (.timings    | to_entries[] | {kind:"accum_ns",  name:.key, value:.value.ns,  unit:"ns"}),
      (.timings    | to_entries[] | {kind:"accum_n",   name:.key, value:.value.n,   unit:"n"}),
      (.quantities | to_entries[] | {kind:"qty_sum",   name:.key, value:.value.sum, unit:"n"}),
      (.quantities | to_entries[] | {kind:"qty_n",     name:.key, value:.value.n,   unit:"n"})
      | {t:"m",run_id:$run_id,corpus:$c,rep:$r,phase:$p} + .' "$g" >> "$JSONL"

  [ -s "$t" ] && jq -c --arg run_id "$RUN_ID" --arg c "$corpus" --argjson r "$rep" --arg p "$phase" '
      .modules[] | {t:"m",run_id:$run_id,corpus:$c,rep:$r,phase:$p,
                    kind:(if .cached then "file_cached" else "file_build" end),
                    name:.module, value:(.build_ns/1e6), unit:"ms"},
                   {t:"m",run_id:$run_id,corpus:$c,rep:$r,phase:$p,
                    kind:"file_parse", name:.module, value:(.parse_ns/1e6), unit:"ms"}' \
      "$t" >> "$JSONL"

  local db; db="$(find "$cache" -name 'modules*.db' -printf '%s\n' 2>/dev/null | awk '{s+=$1} END{print s+0}')"
  emit "$corpus" "$rep" "$phase" "store" "modules_db_bytes" "${db:-0}" "bytes"
}

CORPORA=()
if [ ${#ONLY[@]} -gt 0 ]; then CORPORA=("${ONLY[@]}")
else while IFS= read -r d; do CORPORA+=("$(basename "$d")"); done < <(find "$BULK" -mindepth 1 -maxdepth 1 -type d | sort); fi

echo "run $RUN_ID  sha=$SHA dirty=$DIRTY  [$FEATURES]  reps=$REPS"
echo "-> $JSONL"
for corpus in "${CORPORA[@]}"; do
  root="$BULK/$corpus"
  [ -d "$root" ] || { echo "  skip $corpus (missing)"; continue; }
  for rep in $(seq 1 "$REPS"); do
    # Cold means COLD: a private throwaway cache per rep, or the second rep
    # measures the first rep's cache and calls itself cold.
    cache="$SCRATCH/cache-$corpus-$rep"; rm -rf "$cache"; mkdir -p "$cache"
    printf '  %-14s rep %s cold ' "$corpus" "$rep"; measure_one "$corpus" "$root" "$rep" cold "$cache"
    printf 'warm\n';                                measure_one "$corpus" "$root" "$rep" warm "$cache"
    rm -rf "$cache"
  done
done
echo "done: $(wc -l < "$JSONL") rows"
