---
name: dogfood-loop
description: Run one autonomous dogfood→hitlist→fix→sweep round against real-world corpora for any language perl-lsp supports. Use when hardening a language (new or existing) with real-usage probing instead of synthetic tests.
---

# The dogfood loop

One round = **dogfood → hitlist → xfail-encode → fix → merge-gate → round-close sweep**.
Language-agnostic: parameterize on the target language, its corpus, and its
gold fixture root. Proven over the C/C++ go-live (rounds 1–2 + tightening).

## Inputs (establish before firing anything)

- **Language + corpus**: 3–6 REAL codebases for the target language, cloned
  locally (e.g. cpp used `~/personal/cpp-bench/`: abseil, fmt, folly, json,
  redis; Perl uses `gold-corpus/local/`). Real code finds what fixtures
  can't — the shapes nobody thought to write down.
- **Binary**: `cargo build --release --features all-langs`, and verify the
  binary actually has the language (`perl-lsp --parse <sample>` — a
  default-features rebuild silently drops pack languages).
- **Gold fixture root** for the language (e.g. `gold-corpus/cpp-fixture/`)
  and its fixtures file under `gold-corpus/fixtures/`.

## Phase 1 — dogfood agents (sonnet)

Fire 2–4 agents in parallel worktrees, each owning disjoint corpus repos.
Briefs are **task-driven, not feature-checklists**: "you are a developer
doing <real task: trace this call path / rename this / find all users of X>
in <repo>, using ONLY the perl-lsp CLI (`--workspace-symbol`, `--rename`,
gd/gr/hover/completion via the e2e harness or CLI mirrors)". Agents record
every place the tool lied, undercounted, or went dark.

Non-negotiable disciplines in every brief:

- **Step-0 base guard**: `git fetch && git rebase origin/<branch>` before
  any work — worktree agents branch stale.
- **grep-sanity every gr count**: `gr` says N references → cross-check with
  `grep -rn` (minus comments/strings noise). An unverified count is not a
  finding. Record both numbers.
- **Findings, not fixes**: dogfood agents never patch. They report
  file:line, the capability, expected vs got, and the grep evidence.
- Model: sonnet. Probing doesn't need smarts; it needs diligence.

## Phase 2 — hitlist synthesis (main loop)

Collate findings into `docs/hitlist-<round>.md`: one row per finding with
capability, repro coordinates, evidence, and a first-guess root cause.
Dedup aggressively — 21 raw findings usually collapse into ~5 root causes.

Optionally fire a wave-2 **repro-reducer + root-causer** pair (sonnet):
reduce each finding to a minimal file, then CONFIRM the root cause by
experiment. Guessed root causes get refuted embarrassingly often (the
cpp include-closure hypothesis died this way; the real causes were split
macro identity + namespace-blindness).

## Phase 3 — encode as RED xfail gold rows (before any fix)

Every reducible finding becomes a gold row with `"status": "xfail"` via
`gold-corpus/run.pl --emit --root <fixture-root> <cap> <file> <row> <col>`.
This locks the repro. The harness then enforces the promotion: when a fix
lands, the row XPASSes → flip it to `"status": "gold"` in the same commit.
Non-reducible findings go to `docs/PARKED.md` (residual-bug tier) with the
probe evidence inline.

## Phase 4 — fix slices (opus)

Group root causes into 2–5 disjoint slices; one agent per slice, parallel
worktrees, step-0 base guard in every brief. Architecture-touching slices
(witness bag, resolution core, driver pipeline, extraction) run on **opus**
— sonnet produces layering cruft here (verified: cpp semantics embedded in
the language-generic extraction tier). Pure-mechanical slices may stay
sonnet. Fable is never spawned as an agent.

Brief each slice with the repo's architecture rules that bite: rule #1
(single tree-consumer per tier), rule #10 (no shape special-cases),
edges-not-values, clear-and-emit for re-emittable passes. Agents commit to
their worktree branch and do NOT push.

Architectural forks mid-slice: pick the loosely-coupled/reversible option,
log in `docs/open-forks.md` (options / picked / undo cost / question),
keep moving — never block on ratification.

## Phase 5 — merge gate (main loop, per slice)

Before merging each agent branch:

1. **Diff review against the architecture rules** — read the diff, not the
   agent's summary. Look for: language semantics above/below its tier,
   shape special-cases, parallel stores instead of edges, version-constant
   collisions (two agents bumping `EXTRACT_VERSION` to the same value —
   same-value merges hide incompatibility; reconcile to max+1).
2. Steer in-flight when possible (SendMessage) — cheaper than post-merge
   rework.
3. Merge, then the **full net**: `cargo test --release` both feature sets;
   gold BOTH modes (default + `PERL_LSP_CPP_NO_FASTPATH=1`) cold — 
   `perl-lsp --clear-cache` between; `./e2e/run.sh` + per-language e2e.
   All 0 FAIL / 0 XPASS / 0 CRASH. Rebuild `--features all-langs` LAST
   (a default-features test run leaves a langless binary on disk).
4. Push. Delete the agent branch.

## Phase 6 — round-close sweep (opus, one agent)

One de-cruft/review agent over the round's whole accumulated diff
(`git diff <round-start-sha>..HEAD`): layering leaks, dead code, comment
rot (no history narration), doc currency (`docs/PARKED.md` pruned of
landed items, hitlist rows marked LANDED, KNOWN-GAPS current), warnings.
Leave-alone verdicts get RECORDED so the next sweep doesn't re-litigate.

## Exit criteria for a round

- Hitlist rows all LANDED or explicitly parked with evidence.
- Zero open XPASS (every fixed row promoted).
- Full net green, pushed.
- `docs/PARKED.md` + `docs/open-forks.md` current.
- A round summary appended to the session/brag doc.

Then either fire the next round (new corpus repos debut + re-probes of
everything just fixed) or park the language with its limits pinned.
