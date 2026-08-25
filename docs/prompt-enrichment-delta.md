# Prompt: enrichment as a delta artifact (ladder rung 9)

**Status: design, not started. The prerequisite three closed arcs point at:
level-indexed enrichment's rejection ("make a build cheap"), the FHEM crest
(the overlay clone pair is the per-worker set's majority), and the overlay
retention story (taint + byte caps exist because the artifact is a whole
copy).**

## The pressure, from three directions

An enriched analysis today is a WHOLE private copy: clone the base
`FileAnalysis`, truncate the sealed baselines, append/patch, serve the copy.
Three independent costs all trace to the artifact's size, not enrichment's
content:

1. **The clone is the cost of a build.** ~80 ms on ordinary files, 0.3–1 s+
   on giants (a 48k-line file's analysis measured ~105 MB, estimator-
   verified). Level-indexed enrichment — the algebra that deletes the taint
   rule and the depth cap — was built, worked, and was REJECTED solely
   because K levels × whole-copy builds priced it out
   (`docs/adr/level-indexed-enrichment.md`).
2. **Retention is rationed by the artifact.** The overlay cache is 64
   entries / 128 MiB because entries are whole analyses; giants are declined
   outright (built at full cost, then thrown away), and traversal-order
   taint means a cycle-touched build can never be cached at all.
3. **The crest.** During a batch sweep, each worker holds the clone PAIR
   (copy + source) live for the build's duration — measured as the majority
   of the ~414 MB/worker in-flight set that owns FHEM's RSS crest.

Against all that, the delta itself is TINY: measured 4.13% of base heap
(+10 symbols, +37 refs, +1,618 witnesses on the median enriching file).
We copy 100% to carry 4%.

## What enrichment actually writes (the enumerable surface)

The write surface of `enrich_imported_types_with_keys_for` is closed and
known — this is what makes the delta capturable without redesigning
enrichment:

| write | today | in the delta |
|---|---|---|
| imported-return TCs, mutation extensions, MCB edges, cross-file inheritance edges | `witnesses.push` above `base_witness_count` | `delta.witnesses: Vec<Witness>` |
| gated-emission symbols | `symbols.push` above baseline | `delta.symbols: Vec<Symbol>` |
| gated-emission refs | `refs.push` above baseline | `delta.refs: Vec<Ref>` |
| hash-key owner fixup | IN-PLACE `bind_hash_key_owner` on existing refs | `delta.owner_patches: Vec<(RefIdx, HashKeyOwner)>` |
| MethodCall target stamps | IN-PLACE `method_target` on existing refs | `delta.ref_patches: Vec<(RefIdx, MethodTarget)>` |
| index rebuilds | `rebuild_enrichment_indices` | `delta` carries its own small indices (see below) |

The truncate-to-baseline dance exists ONLY because the artifact is mutable —
a delta replaces it with immutability: re-enrichment derives a NEW delta,
and idempotency becomes structural instead of procedural (the enrichment
ADR predicted exactly this).

## Composition: who reads what

The consumer matrix (`docs/adr/enrichment-build-cost.md`) is the load-bearing
fact: **every recursive consumer reads only the bag.** Only `--check`
diagnostics and `--dump-package` read refs/symbols from an enriched copy.
So composition splits by consumer:

**The bag view (stage 2, the important one).** Registry queries take a bag;
introduce a composed view — base witnesses + delta witnesses, in that order
(enrichment appends, and append order is what latest-wins reducers already
assume). The attachment index composes the same way: probe the delta's
small index, then the base's; reducers fold over the concatenation. The
recursive consumers (`query_sub_return_type`'s imported recursion, the
`PackageSymbol` primary, the bridged bake, the R4 retries, enrichment's own
chase) all route through `ReducerRegistry::query` — the composition seam is
ONE entry point, not N call sites.

**The whole view (stage 3+, or never).** The two CLI verbs materialize a
composed whole copy on demand — one per file per run, exactly what they do
today via the clone, so they get no worse. If a composed `EnrichedView`
that answers the refs/symbols query surface lands later, even that clone
goes; it is deliberately NOT in scope for the first stages.

## Staging

1. **Capture.** Enrichment writes into a `Delta` sink instead of mutating
   the copy. Mechanical: the write surface above is every site, each
   becomes a `delta.…push` — and the truncate machinery deletes. The clone
   path remains the default consumer (apply the delta to a clone), so
   behavior is frozen while capture is validated: `apply(base.clone(),
   delta) == today's enriched copy`, byte-comparable, is the stage-1 test.
2. **Bag-view composition.** The registry accepts (base bag, delta
   witnesses) and the recursive consumers stop needing the whole copy at
   all. This is where the clone leaves the hot path.
3. **Overlay stores deltas.** `enriched_snapshot` retains `Delta` (4% the
   bytes) under the SAME key machinery — the byte cap becomes generous, the
   giant-decline path nearly closes, and repeat consults on cyclic files
   serve their (still honestly raw) base + whatever delta derived.
4. **Level-indexed revival** (separate arc): `enriched_k = base +
   delta_k`, deltas built from level-(k−1) views. Builds are now
   delta-sized, which is the exact prerequisite the rejection named. Taint
   and the depth cap delete here, not before.

## What it does NOT fix

The DENSITY of the base analysis (2.2 KB/line on giant files; refs 41.5% +
witnesses 39.4% of footprint) is untouched — rung 10's territory, and a
companion, not a substitute: rung 9 removes the copy-of-the-base from
enrichment's cost; rung 10 shrinks the base itself. The brk-retention
mechanism (churn through the allocator) is likewise orthogonal — smaller
artifacts churn less, but the ship posture decision stands on its own.

## Blast radius, per stage

| stage | touches | risk |
|---|---|---|
| 1 (capture) | `enrichment.rs` write sites; a `Delta` type in model | LOW — behavior frozen behind the apply-equivalence test; every site enumerable |
| 2 (bag view) | `ReducerRegistry::query` entry, the bag's attachment-index probe | MEDIUM — one seam, but it is THE seam; gold's 503 exact rows + the equiv flags are the net |
| 3 (delta overlay) | `enriched_snapshot` storage + `enriched_present` consumers | MEDIUM — key machinery unchanged; the decline/taint paths simplify rather than grow |
| 4 (level-indexed) | its own arc | HIGH, and it is the payoff — priced separately |

Stages 1–3 are each land-alone slices with the full battery; nothing
requires a flag day.
