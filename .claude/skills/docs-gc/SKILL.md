---
name: docs-gc
description: Garbage-collect the docs tree — delete landed prompt briefs, repoint their references into the owning ADRs, and scrub ADRs to house voice (no historical narrative). Use after a mission/arc lands, or whenever docs/prompt-*.md piles up.
---

# docs-gc: landed prompts die, ADRs carry the load

The docs tree has two kinds of design docs with OPPOSITE lifecycles:

- `docs/prompt-*.md` — implementation BRIEFS. They exist to drive work.
  When the work lands, the brief is garbage: delete it. Its decisions
  live in ADRs, its landing record in `docs/open-forks.md`, its history
  in git.
- `docs/adr/*.md` — load-bearing DECISIONS. They live forever, but in
  house voice: they describe what IS — contracts, invariants,
  trade-offs, failure modes, measured facts — never what WAS or what
  CHANGED. ("Git remembers; the comment shouldn't" — CLAUDE.md
  Comment style. It governs docs the same as code.)

Run this pass with a cost-appropriate model (spawn a subagent — this is
mechanical judgment, not architecture).

## Job 1 — classify and delete landed prompts

For each `docs/prompt-*.md`:

- **LANDED** → delete. Evidence: its own progress/status says complete;
  `docs/open-forks.md` or an ADR records the landing; no live next
  steps that aren't tracked elsewhere.
- **FORWARD / PARKED** → keep unchanged (it still drives future work).
- **MIXED** (landed phases + live next steps) → keep, but strip the
  landed-progress narrative to a one-line pointer at the owning ADR;
  only the live forward work remains.
- **Unsure → keep.** Deletion must be obviously safe.

Before deleting a file, two obligations:

1. **Rescue load-bearing content.** If the brief holds rationale no ADR
   owns (a decision, an invariant, a failure-mode analysis that code
   comments point at), move it into the most relevant `docs/adr/*.md`
   first. A prompt doc is disposable; the reasoning inside it may not be.
2. **Repoint every reference.** Grep the ENTIRE repo (`src/`,
   `CLAUDE.md`, `docs/`, `gold-corpus/`) for the filename. Code comments
   and CLAUDE.md must point at the owning ADR instead — a dangling doc
   link in a comment is a bug. (`docs/open-forks.md` is a historical
   ledger — its references may stay.)

## Job 2 — scrub ADRs to house voice

Keep every contract, invariant, trade-off, failure mode, and number.
State measurements as current facts ("abseil resident payload:
11.2 MB"), not as deltas with history ("reduced 46→11 in phase 5").

Remove: phase-by-phase landing narratives, "Status: landed <date>"
logs, "replaces/subsumes the old X" framing (at most one
`Supersedes: <adr>` line), spike war stories with no reusable lesson.
Never change technical meaning.

An ADR that is ENTIRELY a landing log with no reusable design content:
fold its load-bearing facts into the related ADR and delete it — same
reference-fixing discipline as Job 1.

## Verify (non-negotiable)

```
# every doc reference resolves (open-forks.md exempt):
grep -rn "docs/prompt-\|docs/adr/" src/ CLAUDE.md docs/ gold-corpus/ \
  --include=*.rs --include=*.md --include=*.pl \
  | grep -v "docs/open-forks.md:" \
  | grep -oE "docs/(prompt-[a-z-]+|adr/[a-z-]+)\.md" | sort -u \
  | while read f; do [ -f "$f" ] || echo "DANGLING: $f"; done

cargo test --release   # some tests assert on doc paths
```

Both must be clean before committing. Commit message pattern:
`docs: gc — <N> landed prompts deleted, <M> ADRs scrubbed to house voice`.
