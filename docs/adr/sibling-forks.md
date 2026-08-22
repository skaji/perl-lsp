# Sibling forks: when two code paths answer one question

Two implementations of one decision, never reconciled, are the defect
class behind a run of real bugs (the gd/references disagreements, the
Perl-vs-pack null completion, the bare-cursor insert text). The
projection-consistency net (`src/index/resolve/tests/
projection_consistency_tests.rs`) finds instances on a schedule; this
ADR owns the doctrine for what to DO with one once found. The governing
line is the CandidateSet ADR's: **pairs verify, the seam prevents** — a
test that keeps two implementations in agreement is a written
confession that there are two implementations, and the question is
always whether the confession is escapable.

## The four classes

**1. Forward/backward algorithm pairs — irreducible.** A computation
and its inverse cannot be merged, only checked for composition. The
membership test is strict: a genuinely irreducible pair composes in
BOTH directions and loses information in each. The `requires`
round-trip qualifies (its leg-2 weakening is the proof: forcing
symmetry would break rename, because implementations answers dispatch
reachability while references answers rename correctness — different
questions). Two enumerations that merely disagree do NOT qualify,
however different the verbs sound — the gd/references bugs presented
as forward-vs-backward and were both fixed by projection, so the
default for a new pair is **reducible until it survives the
composition test**. For this class the net's invariant is the
permanent, honest tool.

**2. Scope/perf forks of one question — collapse.** One enumeration
grown a sibling for a smaller scope or a faster path. The template:
one driver, scope-parameterized, with everything cross-cutting hoisted
above the scope split so a new axis lands once (`walk_refs` and its
`WalkScope`; `rehydrate_axes_or_resident`). After a collapse the
paired invariant SHRINKS to the residual claim the scope split leaves
(scope enumeration commutes with filtering) — it is re-documented, not
deleted, because that residue is exactly what a scope split can
silently break.

**3. Language-lane forks — fold the decision into a seam.** A Perl arm
and a pack arm making one decision. The cure is the value-carries-its-
rule pattern already in service: `Slot`, `VisibilityAxis`,
`LanguageScope`. A fork is sanctioned only where the *content*
genuinely differs per language and the *decision* is already seamed
(presenters over a shared resolution; `cursor_context` vs
`cursor_sentinel` under the `Slot` vocabulary).

**4. Eager/lazy twins — a ledger, not a backlog.** An eager writer and
a lazy reader of one rule is a deliberate purchase of speed with a
consistency risk. These are inventoried so the debt is known, and paid
only when one bites; most carry their sanction in a comment or ADR.

## The inventory

Collapsed (the templates): `refs_to`/`refs_to_in_file` → `walk_refs`;
`rehydrate_or_resident`/`rehydrate_rows_or_resident` → one body;
`package_isa_local` → `class_isa` over the `LocalParents` seam;
`walk_ancestry`/`GraphView::walk` → one engine (`graph::bounded_dfs`).
The ancestry collapse is the worked example of the bound rule below:
the two walkers guaranteed different things (visit budget vs depth
cap), so `WalkBound` carries BOTH axes as a type, each family's preset
preserves its pre-collapse guarantee exactly (`GRAPH`: depth 21,
visits unbounded; `ISA`: visits 200, depth unbounded), and the
divergent cases were pinned BEFORE the engines merged — after a
collapse nothing can detect a silently changed guarantee, because the
net compares siblings and there are no siblings left. Tightening a
preset is a deliberate, corpus-measured change to a constant, never a
side effect of routing.

Open, in execution order:

| pair | class | seam / note |
|---|---|---|
| `completion_items` vs `pack_completion` | 3 | assembly skeleton only (two-half gather, cap, `is_incomplete` composition); entity-content gathering stays out per the CandidateSet ADR's honest boundary. Ranked above its size: two of the six net-era bugs lived in this fork |
| `prepare_pack_parts` vs `prepare_workspace_parts` | 2/3 | tier-policy parameter; take it only if it falls out of the completion collapse |
| `index_perl` vs `index_pack` | 3 | **parked as a program, not a slice**: H9 bulk-defer coordination lives in the unshared halves, which is exactly where a mistake stays invisible until a corpus is large; no demonstrated bug yield. The persist harness, chunk writer, and residency tripwire are already shared |

The class-4 ledger: enrichment's eager `HashKeyOwner` stamp vs
`deferred_hash_key_owner` (comment-acknowledged); PostFold's
`invocant_class` bake vs the query-time invocant ladder;
`seed_return_types_from_bag`'s `return_types` map vs the registry
query (the watch item — a materialized map is what "edges, not values"
exists to prevent, but it predates the rule and retiring it is its own
project); `Ref::match_verdict_baked`'s bake-else-bag-fallback
(engineered for the rows lane).

## Discards — audited, not forks

Listed so nobody re-audits them: `sub_return_type_local` (a component
the full query composes), `resolve_symbol`/`resolve_symbol_scoped`
(wrapper), the `_inner`/`_cached` families (layering call-throughs;
`_cached` is the async-handler discipline, not a fork),
`hover_info`/`pack_hover` (presenters over one shared resolution —
sanctioned; flag only if a presenter starts making resolution
decisions).

## Related, distinct: stale agreement

The boundary case of the family, not a code fork: two verdicts that
agree by construction on a question neither is being asked. The worked
instance is a PR stack showing MERGEABLE with all checks GREEN —
MERGEABLE is a statement about text, GREEN is a statement about a base
that has since moved, and neither is a statement about the tree the
merge would produce (a rename sweeping 69 files merges cleanly against
new code referencing the old name; the result names a type that does
not exist). The cure is operational, not structural: a verdict is only
trusted about the tree it was computed on, so re-verify at the
integration tip before merging anything that predates it. Listed here
because a reader hunting fork-shaped hazards should find the edge of
the territory: sibling forks are two answers to one live question;
stale agreement is two answers to a question that expired.
