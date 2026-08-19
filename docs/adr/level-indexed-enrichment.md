> **STATUS: REJECTED — design correct, constant factor fatal.** Kept because
> the reasoning is sound and the next person will reach for this idea. The
> implementation lives on branch `claude/level-indexed-enrichment` (`33c2a02f`)
> and is deliberately not merged. Read the measurements before rebuilding it.
>
> **The blocker is not the design, it is what a "build" costs** — but the
> expensive part of a build is NOT the copy. Measured in
> `docs/adr/enrichment-build-cost.md`: the whole-analysis copy is 3.8% of a
> build once it is a `clone` instead of a bincode round-trip (that swap alone
> took 27.8% off every build), and the enrichment delta really is small
> (4.13% of base). The dominant term is the **cross-file provider chase** at
> 61.6%, re-done from scratch on every build — and a level-indexed design
> pays it once per level. Making levels affordable means memoizing that chase
> across builds; an overlay would remove a further 3.8% and would not change
> this verdict.
>
> The 61.6% has since come down 6.4x, and not by the route this ADR's
> successor guessed at: the chase's cost was `bag_present` LRU misses, not
> provider resolution, and a resident-copy export gate removed it. A build is
> correspondingly cheaper, but K× of a cheaper build is still K×; nothing here
> is reopened by it.

# ADR: Level-indexed enrichment — correct, deterministic, and too slow as built

**Status: measured spike. Do not merge as-is.** The design does what it
promises; the straightforward implementation costs 12x on the healthy
corpus. This ADR exists so the next attempt starts from the measurements
rather than from the idea.

## The defect it targets

`enriched_snapshot`'s cycle guard is a thread-local set of paths on THIS
thread's stack. Two consequences, and the second is the expensive one:

1. **Depth is unbounded.** A chain of DISTINCT files recurses as deep as
   the dependency graph is long, deep-copying and enriching a whole
   `FileAnalysis` per level. At 138k files one `references` consult
   descended 220+ frames of enrich → query → enrich and never came back.
2. **The result is context-dependent, so it can never be cached.** Whether
   B comes back enriched or raw depends on who asked first. The code says
   so and refuses to retain a cycle-tainted build — correctly, given that
   design. Where mutual imports are the norm, "never retained" means every
   consult rebuilds a whole analysis. Raising `PERL_LSP_ENRICHED_CAP` to
   100,000 cannot help: the cap governs retention, and tainted results
   never reach it.

The correctness half is the better argument: the same file's enriched
analysis differs by traversal order today. That is order-dependent output.

## The design

`enriched_0(F) = raw(F)`; `enriched_k(F)` = F enriched against
`enriched_{k-1}` views of its providers. A provider consult raised inside a
level-k build asks for level k-1, so the level strictly decreases and the
recursion terminates in K steps **with no cycle detection**: a mutual pair
resolves as `A_2 → B_1 → A_0`. The overlay keys on `(path, level)`, and a
file's form at `(level, epoch)` no longer depends on who asked first — so
every level is cacheable, cyclic members included. It subsumes both the
taint rule and the recursion depth cap.

All of that holds in the implementation on this branch.

## Why it does not ship

Koha (3,554 files), server-path `references` on `store`, warm cache,
against `32a3bf4e` (the depth-cap containment branch) at 3,331 / 2,264 ms
with the answer at 284,617 bytes:

| K  | refs-1 | refs-2 | answer  | builds | overlay hits |
|----|--------|--------|---------|--------|--------------|
| 4  | 6,803  | 5,732  | 284,617 | 1,267  | 38,064       |
| 8  | 40,858 | 34,834 | 284,617 | 12,385 | 1,165,493    |
| 16 | timeout at 300 s (budget off)  | 20,808 | 1,330,653 |

The answer is right and stable at every K once the resolution budget is
disabled — with the budget ON, K=8 and K=16 are slow enough to trip the
30 s clock and return truncated, and two consecutive identical requests
returned 279,645 and 280,458 bytes. That non-determinism is the wall-clock
budget showing through, not the stratification.

**Builds scale with K, because that is exactly what the design asks for**:
a file is built once per level instead of once. Each build is a whole
`FileAnalysis` bincode round-trip. Stratification buys cacheability and
pays for it in build count, and at these constants the trade is a loss —
2.5x at K=4 (which reproduces the answer) and 15x at K=8.

## What would make it viable

The build has to get cheap before the level count can grow. Enrichment
today serialises and deserialises a whole analysis to obtain a private
copy; an incremental enrichment that produces a small overlay of derived
facts instead would make K× builds affordable and would shrink the
per-level memory the byte cap has to hold. That is the prerequisite, and it
is a larger change than this one.

Meanwhile the containment on `32a3bf4e` — memo, budget at the cross-file
boundary, depth cap, session around the heal — makes the verb return, and
the depth cap's declines are marked incomplete.
