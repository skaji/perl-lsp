#!/usr/bin/env bash
# Is this PR actually safe to merge? Answers the four questions that a green
# checkmark on the PR page does NOT answer.
#
# Every one of these produced a wrong call on this repo:
#
#   1. "Green" can include CANCELLED. `statusCheckRollup` keeps HISTORICAL
#      entries, so a check appears once per run — and `cancel-in-progress` in
#      ci.yml guarantees a CANCELLED entry beside the real one after every
#      force-push. A filter of `conclusion != "SUCCESS"` therefore reports
#      three failures on a fully green PR. Fix: group by check name, take the
#      latest, ignore CANCELLED.
#
#   2. "Green" can be green on a DIFFERENT COMMIT. `pull_request` does not fire
#      on a base retarget (`edited` is not in the default type set), and
#      cancel-in-progress eats the force-push run. A restacked PR can therefore
#      show zero checks, or checks belonging to the pre-restack SHA. Fix: match
#      each run's headSha against the PR's CURRENT head.
#
#   3. "Green" can be green against a MOVED BASE. A verdict is only about the
#      tree it was computed on. Fix: report how far behind the integration tip
#      the PR is — and, because demanding a rebase for every unrelated commit
#      never converges when several agents are landing work, report whether the
#      PR's files actually INTERSECT what the tip gained. No overlap means the
#      base move cannot break this tree.
#
#   4. Squash-merging a PR that is the BASE of another orphans the rest of the
#      stack: the squash creates one new commit, so the child still carries the
#      originals and conflicts against content that is already upstream. Fix:
#      detect dependents and say `--merge`.
#
# Usage:  scripts/pr-status.sh <pr> [<pr>...]
set -uo pipefail

REPO="${BOARD_REPO:-tree-sitter-perl/perl-lsp}"
INTEGRATION="${INTEGRATION_BRANCH:-claude/project-rewrite-cpp-yqutwf}"
RED=$'\033[31m'; GRN=$'\033[32m'; YEL=$'\033[33m'; OFF=$'\033[0m'

[ $# -eq 0 ] && { sed -n '2,34p' "$0" | sed 's/^# \{0,1\}//'; exit 1; }

git fetch origin --quiet 2>/dev/null || true
TIP="$(git rev-parse "origin/$INTEGRATION" 2>/dev/null)"
echo "integration tip: ${TIP:0:8} ($INTEGRATION)"
echo

overall=0
for pr in "$@"; do
  meta="$(gh pr view "$pr" --repo "$REPO" --json number,title,headRefName,headRefOid,baseRefName,mergeable,state 2>/dev/null)" || {
    echo "${RED}#$pr: cannot read${OFF}"; overall=1; continue; }
  head="$(jq -r .headRefOid <<<"$meta")"
  branch="$(jq -r .headRefName <<<"$meta")"
  base="$(jq -r .baseRefName <<<"$meta")"
  state="$(jq -r .state <<<"$meta")"
  mrg="$(jq -r .mergeable <<<"$meta")"
  title="$(jq -r .title <<<"$meta")"
  echo "── #$pr ${title:0:60}"
  echo "   head=${head:0:8} base=$base state=$state mergeable=$mrg"

  ok=1

  # (1) checks: latest per name, CANCELLED ignored
  rollup="$(gh pr view "$pr" --repo "$REPO" --json statusCheckRollup 2>/dev/null)"
  verdict="$(jq -r '
    [.statusCheckRollup[]? | select(.conclusion != "CANCELLED")]
    | group_by(.name) | map(last)
    | if length == 0 then "NONE"
      elif any(.status != "COMPLETED") then "PENDING"
      elif all(.conclusion == "SUCCESS") then "GREEN"
      else "FAILED: " + ([.[] | select(.conclusion != "SUCCESS") | .name] | join(","))
      end' <<<"$rollup")"
  case "$verdict" in
    GREEN)   echo "   ${GRN}checks: GREEN${OFF}" ;;
    NONE)    echo "   ${RED}checks: NONE — CI never ran on this head (base retarget? see note 2)${OFF}"; ok=0 ;;
    PENDING) echo "   ${YEL}checks: PENDING${OFF}"; ok=0 ;;
    *)       echo "   ${RED}checks: $verdict${OFF}"; ok=0 ;;
  esac

  # (2) do those checks belong to the CURRENT head?
  runs_on_head="$(gh run list --repo "$REPO" --limit 40 --json headSha,conclusion \
      --jq "[.[] | select(.headSha == \"$head\")] | length" 2>/dev/null || echo 0)"
  if [ "${runs_on_head:-0}" -eq 0 ]; then
    echo "   ${RED}no workflow run exists for head ${head:0:8} — any green belongs to another commit${OFF}"
    ok=0
  else
    echo "   runs on current head: $runs_on_head"
  fi

  # (3) behind the tip, and does it matter?
  if git fetch origin "$branch" --quiet 2>/dev/null; then
    bh="$(git rev-list --count FETCH_HEAD.."$TIP" 2>/dev/null || echo '?')"
    if [ "$bh" = "0" ]; then
      echo "   behind tip: 0"
    else
      mb="$(git merge-base FETCH_HEAD "$TIP" 2>/dev/null)"
      inter="$(comm -12 \
        <(git diff --name-only "$mb"..FETCH_HEAD 2>/dev/null | sort) \
        <(git diff --name-only "$mb".."$TIP" 2>/dev/null | sort))"
      if [ -z "$inter" ]; then
        echo "   behind tip: $bh commits, ${GRN}no file overlap${OFF} — base move cannot break this tree"
      else
        echo "   behind tip: $bh commits, ${YEL}OVERLAPS:${OFF}"
        printf '     %s\n' $inter
        ok=0
      fi
    fi
  fi

  # (4) is anything stacked on this PR?
  deps="$(gh pr list --repo "$REPO" --state open --json number,baseRefName \
      --jq "[.[] | select(.baseRefName == \"$branch\") | .number] | join(\", \")" 2>/dev/null)"
  if [ -n "$deps" ] && [ "$deps" != "" ]; then
    echo "   ${YEL}dependents: #$deps — merge with --merge, NOT --squash (squash orphans them)${OFF}"
  fi

  if [ $ok -eq 1 ]; then echo "   ${GRN}=> safe to merge${OFF}"; else echo "   ${RED}=> NOT clear${OFF}"; overall=1; fi
  echo
done
exit $overall
