#!/usr/bin/env bash
# Reconstruct the scaling corpus on a fresh box (cloud agent, CI, new laptop).
#
#   corpus/bootstrap.sh [dest]        # default: $PERL_CORPORA/bulk
#   corpus/bootstrap.sh dest FHEM     # one repo
#
# `$PERL_CORPORA` (default ~/perl-corpora) is the ONE root, shared with
# kick.sh: an explicit [dest] that disagrees with it produces a corpus kick.sh
# cannot find.
#
# Needs: git, perl >= 5.20 with Module::CoreList (core), cpm
#   curl -sSL https://raw.githubusercontent.com/skaji/cpm/main/cpm -o /tmp/cpm && chmod +x /tmp/cpm
#
# Idempotent: existing clones are left alone, existing local/ trees are topped up.
set -u
DEST="${1:-${PERL_CORPORA:-$HOME/perl-corpora}/bulk}"
DEPS="$(dirname "$DEST")/deps"
ONLY="${2:-}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CPM="$(command -v cpm || echo /tmp/cpm)"

# The eight keepers. Each earned its slot; see README.md for the measurement.
#   name|url|why
REPOS=(
"FHEM|https://github.com/fhem/fhem-mirror|main-monoculture: 534 providers of one package name"
"Foswiki|https://github.com/foswiki/distro|high fan-out, 'other'-driven — a second mechanism"
"Evergreen|https://github.com/evergreen-library-system/Evergreen|high fan-out, 262/262 properly packaged"
"WeBWorK|https://github.com/openwebwork/webwork2|worst per-file cost, not fan-out"
"Znuny|https://github.com/Znuny/Znuny|largest corpus, LOWEST fan-out — size/fan-out are independent"
"Webmin|https://github.com/webmin/webmin|path-based require, 101 package decls, lowest hit rate"
"BMO|https://github.com/mozilla-bteam/bmo|healthy reference: 206 att/file at 99.90% hit"
"openfoodfacts|https://github.com/openfoodfacts/openfoodfacts-server|densest static graph, near-zero fan-out"
)

mkdir -p "$DEST" || exit 1
for ENTRY in "${REPOS[@]}"; do
  IFS='|' read -r NAME URL WHY <<< "$ENTRY"
  [ -n "$ONLY" ] && [ "$ONLY" != "$NAME" ] && continue
  echo "══ $NAME — $WHY"
  if [ -d "$DEST/$NAME/.git" ]; then
    echo "   clone: present"
  else
    git clone --depth 1 -q "$URL" "$DEST/$NAME" || { echo "   CLONE FAILED"; continue; }
    echo "   clone: ok"
  fi

  # Dependency manifest: use the repo's own if it has one, else derive.
  # Six of the eight ship none — they are applications, not CPAN dists.
  CF=""
  for c in cpanfile Makefile.PL Build.PL; do [ -f "$DEST/$NAME/$c" ] && CF="$c" && break; done
  if [ -z "$CF" ]; then
    perl "$HERE/derive-cpanfile.pl" "$DEST/$NAME" > "$DEST/$NAME/cpanfile.derived" 2>/dev/null
    CF=cpanfile.derived
    echo "   manifest: derived ($(grep -c '^requires' "$DEST/$NAME/$CF") deps)"
  else
    echo "   manifest: $CF (shipped)"
  fi

  # Install into the repo's own local/. Failures are EXPECTED and non-fatal:
  # XS modules wanting system headers we may not have. A partial local/ is
  # strictly better than none — the point is to make imports RESOLVABLE, and an
  # unresolvable import costs the resolver nothing, which is the bias that makes
  # a corpus measure the wrong thing.
  # Install OUTSIDE the repo, into $DEPS/$NAME. This is load-bearing, not tidiness:
  # the workspace walker indexes everything under the root, so a local/ inside the
  # repo joins the WORKSPACE tier. Measured: FHEM went 929 -> 33,912 indexed files,
  # 97% of them Paws (the AWS SDK ships one .pm per API call). The corpus would
  # have been measuring Paws. Deps belong on the @INC/dependency tier, which is
  # also how a real editor session sees them.
  mkdir -p "$DEPS"
  ( cd "$DEST/$NAME" || exit
    ARGS=(install -L "$DEPS/$NAME" --no-test)
    [ "$CF" = cpanfile.derived ] && ARGS+=(--cpanfile cpanfile.derived)
    timeout 2400 "$CPM" "${ARGS[@]}" > "$DEPS/$NAME.cpm.log" 2>&1
    N=$(find "$DEPS/$NAME" -name '*.pm' 2>/dev/null | wc -l)
    R=$(grep -c '^FAIL resolve' "$DEPS/$NAME.cpm.log" 2>/dev/null || echo 0)
    I=$(grep -c '^FAIL install' "$DEPS/$NAME.cpm.log" 2>/dev/null || echo 0)
    echo "   deps: $N modules -> $DEPS/$NAME  ($R unresolvable, $I build-failed)"
  )
done
echo
echo "Workspace corpora: $DEST"
echo "Dependencies:      $DEPS   (per-repo logs: $DEPS/<name>.cpm.log)"
echo
echo "Measure with deps on @INC, NOT in the workspace:"
echo "  PERL5LIB=$DEPS/FHEM/lib/perl5 perl-lsp --check $DEST/FHEM"
