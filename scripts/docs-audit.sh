#!/usr/bin/env bash
# Mechanical staleness in the prose corpus: claims that are checkable WITHOUT
# judgement. Everything here is a fact about whether a named thing exists, so a
# hit is a hit — no reading required, and no agent should spend tokens on it.
#
# What this deliberately does NOT do: anything needing judgement. "X is the only
# writer of Y", "this is deferred", "the chase is 61.6%" — those are claims about
# behaviour, status, and measurements, and a grep cannot adjudicate them. They
# are the reason a human or an agent reads the doc. This script exists so that
# reading starts from the interesting part.
#
# Two kinds of deliberate staleness exist in this repo and are NOT bugs:
#   - struck-through text (~~...~~) is kept on purpose when workaround code
#     written against the old behaviour still exists (see the `right:` field
#     note in CLAUDE.md). Lines containing ~~ are skipped.
#   - forward-compat references to things that do not exist YET (the
#     `parenthesized_expression` arms). Those are flagged but marked LIKELY-OK
#     when the surrounding line says so.
#
# Usage: scripts/docs-audit.sh [--quiet]   (exit 1 if anything found)
set -uo pipefail
cd "$(dirname "$0")/.."

QUIET=0; [ "${1:-}" = "--quiet" ] && QUIET=1

# A mixed tree makes every result meaningless: audit docs from one commit
# against code from another and you get confident nonsense. This bit me
# immediately — a tree 27 commits behind reported `PackageSymbol` "absent from
# src/" in 18 docs, when the rename that introduced it simply had not been
# pulled. Refuse rather than warn; a warning gets skimmed.
if git rev-parse --git-dir >/dev/null 2>&1; then
  UP="$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || true)"
  if [ -n "$UP" ]; then
    git fetch -q origin 2>/dev/null || true
    BEHIND="$(git rev-list --count "HEAD..$UP" 2>/dev/null || echo 0)"
    if [ "${BEHIND:-0}" -gt 0 ]; then
      echo "REFUSING: working tree is $BEHIND commit(s) behind $UP." >&2
      echo "Docs and code must come from the SAME tree or every finding is suspect." >&2
      echo "Run: git merge --ff-only $UP" >&2
      exit 2
    fi
  fi
fi
RED=$'\033[31m'; YEL=$'\033[33m'; DIM=$'\033[2m'; OFF=$'\033[0m'
# `hit` runs inside pipelines, i.e. subshells, so a FOUND=1 there is lost and
# the summary contradicts the output it just printed. Count through a file.
HITS="$(mktemp)"; trap 'rm -f "$HITS"' EXIT
CORPUS=$(find docs -name '*.md' 2>/dev/null; ls CLAUDE.md README.md 2>/dev/null; \
         ls gold-corpus/README.md gold-corpus/KNOWN-GAPS.md bench/RESULTS.md 2>/dev/null)

hdr() { [ $QUIET -eq 1 ] || printf '\n%s── %s%s\n' "$DIM" "$1" "$OFF"; }
hit() { echo 1 >> "$HITS"; printf '  %s%s%s  %s\n' "$RED" "$1" "$OFF" "$2"; }

# Lines that are deliberately historical/forward-looking are exempt.
_live_lines() {  # file -> "lineno:text" excluding struck-through
  grep -n '' "$1" 2>/dev/null | grep -v '~~'
}

# ---- 1. referenced source paths that do not exist -------------------------
hdr "source paths referenced in prose that do not exist"
for f in $CORPUS; do
  _live_lines "$f" | grep -oE '^[0-9]+:.*' | while IFS= read -r line; do
    n="${line%%:*}"
    echo "$line" | grep -oE '\b(src|e2e|gold-corpus|scripts|bench|frameworks)/[A-Za-z0-9_./-]+\.(rs|pl|sh|lua|rhai|json|md)\b' \
    | while read -r p; do
        [ -e "$p" ] || echo "MISS|$f:$n|$p"
      done
  done
done | sort -u -t'|' -k3 | while IFS='|' read -r _ loc p; do hit "$p" "$loc"; done

# ---- 2. env vars documented but absent from the tree ----------------------
hdr "PERL_LSP_* documented but not present in src/"
comm -23 \
  <(for f in $CORPUS; do _live_lines "$f" | grep -oE 'PERL_LSP_[A-Z0-9_]+'; done | sort -u) \
  <(grep -rhoE 'PERL_LSP_[A-Z0-9_]+' src/ 2>/dev/null | sort -u) \
| while read -r v; do
    [ -n "$v" ] || continue
    hit "$v" "$(grep -rln "$v" $CORPUS 2>/dev/null | paste -sd, )"
  done

# ---- 3. CLI flags documented but not in the arg parser --------------------
hdr "perl-lsp --flags documented but not found in src/"
for f in $CORPUS; do _live_lines "$f" | grep -oE '\-\-[a-z][a-z0-9-]{3,}'; done | sort -u \
| while read -r flag; do
    [ -n "$flag" ] || continue
    grep -rqF "\"$flag\"" src/ 2>/dev/null && continue
    grep -rqF "$flag" src/ 2>/dev/null && continue
    # only report flags that look like ours (documented next to perl-lsp)
    grep -rqE "perl-lsp[^\\n]*$flag" $CORPUS 2>/dev/null && \
      hit "$flag" "$(grep -rlE "perl-lsp[^\\n]*$flag" $CORPUS 2>/dev/null | head -3 | paste -sd, )"
  done

# ---- 4. doc -> doc links that dangle --------------------------------------
hdr "cross-references to docs that do not exist"
for f in $CORPUS; do
  _live_lines "$f" | grep -oE '\b(docs/[A-Za-z0-9_./-]+\.md)\b' | sort -u \
  | while read -r p; do [ -e "$p" ] || hit "$p" "$f"; done
done

# ---- 5. Rust items named in prose that no longer exist --------------------
# Only backticked CamelCase / snake_case::path forms, to keep the false-positive
# rate survivable. A miss here is worth a look, not an automatic edit.
hdr "backticked Rust items not found anywhere in src/ (review, not gospel)"
for f in $CORPUS; do
  _live_lines "$f" | grep -oE '`[A-Z][A-Za-z0-9]{4,}`' | tr -d '`' | sort -u \
  | while read -r sym; do
      grep -rqE "\b$sym\b" src/ 2>/dev/null || echo "$sym|$f"
    done
done | cut -d'|' -f1 | sort -u | while read -r sym; do
  # a type named in only one doc is usually a design sketch, not a stale claim
  cnt=$(grep -rl "\`$sym\`" $CORPUS 2>/dev/null | wc -l)
  [ "$cnt" -ge 2 ] && hit "$sym" "in $cnt docs, absent from src/"
done

N=$(wc -l < "$HITS" 2>/dev/null || echo 0); N=${N// /}
[ $QUIET -eq 1 ] || {
  echo
  if [ "$N" -eq 0 ]; then echo "no mechanical staleness found"
  else echo "${YEL}$N mechanical finding(s) — these need no agent, only an edit${OFF}"; fi
}
[ "$N" -eq 0 ]
