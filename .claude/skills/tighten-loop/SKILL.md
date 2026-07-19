---
name: tighten-loop
description: Run one autonomous cleanup/refactoring round over the perl-lsp codebase — rule-audit probes + recent-arc diff audit + design-debt drain → hitlist → behavior-frozen refactor slices → identity gate → close. Use between feature arcs to pay down cruft without changing behavior.
---

# The tighten loop

One round = **generate findings → hitlist → refactor slices → identity gate →
round close**. Sibling of the dogfood loop (`dogfood-loop` skill): same
skeleton, but the "corpus" is our own codebase and rulebook, and the gate is
IDENTITY, not improvement. Ratified framing (2026-07-17, veesh): all three
finding generators; behavior-frozen slices; ADR-first for module splits.

## Phase 1 — finding generators (2–3 agents, parallel, findings-only)

Three lenses, each an agent brief; they never fix, only report file:line +
one-line evidence:

1. **Rule-audit probe**: audit src/ against CLAUDE.md's own rules — rule #10
   shape-branches (name allowlists, `is this X`-flavored enumeration),
   edges-not-values violations (materialized types re-pushed onto
   edge-reachable attachments), layering drift (tier imports, tree-sitter
   surface leaks), residency discipline (unbounded caches, whole-copy pins),
   comment style (history narration, what-not-why), DRY seam misses (two
   callers re-deriving what one FileAnalysis method should own). The rulebook
   evolves, so this probe self-renews.
2. **Recent-arc diff audit**: `git diff <last-round-close>..HEAD` swept for
   integration cruft the per-slice merge reviews missed — duplicated helpers
   landed by parallel slices, comment drift, test overlap, allowlist growth.
3. **Design-debt drain**: PARKED.md's design-debt tier + recorded leave-alone
   verdicts, deliberately re-litigated with fresh eyes. An entry leaves the
   round either FIXED, or RE-RATIFIED with the verdict's date bumped (so the
   next round knows it was recently re-examined and skips it).

Collate into `docs/tighten-<n>.md` — one row per finding, dedup'd, each
tagged with its generator and a slice-sized scope estimate.

## Phase 2 — refactor slices (opus, parallel worktrees)

- **Behavior-frozen.** The identity gate (below) is the contract. A slice
  that discovers a BUG logs it as a finding for a dogfood-style fix round —
  never fixes it inline, no matter how small. No version-constant bumps: a
  needed bump means behavior changed, which means the slice is out of scope.
- **No public-surface renames** without a hitlist row that names every caller
  tier affected.
- **Module splits are ADR-first**: a slice may PROPOSE a split
  (`docs/adr/split-<module>.md`: boundaries, what moves, layering impact,
  mechanical plan) for human ratification; the split executes as its own
  dedicated slice only after sign-off. The 13k-line files
  (file_analysis.rs, builder.rs, builder_tests.rs) are the standing
  candidates.
- **Fork freeze**: never resolve or close `docs/open-forks.md` entries;
  question-only additions allowed.
- All operational discipline from the dogfood-loop skill applies verbatim:
  self-worktree from verified origin tip, push-first after first commit,
  foreground-only, strict-&&, never pipe over a verdict, quiet re-run for
  contended tallies, isolate `XDG_CACHE_HOME` if sibling cache contention
  bites.

## Phase 3 — identity gate (per slice, then once post-merge)

- Both cargo suites: zero failures AND unchanged pass counts (a count delta
  is legal only for deliberate test dedup — named in the report).
- Armed gold, eviction on, both fast-path modes: tallies byte-identical to
  the pre-slice run (record before/after).
- `--check` on one real corpus (mojo or DBIx-Class) — diagnostics count
  unchanged.
- Layering tests green with NO new allowlist entries (an allowlist addition
  in a cleanup round is a smell that reverts the slice).

## Round close

- Findings all FIXED / RE-RATIFIED / promoted to the next fix-round hitlist.
- LOC delta, warnings count, and dupes-extracted recorded in the round doc.
- Leave-alone verdicts written to PARKED.md's design-debt tier with dates —
  the next round's drain generator reads them.
- Split-ADRs (if any) queued for human ratification — the round does NOT
  block on them.

## Exit criteria for the loop (when to stop firing rounds)

Warnings at zero; the rule-audit probe returns no new CONFIRMED violations
two rounds running; design-debt tier fully re-ratified within the last two
rounds; remaining findings are all blocked on human decisions (forks, split
ADRs). Then park the loop until the next feature arc lands.
