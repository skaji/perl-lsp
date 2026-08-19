#!/usr/bin/env bash
# End-to-end test of `run.sh`, because the Python selftest cannot see the
# shell wrapper — and that is exactly where the wrapper broke.
#
# `run_side()` passed the caller's extra arguments as `"$@:3"`. That is not
# slice syntax (`${@:3}` is), and inside a function `$@` is the function's own
# arguments anyway, so it expanded to two args plus a literal `:3` and BOTH
# sides died on `unrecognized arguments`. The wrapper did not run at all. The
# Python selftest passed throughout, which is the point: a green unit suite
# said nothing about whether the entry point worked.
#
# So this runs the real script, on a two-file corpus, and asserts a report
# came out the far end. Usage:
#     bench/sweep/selftest-shell.sh [path-to-perl-lsp]
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BIN=${1:-$HERE/../../target/release/perl-lsp}
[ -x "$BIN" ] || { echo "no binary at $BIN (cargo build --release)"; exit 2; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/corpus/lib"

# The corpus needs a real cross-file edge: the readiness gate demands a
# definition that lands in a DIFFERENT file, so a single-file corpus would
# hang until the budget expired and prove nothing.
cat > "$TMP/corpus/lib/Widget.pm" <<'EOF'
package Widget;
sub new { my $c = shift; return bless {}, $c }
sub spin { return 1 }
1;
EOF
cat > "$TMP/corpus/lib/User.pm" <<'EOF'
package User;
use Widget;
sub go { my $w = Widget->new(); return $w->spin(); }
1;
EOF

# Extra args go through the same path that was broken, so passing one here is
# the actual regression test rather than a smoke test of the happy path.
SWEEP_MAX_FILES=2 SWEEP_PER_FILE=4 SWEEP_SERIAL=1 SWEEP_READY_TIMEOUT=90 \
  "$HERE/run.sh" "$BIN" "$BIN" "$TMP/corpus" "$TMP/out" --timeout 20

fail() { echo "FAIL: $1"; exit 1; }
[ -s "$TMP/out/positions.jsonl" ]   || fail "no positions emitted"
[ -s "$TMP/out/answers-base.jsonl" ]|| fail "base side produced no answers"
[ -s "$TMP/out/answers-head.jsonl" ]|| fail "head side produced no answers"
[ -s "$TMP/out/report.md" ]         || fail "no report produced"
grep -q "Differential sweep report" "$TMP/out/report.md" || fail "report has no header"
grep -q "answers compared"          "$TMP/out/report.md" || fail "report compared nothing"

# Same binary on both sides: anything but agreement means the harness is
# inventing divergences, which is its worst failure mode and its quietest.
n=$(grep -oE '^\| `[a-z-]+` \| [0-9]+' "$TMP/out/report.md" | wc -l || true)
[ "$n" -eq 0 ] || { sed -n '/Divergences by shape/,/^$/p' "$TMP/out/report.md"
                    fail "same binary against itself reported $n divergent shapes"; }

echo "sweep shell selftest: run.sh works end to end, and agrees with itself"
