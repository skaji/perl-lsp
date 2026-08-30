#!/usr/bin/env bash
# Editor-surface baseline runs: every scenario corpus x 3 reps x cold+warm,
# rows appended to one JSONL for bench/seed-baselines.py.
#
#   bench/editor-baseline.sh <out.jsonl> <corpus:root:scenario>...
#   e.g. bench/editor-baseline.sh /tmp/eb.jsonl \
#          "Bugzilla:$HOME/perl-corpora/bulk/Bugzilla:bugzilla" \
#          "mojo:$HOME/personal/mojo:mojo"
#
# QUIET BOX ONLY — run lines record loadavg_at_start, and a baseline taken
# under foreign load is the loadavg trap. The scenario's own
# project.root_subdir is honored (mojo's root is lib/, and ignoring the
# field cost six silently-failed runs once).
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$1"; shift
for spec in "$@"; do
  IFS=: read -r corpus root scen <<<"$spec"
  sub=$(python3 -c "import json;print(json.load(open('$HERE/bench/scenarios/$scen.json'))['project'].get('root_subdir',''))")
  [ -n "$sub" ] && root="$root/$sub"
  PL=""
  [ -d "$HOME/perl-corpora/deps/$corpus/lib/perl5" ] && PL="$HOME/perl-corpora/deps/$corpus/lib/perl5"
  for rep in 1 2 3; do
    cache=$(mktemp -d)
    for phase in cold warm; do
      echo "== $corpus rep $rep $phase =="
      XDG_CACHE_HOME="$cache" PERL5LIB="$PL" \
        python3 "$HERE/bench/lsp_bench.py" --bin "$HERE/target/release/perl-lsp" \
          --root "$root" --scenario "$HERE/bench/scenarios/$scen.json" \
          --out "${OUT%.jsonl}-$corpus-$rep-$phase.json" --label "$phase" \
          --jsonl "$OUT" --corpus "$corpus" --rep "$rep" 2>&1 | tail -1
    done
    rm -rf "$cache"
  done
done
echo "EDITOR BASELINE RUNS DONE: $(wc -l < "$OUT") rows"
